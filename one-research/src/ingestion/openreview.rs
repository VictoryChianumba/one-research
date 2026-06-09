use serde::Deserialize;
use serde_json::Value;

use crate::models::{
  ContentType, FeedItem, SignalLevel, SourcePlatform, WorkflowState,
  detect_subtopics,
};

use super::pipeline::{FetchContext, Source};

/// Default venue invitation strings — the major ML conferences for the
/// current and prior cycle. Lifted from the hardcoded `const VENUES` in
/// the pre-C10 implementation so [`OpenReviewSource::default`] reproduces
/// it byte-for-byte.
pub const DEFAULT_VENUES: &[&str] = &[
  "ICLR.cc/2025/Conference/-/Submission",
  "NeurIPS.cc/2024/Conference/-/Submission",
  "ICML.cc/2024/Conference/-/Submission",
];

/// Bulk-refresh source for the OpenReview v2 notes API. Lifts the
/// previously-hardcoded `VENUES` constant to construction-time `venues:
/// Vec<String>` per ADR-004 §D5 — the orchestrator (via the registry)
/// builds one source seeded with [`DEFAULT_VENUES`]; future per-user
/// venue overrides become a config-level change, not a code change.
pub struct OpenReviewSource {
  pub venues: Vec<String>,
}

impl Default for OpenReviewSource {
  fn default() -> Self {
    Self { venues: DEFAULT_VENUES.iter().map(|s| (*s).to_string()).collect() }
  }
}

impl Source for OpenReviewSource {
  fn name(&self) -> &str {
    "openreview"
  }
  fn host_group(&self) -> &str {
    "openreview"
  }
  fn fetch(&self, _ctx: &FetchContext) -> Result<Vec<FeedItem>, String> {
    fetch_venues(&self.venues)
  }
}

fn fetch_venues(venues: &[String]) -> Result<Vec<FeedItem>, String> {
  // Parallelize venue fetches. Each fetch_venue hits api2.openreview.net —
  // same host — but the upstream has tolerated 3 concurrent requests in
  // testing without 429. If that changes, add per-venue backoff or revert
  // to the sequential loop. Wall-clock is bounded by the slowest single
  // venue (~700ms) instead of the sum of all three (~2s).
  let items: std::sync::Mutex<Vec<FeedItem>> =
    std::sync::Mutex::new(Vec::new());
  std::thread::scope(|scope| {
    for venue in venues {
      let items_ref = &items;
      let venue = venue.as_str();
      scope.spawn(move || match fetch_venue(venue) {
        Ok(mut venue_items) => {
          items_ref
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append(&mut venue_items);
        }
        Err(e) => log::warn!("openreview: venue {venue} failed — {e}"),
      });
    }
  });
  Ok(items.into_inner().unwrap_or_else(|e| e.into_inner()))
}

fn fetch_venue(invitation: &str) -> Result<Vec<FeedItem>, String> {
  let encoded = invitation
    .chars()
    .map(|c| if c == '/' { "%2F".to_string() } else { c.to_string() })
    .collect::<String>();
  let url = format!(
    "https://api2.openreview.net/notes\
     ?invitation={encoded}&limit=50&offset=0"
  );
  let resp = crate::http::client()
    .get(&url)
    .header("User-Agent", "one-research/1.0")
    .send()
    .map_err(|e| format!("HTTP failed: {e}"))?;
  let body =
    crate::http::read_body(resp).map_err(|e| format!("read failed: {e}"))?;

  let parsed: OrResponse = serde_json::from_str(&body)
    .map_err(|e| format!("JSON parse failed: {e}"))?;

  let mut items = Vec::new();
  for note in parsed.notes {
    if let Some(item) = note_to_item(note) {
      items.push(item);
    }
  }
  Ok(items)
}

fn note_to_item(note: OrNote) -> Option<FeedItem> {
  let content = note.content?;

  let title = extract_str(&content, "title")?;
  let title = super::collapse_whitespace(&title);
  if title.is_empty() {
    return None;
  }

  let summary = extract_str(&content, "abstract").unwrap_or_default();
  let summary = super::collapse_whitespace(&summary);

  let authors: Vec<String> = content
    .get("authors")
    .and_then(|v| {
      // authors can be {"value": [...]} or directly [...]
      let arr = v.get("value").unwrap_or(v);
      arr.as_array().map(|a| {
        a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()
      })
    })
    .unwrap_or_default();

  // cdate is milliseconds since epoch
  let published_at = note
    .cdate
    .map(|ms| {
      let secs = ms / 1000;
      let dt = chrono::DateTime::from_timestamp(secs, 0)
        .unwrap_or_default()
        .format("%Y-%m-%d")
        .to_string();
      dt
    })
    .unwrap_or_default();

  // Prefer arXiv URL for deduplication with the arXiv ingestion source.
  let arxiv_id = extract_str(&content, "arxiv");
  let (id, url) = if let Some(ref aid) = arxiv_id {
    let u = format!("https://arxiv.org/abs/{aid}");
    (u.clone(), u)
  } else {
    let u = format!("https://openreview.net/forum?id={}", note.id);
    (u.clone(), u)
  };

  let github_repo = extract_str(&content, "code")
    .or_else(|| super::huggingface::extract_unique_github_from_text(&summary))
    .filter(|r| !super::huggingface::is_anonymous_review_url(r));
  let (github_owner, github_repo_name) = match github_repo.as_deref() {
    Some(repo) => super::huggingface::parse_github_owner_repo(repo),
    None => (None, None),
  };

  let domain_tags: Vec<String> =
    detect_subtopics(&title, &summary).iter().map(|s| s.to_string()).collect();

  let mut item = FeedItem {
    id,
    title,
    source_platform: SourcePlatform::OpenReview,
    content_type: ContentType::Paper,
    domain_tags,
    signal: SignalLevel::Secondary,
    published_at,
    authors,
    summary_short: summary,
    workflow_state: WorkflowState::Inbox,
    url,
    upvote_count: 0,
    github_repo,
    github_owner,
    github_repo_name,
    benchmark_results: vec![],
    full_content: None,
    source_name: "openreview".to_string(),
    title_lower: String::new(),
    authors_lower: Vec::new(),
  };
  item.signal = item.compute_signal();
  item.sanitize_in_place();
  Some(item)
}

// Extract a string from a content object where the value may be stored as
// `{"value": "..."}` (v2 API) or as a plain string.
fn extract_str(content: &Value, key: &str) -> Option<String> {
  let v = content.get(key)?;
  if let Some(s) = v.as_str() {
    return Some(s.to_string());
  }
  v.get("value")?.as_str().map(|s| s.to_string())
}

#[derive(Deserialize)]
struct OrResponse {
  #[serde(default)]
  notes: Vec<OrNote>,
}

#[derive(Deserialize)]
struct OrNote {
  id: String,
  #[serde(default)]
  cdate: Option<i64>,
  content: Option<Value>,
}
