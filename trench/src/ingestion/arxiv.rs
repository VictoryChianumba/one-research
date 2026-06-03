use std::sync::Mutex;
use std::time::{Duration, Instant};

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::models::{
  ContentType, FeedItem, SignalLevel, SourcePlatform, WorkflowState,
  detect_subtopics, map_arxiv_category,
};

use super::pipeline::{FetchContext, Source};

/// arXiv's terms of use ask API clients to make no more than one request
/// every three seconds (https://info.arxiv.org/help/api/tou.html). Every
/// *live* request to `export.arxiv.org` in this app funnels through
/// [`throttle`] first — bulk refresh, Browse paging + arrival auto-fill,
/// the search bar, and HuggingFace abstract enrichment (which also hits
/// the arXiv API). Host-group serialization (ADR-004) only spaces
/// requests *within* one bulk-refresh thread; this gate spaces them
/// across the independent worker threads too, which is where bursts
/// (auto-fill firing while the bulk refresh runs) used to trip the
/// "Rate exceeded." throttle.
const ARXIV_MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(3000);

/// Wall-clock of the last arXiv request start, shared across every worker
/// thread. `None` until the first request.
static ARXIV_LAST_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);

/// How long a caller must wait given the time elapsed since the previous
/// arXiv request (`None` = no prior request). Pure so the spacing policy
/// is unit-testable without sleeping.
fn arxiv_wait_for(elapsed_since_last: Option<Duration>) -> Duration {
  match elapsed_since_last {
    Some(elapsed) => {
      ARXIV_MIN_REQUEST_INTERVAL.checked_sub(elapsed).unwrap_or(Duration::ZERO)
    }
    None => Duration::ZERO,
  }
}

/// Block the calling thread until at least [`ARXIV_MIN_REQUEST_INTERVAL`]
/// has elapsed since the previous arXiv request, then record this one.
///
/// Holding the lock across the sleep is deliberate: it serialises every
/// caller into a single 3s-spaced queue (a leaky bucket of one). Call
/// only from worker threads — never the UI/render thread — so the
/// queueing provides backpressure without stalling the event loop.
pub(crate) fn throttle() {
  let mut last =
    ARXIV_LAST_REQUEST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
  let wait = arxiv_wait_for(last.map(|prev| prev.elapsed()));
  if !wait.is_zero() {
    std::thread::sleep(wait);
  }
  *last = Some(Instant::now());
}

/// Bulk-refresh source for the arXiv export Atom API. Delegates to the
/// pre-C10 [`fetch`] free function so the discovery agent (which still
/// calls `arxiv::fetch` outside any `FetchContext`) doesn't fork.
pub struct ArxivSource {
  pub categories: Vec<String>,
}

impl Source for ArxivSource {
  fn name(&self) -> &str {
    "arxiv"
  }
  fn host_group(&self) -> &str {
    "arxiv"
  }
  fn fetch(&self, _ctx: &FetchContext) -> Result<Vec<FeedItem>, String> {
    fetch(&self.categories)
  }
}

/// Bulk / first-page fetch: the 50 most-recent papers in `categories`.
/// Delegates to [`fetch_page`] at `start = 0` so the bulk-refresh path
/// (and the discovery agent) keep their exact pre-ADR-015 behaviour —
/// the `(0, 50)` literal here is the bulk contract guarded by tripwire R1.
pub fn fetch(categories: &[String]) -> Result<Vec<FeedItem>, String> {
  fetch_page(categories, 0, 50)
}

/// Paginating fetch (ADR-015 §F2). Pulls `max_results` papers starting at
/// arXiv offset `start`, most-recent-first. The Browse pager walks a
/// category deeper by advancing `start`; `fetch` is the `start = 0`
/// special case. Single URL builder so the bulk and paged queries can
/// never drift (tripwire R2).
pub fn fetch_page(
  categories: &[String],
  start: usize,
  max_results: usize,
) -> Result<Vec<FeedItem>, String> {
  let query = if categories.is_empty() {
    "cat:cs.LG+OR+cat:cs.AI+OR+cat:stat.ML".to_string()
  } else {
    categories
      .iter()
      .map(|c| format!("cat:{}", c))
      .collect::<Vec<_>>()
      .join("+OR+")
  };
  let url = format!(
    "https://export.arxiv.org/api/query\
     ?search_query={query}\
     &sortBy=submittedDate&sortOrder=descending\
     &start={start}&max_results={max_results}"
  );
  throttle();
  let resp = crate::http::client()
    .get(&url)
    .send()
    .map_err(|e| format!("HTTP request failed: {e}"))?;
  let body = crate::http::read_body(resp)
    .map_err(|e| format!("Failed to read response body: {e}"))?;

  parse_atom(&body)
}

/// Free-text search across arXiv (title / abstract / authors), ranked by
/// relevance. Unlike [`fetch`], which pulls the *most recent* papers in a
/// set of categories, this spans all of arXiv's history — so a known
/// title resolves even when it isn't in the local cache or among recent
/// submissions. Reuses [`parse_atom`], so results are fully-formed
/// `FeedItem`s (categories, signal, subtopics) identical to ingested
/// papers; they merge into `items_store` and surface like any other item.
///
/// `query` is the user's raw text. reqwest URL-encodes the `search_query`
/// value, so spaces and punctuation in the title are handled without a
/// manual encoder. The arXiv field syntax `all:` is decoded server-side.
///
/// The query is wrapped in quotes so arXiv treats it as a **phrase**.
/// Unquoted multi-word text is parsed as `all:w1 OR all:w2 OR …`, which
/// for a title like "Attention Is All You Need" ranks dozens of papers
/// sharing those common words above the one you meant. The quoted phrase
/// requires the words to appear contiguously, so the target paper lands
/// on the first page of relevance-sorted results.
pub fn fetch_search(query: &str) -> Result<Vec<FeedItem>, String> {
  let query = query.trim();
  if query.is_empty() {
    return Ok(Vec::new());
  }
  // Strip any quotes the user typed so we control the phrase wrapping.
  let cleaned = query.replace('"', " ");
  let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
  let search_query = format!("all:\"{cleaned}\"");
  throttle();
  let resp = crate::http::client()
    .get("https://export.arxiv.org/api/query")
    .query(&[
      ("search_query", search_query.as_str()),
      ("sortBy", "relevance"),
      ("max_results", "50"),
    ])
    .send()
    .map_err(|e| format!("HTTP request failed: {e}"))?;
  let body = crate::http::read_body(resp)
    .map_err(|e| format!("Failed to read response body: {e}"))?;

  parse_atom(&body)
}

/// Free-text arXiv search using `search_query=all:{query}`.
///
/// Sorted by `relevance` (not submission date): this backs the search
/// bar's online lookup, where the user wants the *best* match for a title
/// or topic — including older canonical papers — not just the 100 newest
/// papers that mention the terms.
pub fn search_query(
  query: &str,
  max_results: usize,
) -> Result<Vec<FeedItem>, String> {
  let encoded = encode_arxiv_query(query);
  let url = format!(
    "https://export.arxiv.org/api/query\
     ?search_query=all:{encoded}\
     &sortBy=relevance&sortOrder=descending&max_results={}",
    max_results.min(100)
  );
  throttle();
  let resp = crate::http::client()
    .get(&url)
    .send()
    .map_err(|e| format!("HTTP request failed: {e}"))?;
  let body = crate::http::read_body(resp)
    .map_err(|e| format!("Failed to read response body: {e}"))?;

  parse_atom(&body)
}

/// Fetch specific papers by arXiv ID list.
pub fn fetch_by_ids(ids: &[String]) -> Result<Vec<FeedItem>, String> {
  let normalized: Vec<String> =
    ids.iter().filter_map(|id| normalize_arxiv_id(id)).collect();
  if normalized.is_empty() {
    return Ok(Vec::new());
  }

  let id_list = normalized.join(",");
  let url = format!(
    "https://export.arxiv.org/api/query?id_list={id_list}&max_results={}",
    normalized.len().min(100)
  );
  throttle();
  let resp = crate::http::client()
    .get(&url)
    .send()
    .map_err(|e| format!("HTTP request failed: {e}"))?;
  let body = crate::http::read_body(resp)
    .map_err(|e| format!("Failed to read response body: {e}"))?;

  parse_atom(&body)
}

// ---------------------------------------------------------------------------
// Atom XML parser
// ---------------------------------------------------------------------------

fn parse_atom(xml: &str) -> Result<Vec<FeedItem>, String> {
  // arXiv signals throttling with an HTTP **200** plain-text body
  // ("Rate exceeded.") — not a status code — and serves HTML pages for
  // 5xx. Either way the body is not an Atom feed. Without this guard the
  // parser finds zero `<entry>` elements and returns `Ok(vec![])`, which
  // the Browse pipeline then caches as a genuinely-empty, *exhausted*
  // category ("0 papers loaded", stuck with no retry). Fail loud instead
  // so the fetch routes to `BrowseMessage::Error` and stays retryable.
  // A genuinely empty category still arrives as a valid `<feed>` with
  // `totalResults = 0`, so this only rejects non-feed responses.
  if !xml.contains("<feed") {
    let reason = if xml.contains("Rate exceeded") {
      "arXiv rate limit exceeded — try again in a few seconds"
    } else {
      "unexpected non-Atom response from arXiv"
    };
    return Err(reason.to_string());
  }

  let mut reader = Reader::from_str(xml);
  reader.config_mut().trim_text(true);

  let mut items: Vec<FeedItem> = Vec::new();

  // Per-entry accumulator
  let mut in_entry = false;
  let mut current_tag = String::new();
  let mut in_author = false;

  let mut title = String::new();
  let mut url = String::new();
  let mut published_at = String::new();
  let mut summary = String::new();
  let mut arxiv_comment = String::new();
  let mut authors: Vec<String> = Vec::new();
  let mut current_author_name = String::new();
  let mut domain_tags: Vec<String> = Vec::new();

  loop {
    match reader.read_event() {
      Ok(Event::Start(ref e)) => {
        let tag =
          std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_string();

        match tag.as_str() {
          "entry" => {
            in_entry = true;
            // Reset accumulators
            title.clear();
            url.clear();
            published_at.clear();
            summary.clear();
            arxiv_comment.clear();
            authors.clear();
            domain_tags.clear();
          }
          "author" if in_entry => {
            in_author = true;
            current_author_name.clear();
          }
          _ => {}
        }
        current_tag = tag;
      }

      Ok(Event::Empty(ref e)) => {
        let name = e.name();
        let tag = std::str::from_utf8(name.as_ref()).unwrap_or("");
        if in_entry && tag == "category" {
          // <category term="cs.LG" scheme="..."/>
          for attr in e.attributes().flatten() {
            if attr.key.as_ref() == b"term" {
              if let Ok(val) = std::str::from_utf8(&attr.value) {
                domain_tags.push(val.to_string());
              }
            }
          }
        }
      }

      Ok(Event::Text(ref e)) => {
        let text = e.unescape().unwrap_or_default().to_string();
        if !in_entry {
          continue;
        }
        if in_author && current_tag == "name" {
          current_author_name.push_str(&text);
        } else {
          match current_tag.as_str() {
            "title" => title.push_str(&text),
            "id" => url.push_str(&text),
            "published" => published_at.push_str(&text),
            "summary" => summary.push_str(&text),
            "arxiv:comment" => arxiv_comment.push_str(&text),
            _ => {}
          }
        }
      }

      Ok(Event::End(ref e)) => {
        let tag =
          std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_string();

        if in_entry && in_author && tag == "name" {
          // name closed inside author — nothing extra needed
        }

        if in_entry && tag == "author" {
          in_author = false;
          let name = current_author_name.trim().to_string();
          if !name.is_empty() {
            authors.push(name);
          }
          current_author_name.clear();
        }

        if in_entry && tag == "entry" {
          in_entry = false;
          current_tag.clear();

          let clean_title = collapse_whitespace(&title);
          let mut summary_short = collapse_whitespace(&summary);
          let comment = collapse_whitespace(&arxiv_comment);
          if !comment.is_empty() {
            summary_short = format!("{summary_short} [{comment}]");
          }
          // arXiv published_at looks like "2026-03-15T00:00:00Z" — keep date only
          let date =
            published_at.split('T').next().unwrap_or(&published_at).to_string();

          // Preserve raw category codes alongside human-readable
          // labels and detected subtopics. ADR-011 §E5 (Subject
          // column) + §E4 (subject_follow predicate) both consume the
          // canonical code form (e.g. `cs.LG`), so it must stay in
          // domain_tags — the prior label-only mapping silently broke
          // the subject_follow predicate for the post-ADR-010 broader
          // category set.
          let mut mapped: Vec<String> = Vec::new();
          for code in &domain_tags {
            if !mapped.contains(code) {
              mapped.push(code.clone());
            }
          }
          for code in &domain_tags {
            if let Some(label) = map_arxiv_category(code.as_str()) {
              let l = label.to_string();
              if !mapped.contains(&l) {
                mapped.push(l);
              }
            }
          }
          for label in detect_subtopics(&clean_title, &summary_short) {
            let s = label.to_string();
            if !mapped.contains(&s) {
              mapped.push(s);
            }
          }

          let mut item = FeedItem {
            id: url.clone(),
            title: clean_title,
            source_platform: SourcePlatform::ArXiv,
            content_type: ContentType::Paper,
            domain_tags: mapped,
            signal: SignalLevel::Primary,
            published_at: date,
            authors,
            summary_short,
            workflow_state: WorkflowState::Inbox,
            url,
            upvote_count: 0,
            github_repo: None,
            github_owner: None,
            github_repo_name: None,
            benchmark_results: vec![],
            full_content: None,
            source_name: "arxiv".to_string(),
            title_lower: String::new(),
            authors_lower: Vec::new(),
          };
          item.signal = item.compute_signal();
          item.sanitize_in_place();
          items.push(item);

          // Reset accumulators for next entry
          title = String::new();
          url = String::new();
          published_at = String::new();
          summary = String::new();
          arxiv_comment = String::new();
          authors = Vec::new();
          domain_tags = Vec::new();
        }
      }

      Ok(Event::Eof) => break,
      Err(e) => return Err(format!("XML parse error: {e}")),
      _ => {}
    }
  }

  Ok(items)
}

use super::collapse_whitespace;

fn encode_arxiv_query(query: &str) -> String {
  query
    .split_whitespace()
    .map(percent_encode_query_part)
    .collect::<Vec<_>>()
    .join("+AND+")
}

fn percent_encode_query_part(part: &str) -> String {
  let mut out = String::new();
  for b in part.bytes() {
    let c = b as char;
    if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
      out.push(c);
    } else {
      out.push_str(&format!("%{b:02X}"));
    }
  }
  out
}

pub(crate) fn normalize_arxiv_id(value: &str) -> Option<String> {
  let mut id = value.trim();
  for prefix in
    &["https://arxiv.org/abs/", "http://arxiv.org/abs/", "arxiv.org/abs/"]
  {
    if let Some(rest) = id.strip_prefix(prefix) {
      id = rest;
      break;
    }
  }
  id = id.split(['?', '#']).next().unwrap_or(id);
  if let Some(v_pos) = id.rfind('v') {
    let version = &id[v_pos + 1..];
    if !version.is_empty() && version.chars().all(|c| c.is_ascii_digit()) {
      id = &id[..v_pos];
    }
  }
  let valid = id.contains('.')
    && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
  if valid { Some(id.to_string()) } else { None }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn normalize_arxiv_id_handles_common_shapes() {
    assert_eq!(normalize_arxiv_id("2312.12345").as_deref(), Some("2312.12345"));
    assert_eq!(
      normalize_arxiv_id("https://arxiv.org/abs/2312.12345v3").as_deref(),
      Some("2312.12345"),
    );
    assert_eq!(
      normalize_arxiv_id("http://arxiv.org/abs/2312.12345v1").as_deref(),
      Some("2312.12345"),
    );
    assert_eq!(
      normalize_arxiv_id("arxiv.org/abs/2312.12345?context=cs.LG").as_deref(),
      Some("2312.12345"),
    );
    assert_eq!(normalize_arxiv_id(""), None);
    assert_eq!(normalize_arxiv_id("not an id"), None);
    assert_eq!(normalize_arxiv_id("https://openreview.net/forum?id=abc"), None);
  }

  #[test]
  fn parse_atom_folds_arxiv_comment_into_summary() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:arxiv="http://arxiv.org/schemas/atom">
  <entry>
    <id>http://arxiv.org/abs/2401.00001v1</id>
    <title>Sample Paper</title>
    <summary>This is the abstract.</summary>
    <published>2026-04-01T00:00:00Z</published>
    <arxiv:comment>16 pages, 4 figures. Code at github.com/foo/bar</arxiv:comment>
  </entry>
</feed>"#;
    let items = parse_atom(xml).expect("parse ok");
    assert_eq!(items.len(), 1);
    assert!(
      items[0].summary_short.contains("github.com/foo/bar"),
      "expected comment text in summary_short, got: {}",
      items[0].summary_short
    );
    assert!(items[0].summary_short.contains("This is the abstract."));
  }

  // arXiv throttles with an HTTP-200 plain-text "Rate exceeded." body.
  // It must NOT parse as an empty feed — otherwise the Browse pipeline
  // caches the category as a 0-paper exhausted buffer ("0 papers loaded"
  // with no retry). WHY this matters: the throttle is transient; treating
  // it as data poisons the session. Fail loud → routes to Error → retry.
  #[test]
  fn parse_atom_rejects_rate_limit_body_as_error() {
    let err = parse_atom("Rate exceeded.").expect_err("throttle must error");
    assert!(
      err.contains("rate limit"),
      "rate-limit body should yield a rate-limit error, got: {err}"
    );
  }

  // Any non-Atom body (HTML 5xx page, proxy error) is an error too — the
  // parser must never silently report zero papers for a failed fetch.
  #[test]
  fn parse_atom_rejects_non_feed_body_as_error() {
    assert!(parse_atom("<html><body>502 Bad Gateway</body></html>").is_err());
  }

  // The app-wide arXiv gate's spacing policy (the sleep itself is not
  // exercised — only the decision of how long to wait). WHY it matters:
  // arXiv throttles at ~1 req/3s, so the first request must be immediate,
  // a request inside the window waits the remainder, and one past it does
  // not wait. A regression here re-opens the burst that trips "Rate
  // exceeded.".
  #[test]
  fn arxiv_wait_for_enforces_three_second_spacing() {
    // No prior request → go immediately.
    assert_eq!(arxiv_wait_for(None), Duration::ZERO);
    // Back-to-back → wait the full interval.
    assert_eq!(
      arxiv_wait_for(Some(Duration::ZERO)),
      ARXIV_MIN_REQUEST_INTERVAL
    );
    // Part-way through the window → wait only the remainder.
    assert_eq!(
      arxiv_wait_for(Some(Duration::from_millis(1000))),
      ARXIV_MIN_REQUEST_INTERVAL - Duration::from_millis(1000)
    );
    // At/past the interval → no wait (saturating, never underflows).
    assert_eq!(
      arxiv_wait_for(Some(ARXIV_MIN_REQUEST_INTERVAL)),
      Duration::ZERO
    );
    assert_eq!(arxiv_wait_for(Some(Duration::from_secs(10))), Duration::ZERO);
  }

  // A *genuinely* empty category still returns a valid <feed> with zero
  // entries — that is legitimately `Ok(empty)`, distinct from a throttle.
  // This is the line the guard must not cross.
  #[test]
  fn parse_atom_accepts_empty_feed_as_zero_items() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/">
  <opensearch:totalResults>0</opensearch:totalResults>
</feed>"#;
    let items = parse_atom(xml).expect("an empty feed is a valid response");
    assert!(items.is_empty());
  }
}
