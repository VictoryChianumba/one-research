use std::sync::mpsc;

use crate::ingestion::pipeline::FetchContext;
use crate::ingestion::registry::{build_enrichments, build_sources};
use crate::models;
use crate::{config, ingestion, panic_msg};
use ingestion::message::FetchMessage;

/// Uniform per-source runner. Logs the start, dispatches `fetch_fn`, then
/// either extends the shared accumulator and emits Items + SourceComplete,
/// or routes a SourceError. Mutex-poison recovery follows the W3 voice
/// pattern — a poisoned lock on `all_items` is recovered rather than
/// crashing the refresh.
fn run_source<F>(
  name: &str,
  tx: &mpsc::Sender<FetchMessage>,
  all_items: &std::sync::Mutex<Vec<models::FeedItem>>,
  fetch_fn: F,
) where
  F: FnOnce() -> Result<Vec<models::FeedItem>, String>,
{
  log::info!("source {name}: starting fetch");
  let t = std::time::Instant::now();
  match fetch_fn() {
    Ok(items) => {
      log::info!(
        "source {name}: completed in {}ms, {} items",
        t.elapsed().as_millis(),
        items.len()
      );
      // Clone outside the lock so the critical section is just the
      // `.extend(...)` move. Two consumers (accumulator + channel) so
      // one clone is unavoidable; the win is shrinking the lock window
      // and avoiding torn-state risk on panic mid-clone.
      let to_extend = items.clone();
      all_items.lock().unwrap_or_else(|e| e.into_inner()).extend(to_extend);
      let _ = tx.send(FetchMessage::Items(items));
      let _ = tx.send(FetchMessage::SourceComplete(name.to_string()));
    }
    Err(e) => {
      log::error!(
        "source {name}: failed in {}ms — {e}",
        t.elapsed().as_millis()
      );
      let _ = tx.send(FetchMessage::SourceError(name.to_string(), e));
    }
  }
}

/// Wrap a per-source thread body in `catch_unwind` so a panic does not
/// kill its siblings. Routes the panic to a SourceError + SourceComplete
/// pair so the loading-spinner clears and the UI surfaces the error.
fn run_source_protected<F>(name: &str, tx: &mpsc::Sender<FetchMessage>, body: F)
where
  F: FnOnce(),
{
  let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
  if let Err(payload) = outcome {
    let msg = panic_msg(payload);
    log::error!("source {name}: thread panicked — {msg}");
    let _ = tx.send(FetchMessage::SourceError(
      name.to_string(),
      format!("source thread panicked: {msg}"),
    ));
    let _ = tx.send(FetchMessage::SourceComplete(name.to_string()));
  }
}

pub(crate) fn spawn_fetch(
  tx: mpsc::Sender<FetchMessage>,
  config: config::Config,
) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      // Shared accumulator for enrichment. Each scope thread locks
      // briefly to extend; critical section is one Vec::extend.
      let all_items: std::sync::Mutex<Vec<models::FeedItem>> =
        std::sync::Mutex::new(Vec::new());

      // Build the registries. Gating (enabled-source flag, API key
      // presence, custom feeds) lives in `ingestion::registry`; source
      // impls themselves are unaware of why they were built. See
      // ADR-004 §D6.
      let sources = build_sources(&config);
      let enrichments = build_enrichments(&config);

      // FetchContext outlives every spawn thread because thread::scope
      // joins before this function returns. `'a` on the struct only.
      // `cache_dir` is `~/.config/trench/` (the directory every existing
      // per-source cache file already lives in); falls back to a temp
      // dir only on the unreachable case of `$HOME` being unset.
      let cache_dir = std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".config").join("trench"))
        .unwrap_or_else(std::env::temp_dir);
      let ctx = FetchContext { config: &config, cache_dir: &cache_dir };

      // Group sources by `host_group()`. Sources sharing a tag run
      // serially in one thread (rate-limit politeness); different tags
      // run in parallel. Today's only-multi-source group is "arxiv"
      // (arxiv + huggingface, both hit export.arxiv.org).
      let mut by_group: std::collections::HashMap<String, Vec<&dyn Source>> =
        std::collections::HashMap::new();
      for src in &sources {
        by_group
          .entry(src.host_group().to_string())
          .or_default()
          .push(src.as_ref());
      }

      let all_items_ref = &all_items;
      let ctx_ref = &ctx;
      let pipeline_t0 = std::time::Instant::now();
      std::thread::scope(|scope| {
        for (group, group_sources) in by_group {
          let tx = tx.clone();
          scope.spawn(move || {
            run_source_protected(&group, &tx, || {
              for src in group_sources {
                run_source(src.name(), &tx, all_items_ref, || {
                  src.fetch(ctx_ref)
                });
              }
            });
          });
        }
      });

      // All scope threads have joined; recover ownership from the Mutex.
      let mut all_items =
        all_items.into_inner().unwrap_or_else(|e| e.into_inner());

      log::info!(
        "ingestion: fetch phase completed in {}ms ({} items total)",
        pipeline_t0.elapsed().as_millis(),
        all_items.len()
      );

      // Enrichment phase: sequential, single-threaded, post-scope.
      // Each enrichment owns its cache (RefCell) loaded in `::new()`
      // at registry-build time — see ADR-004 §D4 for the `!Sync`
      // rationale on `EnrichmentSource`.
      for enrichment in &enrichments {
        let t = std::time::Instant::now();
        enrichment.enrich(&mut all_items, &ctx);
        log::info!(
          "ingestion: {} enrichment in {}ms",
          enrichment.name(),
          t.elapsed().as_millis()
        );
      }

      let with_repo =
        all_items.iter().filter(|i| i.github_repo.is_some()).count();
      log::info!(
        "ingestion: total pipeline {}ms ({with_repo}/{} items have github_repo set)",
        pipeline_t0.elapsed().as_millis(),
        all_items.len()
      );
      let _ = tx.send(FetchMessage::EnrichedItems(all_items));
      let _ = tx.send(FetchMessage::SourceComplete("enriching".to_string()));
      let _ = tx.send(FetchMessage::AllComplete);
    }));
    if let Err(payload) = result {
      let msg = panic_msg(payload);
      log::error!("spawn_fetch: background thread panicked — {msg}");
      let _ = tx_panic.send(FetchMessage::SourceError(
        "background".to_string(),
        format!("background thread panicked: {msg}"),
      ));
      let _ = tx_panic.send(FetchMessage::AllComplete);
    }
  });
}

use ingestion::pipeline::Source;
