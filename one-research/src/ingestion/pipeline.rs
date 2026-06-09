//! C10 ingestion seam (ADR-004).
//!
//! Two traits, one module: [`Source`] for bulk fetch, [`EnrichmentSource`]
//! for the post-fetch enrichment phase. Both consumed by the orchestrator
//! in `services::ingestion::spawn_fetch` via `Vec<Box<dyn _>>` registries.
//!
//! PR 1 (this PR) lands the empty trait surface + [`FetchContext`] + smoke
//! tests. The five `impl Source` / two `impl EnrichmentSource` blocks plus
//! the orchestrator rewrite arrive in PR 2 (no behavior change — byte-
//! equivalent network behavior, see ADR-004 risk #2). PR 3 wires the
//! `scripts/check-ingestion-seam.sh` tripwires and flips ADR-004 to
//! Accepted.
//!
//! ## Why two traits, not one
//!
//! Fetch returns items and can fail (`Result<Vec<FeedItem>>`); enrichment
//! mutates a slice in place and is best-effort (no `Result`, failures log
//! internally). Different inputs, different outputs, different scheduling
//! (parallel inside `thread::scope` vs sequential post-scope). A single
//! trait would force `Option<Vec<FeedItem>>` and the orchestrator would
//! branch on it anyway. See ADR-004 §D1.
//!
//! ## Why `EnrichmentSource: Send` without `Sync`
//!
//! Enrichment impls own their per-source caches (`SemanticScholarEnrichment
//! { cache: RefCell<EnrichmentCache> }`). `RefCell<T>: !Sync`. The
//! enrichment phase runs single-threaded post-scope, so `Sync` is provably
//! unnecessary. See ADR-004 §D4.

use std::path::Path;

use crate::config::Config;
use crate::models::FeedItem;
use one_research_http::RetryPolicy;

/// A bulk-ingestion source. One `fetch` call per refresh; returns a fresh
/// batch of [`FeedItem`]s or a string error. The orchestrator handles
/// scheduling (via [`Source::host_group`]), logging (via [`Source::name`]),
/// and panic isolation.
///
/// `Send + Sync` is required because the orchestrator's `std::thread::scope`
/// captures `&dyn Source` across thread boundaries.
pub trait Source: Send + Sync {
  /// Identifier for logs and `FetchMessage::SourceComplete`. Stable across
  /// runs — used in user-visible loading-spinner labels.
  fn name(&self) -> &str;

  /// Scheduling tag used by the orchestrator to group sources sharing a
  /// rate-limit envelope. Sources with the same `host_group` run serially
  /// within one thread; different groups run in parallel.
  ///
  /// Today's groups (per `services::ingestion::spawn_fetch`):
  /// - `"arxiv"` — arxiv + huggingface (both hit `export.arxiv.org`)
  /// - `"openreview"`, `"core"`, `"rss"` — each their own host envelope
  fn host_group(&self) -> &str;

  /// Fetch a batch of [`FeedItem`]s. Blocking; called on the orchestrator's
  /// per-group thread. Use [`FetchContext::http`] / [`FetchContext::with_retry`]
  /// for HTTP — never reach `crate::http::client()` directly (tripwire J4
  /// will fail in PR 3 if you do).
  fn fetch(&self, ctx: &FetchContext) -> Result<Vec<FeedItem>, String>;
}

/// A post-fetch enrichment that mutates items in place. Best-effort —
/// failures log internally and skip the affected item rather than failing
/// the whole batch.
///
/// `Send` only — see module docs for the `!Sync` rationale (ADR-004 §D4).
pub trait EnrichmentSource: Send {
  fn name(&self) -> &str;

  /// Enrich `items` in place. Runs single-threaded after all [`Source`]s
  /// have completed (orchestrator drains `std::thread::scope` first).
  fn enrich(&self, items: &mut [FeedItem], ctx: &FetchContext);
}

/// Shared infrastructure passed to every [`Source::fetch`] / [`EnrichmentSource::enrich`]
/// call. Built by the orchestrator once per refresh.
///
/// The trait method signatures take `&FetchContext`. The lifetime
/// parameter lives on this struct and on the method binding only — not on
/// the traits themselves, which keeps object-safety simple.
pub struct FetchContext<'a> {
  /// User configuration. Sources read API keys, enabled-source flags,
  /// custom arxiv categories etc. from here. Mutable config changes
  /// between refreshes; sources should not cache across calls.
  pub config: &'a Config,

  /// Filesystem path under which per-source caches live (typically
  /// `~/.config/one-research/`). Enrichment impls that own caches load from /
  /// save to this directory.
  ///
  /// Today's enrichment impls (`SemanticScholarEnrichment`,
  /// `HuggingFaceRepoEnrichment`) derive their own paths via the
  /// existing `store::enrichment_cache` / `huggingface::hf_cache_path`
  /// helpers, so `cache_dir` is forward-design: it surfaces the path
  /// uniformly so a future `Store<T>` seam (audit candidate C8) can
  /// route all cache I/O through the context. `#[allow(dead_code)]`
  /// covers the gap until the first consumer lands.
  #[allow(dead_code)]
  pub cache_dir: &'a Path,
}

impl FetchContext<'_> {
  /// Access the process-wide HTTP client. Re-exports `one_research_http::client()`
  /// so source impls never need to import the http crate directly — keeps
  /// tripwire J4 simple ("no `crate::http::client()` in `one-research/src/ingestion/`").
  pub fn http(&self) -> &'static reqwest::blocking::Client {
    crate::http::client()
  }

  /// Issue a request with retry semantics per `policy`. Forwards to
  /// [`one_research_http::with_retry`]. Use `RetryPolicy::arxiv()` for sources
  /// that historically hit 429/503 on the arxiv envelope; `RetryPolicy::none()`
  /// for one-shot reads.
  pub fn with_retry<F>(
    &self,
    policy: &RetryPolicy,
    make: F,
  ) -> Result<reqwest::blocking::Response, String>
  where
    F: Fn(&reqwest::blocking::Client) -> reqwest::blocking::RequestBuilder,
  {
    one_research_http::with_retry(self.http(), policy, make)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // ── Object-safety ───────────────────────────────────────────────────
  //
  // The whole orchestrator design hinges on `Vec<Box<dyn Source>>`. If
  // either trait stops being object-safe (e.g. a method gains a `Self`
  // return), the orchestrator stops compiling. Make the regression loud.

  struct FakeSource;
  impl Source for FakeSource {
    fn name(&self) -> &str {
      "fake"
    }
    fn host_group(&self) -> &str {
      "test"
    }
    fn fetch(&self, _ctx: &FetchContext) -> Result<Vec<FeedItem>, String> {
      Ok(Vec::new())
    }
  }

  struct FakeEnrichment;
  impl EnrichmentSource for FakeEnrichment {
    fn name(&self) -> &str {
      "fake-enrich"
    }
    fn enrich(&self, _items: &mut [FeedItem], _ctx: &FetchContext) {}
  }

  #[test]
  fn source_is_object_safe() {
    // Compile-time assertion: if `Source` ever stops being dyn-compatible,
    // this `Box<dyn Source>` line fails to typecheck.
    let boxed: Box<dyn Source> = Box::new(FakeSource);
    assert_eq!(boxed.name(), "fake");
    assert_eq!(boxed.host_group(), "test");
  }

  #[test]
  fn enrichment_is_object_safe() {
    let boxed: Box<dyn EnrichmentSource> = Box::new(FakeEnrichment);
    assert_eq!(boxed.name(), "fake-enrich");
  }

  #[test]
  fn source_is_send_and_sync() {
    // Compile-fail if `Send + Sync` ever gets dropped from the trait.
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    assert_send_sync::<dyn Source>();
  }

  #[test]
  fn enrichment_is_send_but_not_required_sync() {
    // `EnrichmentSource: Send` is required. We do NOT assert `Sync`
    // here — by design (ADR-004 §D4), enrichment impls own RefCell
    // caches and would fail the Sync bound. A separate Send-only check
    // confirms the bound shipped is the one the orchestrator needs.
    fn assert_send<T: Send + ?Sized>() {}
    assert_send::<dyn EnrichmentSource>();
  }

  // ── FetchContext smoke ──────────────────────────────────────────────

  #[test]
  fn http_returns_the_singleton() {
    // `ctx.http()` and `one_research_http::client()` must alias — sources
    // calling either form should get the same memoized client (DNS pool,
    // TLS state). If this diverges someone's introduced a second client.
    let cfg = Config::default();
    let dir = std::path::PathBuf::from("/tmp");
    let ctx = FetchContext { config: &cfg, cache_dir: &dir };
    assert!(
      std::ptr::eq(ctx.http(), one_research_http::client()),
      "FetchContext::http must alias one_research_http::client (singleton invariant)"
    );
  }

  // ── Method-signature contract (N1, ADR-004) ─────────────────────────
  //
  // The object-safety tests above lock the *trait shape* — that
  // `Box<dyn Source>` and `Box<dyn EnrichmentSource>` exist. These tests
  // lock the *method-call shape* — that `fetch(&ctx)` returns
  // `Result<Vec<FeedItem>, String>` and `enrich(&mut [FeedItem], &ctx)`
  // mutates in place. A rename like `fetch(&ctx, n: u8)` only breaks
  // impls, not the dyn cast — these call sites are what catches it.

  #[test]
  fn source_fetch_call_shape_through_context() {
    let cfg = Config::default();
    let dir = std::path::PathBuf::from("/tmp");
    let ctx = FetchContext { config: &cfg, cache_dir: &dir };
    let source: Box<dyn Source> = Box::new(FakeSource);
    let result = source.fetch(&ctx);
    let items = result.expect("FakeSource always returns Ok");
    assert!(items.is_empty(), "FakeSource is a no-op");
  }

  #[test]
  fn enrichment_enrich_call_shape_mutates_in_place() {
    let cfg = Config::default();
    let dir = std::path::PathBuf::from("/tmp");
    let ctx = FetchContext { config: &cfg, cache_dir: &dir };
    let mut items: Vec<FeedItem> = Vec::new();
    let enrich: Box<dyn EnrichmentSource> = Box::new(FakeEnrichment);
    enrich.enrich(&mut items, &ctx);
    // FakeEnrichment is a no-op; the assert documents the contract —
    // enrich neither adds nor removes items, only mutates in place.
    assert!(items.is_empty(), "FakeEnrichment must not add items");
  }
}
