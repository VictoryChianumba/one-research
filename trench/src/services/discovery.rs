//! URL discovery + AI-driven discovery query pipeline.
//!
//! Two service flavors live here:
//!
//! 1. **URL discovery** (`spawn_discovery`): given an arbitrary URL,
//!    detect whether it's an arXiv category, HuggingFace daily papers,
//!    a Substack feed, or a generic RSS source. Used by the sources
//!    popup's "Add source" input.
//! 2. **AI discovery** (`spawn_ai_discovery`): given a free-text topic,
//!    spawn a multi-turn agent session via the discovery pipeline.
//!    Used by the Discoveries tab.

use std::sync::mpsc;

use crate::app::{App, DiscoverResult};
use crate::config;
use crate::discovery;
use crate::is_safe_url_scheme;
use crate::panic_msg;

pub(crate) fn spawn_discovery(url: String, tx: mpsc::Sender<DiscoverResult>) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let _ = tx.send(discover_feed(&url));
    }));
    if let Err(payload) = result {
      let msg = panic_msg(payload);
      log::error!("spawn_discovery: thread panicked — {msg}");
      let _ = tx_panic.send(DiscoverResult::Failed(format!(
        "discovery thread panicked: {msg}"
      )));
    }
  });
}

fn discover_feed(url: &str) -> DiscoverResult {
  // Step 1: arXiv category patterns.
  for prefix in &[
    "arxiv.org/list/",
    "arxiv.org/abs/",
    "arxiv.org/rss/",
    "export.arxiv.org/rss/",
  ] {
    if let Some(pos) = url.find(prefix) {
      let rest = &url[pos + prefix.len()..];
      let code: String = rest
        .chars()
        .take_while(|&c| c != '/' && c != '?' && c != '#' && c != ' ')
        .collect();
      if !code.is_empty() && (code.contains('.') || code.len() <= 8) {
        return DiscoverResult::ArxivCategory(code);
      }
    }
  }

  // Step 2: HuggingFace.
  if url.contains("huggingface.co/papers")
    || url.contains("huggingface.co/daily-papers")
  {
    return DiscoverResult::HuggingFaceAlreadyEnabled;
  }

  // Step 3: Substack — derive RSS URL from subdomain.
  if url.contains(".substack.com") {
    let stripped =
      url.trim_start_matches("https://").trim_start_matches("http://");
    let subdomain = stripped.split('.').next().unwrap_or("feed");
    let feed_url = format!("https://{subdomain}.substack.com/feed");
    return DiscoverResult::RssFeed {
      url: feed_url,
      name: subdomain.to_string(),
    };
  }

  let client = crate::http::client();
  let base_url = url.trim_end_matches('/').to_string();

  // Step 4: Fetch page and scan <head> for RSS link element.
  if let Ok(resp) = client.get(url).send() {
    if resp.status().is_success() {
      if let Ok(body) = resp.text() {
        if let Some(feed_url) = extract_rss_link(&body, &base_url) {
          let name = domain_name(url);
          return DiscoverResult::RssFeed { url: feed_url, name };
        }
      }
    }
  }

  // Step 5: Try common feed paths.
  let suffixes = ["/feed", "/rss", "/atom.xml", "/feed.xml", "/rss.xml"];
  for suffix in suffixes {
    let candidate = format!("{base_url}{suffix}");
    if let Ok(resp) = client.head(&candidate).send() {
      if resp.status().is_success() {
        let name = domain_name(&candidate);
        return DiscoverResult::RssFeed { url: candidate, name };
      }
    }
  }

  let tried = suffixes
    .iter()
    .map(|s| format!("{base_url}{s}"))
    .collect::<Vec<_>>()
    .join(", ");
  DiscoverResult::Failed(format!("Could not find a feed. Tried: {tried}"))
}

/// Scan HTML for `<link rel="alternate" type="application/rss+xml" href="...">`.
fn extract_rss_link(html: &str, base_url: &str) -> Option<String> {
  let needle = "application/rss+xml";
  let mut search = html;
  while let Some(pos) = search.find(needle) {
    let tag_start = search[..pos].rfind('<').unwrap_or(0);
    let tag_end =
      search[pos..].find('>').map(|p| pos + p + 1).unwrap_or(search.len());
    let tag = &search[tag_start..tag_end];
    if let Some(href) = attr_value(tag, "href") {
      // Reject hrefs that look like a non-http(s) scheme attempt before
      // any joining with base_url. Anything with a `:` before the first
      // path/query/fragment delimiter is a scheme: javascript:, file:,
      // data:, mailto:, etc.
      let scheme_end = href.find(|c: char| c == '/' || c == '?' || c == '#');
      let pre = scheme_end.map(|i| &href[..i]).unwrap_or(&href[..]);
      if pre.contains(':') && !is_safe_url_scheme(&href) {
        search = &search[pos + needle.len()..];
        continue;
      }
      let url = if is_safe_url_scheme(&href) {
        href
      } else if href.starts_with('/') {
        let origin = url_origin(base_url);
        format!("{origin}{href}")
      } else {
        format!("{base_url}/{href}")
      };
      if is_safe_url_scheme(&url) {
        return Some(url);
      }
    }
    search = &search[pos + needle.len()..];
  }
  None
}

/// Extract the value of a named attribute from a tag string.
fn attr_value(tag: &str, attr: &str) -> Option<String> {
  let needle = format!("{attr}=");
  let pos = tag.find(&needle)?;
  let rest = &tag[pos + needle.len()..];
  if rest.starts_with('"') {
    let end = rest[1..].find('"')?;
    Some(rest[1..end + 1].to_string())
  } else if rest.starts_with('\'') {
    let end = rest[1..].find('\'')?;
    Some(rest[1..end + 1].to_string())
  } else {
    let end =
      rest.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(rest.len());
    Some(rest[..end].to_string())
  }
}

/// Extract `https://host` from a URL.
fn url_origin(url: &str) -> String {
  let stripped =
    url.trim_start_matches("https://").trim_start_matches("http://");
  let host = stripped.split('/').next().unwrap_or("");
  if url.starts_with("https://") {
    format!("https://{host}")
  } else {
    format!("http://{host}")
  }
}

/// Derive a short source name from a URL (e.g. `"openai"` from `openai.com/…`).
fn domain_name(url: &str) -> String {
  let stripped =
    url.trim_start_matches("https://").trim_start_matches("http://");
  let host = stripped.split('/').next().unwrap_or("");
  let host = host.strip_prefix("www.").unwrap_or(host);
  host.split('.').next().unwrap_or(host).to_string()
}

/// Spawn an AI discovery query thread using the pipeline and attach the
/// receiver to `app`. Mutates app.discovery state extensively to set up
/// the session before kicking off the worker.
pub(crate) fn spawn_ai_discovery(
  topic: String,
  config: config::Config,
  app: &mut App,
) {
  let has_claude = config
    .claude_api_key
    .as_deref()
    .map(|k| !k.trim().is_empty())
    .unwrap_or(false);

  let is_refinement =
    !app.discovery.session.is_empty() && !app.discovery.force_new && has_claude;

  let prior_history = if is_refinement {
    Some(app.discovery.session.messages.clone())
  } else {
    None
  };

  if !is_refinement {
    app.reset_discovery_items();
  }

  app.discovery.force_new = false;

  let intent = if let Some(forced) = app.discovery.forced_intent.take() {
    forced
  } else if is_refinement {
    app.discovery.session.query_intent
  } else {
    discovery::intent::classify(&topic)
  };
  app.discovery.intent = intent;

  app.record_discovery_query(&topic, intent);

  let (tx, rx) = mpsc::channel::<discovery::DiscoveryMessage>();
  app.discovery.rx = Some(rx);
  app.discovery.loading = true;
  app.discovery.status = if is_refinement {
    format!("Refining [{}]: '{topic}'…", intent.label())
  } else {
    format!("Searching [{}]…", intent.label())
  };

  discovery::pipeline::spawn_discovery(
    topic,
    config,
    tx,
    prior_history,
    intent,
  );
}

#[cfg(test)]
mod extract_rss_link_tests {
  use super::extract_rss_link;

  #[test]
  fn rejects_javascript_href() {
    let html = r#"<link rel="alternate" type="application/rss+xml" href="javascript:alert(1)">"#;
    assert_eq!(extract_rss_link(html, "https://example.com"), None);
  }

  #[test]
  fn rejects_file_href() {
    let html = r#"<link rel="alternate" type="application/rss+xml" href="file:///etc/passwd">"#;
    assert_eq!(extract_rss_link(html, "https://example.com"), None);
  }

  #[test]
  fn accepts_https_href() {
    let html = r#"<link rel="alternate" type="application/rss+xml" href="https://example.com/feed.xml">"#;
    assert_eq!(
      extract_rss_link(html, "https://example.com"),
      Some("https://example.com/feed.xml".to_string())
    );
  }

  #[test]
  fn accepts_relative_href_with_safe_base() {
    let html =
      r#"<link rel="alternate" type="application/rss+xml" href="/feed.xml">"#;
    assert_eq!(
      extract_rss_link(html, "https://example.com"),
      Some("https://example.com/feed.xml".to_string())
    );
  }
}
