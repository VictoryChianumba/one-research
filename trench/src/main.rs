mod app;
mod commands;
mod config;
mod discovery;
mod export;
mod github;
mod history;
// Re-export the workspace http crate so every existing `crate::http::client()`
// call site continues to compile unchanged. The real impl lives at
// `crates/http/`; chat providers + discovery + main feed-discovery now
// share the same hardened client (timeout, redirect cap, UA).
use trench_http as http;
mod ingestion;
mod keys;
mod library;
mod models;
mod sanitize;
mod store;
mod syntax;
mod tags;
pub mod theme;
mod ui;
mod workflows;

use app::{App, DiscoverResult, FocusedReader, PaneId, RepoFetchResult};
use crossterm::{
  event::{
    self, DisableFocusChange, DisableMouseCapture, EnableFocusChange,
    EnableMouseCapture, Event, KeyEventKind, KeyboardEnhancementFlags,
    MouseButton, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
  },
  execute,
  terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode,
  },
};
use ingestion::message::FetchMessage;
use models::{ContentType, SourcePlatform};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::sync::mpsc;

/// Allowlist for URL schemes handed to the OS opener. macOS `open` and
/// Linux `xdg-open` will dispatch any registered scheme — including local
/// handlers like `vscode://`, `slack://`, `mailto:`, or browser-side
/// `javascript:` — so an attacker-controlled `item.url` could pivot into
/// whichever application the user has installed. Restrict to plain web
/// schemes; non-https paper hosts (older proceedings, university servers)
/// are still legitimate, so http is included alongside https.
fn is_safe_url_scheme(url: &str) -> bool {
  url.starts_with("https://") || url.starts_with("http://")
}

pub(crate) fn open_url(url: &str) {
  if !is_safe_url_scheme(url) {
    log::warn!("open_url: rejecting non-http(s) scheme: {url}");
    return;
  }
  #[cfg(target_os = "macos")]
  let _ = std::process::Command::new("open").arg(url).spawn();
  #[cfg(not(target_os = "macos"))]
  let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

/// Extract a human-readable message from a panic payload returned by
/// `std::panic::catch_unwind`. Used by every `spawn_*` helper below so that
/// thread panics surface to the UI as a routed error rather than a silent
/// thread death + forever-spinner. Idiomatic for both `panic!("...")` (which
/// boxes a `&'static str`) and `panic!("{}", x)` (which boxes a `String`).
pub(crate) fn panic_msg(payload: Box<dyn std::any::Any + Send>) -> String {
  if let Some(s) = payload.downcast_ref::<&'static str>() {
    (*s).to_string()
  } else if let Some(s) = payload.downcast_ref::<String>() {
    s.clone()
  } else {
    "thread panicked (non-string payload)".to_string()
  }
}

#[cfg(test)]
mod panic_msg_tests {
  use super::panic_msg;

  #[test]
  fn extracts_static_str_payload() {
    let result = std::panic::catch_unwind(|| panic!("static literal"));
    assert_eq!(panic_msg(result.unwrap_err()), "static literal");
  }

  #[test]
  fn extracts_owned_string_payload() {
    let result = std::panic::catch_unwind(|| {
      let n: i32 = 42;
      panic!("formatted message: {n}")
    });
    assert_eq!(panic_msg(result.unwrap_err()), "formatted message: 42");
  }

  #[test]
  fn falls_back_for_non_string_payload() {
    let result = std::panic::catch_unwind(|| {
      // Payload is a non-string type (a struct).
      std::panic::panic_any(42i32)
    });
    assert_eq!(
      panic_msg(result.unwrap_err()),
      "thread panicked (non-string payload)"
    );
  }

  #[test]
  fn channel_routing_via_catch_unwind_smoke() {
    // Smoke test for the spawn_*-pattern: a thread that panics must surface
    // via the cloned sender rather than dying silently.
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel::<Result<i32, String>>();

    std::thread::spawn(move || {
      let tx_panic = tx.clone();
      let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
          panic!("simulated worker failure");
        }));
      // The closure body is unconditional `panic!`, so the closure's return
      // type is `!` and `outcome` is provably `Err(_)` — let-else makes that
      // explicit and avoids an irrefutable-pattern lint.
      let Err(payload) = outcome else {
        unreachable!("catch_unwind of a panicking closure cannot be Ok");
      };
      let msg = panic_msg(payload);
      let _ = tx_panic.send(Err(format!("panicked: {msg}")));
    })
    .join()
    .ok();

    let received = rx
      .recv_timeout(std::time::Duration::from_secs(1))
      .expect("receiver should get the panic-routed Err");
    match received {
      Err(s) => assert!(s.contains("simulated worker failure"), "got: {s}"),
      Ok(n) => panic!("expected Err, got Ok({n})"),
    }
  }
}

#[cfg(test)]
mod url_scheme_tests {
  use super::is_safe_url_scheme;

  #[test]
  fn accepts_https_and_http() {
    assert!(is_safe_url_scheme("https://arxiv.org/abs/2603.00001"));
    assert!(is_safe_url_scheme("https://github.com/owner/repo"));
    assert!(is_safe_url_scheme("http://legacy.example.edu/proceedings"));
  }

  #[test]
  fn rejects_local_handler_schemes() {
    // These would dispatch to whatever handler the user has registered.
    assert!(!is_safe_url_scheme("javascript:alert(1)"));
    assert!(!is_safe_url_scheme("file:///etc/passwd"));
    assert!(!is_safe_url_scheme("vscode://settings"));
    assert!(!is_safe_url_scheme("slack://channel?id=abc"));
    assert!(!is_safe_url_scheme("mailto:user@example.com"));
  }

  #[test]
  fn rejects_empty_and_scheme_only() {
    assert!(!is_safe_url_scheme(""));
    assert!(!is_safe_url_scheme("https"));
    assert!(!is_safe_url_scheme("http"));
  }

  #[test]
  fn case_sensitive_match_keeps_things_simple() {
    // `Https://` is technically valid per RFC 3986 §3.1 but we don't go out
    // of our way to accept odd casing — the URLs we open come from feed
    // ingestion, which normalizes to lowercase scheme.
    assert!(!is_safe_url_scheme("HTTPS://arxiv.org/abs/2603.00001"));
  }
}

pub(crate) fn truncate_for_notif(s: &str, max: usize) -> String {
  let mut chars = s.chars();
  let mut out = String::new();
  let mut n = 0;
  for c in &mut chars {
    if n >= max {
      if chars.next().is_some() {
        out.push('…');
      }
      break;
    }
    out.push(c);
    n += 1;
  }
  out
}

/// Uniform per-source runner. Logs the start, dispatches `fetch_fn`, then
/// either extends the shared accumulator and emits Items + SourceComplete,
/// or routes a SourceError. Mutex-poison recovery follows the W3 voice
/// pattern — a poisoned lock on `all_items` is recovered rather than
/// crashing the refresh.
fn run_source<F>(
  name: &str,
  tx: &mpsc::Sender<FetchMessage>,
  all_items: &std::sync::Mutex<Vec<models::FeedItem>>,
  fetch_fn: F,
) where
  F: FnOnce() -> Result<Vec<models::FeedItem>, String>,
{
  log::info!("source {name}: starting fetch");
  match fetch_fn() {
    Ok(items) => {
      log::info!("source {name}: completed, {} items", items.len());
      all_items.lock().unwrap_or_else(|e| e.into_inner()).extend(items.clone());
      let _ = tx.send(FetchMessage::Items(items));
      let _ = tx.send(FetchMessage::SourceComplete(name.to_string()));
    }
    Err(e) => {
      log::error!("source {name}: failed — {e}");
      let _ = tx.send(FetchMessage::SourceError(name.to_string(), e));
    }
  }
}

/// Wrap a per-source thread body in `catch_unwind` so a panic does not
/// kill its siblings. Routes the panic to a SourceError + SourceComplete
/// pair so the loading-spinner clears and the UI surfaces the error.
/// Reuses the W1 `panic_msg` helper.
fn run_source_protected<F>(name: &str, tx: &mpsc::Sender<FetchMessage>, body: F)
where
  F: FnOnce(),
{
  let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
  if let Err(payload) = outcome {
    let msg = panic_msg(payload);
    log::error!("source {name}: thread panicked — {msg}");
    let _ = tx.send(FetchMessage::SourceError(
      name.to_string(),
      format!("source thread panicked: {msg}"),
    ));
    let _ = tx.send(FetchMessage::SourceComplete(name.to_string()));
  }
}

fn spawn_fetch(tx: mpsc::Sender<FetchMessage>, config: config::Config) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let enabled = |name: &str| -> bool {
        config.sources.enabled_sources.get(name).copied().unwrap_or(true)
      };

      // Shared accumulator for enrichment. Each scope thread locks briefly
      // to extend; critical section is one Vec::extend.
      let all_items: std::sync::Mutex<Vec<models::FeedItem>> =
        std::sync::Mutex::new(Vec::new());

      log::warn!(
        "source anthropic: no RSS feed available; skipping \
         (https://www.anthropic.com/news has no feed link)"
      );

      let rss_feeds: &[(&str, &str, SourcePlatform, ContentType)] = &[
        (
          "openai",
          "https://openai.com/blog/rss.xml",
          SourcePlatform::Blog,
          ContentType::Article,
        ),
        (
          "deepmind",
          "https://deepmind.google/blog/rss.xml",
          SourcePlatform::Blog,
          ContentType::Article,
        ),
        (
          "import_ai",
          "https://importai.substack.com/feed",
          SourcePlatform::Newsletter,
          ContentType::Digest,
        ),
        (
          "bair",
          "https://bair.berkeley.edu/blog/feed.xml",
          SourcePlatform::Blog,
          ContentType::Article,
        ),
        (
          "mit_news_ai",
          "https://news.mit.edu/rss/topic/machine-learning",
          SourcePlatform::Blog,
          ContentType::Article,
        ),
      ];

      // References-of-references so each `move ||` closure below can
      // Copy these into its environment (references are Copy; the
      // underlying Mutex / closure / Vec / Option are not).
      let all_items_ref = &all_items;
      let enabled_ref = &enabled;
      let arxiv_categories: &[String] = &config.sources.arxiv_categories;
      let core_key: Option<&str> = config.core_api_key.as_deref();
      let custom_feeds: &[config::CustomFeed] = &config.sources.custom_feeds;

      // Concurrency groups:
      //   A — arxiv-family (sequential within: arxiv → huggingface),
      //       both touch export.arxiv.org so we keep them serial
      //       within the group to stay under the 3s/req rate envelope.
      //   B — openreview (alone, api.openreview.net).
      //   C — core (alone, api.core.ac.uk; needs API key).
      //   D — RSS feeds + custom feeds, one thread per feed; each is
      //       on a distinct host.
      // thread::scope auto-joins all spawns at scope exit, so enrichment
      // (which needs the full all_items) naturally runs after fetches.
      std::thread::scope(|scope| {
        // Group A
        let tx_a = tx.clone();
        scope.spawn(move || {
          run_source_protected("arxiv-family", &tx_a, || {
            run_source("arxiv", &tx_a, all_items_ref, || {
              ingestion::arxiv::fetch(arxiv_categories)
            });
            if enabled_ref("huggingface") {
              run_source(
                "huggingface",
                &tx_a,
                all_items_ref,
                ingestion::huggingface::fetch,
              );
            } else {
              log::info!("source huggingface: disabled — skipping");
              let _ = tx_a
                .send(FetchMessage::SourceComplete("huggingface".to_string()));
            }
          });
        });

        // Group B
        let tx_b = tx.clone();
        scope.spawn(move || {
          run_source_protected("openreview", &tx_b, || {
            if enabled_ref("openreview") {
              run_source(
                "openreview",
                &tx_b,
                all_items_ref,
                ingestion::openreview::fetch,
              );
            } else {
              log::info!("source openreview: disabled — skipping");
              let _ = tx_b
                .send(FetchMessage::SourceComplete("openreview".to_string()));
            }
          });
        });

        // Group C
        let tx_c = tx.clone();
        scope.spawn(move || {
          run_source_protected("core", &tx_c, || {
            if !enabled_ref("core") {
              log::info!("source core: disabled — skipping");
              let _ =
                tx_c.send(FetchMessage::SourceComplete("core".to_string()));
              return;
            }
            let Some(key) = core_key else {
              log::info!("source core: no API key configured — skipping");
              let _ =
                tx_c.send(FetchMessage::SourceComplete("core".to_string()));
              return;
            };
            run_source("core", &tx_c, all_items_ref, || {
              ingestion::core::fetch(key)
            });
          });
        });

        // Group D — built-in RSS feeds
        for (name, url, platform, content_type) in rss_feeds.iter() {
          let tx_d = tx.clone();
          let platform = platform.clone();
          let content_type = content_type.clone();
          scope.spawn(move || {
            run_source_protected(name, &tx_d, || {
              if !enabled_ref(name) {
                log::info!("source {name}: disabled — skipping");
                let _ =
                  tx_d.send(FetchMessage::SourceComplete(name.to_string()));
                return;
              }
              run_source(name, &tx_d, all_items_ref, || {
                ingestion::rss::fetch(name, url, platform, content_type)
              });
            });
          });
        }

        // Group D — user-configured custom feeds
        for feed in custom_feeds.iter() {
          let tx_d = tx.clone();
          scope.spawn(move || {
            run_source_protected(&feed.name, &tx_d, || {
              run_source(&feed.name, &tx_d, all_items_ref, || {
                ingestion::rss::fetch(
                  &feed.name,
                  &feed.url,
                  SourcePlatform::Rss,
                  ContentType::Article,
                )
              });
            });
          });
        }
      });

      // All scope threads have joined; recover ownership from the Mutex.
      let mut all_items =
        all_items.into_inner().unwrap_or_else(|e| e.into_inner());

      log::info!(
        "background: {} total items collected across all sources",
        all_items.len()
      );

      let mut ecache = store::enrichment_cache::load();
      ingestion::semantic_scholar::enrich(
        &mut all_items,
        &mut ecache,
        config.semantic_scholar_key.as_deref(),
      );
      ingestion::huggingface::enrich_with_repos(&mut all_items);
      let with_repo =
        all_items.iter().filter(|i| i.github_repo.is_some()).count();
      log::info!(
        "ingestion complete: {with_repo}/{} items have github_repo set",
        all_items.len()
      );
      let _ = tx.send(FetchMessage::EnrichedItems(all_items));
      let _ = tx.send(FetchMessage::SourceComplete("enriching".to_string()));
      let _ = tx.send(FetchMessage::AllComplete);
    }));
    if let Err(payload) = result {
      let msg = panic_msg(payload);
      log::error!("spawn_fetch: background thread panicked — {msg}");
      let _ = tx_panic.send(FetchMessage::SourceError(
        "background".to_string(),
        format!("background thread panicked: {msg}"),
      ));
      let _ = tx_panic.send(FetchMessage::AllComplete);
    }
  });
}

// ── URL discovery pipeline ────────────────────────────────────────────────

pub(crate) fn spawn_discovery(
  url: String,
  tx: std::sync::mpsc::Sender<DiscoverResult>,
) {
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
      // Looks like a category code if it contains a dot or is short (cs.LG, stat.ML …)
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

  // Step 6: Failure.
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
      let url = if href.starts_with("http") {
        href
      } else if href.starts_with('/') {
        let origin = url_origin(base_url);
        format!("{origin}{href}")
      } else {
        format!("{base_url}/{href}")
      };
      return Some(url);
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

// ── Refresh helper ────────────────────────────────────────────────────────

/// Spawn a fresh fetch cycle and attach the receiver to `app`.
pub(crate) fn do_refresh(app: &mut App) {
  if app.is_loading || app.is_refreshing {
    return;
  }
  let (tx, rx) = mpsc::channel::<FetchMessage>();
  app.fetch_rx = Some(rx);
  let mut sources = vec![
    "arxiv".to_string(),
    "huggingface".to_string(),
    "openreview".to_string(),
    "core".to_string(),
    "openai".to_string(),
    "deepmind".to_string(),
    "import_ai".to_string(),
    "bair".to_string(),
    "mit_news_ai".to_string(),
    "enriching".to_string(),
  ];
  for feed in &app.config.sources.custom_feeds {
    sources.push(feed.name.clone());
  }
  app.loading_sources = sources;
  app.is_loading = true;
  app.is_refreshing = true;
  spawn_fetch(tx, app.config.clone());
}

/// Spawn an AI discovery query thread using the pipeline and attach the receiver.
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
    !app.discovery_session.is_empty() && !app.discovery_force_new && has_claude;

  let prior_history = if is_refinement {
    Some(app.discovery_session.messages.clone())
  } else {
    None
  };

  if !is_refinement {
    app.reset_discovery_items();
  }

  app.discovery_force_new = false;

  let intent = if let Some(forced) = app.discovery_forced_intent.take() {
    forced
  } else if is_refinement {
    app.discovery_session.query_intent
  } else {
    discovery::intent::classify(&topic)
  };
  app.discovery_intent = intent;

  app.record_discovery_query(&topic, intent);

  let (tx, rx) = mpsc::channel::<discovery::DiscoveryMessage>();
  app.discovery_rx = Some(rx);
  app.discovery_loading = true;
  app.discovery_status = if is_refinement {
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

/// Like do_refresh, but always runs — reloads config from disk, abandons any
/// in-flight fetch, clears the item cache, then starts a fresh fetch.
pub(crate) fn force_refresh(app: &mut App) {
  app.config = config::Config::load();
  app.is_loading = false;
  app.is_refreshing = false;
  app.fetch_rx = None;
  app.reset_items();
  do_refresh(app);
}

/// Returns true if enough time has elapsed since the last keyboard scroll.
/// Updates `last_scroll_time` on success.
pub(crate) fn kbd_scroll_ok(app: &mut app::App) -> bool {
  let now = std::time::Instant::now();
  if let Some(last) = app.last_scroll_time {
    if last.elapsed().as_millis() < app.scroll_debounce_ms as u128 {
      return false;
    }
  }
  app.last_scroll_time = Some(now);
  true
}

/// Returns true if enough time has elapsed since the last mouse scroll.
/// Uses a higher debounce threshold to tame trackpad inertia.
fn mouse_scroll_ok(app: &mut app::App) -> bool {
  let now = std::time::Instant::now();
  if let Some(last) = app.last_mouse_scroll_time {
    if last.elapsed().as_millis() < app.mouse_scroll_debounce_ms as u128 {
      return false;
    }
  }
  app.last_mouse_scroll_time = Some(now);
  true
}

fn handle_mouse(
  mouse: crossterm::event::MouseEvent,
  app: &mut app::App,
  terminal: &ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) {
  if app.view != app::AppView::Feed {
    return;
  }
  let Ok(size) = terminal.size() else { return };

  // Geometry for scrollbar hit-test.
  let scrollbar_col = size.width.saturating_sub(ui::RIGHT_COL_WIDTH + 1);
  let track_top = 4u16;
  let track_bottom = size.height.saturating_sub(6);

  // Which pane is the cursor in right now?
  let hovered = app.pane_at(mouse.column, mouse.row);

  match mouse.kind {
    // ── Scroll wheel / trackpad ────────────────────────────────────────────
    MouseEventKind::ScrollDown => {
      if mouse_scroll_ok(app) {
        match hovered {
          Some(PaneId::Details) => {}
          Some(PaneId::Notes) => {
            if let Some(note_id) = app
              .notes_tabs
              .get(app.notes_active_tab)
              .map(|t| t.note_id.clone())
            {
              if let Some(notes_app) = app.notes_app.as_mut() {
                notes_app.focus_note(&note_id);
              }
            }
            if let Some(notes_app) = app.notes_app.as_mut() {
              notes_app.select_next_note();
            }
          }
          Some(PaneId::SecondaryNotes) => {
            if let Some(note_id) = app
              .secondary_notes_tabs
              .get(app.secondary_notes_active_tab)
              .map(|t| t.note_id.clone())
            {
              if let Some(notes_app) = app.notes_app.as_mut() {
                notes_app.focus_note(&note_id);
              }
            }
            if let Some(notes_app) = app.notes_app.as_mut() {
              notes_app.select_next_note();
            }
          }
          Some(PaneId::Chat) => {
            if let Some(chat_ui) = app.chat_ui.as_mut() {
              chat_ui.scroll_offset = chat_ui.scroll_offset.saturating_add(3);
            }
          }
          _ => {
            if app.filter_focus {
              app.filter_cursor_down();
            } else {
              app.move_down();
            }
          }
        }
      }
    }
    MouseEventKind::ScrollUp => {
      if mouse_scroll_ok(app) {
        match hovered {
          Some(PaneId::Details) => {}
          Some(PaneId::Notes) => {
            if let Some(note_id) = app
              .notes_tabs
              .get(app.notes_active_tab)
              .map(|t| t.note_id.clone())
            {
              if let Some(notes_app) = app.notes_app.as_mut() {
                notes_app.focus_note(&note_id);
              }
            }
            if let Some(notes_app) = app.notes_app.as_mut() {
              notes_app.select_prev_note();
            }
          }
          Some(PaneId::SecondaryNotes) => {
            if let Some(note_id) = app
              .secondary_notes_tabs
              .get(app.secondary_notes_active_tab)
              .map(|t| t.note_id.clone())
            {
              if let Some(notes_app) = app.notes_app.as_mut() {
                notes_app.focus_note(&note_id);
              }
            }
            if let Some(notes_app) = app.notes_app.as_mut() {
              notes_app.select_prev_note();
            }
          }
          Some(PaneId::Chat) => {
            if let Some(chat_ui) = app.chat_ui.as_mut() {
              chat_ui.scroll_offset = chat_ui.scroll_offset.saturating_sub(3);
            }
          }
          _ => {
            if app.filter_focus {
              app.filter_cursor_up();
            } else {
              app.move_up();
            }
          }
        }
      }
    }
    // ── Left click ─────────────────────────────────────────────────────────
    MouseEventKind::Down(MouseButton::Left) => {
      // Scrollbar track click (feed list jump) — handled before pane focus.
      if mouse.column == scrollbar_col
        && mouse.row >= track_top
        && mouse.row < track_bottom
        && hovered == Some(PaneId::Feed)
      {
        let track_height = (track_bottom - track_top) as usize;
        let click_offset = (mouse.row - track_top) as usize;
        let total = app.visible_count();
        if total > 0 && track_height > 0 {
          let new_index =
            ((click_offset * total) / track_height).min(total - 1);
          app.set_active_selected_index(new_index);
        }
        return;
      }

      // Click any focusable open pane → focus it.
      if let Some(pane) = app.focusable_pane_at(mouse.column, mouse.row) {
        app.focused_pane = pane;
        match pane {
          PaneId::Reader | PaneId::Notes => {
            app.focused_reader = FocusedReader::Primary;
            if pane == PaneId::Notes {
              if let Some(note_id) = app
                .notes_tabs
                .get(app.notes_active_tab)
                .map(|t| t.note_id.clone())
              {
                if let Some(notes_app) = app.notes_app.as_mut() {
                  notes_app.focus_note(&note_id);
                }
              }
            }
          }
          PaneId::SecondaryReader | PaneId::SecondaryNotes => {
            app.focused_reader = FocusedReader::Secondary;
            if pane == PaneId::SecondaryNotes {
              if let Some(note_id) = app
                .secondary_notes_tabs
                .get(app.secondary_notes_active_tab)
                .map(|t| t.note_id.clone())
              {
                if let Some(notes_app) = app.notes_app.as_mut() {
                  notes_app.focus_note(&note_id);
                }
              }
            }
          }
          _ => {}
        }
        if matches!(pane, PaneId::Feed) {
          app.filter_focus = false;
        }
      }
    }
    _ => {}
  }
}

// ── Background fetch helpers ──────────────────────────────────────────────

pub(crate) fn spawn_fulltext_fetch(
  item: models::FeedItem,
  tx: mpsc::Sender<Result<tread::PaperData, String>>,
) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let _ = tx.send(ingestion::fulltext::fetch(&item));
    }));
    if let Err(payload) = result {
      let msg = panic_msg(payload);
      log::error!("spawn_fulltext_fetch: thread panicked — {msg}");
      let _ = tx_panic.send(Err(format!("fulltext thread panicked: {msg}")));
    }
  });
}

pub(crate) fn spawn_repo_open(
  owner: String,
  repo: String,
  token: String,
  tx: mpsc::Sender<RepoFetchResult>,
) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let branch = match github::get_default_branch(&owner, &repo, &token) {
        Err(e) => {
          let _ = tx.send(RepoFetchResult::RepoOpened {
            branch: String::new(),
            tree: Err(e),
          });
          return;
        }
        Ok(b) => b,
      };
      let tree = github::fetch_tree_dir(&owner, &repo, &branch, "", &token);
      let _ = tx.send(RepoFetchResult::RepoOpened { branch, tree });
    }));
    if let Err(payload) = result {
      let msg = panic_msg(payload);
      log::error!("spawn_repo_open: thread panicked — {msg}");
      let _ = tx_panic.send(RepoFetchResult::RepoOpened {
        branch: String::new(),
        tree: Err(format!("repo-open thread panicked: {msg}")),
      });
    }
  });
}

pub(crate) fn spawn_repo_dir(
  owner: String,
  repo: String,
  branch: String,
  path: String,
  token: String,
  tx: mpsc::Sender<RepoFetchResult>,
) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let path_panic = path.clone();
    let outcome =
      std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let result =
          github::fetch_tree_dir(&owner, &repo, &branch, &path, &token);
        let _ = tx.send(RepoFetchResult::DirLoaded { path, result });
      }));
    if let Err(payload) = outcome {
      let msg = panic_msg(payload);
      log::error!("spawn_repo_dir: thread panicked — {msg}");
      let _ = tx_panic.send(RepoFetchResult::DirLoaded {
        path: path_panic,
        result: Err(format!("repo-dir thread panicked: {msg}")),
      });
    }
  });
}

pub(crate) fn spawn_repo_file(
  owner: String,
  repo: String,
  path: String,
  name: String,
  token: String,
  tx: mpsc::Sender<RepoFetchResult>,
) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let path_panic = path.clone();
    let name_panic = name.clone();
    let outcome =
      std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let result = github::fetch_file(&owner, &repo, &path, &token);
        let _ = tx.send(RepoFetchResult::FileLoaded { path, name, result });
      }));
    if let Err(payload) = outcome {
      let msg = panic_msg(payload);
      log::error!("spawn_repo_file: thread panicked — {msg}");
      let _ = tx_panic.send(RepoFetchResult::FileLoaded {
        path: path_panic,
        name: name_panic,
        result: Err(format!("repo-file thread panicked: {msg}")),
      });
    }
  });
}

// ─────────────────────────────────────────────────────────────────────────────

/// `0` = primary pane (Reader if active, else Feed).
/// `1`/`2`/`3` = secondary open panes sorted top-to-bottom, left-to-right.
pub(crate) fn get_pane_by_number(n: u8, app: &App) -> Option<PaneId> {
  match n {
    0 => Some(if app.reader_active { PaneId::Reader } else { PaneId::Feed }),
    1..=3 => app.secondary_panes_sorted().into_iter().nth((n - 1) as usize),
    _ => None,
  }
}

fn migrate_legacy_config_dir() {
  let Some(home) = dirs::home_dir() else {
    return;
  };

  let old_root = home.join(".config/tentative");
  if !old_root.exists() {
    return;
  }

  let new_root = home.join(".config/trench");
  if let Err(e) = std::fs::create_dir_all(&new_root) {
    eprintln!("trench: could not prepare config dir ({e}); continuing");
    return;
  }

  for name in [
    "config.json",
    "state.json",
    "cache.json",
    "enrichment_cache.json",
    "discovery_cache.json",
    "trench.log",
    "hf_repo_cache.json",
    "chats",
    "notes",
  ] {
    let old_path = old_root.join(name);
    let new_path = new_root.join(name);
    if !old_path.exists() || new_path.exists() {
      continue;
    }
    // Reject pre-planted symlinks. If a hostile process briefly had write
    // access to ~/.config/tentative/, planting `state.json` as a symlink
    // pointing to a victim file would let our rename move that file
    // unexpectedly. Skip symlinks; the legacy dir is ephemeral and a
    // subsequent launch can retry (audit Sec MED #17).
    match std::fs::symlink_metadata(&old_path) {
      Ok(m) if m.file_type().is_symlink() => {
        eprintln!(
          "trench: refusing to migrate symlink at {}; skipping",
          old_path.display()
        );
        continue;
      }
      _ => {}
    }
    if let Err(e) = std::fs::rename(&old_path, &new_path) {
      eprintln!(
        "trench: could not migrate {} to new config dir ({e}); continuing",
        old_path.display()
      );
    }
  }

  let old_root_empty = std::fs::read_dir(&old_root)
    .map(|mut entries| entries.next().is_none())
    .unwrap_or(false);
  if old_root_empty {
    let _ = std::fs::remove_dir(&old_root);
  }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let startup_t0 = std::time::Instant::now();
  migrate_legacy_config_dir();

  let log_level = if std::env::var_os("TRENCH_DEBUG_LOG").is_some() {
    log::LevelFilter::Debug
  } else {
    log::LevelFilter::Info
  };
  let log_file = dirs::home_dir().and_then(|home| {
    let path = home.join(".config/trench/trench.log");
    std::fs::create_dir_all(path.parent()?).ok()?;
    // Truncate on each startup — prevents unbounded growth from filling disk.
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).write(true).truncate(true);
    // Owner-read/write only on Unix. Default umask leaves trench.log
    // world-readable, which leaks API error messages and partial system
    // prompt fragments to other local users (audit Sec MED #14).
    #[cfg(unix)]
    {
      use std::os::unix::fs::OpenOptionsExt;
      opts.mode(0o600);
    }
    opts.open(&path).ok()
  });

  // Surface a startup warning on Windows so users on multi-user systems
  // know that cache.json, history, chat sessions, notes, and trench.log
  // are written with default ACLs (typically world-readable). The
  // Unix-only set_private 0o600 step is a no-op on Windows; a real DACL
  // fix via windows-sys is tracked separately (audit Sec MED #8).
  #[cfg(windows)]
  log::warn!(
    "trench on Windows: data files use default ACLs and may be \
     readable by other local users. set_private is a no-op on Windows."
  );
  match log_file {
    Some(f) => {
      env_logger::Builder::new()
        .target(env_logger::Target::Pipe(Box::new(f)))
        .filter_level(log_level)
        .init();
    }
    None => {
      env_logger::Builder::new().filter_level(log::LevelFilter::Off).init();
    }
  }

  // Install a panic hook that restores the terminal before printing the
  // backtrace.  Without this, a panic mid-run leaves the user stuck in
  // alt-screen / raw mode with no visible cursor.  Best-effort — every
  // step ignores its own errors so a partial failure (e.g. stderr
  // already closed) doesn't prevent the rest from running and doesn't
  // double-panic.
  {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
      // Pop the kitty-keyboard flags + focus-change BEFORE leaving the alt
      // screen, otherwise the responses to those sequences leak into the
      // user's shell after the panic and corrupt input until manual `reset`
      // (audit Rel CRIT C3).
      let _ = crossterm::execute!(
        std::io::stderr(),
        crossterm::event::PopKeyboardEnhancementFlags,
        crossterm::event::DisableFocusChange,
      );
      let _ = crossterm::terminal::disable_raw_mode();
      let _ = crossterm::execute!(
        std::io::stderr(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show,
      );
      // Blank line before the panic message so it doesn't collide
      // with any partial frame the alt-screen leave just flushed.
      eprintln!();
      default_hook(info);
    }));
  }

  enable_raw_mode()?;
  let mut stdout = io::stdout();
  // EnableFocusChange so the (eventual) embedded reader can detect tmux
  // pane switches and clear pixel-image placements before they bleed
  // across panes.  No effect on the feed UI — it ignores focus events.
  // DISAMBIGUATE_ESCAPE_CODES so Shift+Enter and other modified specials
  // are distinguishable from plain Enter — needed by tread for the
  // citation-popup binding (`Shift+Enter` vs `Enter` for jump-to-link).
  // Trench's existing keys.rs already uses `KeyCode::Enter` (not
  // `Char('\n')`) at every Enter site, so this flag is a behaviour-
  // preserving addition for the feed UI.  Terminals that don't speak
  // the kitty keyboard protocol silently ignore the push.
  execute!(
    stdout,
    EnterAlternateScreen,
    EnableMouseCapture,
    EnableFocusChange,
  )?;
  let _ = execute!(
    stdout,
    PushKeyboardEnhancementFlags(
      KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
    ),
  );
  let backend = CrosstermBackend::new(stdout);
  let mut terminal = Terminal::new(backend)?;

  let mut app = App::new();
  log::debug!("startup: App::new {}ms", startup_t0.elapsed().as_millis());

  // Load config.
  let t = std::time::Instant::now();
  let cfg = config::Config::load();
  app.github_token = cfg.github_token.clone();
  app.active_theme = cfg.theme;
  app.active_custom_theme_id = cfg.active_custom_theme_id.clone();
  app.config = cfg;
  app.reconcile_custom_theme_selection();
  log::debug!("startup: config load {}ms", t.elapsed().as_millis());

  // Load persisted workflow states and UI state.
  let t = std::time::Instant::now();
  app.persisted_states = store::load();
  let ui = store::load_ui();
  app.last_read = ui.last_read;
  app.last_read_source = ui.last_read_source;
  app.notes_tabs = ui.notes_tabs;
  // Clamp in case ui.json was written with a tab count that has since shrunk.
  app.notes_active_tab =
    ui.notes_active_tab.min(app.notes_tabs.len().saturating_sub(1));
  app.secondary_notes_tabs = ui.secondary_notes_tabs;
  app.secondary_notes_active_tab = ui
    .secondary_notes_active_tab
    .min(app.secondary_notes_tabs.len().saturating_sub(1));
  log::debug!("startup: state/ui load {}ms", t.elapsed().as_millis());

  // 1. Load cache immediately → populate app.items.
  let t = std::time::Instant::now();
  let cached = store::cache::load();
  if !cached.is_empty() {
    app.items = cached;
  }
  // Build url_index + arxiv_id_index over the loaded items so the dedup
  // hot path in process_incoming gets O(1) lookups from the very first
  // batch. Same for discovery_items, which were loaded in App::new.
  app.rebuild_indices();
  app.rebuild_discovery_indices();
  log::debug!(
    "startup: cache load + index rebuild {}ms ({} cached items)",
    t.elapsed().as_millis(),
    app.items.len()
  );

  // 2. Apply persisted states to cached items.
  let t = std::time::Instant::now();
  for item in &mut app.items {
    if let Some(state) = app.persisted_states.get(&item.url) {
      item.workflow_state = *state;
    }
  }
  for item in &mut app.discovery_items {
    if let Some(state) = app.persisted_states.get(&item.url) {
      item.workflow_state = *state;
    }
  }
  log::debug!("startup: persisted state apply {}ms", t.elapsed().as_millis());

  app.list_offset = 0;

  // 4. Spawn background thread to fetch all sources then enrich.
  {
    let (tx, rx) = mpsc::channel::<FetchMessage>();
    app.fetch_rx = Some(rx);
    let mut loading_sources = vec![
      "arxiv".to_string(),
      "huggingface".to_string(),
      "openreview".to_string(),
      "core".to_string(),
      "openai".to_string(),
      "deepmind".to_string(),
      "import_ai".to_string(),
      "bair".to_string(),
      "mit_news_ai".to_string(),
      "enriching".to_string(),
    ];
    for feed in &app.config.sources.custom_feeds {
      loading_sources.push(feed.name.clone());
    }
    app.loading_sources = loading_sources;
    app.is_loading = true;
    spawn_fetch(tx, app.config.clone());
  }

  // 3. Start the TUI loop. Wrap the loop body in an inner closure so
  // cleanup at the end of `main` runs unconditionally — even if
  // `terminal.draw(...)?` returns Err mid-frame (TTY dropped, SSH
  // session died, etc.) we still execute disable_raw_mode +
  // LeaveAlternateScreen + flag-pop (audit Rel HIGH H7). The panic
  // hook covers panics; this closure covers Err returns.
  let mut first_draw_logged = false;
  let run_result: std::io::Result<()> = (|| -> std::io::Result<()> {
    loop {
    // Drain any pending fetch results before drawing. process_incoming +
    // process_incoming_discovery internally call mark_dirty when state
    // changes; the spinner increment is now gated on is_loading.
    app.process_incoming();

    // Tick the embedded reader(s) each frame so voice playback state
    // (active-word highlight, paragraph advance during continuous
    // reading) animates without waiting for a key event.  tread::tick
    // returns true when user-visible state changed; we OR that into
    // trench's dirty flag so the next frame redraws.
    if let Some(editor) = app.reader_editor_mut() {
      if editor.tick() {
        app.mark_dirty();
      }
    }
    if let Some(editor) = app.reader_secondary_editor_mut() {
      if editor.tick() {
        app.mark_dirty();
      }
    }
    if let Some(editor) = app.reader_popup_editor.as_mut() {
      if editor.tick() {
        app.mark_dirty();
      }
    }

    // Tick chat UI each frame (spinner + pending response channel + word-by-
    // word streaming reveal). When chat is streaming we want the next frame
    // to render — capture is_streaming BEFORE tick so the FINAL word still
    // triggers a redraw even though tick clears the flag on completion.
    if let Some(chat_ui) = app.chat_ui.as_mut() {
      let was_streaming = chat_ui.is_streaming;
      chat_ui.tick();
      if was_streaming || chat_ui.is_streaming {
        app.mark_dirty();
      }
    }

    // Tick repo viewer momentum scroll. If any repo context is decaying its
    // velocity, mark dirty so the next frame renders the new scroll offset.
    let was_repo_animating = app
      .repo_context
      .as_ref()
      .map(|c| c.scroll_velocity.abs() >= 0.5)
      .unwrap_or(false);
    app.repo_tick();
    if was_repo_animating {
      app.mark_dirty();
    }

    // ── Drain background fetch results ────────────────────────────────
    if let Some(rx) = app.fulltext_rx.as_ref() {
      let t = std::time::Instant::now();
      match rx.try_recv() {
        Ok(result) => {
          log::debug!(
            "fulltext drain: received result, took {}µs to recv",
            t.elapsed().as_micros()
          );
          app.fulltext_rx = None;
          app.fulltext_loading = false;
          match result {
            Ok(fetched_paper) => {
              log::debug!(
                "reader_open: {} blocks from fetcher",
                fetched_paper.blocks.len()
              );
              // arxiv URLs get the rich LaTeX path via fetch_paper —
              // structured math, tables, figures.  ~2s blocking; v2
              // can background on a worker.  Non-arxiv keeps the
              // PaperData the fetcher already produced (HTML walked
              // by from_html, or summary plain-text).  Image
              // rendering wiring is task #39, hardcode false here.
              let notes_context = app.pending_fulltext_context.take();
              let title = app.last_read.clone().unwrap_or_default();
              let detected_arxiv_id = notes_context
                .as_ref()
                .and_then(|ctx| tread::extract_arxiv_id(&ctx.paper.url));
              let kitty_supported = false;
              let (arxiv_id, paper) = if let Some(id) = detected_arxiv_id {
                match tread::fetch_paper(&id, kitty_supported) {
                  Ok(p) => (Some(id), p),
                  Err(e) => {
                    log::warn!(
                      "tread::fetch_paper failed for {id}, using fetcher result: {e}"
                    );
                    (
                      notes_context.as_ref().map(|ctx| ctx.paper.id.clone()),
                      fetched_paper,
                    )
                  }
                }
              } else {
                (
                  notes_context.as_ref().map(|ctx| ctx.paper.id.clone()),
                  fetched_paper,
                )
              };
              let reader = tread::Reader::init(
                paper,
                None,
                arxiv_id.clone(),
                80,
                24,
                kitty_supported,
                Some(app.voice_controller.clone()),
              );
              if app.fulltext_for_secondary {
                if app.fulltext_new_tab {
                  app.reader_secondary_push_tab(
                    title,
                    arxiv_id,
                    notes_context,
                    reader,
                  );
                } else {
                  app.reader_secondary_replace_active_tab(
                    title,
                    arxiv_id,
                    notes_context,
                    reader,
                  );
                }
                app.focused_reader = FocusedReader::Secondary;
                app.focused_pane = PaneId::SecondaryReader;
                app.fulltext_for_secondary = false;
              } else {
                if app.fulltext_new_tab {
                  app.reader_push_tab(title, arxiv_id, notes_context, reader);
                } else {
                  app.reader_replace_active_tab(
                    title,
                    arxiv_id,
                    notes_context,
                    reader,
                  );
                }
                app.focused_pane = PaneId::Reader;
              }
              app.fulltext_new_tab = false;
              app.clear_notification();
            }
            Err(e) => {
              app.pending_fulltext_context = None;
              app.set_notification(format!("Failed to fetch content: {e}"));
            }
          }
          app.mark_dirty();
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
          log::debug!("fulltext drain: channel disconnected");
          app.fulltext_rx = None;
          app.fulltext_loading = false;
          app.fulltext_for_secondary = false;
          app.fulltext_new_tab = false;
          app.pending_fulltext_context = None;
          app.set_notification("Fetch error: thread disconnected".to_string());
          app.mark_dirty();
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
      }
    }

    // ── Drain reader popup fetch ──────────────────────────────────────
    if let Some(rx) = app.reader_popup_rx.as_ref() {
      match rx.try_recv() {
        Ok(result) => {
          app.reader_popup_rx = None;
          app.fulltext_loading = false;
          app.pending_fulltext_context = None;
          match result {
            Ok(paper) => {
              let reader = tread::Reader::init(
                paper,
                None,
                None,
                80,
                24,
                false,
                Some(app.voice_controller.clone()),
              );
              app.reader_popup_editor = Some(reader);
              // Reset the popup's image cache to a fresh state — the
              // previous occupant (if any) had different kitty_ids.
              app.reader_popup_image_state = tread::ImageState::default();
              app.reader_popup_active = true;
              app.clear_notification();
            }
            Err(e) => {
              app.set_notification(format!("Failed to fetch content: {e}"));
            }
          }
          app.mark_dirty();
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
          app.reader_popup_rx = None;
          app.fulltext_loading = false;
          app.pending_fulltext_context = None;
          app.set_notification("Fetch error: thread disconnected".to_string());
          app.mark_dirty();
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
      }
    }

    if let Some(rx) = app.repo_fetch_rx.as_ref() {
      let t = std::time::Instant::now();
      match rx.try_recv() {
        Ok(result) => {
          log::debug!(
            "repo_fetch drain: received result, took {}µs to recv",
            t.elapsed().as_micros()
          );
          app.repo_fetch_rx = None;
          match result {
            RepoFetchResult::RepoOpened { branch, tree } => match tree {
              Ok(nodes) => {
                if let Some(ctx) = app.repo_context.as_mut() {
                  ctx.default_branch = branch;
                  ctx.tree_nodes = nodes;
                  ctx.status_message = None;
                }
              }
              Err(e) => app.set_repo_status(format!("Error: {e}")),
            },
            RepoFetchResult::DirLoaded { path, result } => {
              app.repo_apply_dir(path, result);
            }
            RepoFetchResult::FileLoaded { path, name, result } => {
              app.repo_apply_file(path, name, result);
            }
          }
          app.mark_dirty();
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
          log::debug!("repo_fetch drain: channel disconnected");
          app.repo_fetch_rx = None;
          app.set_repo_status("Fetch error: thread disconnected");
          app.mark_dirty();
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
      }
    }

    // Gate the draw on the dirty flag. `check_needs_redraw` reads-and-clears
    // in one call (cli-text-reader pattern). Idle frames cost ~0 work since
    // every per-frame allocation lives inside `ui::draw`.
    if app.check_needs_redraw() {
      let t_draw = std::time::Instant::now();
      terminal.draw(|frame| ui::draw(frame, &mut app))?;
      let draw_ms = t_draw.elapsed().as_millis();
      if !first_draw_logged {
        log::debug!(
          "startup: first frame ready in {}ms",
          startup_t0.elapsed().as_millis()
        );
        first_draw_logged = true;
      }
      if draw_ms > 16 {
        log::debug!("terminal.draw took {}ms (slow frame)", draw_ms);
      }
    }

    // Cadence: 16ms when something is animating or already dirty (so we
    // process events at 60Hz during interaction), 250ms when truly idle (so
    // CPU drops to near-zero and battery is preserved). Mirrors
    // cli-text-reader/src/editor/display_loop.rs:233.
    let timeout = if app.needs_redraw || app.has_active_animation() {
      std::time::Duration::from_millis(16)
    } else {
      std::time::Duration::from_millis(250)
    };

    if event::poll(timeout)? {
      match event::read()? {
        Event::Key(key) => {
          if key.kind != KeyEventKind::Press {
            continue;
          }

          log::debug!(
            "key event: {:?} leader_active={} focused_pane={:?}",
            key.code,
            app.leader_active,
            app.focused_pane
          );
          keys::dispatch(key, &mut app);
          app.mark_dirty();
        }
        Event::Mouse(mouse) => {
          handle_mouse(mouse, &mut app, &terminal);
          app.mark_dirty();
        }
        Event::Resize(_, _) => {
          // Pane reflow moves every image's placement coords;
          // clear the cached placements so the next draw re-emits
          // at the new positions instead of stacking ghosts.
          app.clear_all_reader_image_state();
          app.mark_dirty();
        }
        Event::FocusLost => {
          // tmux pane switch: kitty placements painted at absolute
          // screen coords would otherwise bleed into whatever pane
          // is on top.  Delete them; FocusGained re-emits via the
          // next draw cycle.
          app.clear_all_reader_image_state();
        }
        _ => {}
      }
    }

    // Dispatch any stale events that arrived during the draw call. Previous
    // behaviour silently discarded these via `let _ = event::read()`, which
    // dropped user input on slow frames; now they go through the same path
    // as the primary dispatch above.
    while event::poll(std::time::Duration::from_millis(0))? {
      match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
          keys::dispatch(key, &mut app);
          app.mark_dirty();
        }
        Event::Mouse(mouse) => {
          handle_mouse(mouse, &mut app, &terminal);
          app.mark_dirty();
        }
        Event::Resize(_, _) => {
          app.clear_all_reader_image_state();
          app.mark_dirty();
        }
        Event::FocusLost => {
          app.clear_all_reader_image_state();
        }
        _ => {}
      }
    }

    if app.should_quit {
      break;
    }
    }
    Ok(())
  })();

  // Drain any pending cache write the background writer hasn't flushed yet,
  // so the on-disk cache.json reflects the final in-memory state.
  store::cache::flush_blocking();

  store::save_ui(&store::UiState {
    last_read: app.last_read.clone(),
    last_read_source: app.last_read_source.clone(),
    notes_tabs: app.notes_tabs.clone(),
    notes_active_tab: app.notes_active_tab,
    secondary_notes_tabs: app.secondary_notes_tabs.clone(),
    secondary_notes_active_tab: app.secondary_notes_active_tab,
  });

  // Balance the kitty-keyboard push from setup. Best-effort — if any of
  // these cleanup steps fails (already-closed TTY, etc.), keep going so
  // the rest still run. Propagate the loop's error AFTER cleanup so the
  // user sees both the cleanup attempt and the original failure.
  let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
  let _ = disable_raw_mode();
  let _ = execute!(
    terminal.backend_mut(),
    LeaveAlternateScreen,
    DisableMouseCapture,
    DisableFocusChange,
  );
  run_result.map_err(Into::into)
}
