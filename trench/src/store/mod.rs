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

/// On parse failure, rename `<path>` to `<path>.broken-<unix_ts>` so the next
/// save doesn't clobber the user's only recovery copy. Best-effort — rename
/// failure is non-fatal and the caller still returns Default. Pairs with
/// `atomic_write` to close the data-loss class: torn writes can't happen, and
/// pre-existing corruption no longer silently nukes state.
pub(crate) fn quarantine_corrupted(
  path: &Path,
  label: &str,
  err: &dyn std::fmt::Display,
) {
  let ts = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_secs())
    .unwrap_or(0);
  let mut new_name: OsString = path.as_os_str().to_owned();
  new_name.push(format!(".broken-{ts}"));
  let new_path = PathBuf::from(&new_name);
  log::error!(
    "{label}: parse failed at {} — quarantined to {}: {err}",
    path.display(),
    new_path.display()
  );
  let _ = fs::rename(path, &new_path);
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
    let _ = atomic_write(&path, &json);
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
    let _ = atomic_write(&path, &json);
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
