use std::path::PathBuf;

use crate::models::FeedItem;

pub fn path() -> Option<PathBuf> {
  let mut p = std::env::var_os("HOME").map(PathBuf::from)?;
  p.push(".config");
  p.push("one-research");
  p.push("discovery_cache.json");
  Some(p)
}

pub fn load() -> Vec<FeedItem> {
  let Some(path) = path() else { return Vec::new() };
  let mut items: Vec<FeedItem> =
    super::load_json(&path, "one-research/discovery_cache");
  // Items persisted before sanitize-at-ingestion shipped may have raw
  // escape sequences in their string fields, plus `title_lower` /
  // `authors_lower` are `#[serde(skip)]` and need backfill on load
  // (audit Rel HIGH H1).
  for item in &mut items {
    item.sanitize_in_place();
  }
  items
}

pub fn save(items: &[FeedItem]) {
  if let Some(path) = path() {
    super::save_json(&items, &path, "one-research/discovery_cache");
  }
}
