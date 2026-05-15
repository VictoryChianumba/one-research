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
use crate::ui::Viewport;

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

  // ── Pre-draw ──────────────────────────────────────────────────────────
  // Run once per frame after layout knows the viewport, before render.
  // Owns layout-derived list-state reconciliation so render stays read-
  // only. See ADR-001 D3 and Q5 in the slice-1 grilling.

  /// Reconcile the active list's `count` + `viewport` against the current
  /// layout, then apply a 2-item bottom buffer so the cursor never lands
  /// at the absolute last visible row when more items lie below.
  ///
  /// - `viewport`: the height-and-width context for this frame.
  /// - `total_items`: total count of items the active tab would render
  ///   (caller resolves: `visible_count()` for non-history tabs,
  ///   `filtered_history().len()` for History).
  /// - `items_fitting_in_viewport`: how many items actually fit given
  ///   variable per-item heights (caller computes via textwrap; lives
  ///   outside the model because it depends on `Workspace`).
  pub fn pre_draw(
    &mut self,
    viewport: Viewport,
    total_items: usize,
    items_fitting_in_viewport: usize,
  ) {
    let rows = viewport.rows as usize;
    let list = match self.feed_tab {
      FeedTab::Inbox => &mut self.inbox_list,
      FeedTab::Library => &mut self.library_list,
      FeedTab::Discoveries => &mut self.discovery.list,
      FeedTab::History => &mut self.history_list,
    };
    // ListState.set_count + set_viewport call ensure_visible internally,
    // so the basic "selection-on-screen" invariant is restored here.
    list.set_count(total_items);
    list.set_viewport(rows);

    // 2-item bottom buffer (preserves pre-PR-4 visual behaviour: the
    // cursor scrolls forward when within 2 items of the bottom edge,
    // provided more items lie below the current window).
    let selected = list.selected();
    let offset = list.offset();
    if items_fitting_in_viewport >= 2
      && selected >= offset + items_fitting_in_viewport.saturating_sub(2)
      && offset + items_fitting_in_viewport < total_items
    {
      let new_offset =
        (selected + 2).saturating_sub(items_fitting_in_viewport);
      list.set_offset(new_offset);
    }
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

  // ── pre_draw ──────────────────────────────────────────────────────────

  #[test]
  fn pre_draw_makes_selection_visible_when_offscreen() {
    // Set up: selection at item 50, but offset forced to 0 (out of viewport).
    let mut m = FeedModel::default();
    m.inbox_list.set_count(100);
    m.inbox_list.set_viewport(20);
    m.inbox_list.set_selected(50);
    m.inbox_list.set_offset(0);
    assert_eq!(m.inbox_list.offset(), 0);

    m.pre_draw(Viewport::new(80, 20), 100, 20);

    // After pre_draw the cursor must lie within the viewport.
    let offset = m.inbox_list.offset();
    assert!(offset <= 50, "offset {offset} should not exceed selected 50");
    assert!(offset + 20 > 50, "viewport [{offset}..{}) must cover 50", offset + 20);
  }

  #[test]
  fn pre_draw_preserves_offset_when_selection_in_viewport() {
    let mut m = FeedModel::default();
    m.inbox_list.set_count(100);
    m.inbox_list.set_viewport(20);
    m.inbox_list.set_selected(5);
    m.inbox_list.set_offset(0);

    m.pre_draw(Viewport::new(80, 20), 100, 20);

    // Selection 5 is well inside [0, 20) — and the 2-item buffer doesn't
    // fire because 5 is far from the bottom edge. Offset stays at 0.
    assert_eq!(m.inbox_list.offset(), 0);
  }

  #[test]
  fn pre_draw_two_item_buffer_advances_offset_near_bottom() {
    // viewport fits 10 items; selection at row 9 (one short of bottom edge);
    // many more items follow → buffer should kick in.
    let mut m = FeedModel::default();
    m.inbox_list.set_count(50);
    m.inbox_list.set_viewport(10);
    m.inbox_list.set_offset(0);
    m.inbox_list.set_selected(9);

    m.pre_draw(Viewport::new(80, 10), 50, 10);

    // Buffer fires: new_offset = 9 + 2 - 10 = 1.
    assert_eq!(m.inbox_list.offset(), 1);
  }

  #[test]
  fn pre_draw_two_item_buffer_does_not_run_off_the_end() {
    // Selection near the very end — no more items below, buffer should
    // not advance past the end of the list.
    let mut m = FeedModel::default();
    m.inbox_list.set_count(15);
    m.inbox_list.set_viewport(10);
    m.inbox_list.set_offset(5);
    m.inbox_list.set_selected(14);

    m.pre_draw(Viewport::new(80, 10), 15, 10);

    // offset 5 + items_fitting 10 == 15 == total_items, so buffer guard
    // `offset + items_fitting < total_items` blocks the advance.
    // ensure_visible in set_viewport pulls offset to 14+1-10 = 5. Stable.
    assert_eq!(m.inbox_list.offset(), 5);
  }

  #[test]
  fn pre_draw_dispatches_by_feed_tab() {
    // Library tab should reconcile library_list, not inbox_list.
    let mut m = FeedModel::default();
    m.feed_tab = FeedTab::Library;
    m.library_list.set_count(100);
    m.library_list.set_viewport(10);
    m.library_list.set_selected(50);
    m.library_list.set_offset(0);

    m.pre_draw(Viewport::new(80, 10), 100, 10);

    // Library list was reconciled; inbox_list is untouched.
    assert!(m.library_list.offset() > 0);
    assert_eq!(m.inbox_list.offset(), 0);
  }
}
