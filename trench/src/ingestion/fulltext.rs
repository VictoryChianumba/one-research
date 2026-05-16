use crate::models::FeedItem;

/// Fetch a feed item and return a `tread::PaperData` ready for the
/// reader to display.  Replaces the old "Vec<String> of pre-wrapped
/// lines" return type so that HTML structure (headers / lists / code
/// blocks / links) survives all the way to render — tread's
/// `from_html` walks the cleaned DOM rather than receiving flattened
/// text.
///
/// Fallback chain:
///   1. Cached `full_content` from RSS `<content:encoded>` (HTML).
///   2. arXiv HTML render — only for arXiv / HuggingFace paper URLs.
///   3. Readability extraction from the item URL (HTML).
///   4. `summary_short` as last resort (plain text).
///
/// arXiv content is also routed through tread's full LaTeX pipeline
/// at the call site in main.rs (via `tread::fetch_paper`); this
/// fetcher's arXiv-HTML stage is the fallback when LaTeX fetch fails
/// or hasn't been wired (e.g. HuggingFace papers that point at arXiv
/// abstracts but go through this pipeline first).
pub fn fetch(item: &FeedItem) -> Result<tread::PaperData, String> {
  // Step 1: cached full_content from RSS.  content:encoded usually
  // ships HTML — keep the structure rather than stripping it.
  if let Some(ref content) = item.full_content {
    if !content.is_empty() {
      log::debug!(
        "fulltext: step=cached_full_content ({} chars)",
        content.len()
      );
      return Ok(tread::PaperData::from_html(content));
    }
  }

  // Step 2: arXiv HTML.  arxiv.org/html/<id> is the rendered version
  // of the LaTeX source — html5ever handles it cleanly.
  if let Some(id) = extract_arxiv_id(&item.url) {
    let html_url = format!("https://arxiv.org/html/{id}");
    log::debug!("fulltext: step=arxiv_html url={html_url}");
    match get_text(&html_url) {
      Ok(html) => {
        log::debug!("fulltext: arxiv_html response {} bytes", html.len());
        return Ok(tread::PaperData::from_html(&html));
      }
      Err(e) => {
        log::warn!(
          "fulltext: arxiv_html failed for {id} — {e}, trying readability"
        );
      }
    }
  }

  // Step 3: readability extraction.  Returns cleaned HTML; tread
  // walks it the same way it walks any other HTML.
  match fetch_with_readability(&item.url) {
    Ok(html) => {
      log::debug!("fulltext: step=readability ({} chars)", html.len());
      return Ok(tread::PaperData::from_html(&html));
    }
    Err(e) => {
      log::warn!(
        "fulltext: readability failed for {} — {e}, falling back to summary",
        item.url
      );
    }
  }

  // Step 4: summary fallback.  Plain text — wrap to lines.
  log::debug!("fulltext: step=summary_fallback");
  let lines: Vec<String> =
    item.summary_short.lines().map(str::to_string).collect();
  Ok(tread::PaperData::from_plain_lines(lines))
}

// ---------------------------------------------------------------------------
// Readability extraction — returns cleaned HTML (not plain text).
// ---------------------------------------------------------------------------

fn fetch_with_readability(url: &str) -> Result<String, String> {
  // Step-3 fallback consumes `item.url` directly, so it's the only
  // branch in `fetch()` that can pivot on a hostile feed URL. Steps 1
  // (cached full_content) and 2 (arxiv synthetic URL) don't.
  if !crate::is_safe_url_scheme(url) {
    return Err(format!("unsafe scheme: {url}"));
  }
  let html = get_text(url)?;
  log::debug!("fulltext: readability url={url} raw_html={} bytes", html.len());
  apply_readability(&html, url)
}

fn apply_readability(html: &str, url: &str) -> Result<String, String> {
  let parsed_url =
    url::Url::parse(url).map_err(|e| format!("URL parse error: {e}"))?;
  let product =
    readability::extractor::extract(&mut html.as_bytes(), &parsed_url)
      .map_err(|e| format!("readability error: {e}"))?;
  log::debug!(
    "fulltext: readability extracted {} bytes of article content",
    product.content.len()
  );
  if product.content.trim().is_empty() {
    return Err("readability returned empty content".to_string());
  }
  Ok(product.content)
}

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
  crate::http::read_body(resp).map_err(|e| format!("Body read error: {e}"))
}

// ---------------------------------------------------------------------------
// arXiv ID extraction — used to pick step 2 over step 3.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::fetch_with_readability;

  #[test]
  fn rejects_file_scheme() {
    let err = fetch_with_readability("file:///etc/passwd").unwrap_err();
    assert!(
      err.starts_with("unsafe scheme"),
      "expected scheme error, got {err}"
    );
  }

  #[test]
  fn rejects_javascript_scheme() {
    let err = fetch_with_readability("javascript:alert(1)").unwrap_err();
    assert!(
      err.starts_with("unsafe scheme"),
      "expected scheme error, got {err}"
    );
  }
}

fn extract_arxiv_id(url: &str) -> Option<&str> {
  // Validate against the shared arxiv-id pattern before returning. Without
  // this, a hostile feed URL like `https://attacker/abs/../../admin` would
  // produce `id = "../../admin"` and pivot the subsequent
  // `format!("https://arxiv.org/html/{id}")` fetch into a path-traversal
  // request (audit Sec HIGH #6).
  let candidate = if let Some(pos) = url.find("/papers/") {
    let id = &url[pos + "/papers/".len()..];
    let id = id.split('?').next().unwrap_or(id);
    let id = id.split('#').next().unwrap_or(id);
    let id = id.split('/').next().unwrap_or(id);
    Some(id)
  } else {
    let mut found = None;
    for prefix in ["/abs/", "/html/", "/pdf/"] {
      if let Some(pos) = url.find(prefix) {
        let id = &url[pos + prefix.len()..];
        let id = id.split('?').next().unwrap_or(id);
        let id = id.split('#').next().unwrap_or(id);
        let id = id.split('/').next().unwrap_or(id);
        found = Some(id);
        break;
      }
    }
    found
  };
  candidate.filter(|id| !id.is_empty() && crate::models::is_arxiv_id(id))
}
