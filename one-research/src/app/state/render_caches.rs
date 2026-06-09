//! `RenderCaches` — five RefCell-backed memoization caches consulted
//! by the render layer, plus the [`Effect`] observer that knows which
//! caches each semantic event invalidates.
//!
//! ## Caches
//!
//! | Field | Holds | Invalidated by |
//! |---|---|---|
//! | [`visible`] | indices of items visible under the current search/filter, keyed by `(FeedTab, …)` | every items, workflow, search, library/source/tags filter mutation |
//! | [`counts`] | workflow breakdown + recent-48h + queue preview | items + workflow mutations |
//! | [`filter_source_names`] | sorted unique source-label set used by the filter panel | items mutations (changes which sources exist) |
//! | [`filter_summary`] | human-readable summary of `active_filters` | filters mutations only — does NOT depend on items |
//! | [`filtered_history`] | indices of `Workspace.history` entries under the current time-window + search + filters | history + search + history-filter + filters mutations |
//!
//! ## The observer
//!
//! Pre-grouping, surfaces emitted [`Effect`]s and `App::observe_effect`
//! matched each variant to per-cache invalidator calls.  That match is
//! the *contract* between effects and caches: adding a new cache means
//! updating one place, not auditing every emit site.
//!
//! ADR-009 cluster #5 moves the contract onto this struct.
//! `App::route_effects` now delegates to [`RenderCaches::observe`].
//!
//! Introduced by [ADR-009](../../../../docs/adr/ADR-009-app-field-grouping.md).
//! Replaces 5 flat `App` fields (`visible_cache` and 4 siblings) plus
//! the 6 invalidator methods that lived in `app/caches.rs::impl App`.
//!
//! ## Test surface
//!
//! Pre-grouping the effect→cache routing had **zero tests**.  PR 5 adds
//! a witness per Effect variant: the test pre-populates every cache,
//! calls `observe(&effect)`, then asserts the expected subset of caches
//! is now `None` and the unexpected subset is unchanged.  Adding a new
//! Effect variant or changing the routing table fails one of these.

use std::cell::RefCell;

use crate::effect::Effect;

use super::feed::{FeedTab, ItemCounts};

/// See module-level docs.
pub struct RenderCaches {
  /// Cached indices of items visible under the current search/filter.
  /// Keyed by `FeedTab` so a tab switch automatically misses the cache.
  pub visible: RefCell<Option<(FeedTab, Vec<usize>)>>,
  /// Memoized item counts (workflow breakdown + recent-48h + queue preview).
  pub counts: RefCell<Option<ItemCounts>>,
  /// Memoized sorted unique source-label set used by the filter panel.
  /// Invalidated alongside `counts`.
  pub filter_source_names: RefCell<Option<Vec<String>>>,
  /// Memoized filter-summary string.  Invalidated only by `active_filters`
  /// mutation — does NOT depend on items or search query.
  pub filter_summary: RefCell<Option<String>>,
  /// Memoized indices of `Workspace.history` entries visible under the
  /// current time window + search + active filters.
  pub filtered_history: RefCell<Option<Vec<usize>>>,
}

impl Default for RenderCaches {
  fn default() -> Self {
    Self {
      visible: RefCell::new(None),
      counts: RefCell::new(None),
      filter_source_names: RefCell::new(None),
      filter_summary: RefCell::new(None),
      filtered_history: RefCell::new(None),
    }
  }
}

impl RenderCaches {
  // ── Per-cache invalidators ──────────────────────────────────────────

  pub fn invalidate_visible(&self) {
    *self.visible.borrow_mut() = None;
  }

  pub fn invalidate_counts(&self) {
    *self.counts.borrow_mut() = None;
  }

  pub fn invalidate_filter_source_names(&self) {
    *self.filter_source_names.borrow_mut() = None;
  }

  pub fn invalidate_filter_summary(&self) {
    *self.filter_summary.borrow_mut() = None;
  }

  pub fn invalidate_filtered_history(&self) {
    *self.filtered_history.borrow_mut() = None;
  }

  /// Aggregate invalidator for every cache that derives from items in
  /// `Workspace.items_store`.
  pub fn invalidate_items_derived(&self) {
    self.invalidate_counts();
    self.invalidate_filter_source_names();
  }

  // ── Effect routing ──────────────────────────────────────────────────

  /// Translate one [`Effect`] into the correct set of cache
  /// invalidations.  The match is the *contract* between effects and
  /// caches; adding a new cache or a new Effect variant means updating
  /// one match arm here, not auditing every emit site.
  pub fn observe(&self, effect: &Effect) {
    match effect {
      Effect::SearchQueryChanged => {
        self.invalidate_visible();
        self.invalidate_filtered_history();
      }
      Effect::WorkflowStateChanged { .. } => {
        self.invalidate_visible();
        self.invalidate_counts();
      }
      Effect::HistoryMutated | Effect::HistoryFilterChanged => {
        self.invalidate_filtered_history();
      }
      Effect::LibraryFilterChanged
      | Effect::SourcesEnabledChanged
      | Effect::TagsChanged => {
        self.invalidate_visible();
      }
      Effect::FiltersChanged => {
        self.invalidate_visible();
        self.invalidate_filter_summary();
        self.invalidate_filtered_history();
      }
      Effect::ItemsChanged => {
        self.invalidate_visible();
        self.invalidate_items_derived();
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::WorkflowState;

  /// Pre-populate every cache with a sentinel value so the witness
  /// tests can distinguish "still cached" from "invalidated".
  fn primed() -> RenderCaches {
    let c = RenderCaches::default();
    *c.visible.borrow_mut() = Some((FeedTab::Inbox, vec![1, 2]));
    *c.counts.borrow_mut() = Some(ItemCounts::default());
    *c.filter_source_names.borrow_mut() = Some(vec!["arXiv".into()]);
    *c.filter_summary.borrow_mut() = Some("source: arxiv".into());
    *c.filtered_history.borrow_mut() = Some(vec![0, 1, 2]);
    c
  }

  fn populated(c: &RenderCaches) -> (bool, bool, bool, bool, bool) {
    (
      c.visible.borrow().is_some(),
      c.counts.borrow().is_some(),
      c.filter_source_names.borrow().is_some(),
      c.filter_summary.borrow().is_some(),
      c.filtered_history.borrow().is_some(),
    )
  }

  #[test]
  fn default_caches_are_all_unpopulated() {
    assert_eq!(
      populated(&RenderCaches::default()),
      (false, false, false, false, false)
    );
  }

  #[test]
  fn primed_helper_populates_every_cache() {
    // Sanity check on the test fixture itself — every later test
    // relies on `primed()` populating all five.
    assert_eq!(populated(&primed()), (true, true, true, true, true));
  }

  // ── Per-Effect routing witnesses ────────────────────────────────────
  //
  // One test per Effect variant. Each pre-populates everything, fires
  // the effect once, then asserts the documented set of caches was
  // invalidated (false) and the rest survived (true).  This is the
  // first behavioural test for the effect→cache contract; the prior
  // observe_effect lived in caches.rs with no test coverage at all.

  #[test]
  fn observe_search_query_changed_invalidates_visible_and_history() {
    let c = primed();
    c.observe(&Effect::SearchQueryChanged);
    assert_eq!(populated(&c), (false, true, true, true, false));
  }

  #[test]
  fn observe_workflow_state_changed_invalidates_visible_and_counts() {
    let c = primed();
    c.observe(&Effect::WorkflowStateChanged {
      url: "u".into(),
      state: WorkflowState::Inbox,
    });
    assert_eq!(populated(&c), (false, false, true, true, true));
  }

  #[test]
  fn observe_history_mutated_invalidates_only_history() {
    let c = primed();
    c.observe(&Effect::HistoryMutated);
    assert_eq!(populated(&c), (true, true, true, true, false));
  }

  #[test]
  fn observe_history_filter_changed_invalidates_only_history() {
    let c = primed();
    c.observe(&Effect::HistoryFilterChanged);
    assert_eq!(populated(&c), (true, true, true, true, false));
  }

  #[test]
  fn observe_library_filter_changed_invalidates_only_visible() {
    let c = primed();
    c.observe(&Effect::LibraryFilterChanged);
    assert_eq!(populated(&c), (false, true, true, true, true));
  }

  #[test]
  fn observe_sources_enabled_changed_invalidates_only_visible() {
    let c = primed();
    c.observe(&Effect::SourcesEnabledChanged);
    assert_eq!(populated(&c), (false, true, true, true, true));
  }

  #[test]
  fn observe_tags_changed_invalidates_only_visible() {
    let c = primed();
    c.observe(&Effect::TagsChanged);
    assert_eq!(populated(&c), (false, true, true, true, true));
  }

  #[test]
  fn observe_filters_changed_invalidates_three_caches() {
    let c = primed();
    c.observe(&Effect::FiltersChanged);
    assert_eq!(populated(&c), (false, true, true, false, false));
  }

  #[test]
  fn observe_items_changed_invalidates_visible_and_items_derived() {
    let c = primed();
    c.observe(&Effect::ItemsChanged);
    assert_eq!(populated(&c), (false, false, false, true, true));
  }
}
