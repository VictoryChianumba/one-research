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

use fuzzy_matcher::skim::SkimMatcherV2;

use crate::app::{DiscoveryModel, FeedTab, FilterState};
// C7 PR 2 (ADR-005 §S2): `discovery: DiscoveryModel` field moved from
// `FeedModel.discovery` to `App.discovery`. Methods and free fns that
// previously read `self.discovery` now take `&DiscoveryModel` /
// `&mut DiscoveryModel` parameters — dispatch on `FeedTab` stays here,
// but the data lives elsewhere.
use crate::ui::Viewport;

/// Sort key applied across the feed pane (ADR-011 §E3). Mutually
/// exclusive — exactly one is active at any time. Stacks on top of the
/// existing source / workflow-state / signal filters.
///
/// `Dated` is the default and the historical behaviour. `Random` /
/// `Popular` / `Trending` are session-only (not persisted to disk —
/// per ADR-011 §E3 the launch-time surprise of a restored random
/// shuffle outweighs the cost of re-selection).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeedSortMode {
  /// `published_at` descending. The historical default.
  Dated,
  /// Deterministic shuffle keyed off a per-session seed. Stable
  /// within a session so re-renders don't reorder.
  Random,
  /// `upvote_count` descending. HuggingFace items sort naturally;
  /// arXiv items (upvote_count = 0) sink to the bottom unless
  /// enriched with Semantic Scholar citation counts.
  Popular,
  /// Items published in the last 14 days, sorted by `upvote_count`
  /// descending. Older items are filtered out entirely.
  Trending,
}

impl FeedSortMode {
  /// Human-readable label for the filter pane.
  pub fn label(self) -> &'static str {
    match self {
      FeedSortMode::Dated => "Dated",
      FeedSortMode::Random => "Random",
      FeedSortMode::Popular => "Popular",
      FeedSortMode::Trending => "Trending",
    }
  }
}

/// Owned state for the feed pane. Renders take `&FeedModel`, never `&mut`
/// (post-PR 4). PR 2 leaves call sites mutating fields directly through
/// `&mut app.feed.*` — render purification arrives in PR 4.
pub struct FeedModel {
  pub feed_tab: FeedTab,

  // Per-tab list cursors. Each owns the "selection-stays-visible" invariant.
  pub inbox_list: crate::primitives::ListState,
  pub library_list: crate::primitives::ListState,
  pub history_list: crate::primitives::ListState,

  // Tab-specific filter views.
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
  /// An online arXiv search is in flight (submitted from the search bar).
  /// Gates duplicate submits and keeps the event loop at the interactive
  /// cadence so the results merge promptly (see `has_active_animation`).
  pub search_loading: bool,

  // Filter panel state.
  pub filter_focus: bool,
  pub filter_cursor: usize,
  pub active_filters: FilterState,

  // ADR-011 §E3 — sort mode applied across every tab. Session-only,
  // resets to Dated on launch.
  pub sort_mode: FeedSortMode,
  // ADR-011 §E4 — when true, the Subject Browser rail's current
  // drill point narrows the visible items. Default false; toggled
  // from the filter pane or via the `F` quick-toggle in Browse.
  pub subject_follow: bool,
  // Per-session seed for FeedSortMode::Random. Stable so re-renders
  // produce the same shuffle order until next launch or until the
  // user explicitly re-shuffles from the filter pane.
  pub random_seed: u64,
}

impl FeedModel {
  /// Production constructor. Side-effect-free now that discovery cache
  /// hydration moved to `App::new` (C7 PR 2). Kept as a distinct
  /// constructor so callers can stay symmetric with the other models
  /// and so future feed-only I/O has a home.
  pub fn new() -> Self {
    Self::default()
  }

  /// The list cursor for the currently-active tab. Renders read
  /// `.selected()` / `.offset()` through this instead of round-tripping
  /// `App::active_selected_index()` / `App::active_list_offset()` —
  /// keeps the render path free of `&App`.
  ///
  /// Takes `&DiscoveryModel` because the Discoveries-tab cursor lives
  /// there post-C7 PR 2 (ADR-005 §S2). The tab dispatch stays here, the
  /// data lives there.
  pub fn active_list<'a>(
    &'a self,
    discovery: &'a DiscoveryModel,
  ) -> &'a crate::primitives::ListState {
    match self.feed_tab {
      FeedTab::Inbox => &self.inbox_list,
      FeedTab::Library => &self.library_list,
      FeedTab::Discoveries => &discovery.list,
      // Browse owns four column-cursors in BrowseModel; the single-list
      // helpers here are inert for that tab. Returning inbox_list as a
      // placeholder is safe because handle_browse_tab intercepts every
      // navigation gesture before active_list is consulted (same trick
      // used by History at line 419 returning `&[]` for items_for_tab).
      FeedTab::Browse => &self.inbox_list,
      FeedTab::History => &self.history_list,
    }
  }

  // ── Tab navigation ────────────────────────────────────────────────────
  // render_caches.visible is keyed by `feed_tab`, so a tab switch is a natural
  // cache miss — these methods do not emit `Effect`s. Callers (key
  // handlers) follow up with `app.reset_active_feed_position()` when the
  // gesture should also reset the active list cursor.

  /// Advance to the next tab (Inbox → Browse → Library → Discoveries → History → Inbox).
  pub fn cycle_tab(&mut self) {
    self.feed_tab = match self.feed_tab {
      FeedTab::Inbox => FeedTab::Browse,
      FeedTab::Browse => FeedTab::Library,
      FeedTab::Library => FeedTab::Discoveries,
      FeedTab::Discoveries => FeedTab::History,
      FeedTab::History => FeedTab::Inbox,
    };
  }

  /// Walk back one tab (reverse of `cycle_tab`).
  pub fn cycle_tab_back(&mut self) {
    self.feed_tab = match self.feed_tab {
      FeedTab::Inbox => FeedTab::History,
      FeedTab::Browse => FeedTab::Inbox,
      FeedTab::Library => FeedTab::Browse,
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

  /// Exit library bulk-select mode; clear the per-URL selection set and
  /// reset the anchor.
  pub fn exit_library_visual_mode(&mut self) {
    self.library_visual_mode = false;
    self.library_visual_anchor = 0;
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
    discovery: &mut DiscoveryModel,
    viewport: Viewport,
    total_items: usize,
    items_fitting_in_viewport: usize,
  ) {
    let rows = viewport.rows as usize;
    let list = match self.feed_tab {
      FeedTab::Inbox => &mut self.inbox_list,
      FeedTab::Library => &mut self.library_list,
      FeedTab::Discoveries => &mut discovery.list,
      // See FeedTab::Browse note in active_list — column cursors live on
      // BrowseModel, the single-list pre-draw is inert for this tab.
      FeedTab::Browse => &mut self.inbox_list,
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
      let new_offset = (selected + 2).saturating_sub(items_fitting_in_viewport);
      list.set_offset(new_offset);
    }
  }

  // ── W3 hybrid: state-local gestures ───────────────────────────────────
  // Per ADR-001 D5: model methods take `&mut Workspace` directly and emit
  // `Vec<Effect>`. The caller routes effects to the cache observer and
  // persists to disk. Borrow conflicts at the call site are resolved by
  // split borrow on `App` (different fields: feed, workspace, config).

  /// The URL of the item the user's cursor currently points to in the
  /// active tab, or `None` if the visible list is empty / cursor is past
  /// the end. Used by single-item workflow gestures to resolve the
  /// target before mutation.
  pub fn selected_url(
    &self,
    workspace: &crate::data::workspace_store::Workspace,
    discovery: &DiscoveryModel,
    browse: &crate::app::BrowseModel,
    config: &crate::config::Config,
  ) -> Option<String> {
    let visible =
      visible_indices_for(workspace, self, discovery, browse, config);
    let selected = self.active_list(discovery).selected();
    let idx = *visible.get(selected)?;
    let items = items_for_tab(workspace, self, discovery);
    items.get(idx).map(|item| item.url.clone())
  }

  /// Set the workflow state of the item with the given URL, wherever it
  /// lives (workspace.items for Inbox/Library, discovery.items for
  /// Discoveries). Updates `workspace.persisted_states` and returns the
  /// `Effect::WorkflowStateChanged` event for the caller to route.
  ///
  /// Empty Vec when the URL doesn't match any item (no-op, no event).
  pub fn set_workflow_state_for_url(
    &mut self,
    workspace: &mut crate::data::workspace_store::Workspace,
    discovery: &mut DiscoveryModel,
    url: &str,
    state: crate::models::WorkflowState,
  ) -> Vec<crate::effect::Effect> {
    let mut found = false;
    for item in workspace.items_store.iter_mut() {
      if item.url == url {
        item.workflow_state = state;
        found = true;
        break;
      }
    }
    if !found {
      for item in discovery.items.iter_mut() {
        if item.url == url {
          item.workflow_state = state;
          found = true;
          break;
        }
      }
    }
    if found {
      workspace.persisted_states.insert(url.to_string(), state);
      vec![crate::effect::Effect::WorkflowStateChanged {
        url: url.to_string(),
        state,
      }]
    } else {
      Vec::new()
    }
  }

  /// Set the workflow state of the cursor-pointed item. Combines
  /// `selected_url` + `set_workflow_state_for_url`. No-op (empty Vec)
  /// when the cursor isn't over a visible item.
  pub fn set_workflow_state_at_cursor(
    &mut self,
    workspace: &mut crate::data::workspace_store::Workspace,
    discovery: &mut DiscoveryModel,
    browse: &crate::app::BrowseModel,
    config: &crate::config::Config,
    state: crate::models::WorkflowState,
  ) -> Vec<crate::effect::Effect> {
    let Some(url) = self.selected_url(workspace, discovery, browse, config)
    else {
      return Vec::new();
    };
    self.set_workflow_state_for_url(workspace, discovery, &url, state)
  }

  /// Variable-height variant of [`pre_draw`](Self::pre_draw) for the
  /// narrow-feed list-cell (drawer used by the reader's secondary pane).
  ///
  /// Narrow-feed rows wrap their titles at a width that only the layout
  /// pass knows, so per-item heights live outside the model. The caller
  /// supplies `row_heights_up_to_selected[i] = rendered row count for
  /// item i in 0..=selected`; the model owns the resulting offset
  /// arithmetic (the same reverse-walk that previously lived in
  /// `draw_narrow_feed`).
  ///
  /// - `viewport`: rows is the available height for the list area.
  /// - `total_items`: total count from the active tab.
  /// - `row_heights_up_to_selected`: per-item row counts for items
  ///   `[0, selected]`. Pass an empty slice when `total_items == 0`.
  pub fn pre_draw_narrow_feed(
    &mut self,
    discovery: &mut DiscoveryModel,
    viewport: Viewport,
    total_items: usize,
    row_heights_up_to_selected: &[usize],
  ) {
    let viewport_rows = viewport.rows as usize;
    let list = match self.feed_tab {
      FeedTab::Inbox => &mut self.inbox_list,
      FeedTab::Library => &mut self.library_list,
      FeedTab::Discoveries => &mut discovery.list,
      // See FeedTab::Browse note in active_list — narrow-feed pre-draw
      // is inert for the Subject Browser (it draws its own 4-column UI).
      FeedTab::Browse => &mut self.inbox_list,
      FeedTab::History => &mut self.history_list,
    };
    list.set_count(total_items);
    list.set_viewport(viewport_rows);

    if total_items == 0 || row_heights_up_to_selected.is_empty() {
      return;
    }
    let selected = list.selected();
    // After set_viewport, ensure_visible already restored selection
    // visibility using uniform-height logic. With variable heights that
    // estimate may pack more rows than actually fit — count fit-from-
    // offset using real row heights, then reverse-walk if the selection
    // ended up below the true window.
    let mut offset = list.offset();
    let mut rows_used = 0usize;
    let mut vc = 0usize;
    for &h in row_heights_up_to_selected.iter().skip(offset) {
      if rows_used + h > viewport_rows {
        break;
      }
      rows_used += h;
      vc += 1;
    }
    let vc = vc.max(1);
    if selected >= offset + vc {
      let mut rows_used = 0usize;
      offset = selected;
      for i in (0..=selected).rev() {
        let h = row_heights_up_to_selected[i];
        if rows_used + h > viewport_rows {
          break;
        }
        rows_used += h;
        offset = i;
      }
    }
    offset = offset.min(total_items.saturating_sub(1));
    list.set_offset(offset);
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
      search_loading: false,
      filter_focus: false,
      filter_cursor: 0,
      active_filters: FilterState::new(),
      sort_mode: FeedSortMode::Dated,
      subject_follow: false,
      // Session seed for FeedSortMode::Random. Time-based so each
      // launch produces a fresh shuffle while staying stable within
      // the session.
      random_seed: std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0),
    }
  }
}

/// Per-frame, read-only context passed into feed-pane renders alongside
/// `&mut FeedModel`. The caller (orchestrator) constructs this once per
/// frame from `App`, then hands it to `draw_*` together with the model.
///
/// `visible_indices` is owned (no borrow) — renders pair it with
/// [`items_for_tab`] to look up real `&FeedItem`s. The owned shape
/// avoids the dual-source lifetime problem: `workspace.items` and
/// `discovery.items` can't both back one `Vec<&FeedItem>` without
/// tying its lifetime to both, blocking a `&mut FeedModel` borrow at
/// the dispatcher level (ADR-001 D4 / Rust split-borrow).
///
/// `filtered_history` borrows entries from `workspace.history` only —
/// no ambiguity, so the references can ride straight in the struct.
pub struct FeedContext<'a> {
  pub workspace: &'a crate::data::workspace_store::Workspace,
  pub theme: ui_theme::Theme,
  pub browse_feed_focused: bool,
  pub browse_subject_depth: usize,
  /// Indices into [`items_for_tab`]`(workspace, feed, discovery)` after
  /// applying search + filter + tab-scoping. Empty for the History tab.
  pub visible_indices: Vec<usize>,
  /// History entries after the History tab's filter, in render order.
  pub filtered_history: Vec<&'a crate::history::HistoryEntry>,
}

/// The item slice the current tab is reading from.
/// Inbox + Library + Browse read `workspace.items`; Discoveries reads
/// `discovery.items`; History reads nothing (uses
/// `filtered_history` instead).
///
/// ADR-011 §E1: Browse joins Inbox / Library as a workspace.items
/// consumer. The rail is rendered separately by main_row.rs and
/// scopes the feed via subject_follow (see `visible_indices_for`).
pub fn items_for_tab<'a>(
  workspace: &'a crate::data::workspace_store::Workspace,
  feed: &'a FeedModel,
  discovery: &'a DiscoveryModel,
) -> &'a [crate::models::FeedItem] {
  match feed.feed_tab {
    FeedTab::Inbox | FeedTab::Library | FeedTab::Browse => {
      workspace.items_store.items()
    }
    FeedTab::Discoveries => &discovery.items,
    FeedTab::History => &[],
  }
}

/// Compute the filter+search-applied indices into [`items_for_tab`],
/// then apply the active [`FeedSortMode`].
///
/// Pure read; takes field-scoped borrows so the dispatcher can release
/// them before taking `&mut FeedModel`. The `browse` parameter is read
/// only when `feed.subject_follow` is true and the active tab is
/// `Browse` — the rail's current drill point narrows the visible set
/// (ADR-011 §E1, §E4).
pub fn visible_indices_for(
  workspace: &crate::data::workspace_store::Workspace,
  feed: &FeedModel,
  discovery: &DiscoveryModel,
  browse: &crate::app::BrowseModel,
  config: &crate::config::Config,
) -> Vec<usize> {
  let items = items_for_tab(workspace, feed, discovery);
  let query = crate::search::Query::parse(&feed.search_query);
  let matcher = SkimMatcherV2::default();

  // Subject-follow scope only applies on the Browse tab when the user
  // has actually drilled at least once. At depth 0 (Groups level) the
  // feed stays unfiltered — "drill point" implies you've drilled.
  let subject_scope = if feed.feed_tab == FeedTab::Browse
    && feed.subject_follow
    && !browse.rail_path.is_empty()
  {
    Some(subject_follow_scope(browse))
  } else {
    None
  };

  // Non-search filters first (tab, subject scope, source toggles, tags,
  // tab-local views). Search is applied as a separate scoring pass so a
  // live query can reorder the survivors by relevance.
  let mut scored: Vec<(usize, i64)> = items
    .iter()
    .enumerate()
    .filter(|(_, item)| {
      match feed.feed_tab {
        FeedTab::Inbox => {
          if item.workflow_state != crate::models::WorkflowState::Inbox {
            return false;
          }
        }
        FeedTab::Library => {
          if !feed.library_filter.matches(item.workflow_state) {
            return false;
          }
        }
        // Browse, Discoveries, History: no workflow-state pre-filter
        // (Browse shows items regardless of workflow state — the
        // user's drilling, not workflow-managing).
        _ => {}
      }
      if let Some(scope) = &subject_scope {
        if !scope.matches(item) {
          return false;
        }
      }
      let key =
        if item.source_platform == crate::models::SourcePlatform::HuggingFace {
          "huggingface"
        } else {
          &item.source_name
        };
      if let Some(&enabled) = config.sources.enabled_sources.get(key) {
        if !enabled {
          return false;
        }
      }
      if !feed.active_filters.tags.is_empty() {
        let item_tags = crate::tags::for_url(&workspace.item_tags, &item.url);
        if !item_tags.iter().any(|t| feed.active_filters.tags.contains(t)) {
          return false;
        }
      }
      feed.active_filters.matches(item)
    })
    .filter_map(|(i, item)| {
      if query.is_empty() {
        Some((i, 0))
      } else {
        query.score(item, &matcher).map(|score| (i, score))
      }
    })
    .collect();

  if query.is_empty() {
    let mut indices: Vec<usize> = scored.into_iter().map(|(i, _)| i).collect();
    apply_sort_mode(&mut indices, items, feed.sort_mode, feed.random_seed);
    indices
  } else {
    // Relevance order: best score first. `sort_by` is stable, so equal
    // scores keep items_store's published_at-desc order (newest wins
    // ties) and the active FeedSortMode is intentionally bypassed —
    // when you're searching, match quality is the ordering.
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().map(|(i, _)| i).collect()
  }
}

/// Subject-follow predicate (ADR-011 §E4). Resolved once per call to
/// `visible_indices_for` to avoid re-reading the rail per-item.
enum SubjectScope {
  /// Match items tagged with any code under this archive — either the
  /// bare archive id (e.g. `gr-qc`) or any `archive_id.*` (e.g. `cs.LG`,
  /// `cs.CR`).
  Archive(&'static str),
  /// Match items tagged with exactly this category code (e.g. `math.NT`).
  Category(&'static str),
}

impl SubjectScope {
  fn matches(&self, item: &crate::models::FeedItem) -> bool {
    match self {
      SubjectScope::Archive(id) => {
        let prefix = format!("{id}.");
        item.domain_tags.iter().any(|t| {
          t == id
            || t.starts_with(&prefix)
            || taxonomy_archive_id_for_tag(t).is_some_and(|a| a == *id)
            || taxonomy_category_code_for_tag(t)
              .is_some_and(|c| c == *id || c.starts_with(&prefix))
        })
      }
      SubjectScope::Category(code) => item.domain_tags.iter().any(|t| {
        t == code
          || taxonomy_category_code_for_tag(t).is_some_and(|c| c == *code)
      }),
    }
  }
}

fn taxonomy_category_code_for_tag(tag: &str) -> Option<&'static str> {
  crate::models::arxiv_taxonomy::find_category(tag).map(|c| c.code).or_else(
    || {
      crate::models::arxiv_taxonomy::all_categories()
        .find(|c| c.name.eq_ignore_ascii_case(tag))
        .map(|c| c.code)
    },
  )
}

fn taxonomy_archive_id_for_tag(tag: &str) -> Option<&'static str> {
  crate::models::arxiv_taxonomy::TAXONOMY.iter().find_map(|group| {
    group.archives.iter().find_map(|archive| {
      if archive.id.eq_ignore_ascii_case(tag)
        || archive.name.eq_ignore_ascii_case(tag)
      {
        Some(archive.id)
      } else {
        None
      }
    })
  })
}

fn subject_follow_scope(browse: &crate::app::BrowseModel) -> SubjectScope {
  // depth 1 = at Archives level, cursor on one Archive
  // depth 2 = at Categories level, cursor on one Category
  // (depth 0 short-circuited by visible_indices_for before reaching here)
  if browse.rail_path.len() == 1 {
    if let Some(a) = browse.rail_selected_archive() {
      return SubjectScope::Archive(a.id);
    }
  }
  if let Some(c) = browse.rail_selected_category() {
    return SubjectScope::Category(c.code);
  }
  // Fall back to "match everything" by way of an empty-prefix archive.
  // This branch is structurally unreachable given rail_path is
  // non-empty (subject_follow_scope's pre-condition).
  SubjectScope::Archive("")
}

/// Apply the [`FeedSortMode`] to a filtered index list (ADR-011 §E3).
/// `items` is the slice the indices point into; we read `upvote_count`
/// + `published_at` from it for the non-Dated modes.
fn apply_sort_mode(
  indices: &mut Vec<usize>,
  items: &[crate::models::FeedItem],
  mode: FeedSortMode,
  random_seed: u64,
) {
  match mode {
    FeedSortMode::Dated => {
      // items_store is already sorted by published_at desc — no work.
    }
    FeedSortMode::Random => {
      // Fisher-Yates with a seeded LCG. Inline because pulling in the
      // `rand` crate just for a session shuffle isn't worth the dep.
      let mut state = random_seed | 1;
      for i in (1..indices.len()).rev() {
        state = state
          .wrapping_mul(6364136223846793005)
          .wrapping_add(1442695040888963407);
        let j = ((state >> 33) as usize) % (i + 1);
        indices.swap(i, j);
      }
    }
    FeedSortMode::Popular => {
      indices
        .sort_by(|a, b| items[*b].upvote_count.cmp(&items[*a].upvote_count));
    }
    FeedSortMode::Trending => {
      // Cutoff = 14 days before now, formatted as ISO date. ISO 8601
      // date strings compare correctly lexicographically, so we can
      // filter and sort using string compare regardless of whether
      // published_at is a bare date or a full datetime.
      let cutoff = (chrono::Utc::now() - chrono::Duration::days(14))
        .format("%Y-%m-%d")
        .to_string();
      indices.retain(|i| items[*i].published_at.as_str() >= cutoff.as_str());
      indices
        .sort_by(|a, b| items[*b].upvote_count.cmp(&items[*a].upvote_count));
    }
  }
}

/// Apply the History tab's filter to `workspace.history`. Returns owned
/// references — `'a` is the workspace lifetime, the only source.
pub fn filtered_history_for<'a>(
  workspace: &'a crate::data::workspace_store::Workspace,
  feed: &FeedModel,
) -> Vec<&'a crate::history::HistoryEntry> {
  let now = chrono::Utc::now();
  let q = feed.search_query_lower.as_str();
  let src_filter = &feed.active_filters.sources;
  workspace
    .history
    .iter()
    .filter(|e| feed.history_filter.matches(e, now))
    .filter(|e| q.is_empty() || e.title_lower.contains(q))
    .filter(|e| src_filter.is_empty() || src_filter.contains(&e.source))
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_feed_tab_is_inbox() {
    let model = FeedModel::default();
    assert!(model.feed_tab == FeedTab::Inbox);
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
    // Cycle order: Inbox → Browse → Library → Discoveries → History → Inbox.
    let mut m = FeedModel::default();
    m.cycle_tab();
    assert!(m.feed_tab == FeedTab::Browse);
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
    assert!(m.feed_tab == FeedTab::Browse);
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
    let mut discovery = DiscoveryModel::default();
    m.inbox_list.set_count(100);
    m.inbox_list.set_viewport(20);
    m.inbox_list.set_selected(50);
    m.inbox_list.set_offset(0);
    assert_eq!(m.inbox_list.offset(), 0);

    m.pre_draw(&mut discovery, Viewport::new(80, 20), 100, 20);

    // After pre_draw the cursor must lie within the viewport.
    let offset = m.inbox_list.offset();
    assert!(offset <= 50, "offset {offset} should not exceed selected 50");
    assert!(
      offset + 20 > 50,
      "viewport [{offset}..{}) must cover 50",
      offset + 20
    );
  }

  #[test]
  fn pre_draw_preserves_offset_when_selection_in_viewport() {
    let mut m = FeedModel::default();
    let mut discovery = DiscoveryModel::default();
    m.inbox_list.set_count(100);
    m.inbox_list.set_viewport(20);
    m.inbox_list.set_selected(5);
    m.inbox_list.set_offset(0);

    m.pre_draw(&mut discovery, Viewport::new(80, 20), 100, 20);

    // Selection 5 is well inside [0, 20) — and the 2-item buffer doesn't
    // fire because 5 is far from the bottom edge. Offset stays at 0.
    assert_eq!(m.inbox_list.offset(), 0);
  }

  #[test]
  fn pre_draw_two_item_buffer_advances_offset_near_bottom() {
    // viewport fits 10 items; selection at row 9 (one short of bottom edge);
    // many more items follow → buffer should kick in.
    let mut m = FeedModel::default();
    let mut discovery = DiscoveryModel::default();
    m.inbox_list.set_count(50);
    m.inbox_list.set_viewport(10);
    m.inbox_list.set_offset(0);
    m.inbox_list.set_selected(9);

    m.pre_draw(&mut discovery, Viewport::new(80, 10), 50, 10);

    // Buffer fires: new_offset = 9 + 2 - 10 = 1.
    assert_eq!(m.inbox_list.offset(), 1);
  }

  #[test]
  fn pre_draw_two_item_buffer_does_not_run_off_the_end() {
    // Selection near the very end — no more items below, buffer should
    // not advance past the end of the list.
    let mut m = FeedModel::default();
    let mut discovery = DiscoveryModel::default();
    m.inbox_list.set_count(15);
    m.inbox_list.set_viewport(10);
    m.inbox_list.set_offset(5);
    m.inbox_list.set_selected(14);

    m.pre_draw(&mut discovery, Viewport::new(80, 10), 15, 10);

    // offset 5 + items_fitting 10 == 15 == total_items, so buffer guard
    // `offset + items_fitting < total_items` blocks the advance.
    // ensure_visible in set_viewport pulls offset to 14+1-10 = 5. Stable.
    assert_eq!(m.inbox_list.offset(), 5);
  }

  #[test]
  fn pre_draw_dispatches_by_feed_tab() {
    // Library tab should reconcile library_list, not inbox_list.
    let mut m = FeedModel::default();
    let mut discovery = DiscoveryModel::default();
    m.feed_tab = FeedTab::Library;
    m.library_list.set_count(100);
    m.library_list.set_viewport(10);
    m.library_list.set_selected(50);
    m.library_list.set_offset(0);

    m.pre_draw(&mut discovery, Viewport::new(80, 10), 100, 10);

    // Library list was reconciled; inbox_list is untouched.
    assert!(m.library_list.offset() > 0);
    assert_eq!(m.inbox_list.offset(), 0);
  }

  // ── pre_draw_narrow_feed ───────────────────────────────────────────────

  #[test]
  fn pre_draw_narrow_feed_reverse_walks_for_tall_rows() {
    // Viewport 10 rows; first 4 items are 3 rows tall, rest are 1 row.
    // Uniform-height ensure_visible would think 4 items fit (= 10 / ~2);
    // real fit from offset 0 is only 3 (3+3+3=9, 4th wouldn't fit).
    // Selecting item 5 (row 4-5 of one-row band) must reverse-walk to
    // land offset where the cumulative height up to selected fits.
    let mut m = FeedModel::default();
    let mut discovery = DiscoveryModel::default();
    m.inbox_list.set_count(20);
    m.inbox_list.set_viewport(10);
    m.inbox_list.set_offset(0);
    m.inbox_list.set_selected(5);

    // Heights: items 0..=3 are 3 rows; items 4..=5 are 1 row each.
    let heights: Vec<usize> =
      (0..=5).map(|i| if i < 4 { 3 } else { 1 }).collect();
    m.pre_draw_narrow_feed(&mut discovery, Viewport::new(40, 10), 20, &heights);

    // Reverse walk from item 5: 1 + 1 + 3 + 3 + 3 = 11 > 10, so the walk
    // stops; offset lands at item 2 (1+1+3+3 = 8 rows, ≤ 10).
    assert_eq!(m.inbox_list.offset(), 2);
  }

  #[test]
  fn pre_draw_narrow_feed_no_change_when_selection_already_fits() {
    // Selection already on screen at a sensible offset — no adjustment.
    let mut m = FeedModel::default();
    let mut discovery = DiscoveryModel::default();
    m.inbox_list.set_count(20);
    m.inbox_list.set_viewport(10);
    m.inbox_list.set_offset(0);
    m.inbox_list.set_selected(2);

    // Three items, each 2 rows tall — fits in a 10-row window.
    let heights = vec![2usize, 2, 2];
    m.pre_draw_narrow_feed(&mut discovery, Viewport::new(40, 10), 20, &heights);

    assert_eq!(m.inbox_list.offset(), 0);
    assert_eq!(m.inbox_list.selected(), 2);
  }

  #[test]
  fn pre_draw_narrow_feed_handles_empty_list() {
    let mut m = FeedModel::default();
    let mut discovery = DiscoveryModel::default();
    m.pre_draw_narrow_feed(&mut discovery, Viewport::new(40, 10), 0, &[]);
    assert_eq!(m.inbox_list.offset(), 0);
    assert_eq!(m.inbox_list.selected(), 0);
  }

  // ── W3 hybrid: workflow-state gestures ────────────────────────────────

  fn mock_item(
    url: &str,
    state: crate::models::WorkflowState,
  ) -> crate::models::FeedItem {
    crate::models::FeedItem {
      id: url.to_string(),
      title: "T".to_string(),
      source_platform: crate::models::SourcePlatform::ArXiv,
      content_type: crate::models::ContentType::Paper,
      domain_tags: Vec::new(),
      signal: crate::models::SignalLevel::Primary,
      published_at: "2026-01-01".to_string(),
      authors: Vec::new(),
      summary_short: String::new(),
      workflow_state: state,
      url: url.to_string(),
      upvote_count: 0,
      github_repo: None,
      github_owner: None,
      github_repo_name: None,
      benchmark_results: Vec::new(),
      full_content: None,
      source_name: "test".to_string(),
      title_lower: "t".to_string(),
      authors_lower: Vec::new(),
    }
  }

  #[test]
  fn set_workflow_state_for_url_mutates_workspace_and_emits_effect() {
    use crate::data::workspace_store::Workspace;
    use crate::effect::Effect;
    use crate::models::WorkflowState;

    let mut workspace = Workspace::default();
    workspace.items_store.push(mock_item("https://a", WorkflowState::Inbox));
    workspace.items_store.push(mock_item("https://b", WorkflowState::Inbox));
    let mut model = FeedModel::default();
    let mut discovery = DiscoveryModel::default();

    let effects = model.set_workflow_state_for_url(
      &mut workspace,
      &mut discovery,
      "https://b",
      WorkflowState::DeepRead,
    );

    // Item mutated in-place.
    assert_eq!(
      workspace.items_store.get(1).unwrap().workflow_state,
      WorkflowState::DeepRead
    );
    // Persisted-state side table updated.
    assert_eq!(
      workspace.persisted_states.get("https://b"),
      Some(&WorkflowState::DeepRead),
    );
    // Exactly one Effect emitted, naming the cache-invalidation event.
    assert_eq!(effects.len(), 1);
    assert!(matches!(
      &effects[0],
      Effect::WorkflowStateChanged { url, state }
        if url == "https://b" && *state == WorkflowState::DeepRead
    ));
  }

  #[test]
  fn set_workflow_state_for_url_falls_through_to_discovery() {
    use crate::data::workspace_store::Workspace;
    use crate::models::WorkflowState;

    let mut workspace = Workspace::default();
    let mut model = FeedModel::default();
    let mut discovery = DiscoveryModel::default();
    // Item only exists in discovery, not workspace.
    discovery.items.push(mock_item("https://d", WorkflowState::Inbox));

    let effects = model.set_workflow_state_for_url(
      &mut workspace,
      &mut discovery,
      "https://d",
      WorkflowState::Queued,
    );

    assert_eq!(effects.len(), 1);
    assert_eq!(discovery.items[0].workflow_state, WorkflowState::Queued);
    // Persistence is keyed by URL regardless of where the item lived.
    assert!(workspace.persisted_states.contains_key("https://d"));
  }

  #[test]
  fn set_workflow_state_for_url_unknown_url_is_noop() {
    use crate::data::workspace_store::Workspace;
    use crate::models::WorkflowState;

    let mut workspace = Workspace::default();
    workspace.items_store.push(mock_item("https://a", WorkflowState::Inbox));
    let mut model = FeedModel::default();
    let mut discovery = DiscoveryModel::default();

    let effects = model.set_workflow_state_for_url(
      &mut workspace,
      &mut discovery,
      "https://ghost",
      WorkflowState::DeepRead,
    );

    // Empty Vec means no event; no mutation; no persistence write.
    assert!(effects.is_empty());
    assert_eq!(
      workspace.items_store.get(0).unwrap().workflow_state,
      WorkflowState::Inbox
    );
    assert!(workspace.persisted_states.is_empty());
  }

  #[test]
  fn items_for_tab_dispatches_by_tab() {
    use crate::data::workspace_store::Workspace;
    let workspace = Workspace::default();
    let mut model = FeedModel::default();
    let discovery = DiscoveryModel::default();
    // Inbox + Library read workspace.items
    model.feed_tab = FeedTab::Inbox;
    assert_eq!(items_for_tab(&workspace, &model, &discovery).len(), 0);
    model.feed_tab = FeedTab::Library;
    assert_eq!(items_for_tab(&workspace, &model, &discovery).len(), 0);
    // Discoveries reads discovery.items
    model.feed_tab = FeedTab::Discoveries;
    assert!(std::ptr::eq(
      items_for_tab(&workspace, &model, &discovery).as_ptr(),
      discovery.items.as_ptr(),
    ));
    // History returns empty slice
    model.feed_tab = FeedTab::History;
    assert!(items_for_tab(&workspace, &model, &discovery).is_empty());
  }

  #[test]
  fn pre_draw_narrow_feed_dispatches_by_feed_tab() {
    // Library tab is active — library_list should be touched.
    let mut m = FeedModel::default();
    m.feed_tab = FeedTab::Library;
    m.library_list.set_count(50);
    m.library_list.set_viewport(8);
    m.library_list.set_offset(0);
    m.library_list.set_selected(6);

    let heights = vec![2usize; 7]; // 7 items of 2 rows each
    let mut discovery = DiscoveryModel::default();
    m.pre_draw_narrow_feed(&mut discovery, Viewport::new(40, 8), 50, &heights);

    // Reverse walk from item 6: 2+2+2+2 = 8 rows fits; one more (2) would
    // overflow, so offset lands at item 3.
    assert_eq!(m.library_list.offset(), 3);
    assert_eq!(m.inbox_list.offset(), 0);
  }

  // ── ADR-011 §E3: sort modes ───────────────────────────────────────────
  //
  // Tests target apply_sort_mode directly so we don't have to spin up
  // the full visible_indices_for filter pipeline. Each test builds a
  // hand-crafted item set, then verifies the post-sort order.

  use crate::models::{
    ContentType, FeedItem, SignalLevel, SourcePlatform, WorkflowState,
  };

  fn item(url: &str, published: &str, upvotes: u32, tags: &[&str]) -> FeedItem {
    FeedItem {
      id: url.to_string(),
      title: url.to_string(),
      source_platform: SourcePlatform::ArXiv,
      content_type: ContentType::Paper,
      domain_tags: tags.iter().map(|t| t.to_string()).collect(),
      signal: SignalLevel::Primary,
      published_at: published.to_string(),
      authors: vec![],
      summary_short: String::new(),
      workflow_state: WorkflowState::Inbox,
      url: url.to_string(),
      upvote_count: upvotes,
      github_repo: None,
      github_owner: None,
      github_repo_name: None,
      benchmark_results: vec![],
      full_content: None,
      source_name: "test".to_string(),
      title_lower: String::new(),
      authors_lower: vec![],
    }
  }

  #[test]
  fn sort_dated_is_noop_preserves_order() {
    // items_store is conventionally sorted by published_at desc;
    // apply_sort_mode(Dated) must leave the input order untouched.
    let items = vec![
      item("a", "2026-05-19", 100, &[]),
      item("b", "2026-05-18", 50, &[]),
      item("c", "2026-05-17", 200, &[]),
    ];
    let mut indices = vec![0, 1, 2];
    apply_sort_mode(&mut indices, &items, FeedSortMode::Dated, 42);
    assert_eq!(indices, vec![0, 1, 2]);
  }

  #[test]
  fn sort_popular_orders_by_upvote_count_desc() {
    let items = vec![
      item("a", "2026-05-19", 5, &[]),
      item("b", "2026-05-18", 100, &[]),
      item("c", "2026-05-17", 50, &[]),
    ];
    let mut indices = vec![0, 1, 2];
    apply_sort_mode(&mut indices, &items, FeedSortMode::Popular, 42);
    // Expected order: b (100), c (50), a (5).
    assert_eq!(indices, vec![1, 2, 0]);
  }

  #[test]
  fn sort_random_is_deterministic_for_same_seed() {
    let items: Vec<FeedItem> =
      (0..10).map(|i| item(&format!("u{i}"), "2026-05-19", 0, &[])).collect();
    let mut a = (0..10).collect::<Vec<usize>>();
    let mut b = (0..10).collect::<Vec<usize>>();
    apply_sort_mode(&mut a, &items, FeedSortMode::Random, 42);
    apply_sort_mode(&mut b, &items, FeedSortMode::Random, 42);
    assert_eq!(a, b, "same seed must produce same shuffle");
    // And must actually shuffle (not just return input order).
    assert_ne!(a, (0..10).collect::<Vec<usize>>(), "shuffle should reorder");
  }

  #[test]
  fn sort_random_differs_for_different_seeds() {
    let items: Vec<FeedItem> =
      (0..20).map(|i| item(&format!("u{i}"), "2026-05-19", 0, &[])).collect();
    let mut a = (0..20).collect::<Vec<usize>>();
    let mut b = (0..20).collect::<Vec<usize>>();
    apply_sort_mode(&mut a, &items, FeedSortMode::Random, 1);
    apply_sort_mode(&mut b, &items, FeedSortMode::Random, 999_999);
    assert_ne!(a, b, "different seeds should produce different shuffles");
  }

  #[test]
  fn sort_trending_filters_to_last_14_days_then_sorts_by_upvotes() {
    // Build with old + new items mixed. Trending mode should drop the
    // old ones entirely, then sort the rest by upvote_count desc.
    let now = chrono::Utc::now();
    let recent = now.format("%Y-%m-%d").to_string();
    let old = (now - chrono::Duration::days(60)).format("%Y-%m-%d").to_string();
    let items = vec![
      item("old-low", &old, 10, &[]),      // dropped
      item("new-low", &recent, 5, &[]),    // kept, low
      item("old-high", &old, 1000, &[]),   // dropped despite high upvotes
      item("new-high", &recent, 200, &[]), // kept, high
    ];
    let mut indices = vec![0, 1, 2, 3];
    apply_sort_mode(&mut indices, &items, FeedSortMode::Trending, 42);
    // Expected order: new-high (200), new-low (5). old-* dropped.
    assert_eq!(indices, vec![3, 1]);
  }

  // ── ADR-011 §E4: subject-follow predicate ────────────────────────────

  #[test]
  fn subject_scope_archive_matches_prefix_and_bare_id() {
    let scope = SubjectScope::Archive("cs");
    assert!(scope.matches(&item("a", "x", 0, &["cs.LG"])));
    assert!(scope.matches(&item("b", "x", 0, &["cs.AI", "stat.ML"])));
    assert!(scope.matches(&item("label", "x", 0, &["Machine Learning"])));
    assert!(!scope.matches(&item("c", "x", 0, &["math.NT"])));

    // Bare archive id (gr-qc) — items tagged with exactly the
    // archive id match too.
    let gr_qc = SubjectScope::Archive("gr-qc");
    assert!(gr_qc.matches(&item("d", "x", 0, &["gr-qc"])));
    assert!(!gr_qc.matches(&item("e", "x", 0, &["gr-qcology"]))); // not a prefix match
  }

  #[test]
  fn subject_scope_category_matches_exact_code_only() {
    let scope = SubjectScope::Category("math.NT");
    assert!(scope.matches(&item("a", "x", 0, &["math.NT"])));
    assert!(scope.matches(&item("label", "x", 0, &["Number Theory"])));
    assert!(!scope.matches(&item("b", "x", 0, &["math.AG"])));
    assert!(!scope.matches(&item("c", "x", 0, &["math.NT.subspec"])));
  }
}
