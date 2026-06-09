use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sorter::Sorter;

const STATE_FILE_NAME: &str = "state.json";

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AppState {
  pub sorter: Sorter,
  pub full_screen: bool,
}

impl AppState {
  fn state_path() -> PathBuf {
    dirs::config_dir()
      .unwrap_or_else(|| PathBuf::from("."))
      .join("one-research")
      .join("notes")
      .join(STATE_FILE_NAME)
  }

  pub fn load() -> anyhow::Result<Self> {
    let path = Self::state_path();
    if !path.exists() {
      return Ok(AppState::default());
    }
    let bytes = std::fs::read(&path)
      .map_err(|err| anyhow::anyhow!("Failed to open state file: {err}"))?;
    let state = serde_json::from_slice(&bytes)
      .map_err(|err| anyhow::anyhow!("Failed to parse state file: {err}"))?;
    Ok(state)
  }

  pub fn save(&self) -> anyhow::Result<()> {
    self.save_to(&Self::state_path())
  }

  /// Inner save that takes an explicit path so tests can drive it without
  /// touching `dirs::config_dir()`. Routes through `crate::storage::atomic_write`
  /// so a SIGINT / panic / power loss mid-write either leaves the previous
  /// state file intact or atomically replaces it — never truncated.
  fn save_to(&self, path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
      std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(self)?;
    crate::storage::atomic_write(path, &bytes)?;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn save_to_writes_atomically_and_cleans_tmp_sidecar() {
    let dir = std::env::temp_dir()
      .join(format!("one_research_appstate_save_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("state.json");
    let tmp = dir.join("state.json.tmp");

    let state = AppState::default();
    state.save_to(&path).expect("save ok");

    assert!(path.exists(), "state file should be written");
    assert!(!tmp.exists(), "tmp sidecar should be renamed away");

    // Round-trip: the bytes parse back to a valid AppState.
    let bytes = std::fs::read(&path).unwrap();
    let _: AppState = serde_json::from_slice(&bytes).unwrap();

    let _ = std::fs::remove_dir_all(&dir);
  }

  #[cfg(unix)]
  #[test]
  fn save_to_produces_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir()
      .join(format!("one_research_appstate_perms_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("state.json");

    AppState::default().save_to(&path).expect("save ok");

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    let _ = std::fs::remove_dir_all(&dir);
  }
}
