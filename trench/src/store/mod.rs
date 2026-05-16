pub mod cache;
pub mod discovery_cache;
pub mod enrichment_cache;
pub mod history;
pub mod session;
pub mod tags;

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::app::NotesTab;
use crate::models::WorkflowState;

/// Restrict a file to owner-read/write only (0o600). Best-effort on Unix.
pub(crate) fn set_private(path: &PathBuf) {
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
  }
  let _ = path; // suppress unused warning on non-Unix
}

/// Crash-safe write: writes `bytes` to `<path>.tmp`, fsyncs, then renames
/// onto `path`. A panic, SIGINT, or power loss between the two steps either
/// leaves the original file unchanged or replaces it atomically — never
/// truncated. Replaces every prior `fs::write(path, bytes)` callsite in the
/// store layer.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
  let mut tmp_name: OsString = path.as_os_str().to_owned();
  tmp_name.push(".tmp");
  let tmp_path = PathBuf::from(tmp_name);

  // Best-effort cleanup of any stale tmp file from a previous crash.
  let _ = fs::remove_file(&tmp_path);

  // Defense-in-depth against pre-planted symlinks at the tmp path: if
  // the cleanup above failed (perm denied, etc.) and a symlink remains,
  // refuse to follow it. Bounded TOCTOU between this check and the
  // create — closes the easy attack while leaving the corner case open.
  if let Ok(meta) = fs::symlink_metadata(&tmp_path) {
    if meta.file_type().is_symlink() {
      return Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "atomic_write: refusing to write through pre-planted symlink",
      ));
    }
  }

  {
    let mut f = fs::File::create(&tmp_path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
  }

  // Inherit 0o600 mode immediately so the brief 0644 window between rename
  // and the caller's `set_private` no longer exists.
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
  }

  fs::rename(&tmp_path, path)
}

/// On parse failure, rename `<path>` to a unique `<path>.broken-<...>`
/// sidecar so the next save doesn't clobber the user's only recovery
/// copy. Suffix combines unix nanoseconds + pid + a per-process atomic
/// counter so two failures within one nanosecond on the same process
/// cannot collide. On rename failure the corrupted file remains at
/// `path` and a loud log line is emitted so the user knows the next
/// save will overwrite the recovery copy. Pairs with `atomic_write` to
/// close the data-loss class.
pub(crate) fn quarantine_corrupted(
  path: &Path,
  label: &str,
  err: &dyn std::fmt::Display,
) {
  use std::sync::atomic::{AtomicU64, Ordering};
  static COUNTER: AtomicU64 = AtomicU64::new(0);
  let ts_nanos = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  let pid = std::process::id();
  let n = COUNTER.fetch_add(1, Ordering::Relaxed);
  let mut new_name: OsString = path.as_os_str().to_owned();
  new_name.push(format!(".broken-{ts_nanos}-{pid}-{n}"));
  let new_path = PathBuf::from(&new_name);
  log::error!(
    "{label}: parse failed at {} — quarantining to {}: {err}",
    path.display(),
    new_path.display()
  );
  // Defense-in-depth: refuse to quarantine the source if it's a symlink.
  // POSIX `rename` operates on the directory entry, not the target, so
  // the rename itself doesn't follow links — but quarantining an
  // attacker-planted symlink would silently move the link out of the
  // way and let the next save create a fresh real file at the original
  // path, masking the tampering. Stop and surface it instead.
  match fs::symlink_metadata(path) {
    Ok(meta) if meta.file_type().is_symlink() => {
      log::error!(
        "{label}: refusing to quarantine symlink at {} — possible tampering",
        path.display()
      );
      return;
    }
    Ok(_) => {}
    Err(stat_err) => {
      log::error!(
        "{label}: stat failed for {} — skipping quarantine: {stat_err}",
        path.display()
      );
      return;
    }
  }
  if let Err(rename_err) = fs::rename(path, &new_path) {
    log::error!(
      "{label}: rename to quarantine path failed — corrupted file \
       remains at {} and will be overwritten on the next save: \
       {rename_err}",
      path.display()
    );
  }
}

fn state_path() -> Option<PathBuf> {
  let mut p = dirs_home()?;
  p.push(".config");
  p.push("trench");
  p.push("state.json");
  Some(p)
}

/// Best-effort home directory — uses $HOME, falls back to nothing.
fn dirs_home() -> Option<PathBuf> {
  std::env::var_os("HOME").map(PathBuf::from)
}

pub fn load() -> HashMap<String, WorkflowState> {
  let path = match state_path() {
    Some(p) => p,
    None => return HashMap::new(),
  };

  let bytes = match fs::read(&path) {
    Ok(b) => b,
    Err(_) => return HashMap::new(),
  };

  // Tolerant load: parse per-key so unknown variants (e.g. legacy "skimmed")
  // fall back to Inbox instead of wiping the entire map.
  let raw: HashMap<String, serde_json::Value> =
    match serde_json::from_slice(&bytes) {
      Ok(m) => m,
      Err(e) => {
        quarantine_corrupted(&path, "trench/state", &e);
        return HashMap::new();
      }
    };

  raw
    .into_iter()
    .map(|(k, v)| {
      let state = serde_json::from_value::<WorkflowState>(v)
        .unwrap_or(WorkflowState::Inbox);
      (k, state)
    })
    .collect()
}

pub fn save(state: &HashMap<String, WorkflowState>) {
  let path = match state_path() {
    Some(p) => p,
    None => return,
  };

  // Create parent dirs if needed — silently ignore failure
  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }

  if let Ok(json) = serde_json::to_vec(state) {
    if let Err(e) = atomic_write(&path, &json) {
      log::error!(
        "trench/state: atomic_write failed at {}: {e}",
        path.display()
      );
    }
    set_private(&path);
  }
}

// ── UI state (last_read, etc.) ─────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct UiState {
  pub last_read: Option<String>,
  pub last_read_source: Option<String>,
  #[serde(default)]
  pub notes_tabs: Vec<NotesTab>,
  #[serde(default)]
  pub notes_active_tab: usize,
  #[serde(default)]
  pub secondary_notes_tabs: Vec<NotesTab>,
  #[serde(default)]
  pub secondary_notes_active_tab: usize,
}

fn ui_path() -> Option<PathBuf> {
  let mut p = dirs_home()?;
  p.push(".config");
  p.push("trench");
  p.push("ui.json");
  Some(p)
}

pub fn load_ui() -> UiState {
  let path = match ui_path() {
    Some(p) => p,
    None => return UiState::default(),
  };
  let bytes = match fs::read(&path) {
    Ok(b) => b,
    Err(_) => return UiState::default(),
  };
  match serde_json::from_slice(&bytes) {
    Ok(s) => s,
    Err(e) => {
      quarantine_corrupted(&path, "trench/ui", &e);
      UiState::default()
    }
  }
}

pub fn save_ui(state: &UiState) {
  let path = match ui_path() {
    Some(p) => p,
    None => return,
  };
  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }
  if let Ok(json) = serde_json::to_vec(state) {
    if let Err(e) = atomic_write(&path, &json) {
      log::error!("trench/ui: atomic_write failed at {}: {e}", path.display());
    }
    set_private(&path);
  }
}

#[cfg(test)]
mod atomic_write_tests {
  use super::atomic_write;
  use std::fs;

  #[test]
  fn writes_bytes_and_cleans_tmp() {
    let dir = std::env::temp_dir()
      .join(format!("trench_atomic_write_test_{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("payload.json");
    let tmp = dir.join("payload.json.tmp");

    atomic_write(&path, b"hello world").expect("write ok");
    assert_eq!(fs::read(&path).unwrap(), b"hello world");
    assert!(!tmp.exists(), "tmp sidecar should be renamed away");
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn overwrite_replaces_existing_content() {
    let dir = std::env::temp_dir()
      .join(format!("trench_atomic_overwrite_test_{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("payload.json");
    fs::write(&path, b"original").unwrap();

    atomic_write(&path, b"replaced").expect("write ok");
    assert_eq!(fs::read(&path).unwrap(), b"replaced");
    let _ = fs::remove_dir_all(&dir);
  }

  #[cfg(unix)]
  #[test]
  fn produces_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir()
      .join(format!("trench_atomic_perms_test_{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("payload.json");

    atomic_write(&path, b"sensitive").expect("write ok");
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "atomic_write should produce 0600");
    let _ = fs::remove_dir_all(&dir);
  }
}
