use crate::models::{
  ContentType, FeedItem, SignalLevel, SourcePlatform, WorkflowState,
  detect_subtopics,
};
use quick_xml::Reader;
use quick_xml::events::Event as XmlEvent;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use trench_http::RetryPolicy;

use super::pipeline::{EnrichmentSource, FetchContext, Source};

/// Post-fetch enrichment that fills `github_repo` on items lacking one,
/// via the HuggingFace papers API + GitHub URL regex fallback. Caches
/// results at `~/.config/trench/hf_repo_cache.json` (7-day TTL) loaded
/// at registry-build time. Same `RefCell` rationale as
/// [`super::semantic_scholar::SemanticScholarEnrichment`] (ADR-004 §D4).
pub struct HuggingFaceRepoEnrichment {
  cache: RefCell<HashMap<String, HfRepoCacheEntry>>,
}

impl HuggingFaceRepoEnrichment {
  pub fn new() -> Self {
    Self { cache: RefCell::new(load_hf_cache()) }
  }
}

impl Default for HuggingFaceRepoEnrichment {
  fn default() -> Self {
    Self::new()
  }
}

impl EnrichmentSource for HuggingFaceRepoEnrichment {
  fn name(&self) -> &str {
    "huggingface_repo"
  }
  fn enrich(&self, items: &mut [FeedItem], _ctx: &FetchContext) {
    enrich_with_repos_using_cache(items, &mut self.cache.borrow_mut());
  }
}

const HF_PAPERS_URL: &str = "https://huggingface.co/papers";

/// Bulk-refresh source for HuggingFace daily papers. Two-pass internally:
/// scrape the daily papers page, then batched arXiv API call to backfill
/// abstracts. The batched call is the one historic 429/503 culprit —
/// `HuggingFaceSource::fetch` is the only `Source` impl in trench that
/// uses [`FetchContext::with_retry`].
pub struct HuggingFaceSource;

impl Source for HuggingFaceSource {
  fn name(&self) -> &str {
    "huggingface"
  }
  fn host_group(&self) -> &str {
    // Same envelope as arxiv — `fetch_abstracts` hits `export.arxiv.org`,
    // which arxiv::fetch also touches. Grouping both as "arxiv" makes the
    // orchestrator schedule them serially on one thread per ADR-004 §D5.
    "arxiv"
  }
  fn fetch(&self, ctx: &FetchContext) -> Result<Vec<FeedItem>, String> {
    fetch(ctx)
  }
}

pub fn fetch(ctx: &FetchContext) -> Result<Vec<FeedItem>, String> {
  let body = get_text(HF_PAPERS_URL)?;
  let today = today_date();
  let mut items = parse_papers(&body, &today);
  fetch_abstracts(&mut items, ctx);
  Ok(items)
}

// ---------------------------------------------------------------------------
// Abstract fetcher
// ---------------------------------------------------------------------------

/// Fetch abstracts from the arXiv API in a single batched request and fill in
/// `summary_short` on each item. Failures are logged and silently skipped.
///
/// The batched arXiv call sits behind [`FetchContext::with_retry`] using
/// [`RetryPolicy::arxiv`] (3s / 6s on 429 / 503) — the only retry-using
/// site in trench. Pre-C10 this lived in a local `fetch_arxiv_with_retry`
/// helper (commit `5491470`); the audit's C13 lifted it into `trench_http`.
fn fetch_abstracts(items: &mut [FeedItem], ctx: &FetchContext) {
  if items.is_empty() {
    return;
  }

  let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
  let id_list = ids.join(",");
  // `max_results` must scale with the id_list length. The previous hardcoded
  // 50 silently truncated whenever HF returned >50 papers (~3 items missing
  // per refresh in the days observed). `.min(100)` matches arxiv::fetch_by_ids
  // for the same reason — arXiv accepts larger but the cap stays sane.
  let max_results = ids.len().min(100);
  let url = format!(
    "https://export.arxiv.org/api/query?id_list={id_list}&max_results={max_results}"
  );

  log::info!(
    "huggingface: fetching abstracts for {} papers from arXiv",
    ids.len()
  );

  let resp = match ctx.with_retry(&RetryPolicy::arxiv(), |c| c.get(&url)) {
    Ok(r) => r,
    Err(e) => {
      log::warn!("huggingface: abstract batch fetch failed — {e}");
      return;
    }
  };
  let body = match trench_http::read_body(resp) {
    Ok(b) => b,
    Err(e) => {
      log::warn!("huggingface: abstract batch read failed — {e}");
      return;
    }
  };

  let abstracts = parse_abstracts(&body);
  log::debug!("huggingface: received {} abstracts from arXiv", abstracts.len());

  for item in items.iter_mut() {
    if let Some(text) = abstracts.get(&item.id) {
      item.summary_short = text.clone();
    } else {
      log::warn!("huggingface: no abstract matched for {}", item.id);
    }
  }
}

/// Parse an arXiv Atom response and return a map of arXiv ID → full abstract.
fn parse_abstracts(xml: &str) -> HashMap<String, String> {
  let mut map = HashMap::new();
  let mut reader = Reader::from_str(xml);
  reader.config_mut().trim_text(true);

  let mut in_entry = false;
  let mut current_tag = String::new();
  let mut entry_id = String::new();
  let mut summary = String::new();

  loop {
    match reader.read_event() {
      Ok(XmlEvent::Start(ref e)) => {
        let tag =
          std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_string();
        if tag == "entry" {
          in_entry = true;
          entry_id.clear();
          summary.clear();
        }
        current_tag = tag;
      }

      Ok(XmlEvent::Text(ref e)) => {
        if !in_entry {
          continue;
        }
        let text = e.unescape().unwrap_or_default().to_string();
        match current_tag.as_str() {
          "id" => entry_id.push_str(&text),
          "summary" => summary.push_str(&text),
          _ => {}
        }
      }

      Ok(XmlEvent::End(ref e)) => {
        let tag =
          std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_string();
        if in_entry && tag == "entry" {
          in_entry = false;
          if let Some(id) = extract_arxiv_id(&entry_id) {
            let clean = collapse_whitespace(summary.trim());
            map.insert(id, clean);
          }
        }
      }

      Ok(XmlEvent::Eof) => break,
      Err(_) => break,
      _ => {}
    }
  }

  map
}

/// Extract a bare arXiv ID from a full URL like `http://arxiv.org/abs/2403.00001v2`.
fn extract_arxiv_id(url: &str) -> Option<String> {
  let last = url.rsplit('/').next()?;
  // Strip version suffix (e.g. "v2").
  let id = match last.rfind('v') {
    Some(pos) if last[pos + 1..].chars().all(|c| c.is_ascii_digit()) => {
      &last[..pos]
    }
    _ => last,
  };
  if id.is_empty() { None } else { Some(id.to_string()) }
}

use super::collapse_whitespace;

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

fn get_text(url: &str) -> Result<String, String> {
  let resp = crate::http::client()
    .get(url)
    .send()
    .map_err(|e| format!("HTTP error: {e}"))?;
  if !resp.status().is_success() {
    return Err(format!("HTTP {}", resp.status()));
  }
  crate::http::read_body(resp)
}

fn today_date() -> String {
  chrono::Utc::now().format("%Y-%m-%d").to_string()
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse the HF daily papers page.
///
/// The page has two sources of data:
/// 1. `<h3><a href="/papers/{id}" ...>TITLE</a></h3>` — one per paper.
/// 2. HTML-entity-encoded JSON blobs in the page body containing:
///    `&quot;id&quot;:&quot;{id}&quot;`
///    `&quot;upvotes&quot;:{n}`
///    `&quot;name&quot;:&quot;AUTHOR&quot;` (within an authors array)
fn parse_papers(html: &str, today: &str) -> Vec<FeedItem> {
  // Pass 1: extract (id, title) from <h3> tags.
  let titles = extract_h3_papers(html);

  // Pass 2: extract upvotes and authors from the encoded JSON blobs.
  let meta = extract_json_meta(html);

  // Merge, preserving the order titles were found.
  let mut seen = std::collections::HashSet::new();
  let mut items = Vec::new();

  for (id, title) in titles {
    if !seen.insert(id.clone()) {
      continue; // deduplicate
    }
    let (upvotes, authors) = meta.get(&id).cloned().unwrap_or((None, vec![]));

    let upvote_count = upvotes.unwrap_or(0);

    // Detect subtopics from title; fall back to generic "ml" tag.
    let subtopics: Vec<String> =
      detect_subtopics(&title, "").into_iter().map(|s| s.to_string()).collect();
    let domain_tags =
      if subtopics.is_empty() { vec!["ml".to_string()] } else { subtopics };

    let mut item = FeedItem {
      id: id.clone(),
      title,
      source_platform: SourcePlatform::HuggingFace,
      content_type: ContentType::Paper,
      domain_tags,
      signal: SignalLevel::Secondary,
      published_at: today.to_string(),
      authors,
      summary_short: String::new(),
      workflow_state: WorkflowState::Inbox,
      url: format!("https://huggingface.co/papers/{id}"),
      upvote_count,
      github_repo: None,
      github_owner: None,
      github_repo_name: None,
      benchmark_results: vec![],
      full_content: None,
      source_name: "huggingface".to_string(),
      title_lower: String::new(),
      authors_lower: Vec::new(),
    };
    item.signal = item.compute_signal();
    item.sanitize_in_place();
    items.push(item);
  }

  items
}

// ---------------------------------------------------------------------------
// Pass 1 — titles from <h3> elements
// ---------------------------------------------------------------------------

/// Returns (arxiv_id, title) pairs from `<h3><a href="/papers/{id}">TITLE</a></h3>`.
fn extract_h3_papers(html: &str) -> Vec<(String, String)> {
  let mut results = Vec::new();
  let mut pos = 0;

  while let Some(h3_start) = html[pos..].find("<h3").map(|i| i + pos) {
    // Find the end of the h3 block
    let Some(h3_end_rel) = html[h3_start..].find("</h3>") else {
      break;
    };
    let h3_end = h3_start + h3_end_rel + 5; // include </h3>
    let block = &html[h3_start..h3_end];

    pos = h3_end;

    // Inside the block, find href="/papers/{id}"
    let needle = "href=\"/papers/";
    let Some(href_pos) = block.find(needle) else {
      continue;
    };
    let id_start = href_pos + needle.len();
    let id_raw = &block[id_start..];
    let id_end = id_raw.find('"').unwrap_or(id_raw.len());
    let paper_id = &id_raw[..id_end];

    // Validate: must look like an arXiv ID (digits, dot, digits, optional v+digits)
    if !crate::models::is_arxiv_id(paper_id) {
      continue;
    }

    // Extract the link text (strip all inner tags)
    let title = strip_tags(block).trim().to_string();
    if title.is_empty() {
      continue;
    }

    results.push((paper_id.to_string(), title));
  }

  results
}

fn strip_tags(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  let mut in_tag = false;
  for c in s.chars() {
    match c {
      '<' => in_tag = true,
      '>' => in_tag = false,
      _ if !in_tag => out.push(c),
      _ => {}
    }
  }
  out
}

// ---------------------------------------------------------------------------
// Pass 2 — upvotes + authors from encoded JSON
// ---------------------------------------------------------------------------

/// Returns HashMap<arxiv_id, (Option<upvotes>, Vec<author_name>)>
fn extract_json_meta(
  html: &str,
) -> HashMap<String, (Option<u32>, Vec<String>)> {
  let mut map: HashMap<String, (Option<u32>, Vec<String>)> = HashMap::new();

  // The page embeds server data as HTML-entity-encoded JSON.
  // Pattern per paper block:
  //   &quot;id&quot;:&quot;{id}&quot; ... &quot;upvotes&quot;:{n}
  // Authors appear as &quot;name&quot;:&quot;{name}&quot; within the same block.

  let id_needle = "&quot;id&quot;:&quot;";
  let mut pos = 0;

  while let Some(rel) = html[pos..].find(id_needle) {
    let id_val_start = pos + rel + id_needle.len();
    let Some(id_val_end_rel) = html[id_val_start..].find("&quot;") else {
      pos = id_val_start;
      continue;
    };
    let paper_id = &html[id_val_start..id_val_start + id_val_end_rel];

    if !crate::models::is_arxiv_id(paper_id) {
      pos = id_val_start + id_val_end_rel;
      continue;
    }
    let paper_id = paper_id.to_string();

    // Take the next ~4 KB as the JSON block for this paper
    let block_end = (id_val_start + 4096).min(html.len());
    let block = &html[id_val_start..block_end];

    let upvotes = extract_upvotes(block);
    let authors = extract_authors(block);

    map.entry(paper_id).or_insert((upvotes, authors));

    pos = id_val_start + id_val_end_rel;
  }

  map
}

fn extract_upvotes(block: &str) -> Option<u32> {
  let needle = "&quot;upvotes&quot;:";
  let start = block.find(needle)? + needle.len();
  let rest = &block[start..];
  let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
  rest[..end].parse().ok()
}

/// Extract author names from `&quot;name&quot;:&quot;{name}&quot;` entries.
/// Stops when it hits a field that indicates we've left the authors array.
fn extract_authors(block: &str) -> Vec<String> {
  let mut authors = Vec::new();
  let needle = "&quot;name&quot;:&quot;";
  let mut pos = 0;

  // Only look within the authors array: stop at the first non-author &quot;name&quot;
  // that follows a field clearly outside author objects (e.g. &quot;title&quot;).
  // Simple heuristic: stop after 20 authors or when we leave the first 2 KB.
  // Use char-aware truncation so a multi-byte char straddling byte 2048
  // doesn't panic the parser thread (audit Sec HIGH #7).
  let block: String = block.chars().take(2048).collect();
  let block = block.as_str();

  while let Some(rel) = block[pos..].find(needle) {
    let val_start = pos + rel + needle.len();
    let Some(val_end_rel) = block[val_start..].find("&quot;") else {
      break;
    };
    let name = &block[val_start..val_start + val_end_rel];
    // Skip internal/system fields that look like object IDs (24-char hex)
    if name.len() < 64
      && !name.is_empty()
      && !name.chars().all(|c| c.is_ascii_hexdigit())
    {
      authors.push(name.to_string());
    }
    pos = val_start + val_end_rel;
    if authors.len() >= 20 {
      break;
    }
  }

  authors
}

// ---------------------------------------------------------------------------
// Repo enrichment — HuggingFace papers API + abstract fallback
// ---------------------------------------------------------------------------

const MAX_ENRICH_REQUESTS: usize = 20;
const CACHE_TTL_DAYS: i32 = 7;

/// Fetch the GitHub repo URL for an arXiv paper from the HuggingFace papers API.
/// Returns `None` on any failure or when the field is absent / empty.
pub fn fetch_paper_repo(arxiv_id: &str) -> Option<String> {
  let url = format!("https://huggingface.co/api/papers/{}", arxiv_id);
  let resp = crate::http::client().get(&url).send().ok()?;
  if !resp.status().is_success() {
    return None;
  }
  let body = crate::http::read_body(resp).ok()?;
  let json: serde_json::Value = serde_json::from_str(&body).ok()?;
  json
    .get("githubRepo")
    .and_then(|v| v.as_str())
    .filter(|s| !s.is_empty() && s.contains("github.com"))
    .map(|s| s.to_string())
}

/// For items that have an arXiv ID and no `github_repo` set, attempt to fill
/// `github_repo`, `github_owner`, and `github_repo_name` via:
///   1. HuggingFace papers API (`/api/papers/{id}`)
///   2. GitHub URL regex over the abstract text (fallback)
///
/// Results are cached at `~/.config/trench/hf_repo_cache.json` with a
/// 7-day TTL so subsequent startups don't re-fetch. Capped at 20 live
/// requests per ingestion cycle; items with the most upvotes are tried first.
/// Inner implementation that takes an external cache — used by the
/// [`HuggingFaceRepoEnrichment`] trait impl so the registry-owned
/// `RefCell` can be borrowed at the call site without re-loading from
/// disk each refresh.
fn enrich_with_repos_using_cache(
  items: &mut [FeedItem],
  cache: &mut HashMap<String, HfRepoCacheEntry>,
) {
  let mut request_count: usize = 0;
  let mut enriched: usize = 0;
  let mut attempted: usize = 0;

  // Build candidate list sorted by upvotes descending.
  let mut candidates: Vec<(usize, String, u32)> = items
    .iter()
    .enumerate()
    .filter(|(_, item)| item.github_repo.is_none())
    .filter_map(|(i, item)| {
      let id = extract_paper_arxiv_id(item)?;
      Some((i, id, item.upvote_count))
    })
    .collect();
  candidates.sort_by(|a, b| b.2.cmp(&a.2));

  for (idx, arxiv_id, _) in candidates {
    // Cache hit — apply without a network call.
    if let Some(entry) = cache.get(&arxiv_id) {
      if !hf_cache_stale(entry) {
        if let Some(ref repo) = entry.github_repo.clone() {
          apply_repo(items, idx, repo);
          enriched += 1;
        }
        continue;
      }
    }

    if request_count >= MAX_ENRICH_REQUESTS {
      break;
    }

    attempted += 1;

    // Primary: HuggingFace API. Fallback: regex over abstract.
    let abstract_text = items[idx].summary_short.clone();
    let repo_url = fetch_paper_repo(&arxiv_id)
      .or_else(|| extract_github_from_text(&abstract_text));

    request_count += 1;

    // Cache the outcome (including None — so we don't retry papers with no repo).
    cache.insert(
      arxiv_id,
      HfRepoCacheEntry {
        github_repo: repo_url.clone(),
        cached_at: today_date(),
      },
    );

    if let Some(ref repo) = repo_url {
      apply_repo(items, idx, repo);
      enriched += 1;
    }
  }

  log::info!(
    "repo enrichment: {enriched}/{attempted} items enriched with github_repo"
  );
  save_hf_cache(cache);
}

fn apply_repo(items: &mut [FeedItem], idx: usize, repo: &str) {
  items[idx].github_repo = Some(repo.to_string());
  let (owner, name) = parse_github_owner_repo(repo);
  items[idx].github_owner = owner;
  items[idx].github_repo_name = name;
}

/// Return the arXiv ID for any item whose `id` or `url` references one.
/// Delegates to `arxiv::normalize_arxiv_id` so that bare IDs (HF), full Atom
/// URLs (arXiv), and arxiv-flavoured OpenReview URLs all resolve to the same
/// canonical form.
fn extract_paper_arxiv_id(item: &FeedItem) -> Option<String> {
  super::arxiv::normalize_arxiv_id(&item.id)
    .or_else(|| super::arxiv::normalize_arxiv_id(&item.url))
}

fn github_url_re() -> &'static regex::Regex {
  use std::sync::OnceLock;
  static RE: OnceLock<regex::Regex> = OnceLock::new();
  RE.get_or_init(|| {
    regex::Regex::new(
      r"(?:https?://)?(?:www\.)?github\.com/([a-zA-Z0-9_.-]+)/([a-zA-Z0-9_.-]+)",
    )
    .expect("valid regex")
  })
}

fn canonicalise_match(caps: &regex::Captures<'_>) -> Option<String> {
  let owner = caps.get(1)?.as_str();
  let repo =
    caps.get(2)?.as_str().trim_end_matches(['.', ',', ';', ':', ')', ']', '}']);
  let repo = repo.strip_suffix(".git").unwrap_or(repo);
  if owner.is_empty() || repo.is_empty() {
    return None;
  }
  Some(format!("https://github.com/{owner}/{repo}"))
}

/// Return the first plausible GitHub repo URL in `text`, normalised to
/// `https://github.com/owner/repo` form. Accepts protocol-less and `www.`
/// variants; trims trailing punctuation and balanced wrappers (`)`, `]`, `}`).
pub(crate) fn extract_github_from_text(text: &str) -> Option<String> {
  let caps = github_url_re().captures(text)?;
  canonicalise_match(&caps)
}

/// Return a GitHub repo URL only if the text mentions exactly one distinct
/// canonical repo. Used by the RSS body scan, where multiple unrelated
/// `github.com/...` mentions in a long blog post are likely asides rather than
/// the post's own release.
pub(crate) fn extract_unique_github_from_text(text: &str) -> Option<String> {
  let mut found: Option<String> = None;
  for caps in github_url_re().captures_iter(text) {
    if let Some(canonical) = canonicalise_match(&caps) {
      match &found {
        None => found = Some(canonical),
        Some(existing) if existing == &canonical => {} // dup of same repo
        Some(_) => return None,                        // distinct second hit
      }
    }
  }
  found
}

pub(crate) fn parse_github_owner_repo(
  url: &str,
) -> (Option<String>, Option<String>) {
  let path = url
    .trim_end_matches('/')
    .strip_prefix("https://github.com/")
    .or_else(|| url.strip_prefix("http://github.com/"))
    .unwrap_or("");
  let mut parts = path.splitn(2, '/');
  let owner = parts.next().filter(|s| !s.is_empty()).map(|s| s.to_string());
  let name = parts
    .next()
    .filter(|s| !s.is_empty())
    .map(|s| s.trim_end_matches('/'))
    .map(|s| s.strip_suffix(".git").unwrap_or(s).to_string())
    .filter(|s| !s.is_empty());
  (owner, name)
}

/// True for github URLs whose owner starts with "anonymous" — these are
/// double-blind-review placeholders (anonymousforneurips, anonymous-iclr, etc.)
/// that resolve to dead or empty repos and should not be auto-linked.
pub(crate) fn is_anonymous_review_url(url: &str) -> bool {
  let path = url
    .strip_prefix("https://github.com/")
    .or_else(|| url.strip_prefix("http://github.com/"))
    .unwrap_or(url);
  let owner = path.split('/').next().unwrap_or("");
  owner.to_ascii_lowercase().starts_with("anonymous")
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize)]
struct HfRepoCacheEntry {
  github_repo: Option<String>,
  cached_at: String,
}

fn hf_cache_path() -> Option<PathBuf> {
  let mut p = std::env::var_os("HOME").map(PathBuf::from)?;
  p.push(".config");
  p.push("trench");
  p.push("hf_repo_cache.json");
  Some(p)
}

fn load_hf_cache() -> HashMap<String, HfRepoCacheEntry> {
  let path = match hf_cache_path() {
    Some(p) => p,
    None => return HashMap::new(),
  };
  match fs::read(&path) {
    Ok(b) => serde_json::from_slice(&b).unwrap_or_default(),
    Err(_) => HashMap::new(),
  }
}

fn save_hf_cache(cache: &HashMap<String, HfRepoCacheEntry>) {
  let path = match hf_cache_path() {
    Some(p) => p,
    None => return,
  };
  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }
  if let Ok(json) = serde_json::to_vec(cache) {
    if let Err(e) = fs::write(&path, &json) {
      log::warn!("hf repo cache: save failed — {e}");
    } else {
      crate::store::set_private(&path);
    }
  }
}

fn hf_cache_stale(entry: &HfRepoCacheEntry) -> bool {
  use chrono::NaiveDate;
  let cached = match NaiveDate::parse_from_str(&entry.cached_at, "%Y-%m-%d") {
    Ok(d) => d,
    Err(_) => return true,
  };
  let today = chrono::Utc::now().date_naive();
  (today - cached).num_days() > CACHE_TTL_DAYS as i64
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::{
    ContentType, FeedItem, SignalLevel, SourcePlatform, WorkflowState,
  };

  fn item(id: &str, url: &str) -> FeedItem {
    FeedItem {
      id: id.to_string(),
      title: String::new(),
      source_platform: SourcePlatform::ArXiv,
      content_type: ContentType::Paper,
      domain_tags: vec![],
      signal: SignalLevel::Tertiary,
      published_at: String::new(),
      authors: vec![],
      summary_short: String::new(),
      workflow_state: WorkflowState::Inbox,
      url: url.to_string(),
      upvote_count: 0,
      github_repo: None,
      github_owner: None,
      github_repo_name: None,
      benchmark_results: vec![],
      full_content: None,
      source_name: String::new(),
      title_lower: String::new(),
      authors_lower: Vec::new(),
    }
  }

  #[test]
  fn extract_github_from_text_handles_common_styles() {
    let cases = [
      (
        "Code at https://github.com/foo/bar",
        Some("https://github.com/foo/bar"),
      ),
      ("Code at github.com/foo/bar", Some("https://github.com/foo/bar")),
      ("see www.github.com/foo/bar.", Some("https://github.com/foo/bar")),
      ("(https://github.com/foo/bar).", Some("https://github.com/foo/bar")),
      ("\\url{https://github.com/foo/bar}", Some("https://github.com/foo/bar")),
      (
        "Trailing comma github.com/foo/bar,",
        Some("https://github.com/foo/bar"),
      ),
      // .git clone-URL suffix must be stripped from the canonical form.
      (
        "git clone https://github.com/foo/bar.git",
        Some("https://github.com/foo/bar"),
      ),
      (
        "see https://github.com/foo/bar.git/",
        Some("https://github.com/foo/bar"),
      ),
      ("(https://github.com/foo/bar.git)", Some("https://github.com/foo/bar")),
      // .gitignore is NOT a .git suffix — must remain intact.
      (
        "https://github.com/foo/bar.gitignore",
        Some("https://github.com/foo/bar.gitignore"),
      ),
      ("No code link in this abstract.", None),
    ];
    for (input, expected) in cases {
      assert_eq!(
        extract_github_from_text(input).as_deref(),
        expected,
        "input: {input}",
      );
    }
  }

  #[test]
  fn parse_github_owner_repo_round_trips() {
    let (o, n) = parse_github_owner_repo("https://github.com/foo/bar");
    assert_eq!(o.as_deref(), Some("foo"));
    assert_eq!(n.as_deref(), Some("bar"));

    // .git suffix on a raw URL should be stripped from the structured name.
    let (o, n) = parse_github_owner_repo("https://github.com/foo/bar.git");
    assert_eq!(o.as_deref(), Some("foo"));
    assert_eq!(n.as_deref(), Some("bar"));

    // Trailing slash after .git also handled.
    let (o, n) = parse_github_owner_repo("https://github.com/foo/bar.git/");
    assert_eq!(o.as_deref(), Some("foo"));
    assert_eq!(n.as_deref(), Some("bar"));
  }

  #[test]
  fn is_anonymous_review_url_battery() {
    // True cases: owner starts with "anonymous" (case-insensitive).
    assert!(is_anonymous_review_url(
      "https://github.com/anonymousforneurips64/repo"
    ));
    assert!(is_anonymous_review_url("https://github.com/anonymous-iclr2024/x"));
    assert!(is_anonymous_review_url("https://github.com/Anonymous/x"));
    // Even without the protocol prefix the owner check still works.
    assert!(is_anonymous_review_url("anonymousfoo/x"));

    // False: only the OWNER segment is checked, not the repo name.
    assert!(!is_anonymous_review_url(
      "https://github.com/normalowner/anonymous-repo"
    ));
    assert!(!is_anonymous_review_url("https://github.com/foo/bar"));
    // Embedded "anonymous" in the middle of an owner shouldn't match.
    assert!(!is_anonymous_review_url("https://github.com/notanonymousowner/x"));
  }

  #[test]
  fn extract_unique_github_returns_some_only_for_single_distinct() {
    // Single mention.
    assert_eq!(
      extract_unique_github_from_text("Release at github.com/foo/bar.")
        .as_deref(),
      Some("https://github.com/foo/bar"),
    );
    // Same repo mentioned twice — still unique.
    assert_eq!(
      extract_unique_github_from_text(
        "github.com/foo/bar — see https://github.com/foo/bar"
      )
      .as_deref(),
      Some("https://github.com/foo/bar"),
    );
    // Two distinct repos — abstain.
    assert_eq!(
      extract_unique_github_from_text(
        "Compares github.com/foo/bar against github.com/baz/qux"
      ),
      None,
    );
    // None at all.
    assert_eq!(extract_unique_github_from_text("plain prose"), None);
  }

  #[test]
  fn extract_paper_arxiv_id_accepts_id_or_url() {
    // Bare ID via id (HF case).
    assert_eq!(
      extract_paper_arxiv_id(&item(
        "2312.12345",
        "https://huggingface.co/papers/2312.12345"
      ))
      .as_deref(),
      Some("2312.12345"),
    );
    // Full Atom URL via id (arXiv case).
    assert_eq!(
      extract_paper_arxiv_id(&item(
        "http://arxiv.org/abs/2312.12345v1",
        "http://arxiv.org/abs/2312.12345v1"
      ))
      .as_deref(),
      Some("2312.12345"),
    );
    // OpenReview-shaped id but arxiv URL.
    assert_eq!(
      extract_paper_arxiv_id(&item(
        "https://openreview.net/forum?id=abcdef",
        "https://arxiv.org/abs/2401.99999"
      ))
      .as_deref(),
      Some("2401.99999"),
    );
    // Garbage in both fields.
    assert_eq!(extract_paper_arxiv_id(&item("nope", "also nope")), None);
  }
}
