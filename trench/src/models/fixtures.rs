//! Synthetic FeedItem factories for benchmarks and tests.
//!
//! These helpers exist because trench has two non-production callers
//! that need to fabricate FeedItems: the in-process render bench
//! (`bench.rs`, invoked via `--bench-render` on the release binary)
//! and integration tests that need a populated workspace.
//!
//! No `#[cfg(test)]` gate — the bench harness runs in release mode,
//! which is incompatible with both `cfg(test)` and `cfg(debug_assertions)`.
//! The factory ships in the release binary; LTO can't strip it because
//! `bench.rs` is a legitimate caller.
//!
//! Two distinct shapes:
//! - **`variant(idx)`** — deterministic, index-keyed; spreads coverage
//!   across every enum variant (platform/content/workflow/signal) so
//!   benchmarks exercise every styling branch. Use this when you want
//!   N items with variety.
//! - **`minimal(id, url)`** — TODO (not yet extracted). Use this when
//!   a test wants one item with specific fields and everything else
//!   defaulted. Existing inline `FeedItem { ... }` blocks in app tests
//!   are candidates for migration when the helper is built.

use super::{
  ContentType, FeedItem, SignalLevel, SourcePlatform, WorkflowState,
};

/// Deterministic FeedItem factory keyed on index. Two callers of the
/// same `variant(idx)` get byte-identical FeedItems, isolating
/// run-to-run timing variance from the input itself.
///
/// Spreads items across all platforms / workflow states / content
/// types / signals so a renderer exercises every styling branch
/// (signal-tier highlighting, workflow-state column, source badge),
/// not just the dominant case.
pub fn variant(idx: usize) -> FeedItem {
  let title = format!(
    "Synthetic Paper {idx:05}: Studies on Benchmark-Driven Foo with Bar"
  );
  let summary = format!(
    "Abstract for synthetic item {idx}. We explore the implications of \
     benchmark-driven design in a system with N items. Results show that the \
     approach scales linearly with N under the conditions tested, with \
     constant factors comparable to baseline implementations."
  );
  let authors = vec![
    format!("Alice {:03}", idx % 100),
    format!("Bob {:03}", (idx + 7) % 100),
    format!("Carol {:03}", (idx + 13) % 100),
  ];
  let platform = match idx % 5 {
    0 => SourcePlatform::ArXiv,
    1 => SourcePlatform::HuggingFace,
    2 => SourcePlatform::Blog,
    3 => SourcePlatform::OpenReview,
    _ => SourcePlatform::Rss,
  };
  let content = match idx % 5 {
    0 => ContentType::Paper,
    1 => ContentType::Thread,
    2 => ContentType::Article,
    3 => ContentType::Repo,
    _ => ContentType::Digest,
  };
  let state = match idx % 4 {
    0 => WorkflowState::Inbox,
    1 => WorkflowState::Queued,
    2 => WorkflowState::DeepRead,
    _ => WorkflowState::Archived,
  };
  let signal = match idx % 3 {
    0 => SignalLevel::Primary,
    1 => SignalLevel::Secondary,
    _ => SignalLevel::Tertiary,
  };
  let mut item = FeedItem {
    id: format!("bench-{idx}"),
    title,
    source_platform: platform,
    content_type: content,
    domain_tags: vec!["ml".into(), "nlp".into()],
    signal,
    published_at: "2026-05-17T00:00:00Z".into(),
    authors,
    summary_short: summary,
    workflow_state: state,
    url: format!("https://arxiv.org/abs/2605.{idx:05}"),
    upvote_count: (idx as u32 * 3) % 50,
    github_repo: None,
    github_owner: None,
    github_repo_name: None,
    benchmark_results: vec![],
    full_content: None,
    source_name: "bench".into(),
    title_lower: String::new(),
    authors_lower: Vec::new(),
  };
  item.sanitize_in_place();
  item
}
