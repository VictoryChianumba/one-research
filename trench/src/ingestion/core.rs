use serde::Deserialize;

use crate::models::{
  ContentType, FeedItem, SignalLevel, SourcePlatform, WorkflowState,
  detect_subtopics,
};

use super::pipeline::{FetchContext, Source};

/// Bulk-refresh source for the CORE API. Requires an API key — the
/// registry skips building this source when `config.core_api_key` is
/// absent (preserving the "skipped — no API key" log line at the
/// orchestrator boundary, ADR-004 §D6).
pub struct CoreSource;

impl Source for CoreSource {
  fn name(&self) -> &str {
    "core"
  }
  fn host_group(&self) -> &str {
    "core"
  }
  fn fetch(&self, ctx: &FetchContext) -> Result<Vec<FeedItem>, String> {
    let key = ctx
      .config
      .core_api_key
      .as_deref()
      .ok_or_else(|| "core: no API key configured".to_string())?;
    fetch(key)
  }
}

pub fn fetch(api_key: &str) -> Result<Vec<FeedItem>, String> {
  // CORE API key moved off the URL query string and into the Authorization
  // header. Previously the key landed in proxy logs, the server access log,
  // and (on error) reqwest's error message which we log at error level —
  // the audit's Sec HIGH #8 finding. CORE accepts both `Bearer` and
  // `X-API-Key` per their docs; Bearer is the standard idiom for this
  // codebase (chat providers + GitHub all use Bearer).
  let url = "https://api.core.ac.uk/v3/search/works\
             ?q=artificial+intelligence+machine+learning\
             &limit=25&sort=recency";
  let resp = crate::http::client()
    .get(url)
    .header("Authorization", format!("Bearer {api_key}"))
    .send()
    .map_err(|e| format!("core: HTTP failed: {e}"))?;
  let body = crate::http::read_body(resp)
    .map_err(|e| format!("core: read failed: {e}"))?;

  let parsed: CoreResponse = serde_json::from_str(&body)
    .map_err(|e| format!("core: JSON parse failed: {e}"))?;

  let mut items = Vec::new();
  for work in parsed.results {
    if let Some(item) = work_to_item(work) {
      items.push(item);
    }
  }
  Ok(items)
}

fn work_to_item(work: CoreWork) -> Option<FeedItem> {
  let title = super::collapse_whitespace(work.title.as_deref()?);
  if title.is_empty() {
    return None;
  }

  let summary =
    super::collapse_whitespace(&work.abstract_text.unwrap_or_default());
  let authors: Vec<String> = work
    .authors
    .into_iter()
    .filter_map(|a| a.name)
    .filter(|n| !n.is_empty())
    .collect();
  let published_at = work
    .published_date
    .as_deref()
    .or(work.year_published.as_deref())
    .unwrap_or("")
    .to_string();

  // Prefer arXiv URL if available for deduplication.
  let arxiv_url = work
    .source_fulltext_urls
    .iter()
    .find(|u| u.contains("arxiv.org/abs/"))
    .cloned();

  let best_url = arxiv_url.clone().or_else(|| {
    work.links.into_iter().find(|l| l.url.starts_with("http")).map(|l| l.url)
  });
  let url = match best_url {
    Some(u) => u,
    None => return None,
  };

  let domain_tags: Vec<String> =
    detect_subtopics(&title, &summary).iter().map(|s| s.to_string()).collect();

  let mut item = FeedItem {
    id: url.clone(),
    title,
    source_platform: SourcePlatform::Core,
    content_type: ContentType::Paper,
    domain_tags,
    signal: SignalLevel::Tertiary,
    published_at,
    authors,
    summary_short: summary,
    workflow_state: WorkflowState::Inbox,
    url,
    upvote_count: 0,
    github_repo: None,
    github_owner: None,
    github_repo_name: None,
    benchmark_results: vec![],
    full_content: None,
    source_name: "core".to_string(),
    title_lower: String::new(),
    authors_lower: Vec::new(),
  };
  item.signal = item.compute_signal();
  item.sanitize_in_place();
  Some(item)
}

#[derive(Deserialize)]
struct CoreResponse {
  #[serde(default)]
  results: Vec<CoreWork>,
}

#[derive(Deserialize)]
struct CoreWork {
  title: Option<String>,
  #[serde(rename = "abstract")]
  abstract_text: Option<String>,
  #[serde(default)]
  authors: Vec<CoreAuthor>,
  #[serde(rename = "publishedDate")]
  published_date: Option<String>,
  #[serde(rename = "yearPublished")]
  year_published: Option<String>,
  #[serde(rename = "sourceFulltextUrls", default)]
  source_fulltext_urls: Vec<String>,
  #[serde(default)]
  links: Vec<CoreLink>,
}

#[derive(Deserialize)]
struct CoreAuthor {
  name: Option<String>,
}

#[derive(Deserialize)]
struct CoreLink {
  url: String,
}
