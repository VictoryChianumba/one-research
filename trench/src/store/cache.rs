use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use crate::models::FeedItem;

/// Debounce window for the background writer. The writer collects `queue_save`
/// messages within this window and writes only the latest snapshot to disk —
/// so a burst of 10 process_incoming calls during a refresh produces 1 disk
/// write, not 10. Tuned long enough to coalesce a typical refresh batch but
/// short enough that quit-without-flush would lose ≤ 1 batch.
const DEBOUNCE_MS: u64 = 500;

fn cache_path() -> Option<PathBuf> {
  let mut p = std::env::var_os("HOME").map(PathBuf::from)?;
  p.push(".config");
  p.push("trench");
  p.push("cache.json");
  Some(p)
}

pub fn load() -> Vec<FeedItem> {
  let path = match cache_path() {
    Some(p) => p,
    None => return Vec::new(),
  };

  let bytes = match fs::read(&path) {
    Ok(b) => b,
    Err(_) => return Vec::new(),
  };

  let mut items: Vec<FeedItem> = match serde_json::from_slice(&bytes) {
    Ok(v) => v,
    Err(e) => {
      super::quarantine_corrupted(&path, "trench/cache", &e);
      return Vec::new();
    }
  };
  // Defense-in-depth: items persisted before sanitize-at-ingestion shipped
  // may have raw escape sequences baked into their string fields. Idempotent
  // re-sanitize protects against terminal-hijack via the cache.
  for item in &mut items {
    item.sanitize_in_place();
  }
  items
}

/// Synchronous serialize + atomic disk write. Used by the background writer
/// thread and by `flush_blocking` on shutdown. UI-thread callers should
/// prefer `queue_save` to avoid hitching during a refresh batch.
pub fn save(items: &[FeedItem]) {
  let path = match cache_path() {
    Some(p) => p,
    None => return,
  };

  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }

  if let Ok(json) = serde_json::to_vec(items) {
    if let Err(e) = super::atomic_write(&path, &json) {
      log::error!("trench/cache: atomic_write failed at {}: {e}", path.display());
    }
    crate::store::set_private(&path);
  }
}

enum WriterMsg {
  Items(Vec<FeedItem>),
  /// On receipt, flush any pending snapshot, then ack so the caller can
  /// proceed (used at process shutdown).
  Flush(Sender<()>),
}

static WRITER_TX: OnceLock<Sender<WriterMsg>> = OnceLock::new();

fn writer_handle() -> &'static Sender<WriterMsg> {
  WRITER_TX.get_or_init(|| {
    let (tx, rx) = mpsc::channel::<WriterMsg>();
    let _ = thread::Builder::new().name("trench-cache-writer".into()).spawn(
      move || {
        // Wrap the loop body in catch_unwind so a panic in serde_json /
        // fs / atomic_write surfaces a diagnostic instead of dying
        // silently. Without this, a panicking writer leaves WRITER_TX
        // holding a stale Sender — every subsequent queue_save returns
        // Ok but the saves vanish (audit Rel HIGH H5).
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
          move || writer_loop(rx),
        ));
        if let Err(payload) = result {
          let msg = if let Some(s) = payload.downcast_ref::<&'static str>() {
            (*s).to_string()
          } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
          } else {
            "non-string payload".to_string()
          };
          log::error!(
            "trench-cache-writer thread panicked: {msg} — subsequent \
             queue_save calls will send to a closed channel and saves \
             will silently fail. Restart trench to recover."
          );
        }
      },
    );
    tx
  })
}

/// Inner loop body extracted so `catch_unwind` can wrap it cleanly.
/// Single-slot coalesce: latest snapshot wins. While `pending` is Some we
/// cap waits at DEBOUNCE_MS so a burst flushes promptly; while empty we
/// block forever to avoid spinning.
fn writer_loop(rx: std::sync::mpsc::Receiver<WriterMsg>) {
  let mut pending: Option<Vec<FeedItem>> = None;
  loop {
    let msg = if pending.is_some() {
      match rx.recv_timeout(Duration::from_millis(DEBOUNCE_MS)) {
        Ok(m) => Some(m),
        Err(RecvTimeoutError::Timeout) => None,
        Err(RecvTimeoutError::Disconnected) => {
          if let Some(items) = pending.take() {
            save(&items);
          }
          return;
        }
      }
    } else {
      match rx.recv() {
        Ok(m) => Some(m),
        Err(_) => return,
      }
    };
    match msg {
      Some(WriterMsg::Items(items)) => {
        pending = Some(items);
      }
      Some(WriterMsg::Flush(ack)) => {
        if let Some(items) = pending.take() {
          save(&items);
        }
        let _ = ack.send(());
      }
      None => {
        // Debounce window expired — flush.
        if let Some(items) = pending.take() {
          save(&items);
        }
      }
    }
  }
}

/// Hand a snapshot off to the background writer. Returns immediately.
/// Multiple calls within DEBOUNCE_MS coalesce into a single disk write.
pub fn queue_save(items: Vec<FeedItem>) {
  let _ = writer_handle().send(WriterMsg::Items(items));
}

/// Wait for the writer to finish any pending work. Call once on shutdown so
/// the on-disk cache reflects the final in-memory state. Bounded so a wedged
/// I/O can't hang process exit. Logs both failure modes loudly — silent
/// final-batch loss is the symptom audit Rel MED #3 / HIGH H5 cluster
/// describes.
pub fn flush_blocking() {
  let (ack_tx, ack_rx) = mpsc::channel();
  match writer_handle().send(WriterMsg::Flush(ack_tx)) {
    Err(_) => {
      log::error!(
        "trench-cache-writer: flush send failed — writer thread is gone, \
         the final cache.json snapshot was not written."
      );
    }
    Ok(()) => {
      if ack_rx.recv_timeout(Duration::from_secs(5)).is_err() {
        log::error!(
          "trench-cache-writer: flush ack timeout (5s) — disk wedged or \
           writer stuck, final cache snapshot may not be on disk."
        );
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// End-to-end smoke: a cache file that already contains terminal-escape
  /// sequences in `title` / `summary_short` (e.g. an item ingested before
  /// sanitize-at-ingestion shipped) must be sanitized on load. This protects
  /// against terminal hijack via the cache.
  #[test]
  fn load_sanitizes_escape_sequences_in_existing_cache() {
    use crate::models::{
      ContentType, SignalLevel, SourcePlatform, WorkflowState,
    };

    // Hostile payload: clear-screen CSI in title, OSC 52 (clipboard) in
    // summary, OSC 8 hyperlink in authors.
    let raw_item = FeedItem {
      id: "test-1".into(),
      title: "Paper Title\x1b[2J\x1b[Hhijack".into(),
      source_platform: SourcePlatform::ArXiv,
      content_type: ContentType::Paper,
      domain_tags: vec!["legit\x1b]52;c;evil\x07tag".into()],
      signal: SignalLevel::Primary,
      published_at: "2026-05-04".into(),
      authors: vec!["A\x1b]8;;https://evil/\x1b\\Real\x1b]8;;\x1b\\B".into()],
      summary_short: "Abstract\x1b[31mred\x1b[0m end".into(),
      workflow_state: WorkflowState::Inbox,
      url: "http://arxiv.org/abs/test-1".into(),
      upvote_count: 0,
      github_repo: None,
      github_owner: None,
      github_repo_name: None,
      benchmark_results: vec![],
      full_content: Some("body\x1b[2Kclear".into()),
      source_name: "arxiv".into(),
      title_lower: String::new(),
      authors_lower: Vec::new(),
    };
    let json = serde_json::to_vec(&vec![raw_item]).unwrap();

    // Set HOME to a temp dir so cache_path() resolves there. Running this in
    // serial would be safer; for a single test it's fine since other tests
    // don't rely on $HOME.
    let dir = std::env::temp_dir()
      .join(format!("trench_cache_load_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".config/trench")).unwrap();
    std::fs::write(dir.join(".config/trench/cache.json"), &json).unwrap();

    // SAFETY: tests run in a single-process address space; we override HOME
    // for the duration of this test only. Acceptable risk inside a #[cfg(test)]
    // block that doesn't run in parallel with other HOME-dependent tests.
    let prev_home = std::env::var_os("HOME");
    unsafe {
      std::env::set_var("HOME", &dir);
    }

    let items = load();

    // Restore HOME before any assertion can fail and skip the cleanup.
    unsafe {
      match prev_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
      }
    }
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(items.len(), 1);
    let it = &items[0];
    assert!(!it.title.contains('\x1b'), "title still has ESC: {:?}", it.title);
    assert!(
      !it.summary_short.contains('\x1b'),
      "summary still has ESC: {:?}",
      it.summary_short
    );
    assert!(
      it.authors.iter().all(|a| !a.contains('\x1b')),
      "authors still have ESC: {:?}",
      it.authors
    );
    assert!(
      it.domain_tags.iter().all(|t| !t.contains('\x1b')),
      "tags still have ESC: {:?}",
      it.domain_tags
    );
    assert!(
      it.full_content.as_deref().is_none_or(|s| !s.contains('\x1b')),
      "full_content still has ESC: {:?}",
      it.full_content
    );
    // Visible text is preserved.
    assert!(it.title.contains("Paper Title"));
    assert!(it.title.contains("hijack"));
    assert!(it.summary_short.contains("Abstract"));
    assert!(it.summary_short.contains("end"));
  }
}
