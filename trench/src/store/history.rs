use std::fs;
use std::path::PathBuf;

use crate::history::{HISTORY_CAP, HistoryEntry};

pub fn load() -> Vec<HistoryEntry> {
  let Some(path) = path() else { return Vec::new() };
  let Ok(bytes) = fs::read(&path) else { return Vec::new() };
  let mut entries: Vec<HistoryEntry> = match serde_json::from_slice(&bytes) {
    Ok(v) => v,
    Err(e) => {
      super::quarantine_corrupted(&path, "trench/history", &e);
      return Vec::new();
    }
  };
  entries.sort_by(|a, b| b.opened_at.cmp(&a.opened_at));
  entries.truncate(HISTORY_CAP);
  // title_lower is `#[serde(skip)]` — backfill so the search filter doesn't
  // see empty strings on entries persisted from prior sessions.
  for entry in &mut entries {
    entry.title_lower = entry.title.to_lowercase();
  }
  entries
}

pub fn save(entries: &[HistoryEntry]) {
  let Some(path) = path() else { return };
  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }
  let trimmed: &[HistoryEntry] =
    if entries.len() > HISTORY_CAP { &entries[..HISTORY_CAP] } else { entries };
  if let Ok(json) = serde_json::to_vec(trimmed) {
    let _ = super::atomic_write(&path, &json);
  }
}

fn path() -> Option<PathBuf> {
  let mut p = std::env::var_os("HOME").map(PathBuf::from)?;
  p.push(".config/trench/history.json");
  Some(p)
}
