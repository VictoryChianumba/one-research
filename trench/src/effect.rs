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
  /// Observer invalidates: render_caches.visible, render_caches.filtered_history.
  SearchQueryChanged,

  /// An item's workflow state transitioned (Inbox → Read, etc.).
  /// Observer invalidates: render_caches.visible, render_caches.counts.
  /// Fields carried for future cache-invalidation precision (per-URL /
  /// per-state); observers today drain the whole cache. ADR-001 Effect
  /// vocabulary — read by the typed envelope, not the consumer match.
  #[allow(dead_code)]
  WorkflowStateChanged { url: String, state: WorkflowState },

  /// History was mutated — entry added, removed, or modified in place.
  /// Observer invalidates: render_caches.filtered_history.
  HistoryMutated,

  /// The history time-window filter changed.
  /// Observer invalidates: render_caches.filtered_history.
  HistoryFilterChanged,

  /// The library workflow-state chip filter changed.
  /// Observer invalidates: render_caches.visible.
  LibraryFilterChanged,

  /// One or more `active_filters` (source/signal/content type) changed.
  /// Observer invalidates: render_caches.visible, render_caches.filter_summary,
  /// render_caches.filtered_history.
  FiltersChanged,

  /// Items were merged into the corpus from a fetch / discovery /
  /// reopen path. Observer invalidates: render_caches.visible plus all
  /// items-derived caches (counts, filter_source_names).
  ItemsChanged,

  /// Sources configuration toggled (arxiv categories, RSS feeds,
  /// predefined sources). Observer invalidates: render_caches.visible.
  SourcesEnabledChanged,

  /// Tags applied to or removed from items.
  /// Observer invalidates: render_caches.visible.
  TagsChanged,
}
