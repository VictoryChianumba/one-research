//! `AsyncJobs` — every in-flight or recently-resolved background fetch
//! lives here, grouped by job class.
//!
//! Four job classes today, each with its own `Option<Receiver<...>>`
//! channel + loading-state bookkeeping:
//!
//!   - **Bulk fetch** ([`fetch_rx`], [`is_loading`], [`loading_sources`],
//!     [`loaded_sources`], [`spinner_frame`]): the periodic
//!     arxiv/huggingface/rss/openreview ingestion pass.
//!   - **Fulltext fetch** ([`fulltext_rx`], [`fulltext_loading`],
//!     [`pending_fulltext_context`]): stage-1 fast-path fetch when the
//!     user opens a paper.
//!   - **Tread fetch** ([`tread_fetch_rx`], [`pending_tread_fetch`]):
//!     stage-2 rich-LaTeX rendering for arXiv items.
//!   - **Repo fetch** ([`repo_fetch_rx`]): repo-viewer's GitHub fetch.
//!
//! Plus two routing flags reclassified from `ViewFlags` per ADR-009
//! PR 4: [`fulltext_new_tab`] and [`fulltext_for_secondary`].  These
//! configure where the in-flight fulltext fetch's result will land
//! when it resolves — they're consumed by the async path in
//! `main.rs`, not by popup visibility logic.
//!
//! Introduced by [ADR-009](../../../../docs/adr/ADR-009-app-field-grouping.md)
//! as cluster #6 — the largest of the ADR-009 series and the one that
//! closes the punch list.
//!
//! ## Pub fields, no protocol methods (this PR)
//!
//! Each job class has a real start→poll→resolve protocol, but extracting
//! it requires writing new code (today the protocol is open-coded
//! across `main.rs::spawn_*` and `app/methods/process.rs::process_incoming`).
//! That's larger scope than fits ADR-009; the structural grouping is
//! the win.  Future PRs can add `AsyncJobs::start_fetch(rx)` /
//! `finish_fetch()` and siblings when the protocol pressure justifies
//! the extraction — same lazy-rollout cadence ADR-001 established.

use std::sync::mpsc::Receiver;

use crate::app::PendingTreadFetch;
use crate::ingestion::message::FetchMessage;

use super::{NotesContext, RepoFetchResult};

/// See module-level docs.
pub struct AsyncJobs {
  // ── Bulk fetch (arxiv + huggingface + rss + …) ────────────────────────
  pub fetch_rx: Option<Receiver<FetchMessage>>,
  /// Set true while the bulk fetch cycle is active.  Gates the spinner.
  pub is_loading: bool,
  /// Sources currently loading.  Populated as each `Source::fetch`
  /// spawns; drained as each `SourceComplete` arrives.
  pub loading_sources: Vec<String>,
  /// Sources that have completed in the current cycle.
  pub loaded_sources: Vec<String>,
  /// Animation frame for the loading spinner.
  pub spinner_frame: usize,

  // ── Fulltext fetch (stage 1) ──────────────────────────────────────────
  pub fulltext_rx: Option<Receiver<Result<tread::PaperData, String>>>,
  pub fulltext_loading: bool,
  /// Notes anchor for the in-flight fulltext fetch — moved into the
  /// resolved `ReaderTab` once the fetch completes.
  pub pending_fulltext_context: Option<NotesContext>,

  // ── Tread fetch (stage 2 — rich LaTeX for arXiv) ──────────────────────
  pub tread_fetch_rx: Option<Receiver<Result<tread::PaperData, String>>>,
  /// Full context for the in-flight tread fetch — target pane, mode,
  /// title, notes anchor, plus the fulltext fallback if stage 2 errors.
  pub pending_tread_fetch: Option<PendingTreadFetch>,

  // ── Repo viewer fetch ─────────────────────────────────────────────────
  pub repo_fetch_rx: Option<Receiver<RepoFetchResult>>,

  // ── Fulltext routing (reclassified from ViewFlags per ADR-009 PR 4) ──
  /// Routing flag — true means route the upcoming fetch to the secondary
  /// reader pane rather than the primary (set by the `[1]`/`[2]` pane
  /// prompt in dual mode).
  pub fulltext_for_secondary: bool,
}

impl Default for AsyncJobs {
  fn default() -> Self {
    Self {
      fetch_rx: None,
      is_loading: false,
      loading_sources: Vec::new(),
      loaded_sources: Vec::new(),
      spinner_frame: 0,
      fulltext_rx: None,
      fulltext_loading: false,
      pending_fulltext_context: None,
      tread_fetch_rx: None,
      pending_tread_fetch: None,
      repo_fetch_rx: None,
      fulltext_for_secondary: false,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_matches_pre_grouping_initialisers() {
    // Load-bearing: pre-grouping the 13 flat App fields all initialised
    // to None / false / empty / 0.  Byte-equivalent behaviour is the
    // correctness criterion for ADR-009 migrations.
    let j = AsyncJobs::default();
    assert!(j.fetch_rx.is_none());
    assert!(!j.is_loading);
    assert!(j.loading_sources.is_empty());
    assert!(j.loaded_sources.is_empty());
    assert_eq!(j.spinner_frame, 0);
    assert!(j.fulltext_rx.is_none());
    assert!(!j.fulltext_loading);
    assert!(j.pending_fulltext_context.is_none());
    assert!(j.tread_fetch_rx.is_none());
    assert!(j.pending_tread_fetch.is_none());
    assert!(j.repo_fetch_rx.is_none());
    assert!(!j.fulltext_for_secondary);
  }

  #[test]
  fn job_classes_are_independent() {
    // Witness: the cluster is a grouping by lifecycle, not a single
    // shared state.  Setting fulltext_loading must not touch
    // is_loading or fulltext_for_secondary.  Bug class to avoid: a
    // future method that "starts the next fetch" accidentally crossing
    // job boundaries (e.g., clearing the bulk-fetch state on a fulltext
    // start).
    let mut j = AsyncJobs::default();
    j.is_loading = true;
    j.fulltext_loading = true;
    j.fulltext_for_secondary = true;

    j.fulltext_loading = false;
    assert!(j.is_loading, "bulk-fetch flag survives fulltext_loading flip");
    assert!(
      j.fulltext_for_secondary,
      "routing flag survives fulltext_loading flip"
    );

    j.is_loading = false;
    assert!(j.fulltext_for_secondary, "routing flag survives is_loading flip");
  }

  #[test]
  fn spinner_frame_advances_independently() {
    // The spinner is a tick counter; it advances regardless of which
    // fetch is loading.  Pre-grouping, callers wrote
    // `app.spinner_frame = app.spinner_frame.wrapping_add(1)`; the
    // grouped field continues to support that pattern.
    let mut j = AsyncJobs::default();
    j.spinner_frame = j.spinner_frame.wrapping_add(1);
    j.spinner_frame = j.spinner_frame.wrapping_add(1);
    assert_eq!(j.spinner_frame, 2);
  }
}
