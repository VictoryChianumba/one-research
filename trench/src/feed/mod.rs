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

  // ── Tab navigation ────────────────────────────────────────────────────
  // visible_cache is keyed by `feed_tab`, so a tab switch is a natural
  // cache miss — these methods do not emit `Effect`s. Callers (key
  // handlers) follow up with `app.reset_active_feed_position()` when the
  // gesture should also reset the active list cursor.

  /// Advance to the next tab (Inbox → Library → Discoveries → History → Inbox).
  pub fn cycle_tab(&mut self) {
    self.feed_tab = match self.feed_tab {
      FeedTab::Inbox => FeedTab::Library,
      FeedTab::Library => FeedTab::Discoveries,
      FeedTab::Discoveries => FeedTab::History,
      FeedTab::History => FeedTab::Inbox,
    };
  }

  /// Walk back one tab (reverse of `cycle_tab`).
  pub fn cycle_tab_back(&mut self) {
    self.feed_tab = match self.feed_tab {
      FeedTab::Inbox => FeedTab::History,
      FeedTab::Library => FeedTab::Inbox,
      FeedTab::Discoveries => FeedTab::Library,
      FeedTab::History => FeedTab::Discoveries,
    };
  }

  /// Jump directly to a specific tab.
  pub fn set_tab(&mut self, tab: FeedTab) {
    self.feed_tab = tab;
  }

  // ── Search bar ────────────────────────────────────────────────────────
  // Search text edits go through App's `push_search_char` / `pop_search_char`
  // chokepoints (they emit `Effect::SearchQueryChanged`). These methods
  // only toggle the *active* flag — entering/exiting search mode.

  /// Enter search-bar input mode.
  pub fn enter_search(&mut self) {
    self.search_active = true;
  }

  /// Exit search-bar input mode. Does not clear the query.
  pub fn exit_search(&mut self) {
    self.search_active = false;
  }

  // ── Filter panel ──────────────────────────────────────────────────────

  /// Move keyboard focus into the filter panel.
  pub fn enter_filter_focus(&mut self) {
    self.filter_focus = true;
  }

  /// Move keyboard focus out of the filter panel.
  pub fn exit_filter_focus(&mut self) {
    self.filter_focus = false;
  }

  // ── Library visual-select mode ────────────────────────────────────────
  // Anchor is captured at entry from the current `library_list` cursor.
  // Selection always covers the contiguous range from anchor to cursor.

  /// Enter library bulk-select mode; anchor at current library cursor.
  pub fn enter_library_visual_mode(&mut self) {
    self.library_visual_mode = true;
    self.library_visual_anchor = self.library_list.selected();
  }

  /// Exit library bulk-select mode; clear the per-URL selection set.
  pub fn exit_library_visual_mode(&mut self) {
    self.library_visual_mode = false;
    self.library_selected_urls.clear();
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

  #[test]
  fn cycle_tab_walks_forward_and_wraps() {
    let mut m = FeedModel::default();
    m.cycle_tab();
    assert!(m.feed_tab == FeedTab::Library);
    m.cycle_tab();
    assert!(m.feed_tab == FeedTab::Discoveries);
    m.cycle_tab();
    assert!(m.feed_tab == FeedTab::History);
    m.cycle_tab();
    assert!(m.feed_tab == FeedTab::Inbox);
  }

  #[test]
  fn cycle_tab_back_walks_backward_and_wraps() {
    let mut m = FeedModel::default();
    m.cycle_tab_back();
    assert!(m.feed_tab == FeedTab::History);
    m.cycle_tab_back();
    assert!(m.feed_tab == FeedTab::Discoveries);
    m.cycle_tab_back();
    assert!(m.feed_tab == FeedTab::Library);
    m.cycle_tab_back();
    assert!(m.feed_tab == FeedTab::Inbox);
  }

  #[test]
  fn set_tab_jumps_directly() {
    let mut m = FeedModel::default();
    m.set_tab(FeedTab::Discoveries);
    assert!(m.feed_tab == FeedTab::Discoveries);
    m.set_tab(FeedTab::Inbox);
    assert!(m.feed_tab == FeedTab::Inbox);
  }

  #[test]
  fn search_mode_toggles() {
    let mut m = FeedModel::default();
    assert!(!m.search_active);
    m.enter_search();
    assert!(m.search_active);
    m.exit_search();
    assert!(!m.search_active);
  }

  #[test]
  fn exit_search_preserves_query_text() {
    // Exiting search mode is a focus change, not a clear.
    let mut m = FeedModel::default();
    m.search_query = "transformers".to_string();
    m.search_query_lower = "transformers".to_string();
    m.enter_search();
    m.exit_search();
    assert_eq!(m.search_query, "transformers");
    assert_eq!(m.search_query_lower, "transformers");
  }

  #[test]
  fn filter_focus_toggles() {
    let mut m = FeedModel::default();
    assert!(!m.filter_focus);
    m.enter_filter_focus();
    assert!(m.filter_focus);
    m.exit_filter_focus();
    assert!(!m.filter_focus);
  }

  #[test]
  fn library_visual_mode_captures_anchor_at_entry() {
    let mut m = FeedModel::default();
    m.library_list.set_count(50);
    m.library_list.set_viewport(20);
    m.library_list.set_selected(17);
    m.enter_library_visual_mode();
    assert!(m.library_visual_mode);
    assert_eq!(m.library_visual_anchor, 17);
  }

  #[test]
  fn exit_library_visual_mode_clears_selection() {
    let mut m = FeedModel::default();
    m.library_selected_urls.insert("https://example.com/a".into());
    m.library_selected_urls.insert("https://example.com/b".into());
    m.library_visual_mode = true;
    m.exit_library_visual_mode();
    assert!(!m.library_visual_mode);
    assert!(m.library_selected_urls.is_empty());
  }
}
