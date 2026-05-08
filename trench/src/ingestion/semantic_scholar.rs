use std::collections::HashMap;

use crate::models::{FeedItem, arxiv_id_from_url};
use crate::store::enrichment_cache::{
  EnrichmentEntry, is_stale, save, today_str,
};

/// Enrich arXiv items with Semantic Scholar metadata.
///
/// Hits the S2 API for each item with an arXiv URL that is missing from the
/// cache or whose cache entry is stale.  Best-effort: failures are silently
/// skipped.  Saves the updated cache to disk once after all items are
/// processed.
const MAX_REQUESTS: usize = 10;

pub fn enrich(
  items: &mut Vec<FeedItem>,
  cache: &mut HashMap<String, EnrichmentEntry>,
  api_key: Option<&str>,
) {
  let client = crate::http::client();
  let mut enriched: usize = 0;
  let mut skipped: usize = 0;
  let mut request_count: usize = 0;

  if api_key.is_some() {
    log::warn!(
      "semantic_scholar: starting enrichment for {} items (API key set — no cap)",
      items.len()
    );
  } else {
    log::warn!(
      "semantic_scholar: starting enrichment for {} items (network cap = {})",
      items.len(),
      MAX_REQUESTS
    );
  }

  for item in items.iter_mut() {
    // Own the id here so we can call `apply_entry(item, ...)` later without
    // a borrow conflict. The hot-path dedup in app.rs uses `&str` directly;
    // this enrichment loop runs once per refresh, so the per-item allocation
    // is negligible.
    let id = match arxiv_id_from_url(&item.url) {
      Some(id) => id.to_string(),
      None => {
        skipped += 1;
        continue;
      }
    };

    log::debug!("semantic_scholar: enriching arXiv:{id}");

    // Use cached entry if still fresh.
    if let Some(entry) = cache.get(&id) {
      if !is_stale(entry, &id) {
        log::debug!(
          "semantic_scholar: cache hit for arXiv:{id} (cached_at={})",
          entry.cached_at
        );
        apply_entry(item, entry);
        enriched += 1;
        continue;
      }
    }

    // Enforce request cap (lifted when API key is present).
    if api_key.is_none() && request_count >= MAX_REQUESTS {
      log::warn!(
        "semantic_scholar: request cap ({MAX_REQUESTS}) reached after \
         {request_count} network requests — stopping"
      );
      break;
    }

    // Fetch from Semantic Scholar — skip silently on any failure.
    let api_url = format!(
      "https://api.semanticscholar.org/graph/v1/paper/arXiv:{}?fields=authors,citationCount,fieldsOfStudy",
      id
    );
    // Pace before each request; no leading delay on the first.
    if request_count > 0 {
      std::thread::sleep(std::time::Duration::from_secs(1));
    }

    log::debug!("semantic_scholar: network request for arXiv:{id}");
    request_count += 1;
    let entry = match fetch_entry(&client, &api_url, &id, api_key) {
      Ok(Some(e)) => e,
      Ok(None) => {
        skipped += 1;
        continue;
      }
      Err(()) => break,
    };

    apply_entry(item, &entry);
    enriched += 1;
    cache.insert(id, entry);
  }

  log::info!("semantic_scholar: enriched {enriched} items, skipped {skipped}");

  save(cache);
}

// ---------------------------------------------------------------------------

fn apply_entry(item: &mut FeedItem, entry: &EnrichmentEntry) {
  if item.authors.is_empty() && !entry.authors.is_empty() {
    item.authors = entry.authors.clone();
  }
  if item.domain_tags == vec!["ml".to_string()]
    && !entry.fields_of_study.is_empty()
  {
    item.domain_tags =
      entry.fields_of_study.iter().map(|s| s.to_lowercase()).collect();
  }
}

fn fetch_entry(
  client: &reqwest::blocking::Client,
  url: &str,
  id: &str,
  api_key: Option<&str>,
) -> Result<Option<EnrichmentEntry>, ()> {
  let mut builder = client.get(url);
  if let Some(key) = api_key {
    builder = builder.header("x-api-key", key);
  }
  let resp = match builder.send() {
    Ok(r) => r,
    Err(e) => {
      log::warn!("semantic_scholar: request failed for arXiv:{id} — {e}");
      return Ok(None);
    }
  };

  if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
    log::warn!(
      "semantic_scholar: rate limited (429) for arXiv:{id} — stopping enrichment run"
    );
    return Err(());
  }

  let body = match crate::http::read_body(resp) {
    Ok(b) => b,
    Err(e) => {
      log::warn!(
        "semantic_scholar: failed to read response body for arXiv:{id} — {e}"
      );
      return Ok(None);
    }
  };

  let json: serde_json::Value = match serde_json::from_str(&body) {
    Ok(v) => v,
    Err(e) => {
      log::warn!("semantic_scholar: JSON parse error for arXiv:{id} — {e}");
      return Ok(None);
    }
  };

  // S2 returns {"error": "..."} or {"message": "..."} on failure.
  if json.get("error").is_some() || json.get("message").is_some() {
    log::warn!(
      "semantic_scholar: API error for arXiv:{id} — {}",
      json
        .get("error")
        .or_else(|| json.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
    );
    return Ok(None);
  }

  let empty_vec = vec![];
  let authors_arr = json["authors"].as_array().unwrap_or(&empty_vec);

  let authors: Vec<String> = authors_arr
    .iter()
    .filter_map(|a| a["name"].as_str().map(|s| s.to_string()))
    .collect();

  if authors.is_empty() {
    log::warn!("semantic_scholar: no authors in response for arXiv:{id}");
  }

  let institution = authors_arr
    .first()
    .and_then(|a| a["affiliations"].as_array())
    .and_then(|aff| aff.first())
    .and_then(|a| a["name"].as_str())
    .unwrap_or("")
    .to_string();

  if institution.is_empty() {
    log::warn!("semantic_scholar: no institution in response for arXiv:{id}");
  }

  let citation_count =
    json["citationCount"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32;

  let fields_of_study: Vec<String> = json["fieldsOfStudy"]
    .as_array()
    .unwrap_or(&empty_vec)
    .iter()
    .filter_map(|f| f["category"].as_str().map(|s| s.to_string()))
    .collect();

  Ok(Some(EnrichmentEntry {
    authors,
    institution,
    citation_count,
    fields_of_study,
    cached_at: today_str(),
  }))
}
