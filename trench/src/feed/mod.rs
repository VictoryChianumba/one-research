//! `FeedModel` — composition-root state owner for the feed pane.
//!
//! Slice 1 PR 2 moves feed-related fields off `App` into this struct.
//! Subsequent PRs wire `Action` routing (PR 3), introduce `pre_draw` +
//! read-only render (PR 4), and add cross-pane `Action` emission (PR 5).
//!
//! Fields are `pub` during the migration so call sites read
//! `app.feed.feed_tab` without further wrapping. Accessor methods land
//! when PR 3 introduces `Action` routing.
//!
//! Contract and rationale: `docs/adr/ADR-001-render-purification.md`.
//! Vocabulary: `docs/CONTEXT.md`.

use std::collections::HashSet;

use crate::app::{DiscoveryState, FeedTab, FilterState};

/// Owned state for the feed pane. Renders take `&FeedModel`, never `&mut`
/// (post-PR 4). PR 2 leaves call sites mutating fields directly through
/// `&mut app.feed.*` — render purification arrives in PR 4.
pub struct FeedModel {
  pub feed_tab: FeedTab,

  // Per-tab list cursors. Each owns the "selection-stays-visible" invariant.
  pub inbox_list: crate::primitives::ListState,
  pub library_list: crate::primitives::ListState,
  pub history_list: crate::primitives::ListState,

  // Tab-specific filter chips.
  pub library_filter: crate::library::LibraryFilter,
  pub history_filter: crate::history::HistoryFilter,

  // Library bulk-select state.
  pub library_visual_mode: bool,
  pub library_visual_anchor: usize,
  pub library_selected_urls: HashSet<String>,

  // Search (top bar). `search_query_lower` is the cached lowercase mirror —
  // populated by the search-mutator helpers so the visible-items filter pass
  // skips a per-frame `to_lowercase` heap alloc.
  pub search_query: String,
  pub search_query_lower: String,
  pub search_active: bool,

  // Filter panel state.
  pub filter_focus: bool,
  pub filter_cursor: usize,
  pub active_filters: FilterState,

  // Discovery sub-state (Q4 sub-model decision; lifted as DiscoveryModel
  // in a later slice if it grows enough surface to deserve its own seam).
  pub discovery: DiscoveryState,
}

impl FeedModel {
  /// Production constructor — performs disk I/O to hydrate the discovery
  /// cache and the session log. `App::new` calls this.
  pub fn new() -> Self {
    let mut model = Self::default();
    model.discovery.items = crate::store::discovery_cache::load();
    model.discovery.session = crate::store::session::load();
    model
  }

  /// The currently-selected feed tab.
  pub fn feed_tab(&self) -> FeedTab {
    self.feed_tab
  }
}

impl Default for FeedModel {
  /// Side-effect-free defaults. Tests build a `FeedModel::default()`
  /// without touching disk; production code uses `FeedModel::new()`.
  fn default() -> Self {
    Self {
      feed_tab: FeedTab::Inbox,
      inbox_list: crate::primitives::ListState::new(),
      library_list: crate::primitives::ListState::new(),
      history_list: crate::primitives::ListState::new(),
      library_filter: crate::library::LibraryFilter::default(),
      history_filter: crate::history::HistoryFilter::default(),
      library_visual_mode: false,
      library_visual_anchor: 0,
      library_selected_urls: HashSet::new(),
      search_query: String::new(),
      search_query_lower: String::new(),
      search_active: false,
      filter_focus: false,
      filter_cursor: 0,
      active_filters: FilterState::new(),
      discovery: DiscoveryState::default(),
    }
  }
}

/// Per-frame, read-only context passed into feed-pane renders alongside
/// `&FeedModel`. Lands properly in PR 4 once the render flip happens —
/// will carry `&Workspace`, the active theme, and the current `Viewport`.
pub struct FeedContext;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_feed_tab_is_inbox() {
    let model = FeedModel::default();
    assert!(model.feed_tab() == FeedTab::Inbox);
  }

  #[test]
  fn default_search_is_empty_and_inactive() {
    let model = FeedModel::default();
    assert!(model.search_query.is_empty());
    assert!(model.search_query_lower.is_empty());
    assert!(!model.search_active);
  }

  #[test]
  fn default_filter_panel_is_unfocused_at_origin() {
    let model = FeedModel::default();
    assert!(!model.filter_focus);
    assert_eq!(model.filter_cursor, 0);
  }

  #[test]
  fn default_library_visual_mode_is_off() {
    let model = FeedModel::default();
    assert!(!model.library_visual_mode);
    assert_eq!(model.library_visual_anchor, 0);
    assert!(model.library_selected_urls.is_empty());
  }
}
