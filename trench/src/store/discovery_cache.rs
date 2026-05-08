use std::fs;
use std::path::PathBuf;

use crate::models::FeedItem;

pub fn path() -> Option<PathBuf> {
  let mut p = std::env::var_os("HOME").map(PathBuf::from)?;
  p.push(".config");
  p.push("trench");
  p.push("discovery_cache.json");
  Some(p)
}

pub fn load() -> Vec<FeedItem> {
  let path = match path() {
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
      super::quarantine_corrupted(&path, "trench/discovery_cache", &e);
      return Vec::new();
    }
  };
  // Mirror `cache::load` — items persisted before sanitize-at-ingestion
  // shipped may have raw escape sequences in their string fields, plus
  // `title_lower` / `authors_lower` are `#[serde(skip)]` and need
  // backfill on load (audit Rel HIGH H1).
  for item in &mut items {
    item.sanitize_in_place();
  }
  items
}

pub fn save(items: &[FeedItem]) {
  let path = match path() {
    Some(p) => p,
    None => return,
  };

  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }

  if let Ok(json) = serde_json::to_vec(items) {
    let _ = super::atomic_write(&path, &json);
  }
}
