//! Registry that builds the source / enrichment pipelines from `Config`.
//!
//! Per ADR-004 §D6, the registry owns the *gating* decisions (enabled-source
//! flags, API-key presence, custom-feeds expansion). [`Source`] impls stay
//! unaware of why they were instantiated — every gating reason that
//! produced today's "skipped — disabled" / "skipped — no API key" log
//! lines lives here, not in any source impl. The trade-off: an explicit
//! ~80 LOC registry rather than the spread-out conditional construction
//! the audit's single-trait phrasing implied.

use crate::config::Config;
use crate::models::{ContentType, SourcePlatform};

use super::arxiv::ArxivSource;
use super::core::CoreSource;
use super::huggingface::{HuggingFaceRepoEnrichment, HuggingFaceSource};
use super::openreview::OpenReviewSource;
use super::pipeline::{EnrichmentSource, Source};
use super::rss::RssSource;
use super::semantic_scholar::SemanticScholarEnrichment;

/// Built-in RSS feeds. Each is on a distinct host, so each becomes its
/// own [`RssSource`] with its own `host_group()` and runs in parallel.
/// Lifted verbatim from `services::ingestion::spawn_fetch`'s pre-C10
/// `rss_feeds` slice — same order, same labels, same platforms.
///
/// `anthropic` is intentionally absent: it has no RSS feed
/// (https://www.anthropic.com/news has no `<link rel="alternate" …>`)
/// and the warning that used to fire at orchestrator startup now lives
/// in [`build_sources`] below.
const BUILTIN_RSS: &[(&str, &str, SourcePlatform, ContentType)] = &[
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

/// True when a source name is enabled (default: enabled if absent).
fn is_enabled(config: &Config, name: &str) -> bool {
  config.sources.enabled_sources.get(name).copied().unwrap_or(true)
}

/// Build the [`Source`] registry for one refresh. Reads gating state
/// from `config`; any source disabled or missing required credentials
/// is omitted entirely (with a log line at orchestrator scheduling
/// time matching the pre-C10 messaging).
pub fn build_sources(config: &Config) -> Vec<Box<dyn Source>> {
  log::warn!(
    "source anthropic: no RSS feed available; skipping \
     (https://www.anthropic.com/news has no feed link)"
  );

  let mut sources: Vec<Box<dyn Source>> = Vec::new();

  // arxiv: always-on; categories sourced from config.
  sources.push(Box::new(ArxivSource {
    categories: config.sources.arxiv_categories.clone(),
  }));

  // huggingface: enabled flag. Same host_group as arxiv ("arxiv") so
  // the orchestrator runs them serially on one thread to stay under
  // export.arxiv.org's 1/3s envelope.
  if is_enabled(config, "huggingface") {
    sources.push(Box::new(HuggingFaceSource));
  } else {
    log::info!("source huggingface: disabled — skipping");
  }

  // openreview: enabled flag. Venues are construction-time per ADR-004
  // §D5 — `OpenReviewSource::default` reproduces the pre-C10 hardcode.
  if is_enabled(config, "openreview") {
    sources.push(Box::new(OpenReviewSource::default()));
  } else {
    log::info!("source openreview: disabled — skipping");
  }

  // core: enabled flag + api-key gating. The api-key check produces a
  // distinct log line from the disabled-flag check (preserved from the
  // pre-C10 behavior).
  if !is_enabled(config, "core") {
    log::info!("source core: disabled — skipping");
  } else if config.core_api_key.as_deref().is_none() {
    log::info!("source core: no API key configured — skipping");
  } else {
    sources.push(Box::new(CoreSource));
  }

  // RSS: enabled flag per feed. Built-in feeds first, then user customs.
  for (name, url, platform, content_type) in BUILTIN_RSS {
    if !is_enabled(config, name) {
      log::info!("source {name}: disabled — skipping");
      continue;
    }
    sources.push(Box::new(RssSource {
      source_name: (*name).to_string(),
      feed_url: (*url).to_string(),
      platform: platform.clone(),
      content_type: content_type.clone(),
    }));
  }
  for feed in &config.sources.custom_feeds {
    sources.push(Box::new(RssSource {
      source_name: feed.name.clone(),
      feed_url: feed.url.clone(),
      platform: SourcePlatform::Rss,
      content_type: ContentType::Article,
    }));
  }

  sources
}

/// Build the [`EnrichmentSource`] registry for one refresh. Order is
/// load-bearing — semantic_scholar runs first (so `apply_entry` populates
/// `authors` / `domain_tags`) before huggingface_repo enrichment looks
/// at them.
pub fn build_enrichments(config: &Config) -> Vec<Box<dyn EnrichmentSource>> {
  let mut enrichments: Vec<Box<dyn EnrichmentSource>> = Vec::new();

  // semantic_scholar: gated on API key. The unauthenticated path
  // enriches 0 items and consumes ~0.5-2s on every refresh waiting for
  // 429+retry budget, so we skip cleanly when no key is set (same
  // rationale as the pre-C10 `services::ingestion::spawn_fetch` check).
  let s2_key =
    config.semantic_scholar_key.as_deref().filter(|k| !k.trim().is_empty());
  if s2_key.is_some() {
    enrichments.push(Box::new(SemanticScholarEnrichment::new()));
  } else {
    log::info!("ingestion: semantic_scholar skipped — no API key configured");
  }

  enrichments.push(Box::new(HuggingFaceRepoEnrichment::new()));

  enrichments
}
