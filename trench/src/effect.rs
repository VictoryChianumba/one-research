//! Cross-surface effects emitted by surface action handlers, drained
//! and routed by the orchestrator.
//!
//! Phase 3 vocabulary is concentrated on cache-invalidation semantics —
//! the load-bearing test for whether effect routing can preserve the
//! cache contracts that the prior chokepoint mutators encoded directly.
//! Each variant names a *semantic event*, not a cache to invalidate;
//! the cache layer in `app/caches.rs` translates each event into the
//! correct set of invalidator calls.
//!
//! Variants accrete as more surfaces emit. Phase 4 adds rendering /
//! focus / notification effects.

use crate::models::WorkflowState;

#[derive(Debug, Clone)]
pub enum Effect {
  /// The user changed the search query (via push/pop/clear/set).
  /// Observer invalidates: visible_cache, filtered_history_cache.
  SearchQueryChanged,

  /// An item's workflow state transitioned (Inbox → Read, etc.).
  /// Observer invalidates: visible_cache, counts_cache.
  WorkflowStateChanged { url: String, state: WorkflowState },

  /// History was mutated — entry added, removed, or modified in place.
  /// Observer invalidates: filtered_history_cache.
  HistoryMutated,

  /// The history time-window filter changed.
  /// Observer invalidates: filtered_history_cache.
  HistoryFilterChanged,

  /// The library workflow-state chip filter changed.
  /// Observer invalidates: visible_cache.
  LibraryFilterChanged,

  /// One or more `active_filters` (source/signal/content type) changed.
  /// Observer invalidates: visible_cache, filter_summary_cache,
  /// filtered_history_cache.
  FiltersChanged,

  /// Items were merged into the corpus from a fetch / discovery /
  /// reopen path. Observer invalidates: visible_cache plus all
  /// items-derived caches (counts, filter_source_names).
  ItemsChanged,

  /// Sources configuration toggled (arxiv categories, RSS feeds,
  /// predefined sources). Observer invalidates: visible_cache.
  SourcesEnabledChanged,

  /// Tags applied to or removed from items.
  /// Observer invalidates: visible_cache.
  TagsChanged,
}
