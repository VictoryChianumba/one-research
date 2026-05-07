use std::fs;
use std::path::PathBuf;

use crate::discovery::SessionHistory;

pub fn load() -> SessionHistory {
  let path = match path() {
    Some(p) => p,
    None => return SessionHistory::default(),
  };
  let bytes = match fs::read(&path) {
    Ok(b) => b,
    Err(_) => return SessionHistory::default(),
  };
  match serde_json::from_slice(&bytes) {
    Ok(v) => v,
    Err(e) => {
      super::quarantine_corrupted(&path, "trench/discovery_session", &e);
      SessionHistory::default()
    }
  }
}

pub fn save(session: &SessionHistory) {
  let path = match path() {
    Some(p) => p,
    None => return,
  };
  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }
  if let Ok(json) = serde_json::to_vec(session) {
    let _ = super::atomic_write(&path, &json);
  }
}

pub fn clear() {
  if let Some(path) = path() {
    let _ = super::atomic_write(&path, b"{}");
  }
}

fn path() -> Option<PathBuf> {
  let mut p = std::env::var_os("HOME").map(PathBuf::from)?;
  p.push(".config/trench/discovery_session.json");
  Some(p)
}
