use std::collections::HashMap;
use std::path::PathBuf;

fn cache_path() -> Option<PathBuf> {
  let mut p = std::env::var_os("HOME").map(PathBuf::from)?;
  p.push(".config");
  p.push("one-research");
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
  let Some(path) = cache_path() else { return HashMap::new() };
  let mut cache: HashMap<String, EnrichmentEntry> =
    super::load_json(&path, "one-research/enrichment_cache");
  // Invalidate entries with no field data so they are re-fetched once an
  // API key is configured.
  for entry in cache.values_mut() {
    if entry.fields_of_study.is_empty() {
      entry.cached_at = "1970-01-01".to_string();
    }
  }
  log::info!("enrichment_cache: loaded {} entries", cache.len());
  cache
}

pub fn save(cache: &HashMap<String, EnrichmentEntry>) {
  let Some(path) = cache_path() else { return };
  super::save_json(cache, &path, "one-research/enrichment_cache");
  crate::store::set_private(&path);
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
