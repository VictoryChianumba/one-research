use std::path::PathBuf;

use crate::history::{HISTORY_CAP, HistoryEntry};

pub fn load() -> Vec<HistoryEntry> {
  let Some(path) = path() else { return Vec::new() };
  let mut entries: Vec<HistoryEntry> =
    super::load_json(&path, "one-research/history");
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
  let trimmed: &[HistoryEntry] =
    if entries.len() > HISTORY_CAP { &entries[..HISTORY_CAP] } else { entries };
  super::save_json(&trimmed, &path, "one-research/history");
}

fn path() -> Option<PathBuf> {
  let mut p = std::env::var_os("HOME").map(PathBuf::from)?;
  p.push(".config/one-research/history.json");
  Some(p)
}
