use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn cache_path() -> Option<PathBuf> {
  let mut p = std::env::var_os("HOME").map(PathBuf::from)?;
  p.push(".config");
  p.push("trench");
  p.push("enrichment_cache.json");
  Some(p)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EnrichmentEntry {
  pub authors: Vec<String>,
  pub institution: String,
  pub citation_count: u32,
  pub fields_of_study: Vec<String>,
  pub cached_at: String,
}

pub fn load() -> HashMap<String, EnrichmentEntry> {
  let path = match cache_path() {
    Some(p) => p,
    None => return HashMap::new(),
  };
  let bytes = match fs::read(&path) {
    Ok(b) => b,
    Err(_) => return HashMap::new(),
  };
  let mut cache: HashMap<String, EnrichmentEntry> =
    match serde_json::from_slice(&bytes) {
      Ok(v) => v,
      Err(e) => {
        super::quarantine_corrupted(&path, "trench/enrichment_cache", &e);
        return HashMap::new();
      }
    };
  // Invalidate entries with no field data so they are re-fetched once an API
  // key is configured.
  for entry in cache.values_mut() {
    if entry.fields_of_study.is_empty() {
      entry.cached_at = "1970-01-01".to_string();
    }
  }
  log::info!("enrichment_cache: loaded {} entries", cache.len());
  cache
}

pub fn save(cache: &HashMap<String, EnrichmentEntry>) {
  let path = match cache_path() {
    Some(p) => p,
    None => return,
  };
  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }
  if let Ok(json) = serde_json::to_vec(cache) {
    if let Err(e) = super::atomic_write(&path, &json) {
      log::warn!("enrichment_cache: failed to save to {path:?} — {e}");
    } else {
      crate::store::set_private(&path);
    }
  }
}

/// Returns true if the entry was cached more than 7 days ago.
pub fn is_stale(entry: &EnrichmentEntry, id: &str) -> bool {
  use chrono::NaiveDate;
  let cached = match NaiveDate::parse_from_str(&entry.cached_at, "%Y-%m-%d") {
    Ok(d) => d,
    Err(_) => return true,
  };
  let today = chrono::Utc::now().date_naive();
  let stale = (today - cached).num_days() > 7;
  if stale {
    log::debug!(
      "enrichment_cache: stale entry for arXiv:{id} (cached_at={})",
      entry.cached_at
    );
  }
  stale
}

/// Today's date as "YYYY-MM-DD".
pub fn today_str() -> String {
  chrono::Utc::now().format("%Y-%m-%d").to_string()
}
