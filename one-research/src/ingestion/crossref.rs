#![allow(dead_code)] // called by discovery agent (Phase 2)

use serde::Deserialize;

use crate::models::{
  ContentType, FeedItem, SignalLevel, SourcePlatform, WorkflowState,
  detect_subtopics,
};

/// Look up a paper by title fragment, DOI, or arXiv ID.
/// Returns the best match or None if nothing found.
pub fn lookup(query: &str) -> Option<FeedItem> {
  // Strip leading https://arxiv.org/abs/ so bare IDs work too.
  let q = query
    .trim()
    .trim_start_matches("https://arxiv.org/abs/")
    .trim_start_matches("http://arxiv.org/abs/");

  let encoded: String = q
    .chars()
    .map(|c| match c {
      ' ' => '+',
      '/' | ':' | '?' | '#' | '&' | '=' => '_',
      _ => c,
    })
    .collect();

  let url = format!(
    "https://api.crossref.org/works?query={encoded}&rows=3&select=DOI,title,abstract,author,published,URL,type"
  );

  let resp = crate::http::client()
    .get(&url)
    .header(
      "User-Agent",
      "one-research/1.0 (mailto:research@example.com; https://github.com/one-research)",
    )
    .send()
    .ok()?;
  let body = crate::http::read_body(resp).ok()?;

  let parsed: CrResponse = serde_json::from_str(&body).ok()?;
  parsed.message?.items.into_iter().find_map(item_to_feed)
}

fn item_to_feed(work: CrWork) -> Option<FeedItem> {
  let title = work.title.into_iter().next()?;
  let title = super::collapse_whitespace(&title);
  if title.is_empty() {
    return None;
  }

  let summary = super::collapse_whitespace(
    &work
      .r#abstract
      .unwrap_or_default()
      .replace("<jats:p>", "")
      .replace("</jats:p>", " "),
  );
  let authors: Vec<String> = work
    .author
    .into_iter()
    .map(|a| match (a.given, a.family) {
      (Some(g), Some(f)) => format!("{g} {f}"),
      (None, Some(f)) => f,
      (Some(g), None) => g,
      _ => String::new(),
    })
    .filter(|s| !s.is_empty())
    .collect();

  let published_at = work
    .published
    .and_then(|p| p.date_parts.into_iter().next())
    .map(|parts| match parts.as_slice() {
      [y, m, d] => format!("{y:04}-{m:02}-{d:02}"),
      [y, m] => format!("{y:04}-{m:02}-01"),
      [y] => format!("{y:04}-01-01"),
      _ => String::new(),
    })
    .unwrap_or_default();

  let url = work.url.unwrap_or_else(|| {
    format!("https://doi.org/{}", work.doi.as_deref().unwrap_or(""))
  });
  if url.is_empty() {
    return None;
  }

  let domain_tags: Vec<String> =
    detect_subtopics(&title, &summary).iter().map(|s| s.to_string()).collect();

  let mut item = FeedItem {
    id: url.clone(),
    title,
    source_platform: SourcePlatform::Blog, // Crossref covers all publisher types
    content_type: ContentType::Paper,
    domain_tags,
    signal: SignalLevel::Secondary,
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
    source_name: "crossref".to_string(),
    title_lower: String::new(),
    authors_lower: Vec::new(),
  };
  item.signal = item.compute_signal();
  item.sanitize_in_place();
  Some(item)
}

#[derive(Deserialize)]
struct CrResponse {
  message: Option<CrMessage>,
}

#[derive(Deserialize)]
struct CrMessage {
  items: Vec<CrWork>,
}

#[derive(Deserialize)]
struct CrWork {
  #[serde(rename = "DOI")]
  doi: Option<String>,
  #[serde(rename = "URL")]
  url: Option<String>,
  #[serde(default)]
  title: Vec<String>,
  #[serde(rename = "abstract")]
  r#abstract: Option<String>,
  #[serde(default)]
  author: Vec<CrAuthor>,
  published: Option<CrDate>,
}

#[derive(Deserialize)]
struct CrAuthor {
  given: Option<String>,
  family: Option<String>,
}

#[derive(Deserialize)]
struct CrDate {
  #[serde(rename = "date-parts", default)]
  date_parts: Vec<Vec<i32>>,
}
