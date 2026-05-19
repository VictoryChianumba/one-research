use crate::config::{Config, CustomThemeConfig};
use crate::ingestion::message::FetchMessage;
use crate::models::*;
use chrono::Utc;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

mod caches;
pub mod focus;
mod methods;
pub mod notifications;
mod state;
pub use focus::FocusManager;
pub use notifications::NotificationState;
pub use state::*;

pub struct App {
  /// True when the UI needs to be redrawn. Set by `mark_dirty()`, cleared by
  /// `check_needs_redraw()`. Defaults to `true` so the first frame always draws.
  pub needs_redraw: bool,

  /// Authoritative in-memory data corpus (items + dedup indices +
  /// history + tags + persisted workflow states). Phase 3 extracted
  /// these from scattered App fields into one struct.
  pub workspace: crate::data::Workspace,

  pub should_quit: bool,
  pub quit_popup: QuitPopupState,

  /// Feed-pane composition-root model. Owns: tab selection, per-tab list
  /// cursors, library/history filter chips, library bulk-select, search
  /// bar state, filter panel state. Slice 1 PR 2 lifted these fields off
  /// `App`; see `docs/adr/ADR-001-render-purification.md`. Discovery used
  /// to live nested at `feed.discovery` — C7 PR 2 (ADR-005) promoted it
  /// to a sibling composition root (see `discovery` below).
  pub feed: crate::feed::FeedModel,

  /// Discovery-pane composition-root model. Owns: agent search bar,
  /// discovered-items list + dedup indices, multi-turn session, intent
  /// classification, slash-command palette, agent receiver. Sibling to
  /// `feed`, `reader`, `notes`. Lifted out of `feed` by C7 PR 2 (ADR-005).
  pub discovery: DiscoveryModel,

  /// Tag picker popup state.
  pub tag_picker: TagPickerState,
  pub status_message: Option<String>,

  // Background fetching
  pub fetch_rx: Option<Receiver<FetchMessage>>,
  pub loading_sources: Vec<String>,
  pub loaded_sources: Vec<String>,
  pub is_loading: bool,
  pub spinner_frame: usize,

  // View state
  pub view: AppView,

  // Repo viewer
  pub repo_context: Option<RepoContext>,
  pub github_token: Option<String>,

  // Manual refresh
  pub is_refreshing: bool,

  // Details panel
  /// Banner notification state — relocated into a struct in Phase 2.
  /// Was two paired fields (`notification`, `notification_item_id`).
  pub notification: NotificationState,
  /// Scroll offset + max for the details pane. Migrated from raw
  /// (details_scroll, details_max_scroll) scalar pair in Migration #5.
  /// Each render path sets max appropriately for its mode.
  pub details_scroll: crate::primitives::ScrollState,
  /// URL of the item that was selected when details_scroll was last set.
  /// Used to reset scroll when the user moves to a different item.
  pub details_last_item_url: Option<String>,

  // Config (full, persisted)
  pub config: Config,

  // Active theme (mirrors config.theme; applied live each frame)
  pub active_theme: ui_theme::ThemeId,
  pub active_custom_theme_id: Option<String>,

  // Settings screen
  pub settings: SettingsEditState,
  pub theme_picker: ThemePickerState,

  // Sources popup — first surface conversion (Phase 2). Field name
  // `sources_popup` retained to keep the diff narrow; type is now the
  // surface struct from surfaces/overlays/sources.rs.
  pub sources_popup: crate::surfaces::overlays::SourcesSurface,

  /// Active overlay stack (Phase 2). Top of the stack intercepts
  /// input. Variants accrete as more overlays migrate; for now only
  /// Sources tags itself here.
  pub modals: crate::surfaces::overlays::ModalStack,

  // Embedded notes pane — composition root per ADR-003.
  // Owns the shared backend + primary/secondary instances + visibility.
  pub notes: NotesPaneModel,

  // Embedded chat pane
  pub chat: ChatState,

  /// Slice 2 composition root (ADR-002): owns primary + optional
  /// secondary reader instances plus the split/dual/focus state. The
  /// three-state layout cycle (Ldr+f) reads `reader.active`,
  /// `reader.split_active`, `reader.dual_active`.
  pub reader: crate::reader::ReaderPaneModel,

  /// Slice 2 sibling Model: the floating popup reader (Ldr+Enter).
  /// Lifecycle is async-load (`rx`) + dismissible (`active`) +
  /// editor state; collapsed from the 5 reader_popup_* fields that
  /// used to live at App top level.
  pub reader_popup: crate::reader::ReaderPopupModel,

  /// Shared TTS playback controller.  Cloned into each `ReaderTab`'s
  /// Reader so all open papers use one audio thread / one rodio sink.
  /// Cross-tab preemption (only one paper speaks at a time) is handled
  /// by tread's session-id machinery — see voice/playback.rs.
  pub voice_controller: Arc<tread::PlaybackController>,
  /// Whether the host terminal speaks the Kitty graphics protocol.
  /// Detected once at App::new via `tread::detect_kitty_supported`.
  /// Threaded into `tread::after_draw` calls so figures emit APC
  /// escapes only on graphics-capable terminals; on others, the
  /// hook is a no-op and tread's text-fallback caption renders.
  pub kitty_supported: bool,
  pub fulltext_for_secondary: bool,
  pub fulltext_new_tab: bool,
  // True while waiting for [1]/[2] to choose which reader window gets a new tab.
  pub tab_window_prompt_active: bool,
  // Bottom pane in State 3 (summoned by Ldr+f, dismissed by q/Esc)
  pub reader_bottom_open: bool,    // pane is visible
  pub reader_bottom_focused: bool, // pane has keyboard focus
  pub reader_bottom_details: bool, // showing details (true) or feed list (false)
  /// Scroll offset for the reader bottom drawer — used in both modes
  /// (`draw_bottom_pane_feed` and `draw_bottom_pane_details`). Each
  /// render path sets max appropriately for its mode.
  pub reader_bottom_scroll: crate::primitives::ScrollState,
  pub narrow_feed_details_open: bool, // State 2: description popup over reader
  pub abstract_popup_active: bool,    // Space: quick abstract view
  pub reader_feed_popup_selected: usize, // selected item in bottom feed list

  // Last opened paper (shown in dashboard "Continue Reading")
  pub last_read: Option<String>,
  pub last_read_source: Option<String>,

  // Background fulltext fetch (article reader)
  pub fulltext_rx: Option<Receiver<Result<tread::PaperData, String>>>,
  pub fulltext_loading: bool,
  pub pending_fulltext_context: Option<NotesContext>,
  // Stage 2 of the arxiv path: after fulltext drains we kick off the
  // rich-LaTeX `tread::fetch_paper` on its own worker.  This receiver
  // is the channel from that worker; `pending_tread_fetch` holds the
  // context we need to finish opening the reader once it returns
  // (target pane, mode, title, notes context, plus the fulltext
  // result we'll fall back to if the arxiv fetch errors).  Both are
  // populated together in main.rs's fulltext drain Ok branch and
  // cleared together by the tread drain Ok / Err / Disconnect branches.
  pub tread_fetch_rx: Option<Receiver<Result<tread::PaperData, String>>>,
  pub pending_tread_fetch: Option<PendingTreadFetch>,
  // Background repo fetch (repo viewer)
  pub repo_fetch_rx: Option<Receiver<RepoFetchResult>>,

  /// Scroll debounce gates (keyboard + mouse). ADR-009 pilot cluster:
  /// the four flat fields this replaces moved into `DebounceState`,
  /// which owns the cooldown protocol via `try_kbd_scroll` /
  /// `try_mouse_scroll`.
  pub debounce: DebounceState,

  /// Ctrl+T leader-key arming + auto-expire.  ADR-009 cluster #2:
  /// replaces three flat fields with the typed state machine.
  pub leader: LeaderState,

  /// Focus + pane registry. Holds `focused_pane` and the per-pane
  /// rect cache populated by layout each frame.
  pub focus: FocusManager,

  // Help overlay
  pub help: HelpState,

  // Cached indices of items visible under the current search/filter.
  // Keyed by (FeedTab) so a tab switch automatically misses the cache.
  visible_cache: RefCell<Option<(FeedTab, Vec<usize>)>>,
  /// Memoized item counts (workflow breakdown + recent-48h + queue preview).
  /// Invalidated by every items/workflow mutation site.
  counts_cache: RefCell<Option<ItemCounts>>,
  /// Memoized sorted unique source-label set used by the filter panel.
  /// Invalidated alongside `counts_cache`.
  filter_source_names_cache: RefCell<Option<Vec<String>>>,
  /// Memoized filter-summary string. Invalidated only by `active_filters`
  /// mutation — does NOT depend on items or search query.
  pub filter_summary_cache: RefCell<Option<String>>,
  /// Memoized `filtered_history` indices into `self.workspace.history`. Invalidated
  /// by history mutation, search_query mutation, history_filter mutation,
  /// and active_filters mutation.
  pub filtered_history_cache: RefCell<Option<Vec<usize>>>,
}

// Filter panel cursor positions are computed dynamically in
// `toggle_filter_at_cursor` based on the current source / tag counts. Static
// offsets aren't used anymore.

/// Deferred-state for the arxiv "rich LaTeX" leg of opening a paper.
///
/// We can't carry the in-flight `tread::PaperData` directly across the
/// fulltext-drain → tread-drain boundary because the future caller
/// needs more than just the paper: which pane, which tab mode, the
/// title for the status line, the optional notes context, and a
/// fulltext-derived `PaperData` to fall back on if the arxiv fetch
/// errors out.  Saved when the fulltext drain spawns the tread worker
/// and consumed by the tread drain on the next loop tick(s).
pub struct PendingTreadFetch {
  pub arxiv_id: String,
  pub title: String,
  pub notes_context: Option<NotesContext>,
  /// Optional fulltext-derived `PaperData` to fall back on when the
  /// tread fetch errors out.  `Some` on the original two-stage path
  /// (where fulltext fired first and its result is still in memory).
  /// `None` on the arxiv-shortcut path where we skip fulltext entirely
  /// and go straight to tread — a tread error there shows a
  /// notification and leaves the reader closed; the user can re-click
  /// to retry.
  pub fallback_paper: Option<tread::PaperData>,
  pub target: crate::action::ReaderTarget,
  pub mode: crate::action::OpenMode,
}

impl App {
  pub fn new() -> Self {
    Self {
      needs_redraw: true,
      workspace: crate::data::Workspace {
        items_store: crate::data::ItemStore::default(),
        history: crate::store::history::load(),
        item_tags: crate::store::tags::load(),
        persisted_states: HashMap::new(),
      },
      should_quit: false,
      quit_popup: QuitPopupState::default(),
      feed: crate::feed::FeedModel::new(),
      discovery: DiscoveryModel {
        items: crate::store::discovery_cache::load(),
        session: crate::store::session::load(),
        ..DiscoveryModel::default()
      },
      tag_picker: TagPickerState::default(),
      status_message: None,
      fetch_rx: None,
      loading_sources: Vec::new(),
      loaded_sources: Vec::new(),
      is_loading: false,
      spinner_frame: 0,
      view: AppView::Feed,
      repo_context: None,
      github_token: None,
      is_refreshing: false,
      notification: NotificationState::new(),
      details_scroll: crate::primitives::ScrollState::new(),
      details_last_item_url: None,
      config: Config::default(),
      active_theme: ui_theme::ThemeId::Dark,
      active_custom_theme_id: None,
      settings: SettingsEditState::default(),
      theme_picker: ThemePickerState::default(),
      sources_popup: crate::surfaces::overlays::SourcesSurface::new(),
      modals: crate::surfaces::overlays::ModalStack::new(),
      notes: NotesPaneModel::default(),
      chat: ChatState::default(),
      reader: crate::reader::ReaderPaneModel::new(),
      reader_popup: crate::reader::ReaderPopupModel::new(),
      voice_controller: tread::build_voice_controller(),
      kitty_supported: tread::detect_kitty_supported(),
      fulltext_for_secondary: false,
      fulltext_new_tab: false,
      tab_window_prompt_active: false,
      reader_bottom_open: false,
      reader_bottom_focused: false,
      reader_bottom_details: false,
      narrow_feed_details_open: false,
      abstract_popup_active: false,
      reader_bottom_scroll: crate::primitives::ScrollState::new(),
      reader_feed_popup_selected: 0,
      last_read: None,
      last_read_source: None,
      fulltext_rx: None,
      fulltext_loading: false,
      pending_fulltext_context: None,
      tread_fetch_rx: None,
      pending_tread_fetch: None,
      repo_fetch_rx: None,
      debounce: DebounceState::default(),
      leader: LeaderState::default(),
      focus: FocusManager::new(),
      help: HelpState::default(),
      visible_cache: RefCell::new(None),
      counts_cache: RefCell::new(None),
      filter_source_names_cache: RefCell::new(None),
      filter_summary_cache: RefCell::new(None),
      filtered_history_cache: RefCell::new(None),
    }
  }

  pub fn theme(&self) -> ui_theme::Theme {
    if let Some(id) = &self.active_custom_theme_id {
      if let Some(custom) =
        self.config.custom_themes.iter().find(|t| &t.id == id)
      {
        return custom.to_theme();
      }
    }
    self.active_theme.theme()
  }

  /// Convert trench's `ui_theme::Theme` to tread's `tread::Theme`.
  /// The two `ui_theme` crates are separate (different workspaces),
  /// so the `Theme` STRUCTS don't unify — but their fields are all
  /// `ratatui::style::Color`, which IS the same type across the build,
  /// so the field values copy directly with no enum mapping.  We just
  /// need to fill in the two extra fields tread carries
  /// (`bg_highlight`, `link_fg`) with sensible defaults.
  pub fn theme_for_tread(&self) -> tread::Theme {
    use ratatui::style::Color;
    let t = self.theme();
    tread::Theme {
      accent: t.accent,
      header: t.header,
      text: t.text,
      text_dim: t.text_dim,
      border: t.border,
      border_active: t.border_active,
      bg: t.bg,
      bg_panel: t.bg_panel,
      bg_input: t.bg_input,
      bg_selection: t.bg_selection,
      bg_code: t.bg_code,
      bg_chat: t.bg_chat,
      bg_user_msg: t.bg_user_msg,
      bg_popup: t.bg_popup,
      text_on_accent: t.text_on_accent,
      success: t.success,
      warning: t.warning,
      error: t.error,
      math: t.math,
      mono: t.mono,
      rule: t.rule,
      toc_dim: t.toc_dim,
      bookmark_bg: t.bookmark_bg,
      cursor_bg: t.cursor_bg,
      cursor_fg: t.cursor_fg,
      search_match_bg: t.search_match_bg,
      search_match_fg: t.search_match_fg,
      // Tread-only fields: pick defaults that match tread's standalone
      // dark theme.  bg_highlight is the marked-line tint; link_fg is
      // the cross-ref / citation underline colour.
      bg_highlight: Color::Rgb(80, 60, 0),
      link_fg: Color::Rgb(120, 195, 220),
    }
  }

  pub fn active_theme_name(&self) -> String {
    if let Some(id) = &self.active_custom_theme_id {
      if let Some(custom) =
        self.config.custom_themes.iter().find(|t| &t.id == id)
      {
        return custom.name.clone();
      }
    }
    self.active_theme.info().name.to_string()
  }

  pub fn active_custom_theme(&self) -> Option<&CustomThemeConfig> {
    let id = self.active_custom_theme_id.as_ref()?;
    self.config.custom_themes.iter().find(|t| &t.id == id)
  }

  pub fn reconcile_custom_theme_selection(&mut self) {
    if let Some(id) = &self.active_custom_theme_id {
      if !self.config.custom_themes.iter().any(|t| &t.id == id) {
        self.active_custom_theme_id = None;
        self.config.active_custom_theme_id = None;
      }
    }
  }

  // ── Pane registry ──────────────────────────────────────────────────────────

  /// Set the redraw flag. Cheap — call from any code path that mutates
  /// state visible to the user. Mirrors `cli-text-reader::Editor::mark_dirty`
  /// so the embedded reader and trench's outer UI use identical semantics.
  pub fn mark_dirty(&mut self) {
    self.needs_redraw = true;
  }

  // C9 PR 2 (ADR-007): App::rebuild_indices is gone — `ItemStore`
  // owns the invariant, and `sort_by` rebuilds internally. Cache load
  // uses `ItemStore::from_items` which calls `rebuild_indices` on the
  // fresh store.

  /// Same as `rebuild_indices` but for `discovery_items`.
  pub fn rebuild_discovery_indices(&mut self) {
    self.discovery.url_index.clear();
    self.discovery.arxiv_id_index.clear();
    self.discovery.url_index.reserve(self.discovery.items.len());
    for (idx, item) in self.discovery.items.iter().enumerate() {
      self.discovery.url_index.insert(item.url.clone(), idx);
      if let Some(aid) = arxiv_id_from_url(&item.url) {
        self.discovery.arxiv_id_index.insert(aid.to_string(), idx);
      }
    }
  }

  /// Atomically read and clear the redraw flag. Returns `true` if a redraw
  /// is needed for this frame.
  pub fn check_needs_redraw(&mut self) -> bool {
    let needs = self.needs_redraw;
    self.needs_redraw = false;
    needs
  }

  /// True if any continuous animation or background activity is in flight
  /// that requires fast (~16ms) event-poll cadence. Used by the main loop
  /// to decide whether to block long (idle) or short (animating).
  ///
  /// Self-stopping animations covered:
  /// - `is_loading` — spinner needs to tick while a fetch cycle is active
  /// - `is_refreshing` — same
  /// - any open `repo_context.scroll_velocity` non-zero (momentum scroll)
  /// - `discovery_loading` — discovery agent in flight
  /// - `settings_save_time` — TTL window for the "Saved." indicator
  pub fn has_active_animation(&self) -> bool {
    if self.is_loading || self.is_refreshing || self.discovery.loading {
      return true;
    }
    if self.settings.save_time.is_some() {
      return true;
    }
    if self
      .repo_context
      .as_ref()
      .map(|c| c.scroll_velocity.abs() >= 0.5)
      .unwrap_or(false)
    {
      return true;
    }
    false
  }

  /// Length of the currently-visible item slice. Cheaper than
  /// `visible_items().len()` because it skips the per-call `Vec<&FeedItem>`
  /// allocation. Use this everywhere a length-only check is needed.
  pub fn visible_count(&self) -> usize {
    {
      let cache = self.visible_cache.borrow();
      if let Some((tab, ref indices)) = *cache {
        if tab == self.feed.feed_tab {
          return indices.len();
        }
      }
    }
    // Cache miss: fall through and use visible_items to populate it.
    self.visible_items().len()
  }

  /// Random access into the currently-visible items by display position.
  /// Cheaper than `visible_items().into_iter().nth(idx)` since it skips the
  /// per-call `Vec<&FeedItem>` allocation when the cache is warm. Falls back
  /// to a full `visible_items()` invocation on cold cache so callers don't
  /// need to know which path they're on.
  pub fn visible_get(&self, idx: usize) -> Option<&FeedItem> {
    // Try the warm-cache fast path first.
    {
      let cache = self.visible_cache.borrow();
      if let Some((tab, indices)) = cache.as_ref() {
        if *tab == self.feed.feed_tab {
          let item_idx = *indices.get(idx)?;
          let items = self.items_for_tab();
          return items.get(item_idx);
        }
      }
    }
    // Cold cache: populate via visible_items, then retry. visible_items
    // borrows the cache mutably so the immutable borrow above must be
    // dropped before we call it (the explicit block above ensures that).
    let v = self.visible_items();
    v.into_iter().nth(idx)
  }

  /// Items visible after applying search and category filters.
  pub fn visible_items(&self) -> Vec<&FeedItem> {
    {
      let cache = self.visible_cache.borrow();
      if let Some((tab, ref indices)) = *cache {
        if tab == self.feed.feed_tab {
          let items = self.items_for_tab();
          return indices.iter().map(|&i| &items[i]).collect();
        }
      }
    }
    let q = self.feed.search_query_lower.as_str();
    let items = self.items_for_tab();
    let indices: Vec<usize> = items
      .iter()
      .enumerate()
      .filter(|(_, item)| {
        // Tab-scoped pre-filter: Inbox shows only Inbox-state items, Library
        // shows whichever workflow chip is active.
        match self.feed.feed_tab {
          FeedTab::Inbox => {
            if item.workflow_state != WorkflowState::Inbox {
              return false;
            }
          }
          FeedTab::Library => {
            if !self.feed.library_filter.matches(item.workflow_state) {
              return false;
            }
          }
          _ => {}
        }
        let key = if item.source_platform
          == crate::models::SourcePlatform::HuggingFace
        {
          "huggingface"
        } else {
          &item.source_name
        };
        if let Some(&enabled) = self.config.sources.enabled_sources.get(key) {
          if !enabled {
            return false;
          }
        }
        if !q.is_empty()
          && !item.title_lower.contains(&q)
          && !item.authors_lower.iter().any(|a| a.contains(&q))
        {
          return false;
        }
        if !self.feed.active_filters.tags.is_empty() {
          let item_tags =
            crate::tags::for_url(&self.workspace.item_tags, &item.url);
          if !item_tags
            .iter()
            .any(|t| self.feed.active_filters.tags.contains(t))
          {
            return false;
          }
        }
        self.feed.active_filters.matches(item)
      })
      .map(|(i, _)| i)
      .collect();
    *self.visible_cache.borrow_mut() =
      Some((self.feed.feed_tab, indices.clone()));
    indices.iter().map(|&i| &items[i]).collect()
  }

  pub fn visible_window(&self, start: usize, end: usize) -> Vec<&FeedItem> {
    {
      let cache = self.visible_cache.borrow();
      if let Some((tab, indices)) = cache.as_ref() {
        if *tab == self.feed.feed_tab {
          let items = self.items_for_tab();
          let start = start.min(indices.len());
          let end = end.min(indices.len());
          return indices[start..end].iter().map(|&i| &items[i]).collect();
        }
      }
    }
    let visible = self.visible_items();
    let start = start.min(visible.len());
    let end = end.min(visible.len());
    visible[start..end].to_vec()
  }

  /// Bare cache invalidators. Prefer the `mutate_*` helpers (mutate_search_query,
  /// mutate_filters, mutate_history, mutate_history_filter, mutate_library_filter,
  /// set_workflow_state_for_url) — they invalidate the right caches for you.
  /// These bare methods exist as building blocks for the mutators and as
  /// escape hatches for the rare external mutation sites that don't fit a
  /// mutator (e.g. config.sources toggling in keys.rs).
  /// Reset the primary items vec and every parallel index/cache that mirrors
  /// it. Direct `app.workspace.items_store.clear()` would leave `url_index` /
  /// `arxiv_id_index` populated with stale offsets and panic on the next
  /// `process_incoming` batch.
  pub fn reset_items(&mut self) {
    // ItemStore::clear drops items + both indices in lockstep.  Before
    // C9 this was three separate field-level clears, with the same
    // invariant-drift risk this whole slice closes.
    self.workspace.items_store.clear();
    self.route_effects(&[crate::effect::Effect::ItemsChanged]);
  }

  /// Same shape as `reset_items` but for the discovery-side mirrors —
  /// keeps the parallel indexes in sync.
  pub fn reset_discovery_items(&mut self) {
    self.discovery.items.clear();
    self.discovery.url_index.clear();
    self.discovery.arxiv_id_index.clear();
    self.route_effects(&[crate::effect::Effect::ItemsChanged]);
  }

  /// Cheap O(1) read from the memoized count cache. On miss, runs a single
  /// Fused pass over `self.workspace.items` that produces every counter the dashboard
  /// and chip bar need. Returns a `Ref` so cache hits don't pay a clone.
  pub fn item_counts(&self) -> std::cell::Ref<'_, ItemCounts> {
    if self.counts_cache.borrow().is_none() {
      let counts = self.compute_item_counts();
      *self.counts_cache.borrow_mut() = Some(counts);
    }
    std::cell::Ref::map(self.counts_cache.borrow(), |opt| {
      opt.as_ref().expect("counts_cache populated above")
    })
  }

  fn compute_item_counts(&self) -> ItemCounts {
    let today = crate::store::enrichment_cache::today_str();
    let yesterday = (chrono::Utc::now() - chrono::Duration::days(1))
      .format("%Y-%m-%d")
      .to_string();
    let mut counts = ItemCounts::default();
    for item in self.workspace.items_store.items() {
      counts.total += 1;
      match item.workflow_state {
        WorkflowState::Inbox => counts.inbox += 1,
        WorkflowState::Queued => {
          counts.queued += 1;
          if counts.queue_preview.len() < 2 {
            counts.queue_preview.push(item.title.clone());
          }
        }
        WorkflowState::DeepRead => counts.deep_read += 1,
        WorkflowState::Archived => counts.archived += 1,
      }
      if item.published_at == today || item.published_at == yesterday {
        counts.recent_total += 1;
        if item.published_at == today {
          counts.recent_today += 1;
        }
        match item.source_platform {
          crate::models::SourcePlatform::HuggingFace => counts.recent_hf += 1,
          crate::models::SourcePlatform::ArXiv => counts.recent_arxiv += 1,
          _ => counts.recent_other += 1,
        }
      }
    }
    counts
  }

  pub fn items_for_tab(&self) -> &[FeedItem] {
    match self.feed.feed_tab {
      FeedTab::Inbox => self.workspace.items_store.items(),
      FeedTab::Library => self.workspace.items_store.items(),
      FeedTab::Discoveries => &self.discovery.items,
      FeedTab::History => &[],
    }
  }

  pub fn active_selected_index(&self) -> usize {
    match self.feed.feed_tab {
      FeedTab::Inbox => self.feed.inbox_list.selected(),
      FeedTab::Library => self.feed.library_list.selected(),
      FeedTab::Discoveries => self.discovery.list.selected(),
      FeedTab::History => self.feed.history_list.selected(),
    }
  }

  pub fn set_active_selected_index(&mut self, value: usize) {
    // Sync count before set_selected so the clamp uses the current
    // length rather than ListState's stale (initially zero) count.
    // History reads from `workspace.history` via filtered_history()
    // — `visible_count()` returns 0 for History because items_for_tab
    // returns an empty slice. Other tabs use the items path.
    let count = match self.feed.feed_tab {
      FeedTab::History => self.filtered_history().len(),
      _ => self.visible_count(),
    };
    match self.feed.feed_tab {
      FeedTab::Inbox => {
        self.feed.inbox_list.set_count(count);
        self.feed.inbox_list.set_selected(value);
      }
      FeedTab::Library => {
        self.feed.library_list.set_count(count);
        self.feed.library_list.set_selected(value);
      }
      FeedTab::Discoveries => {
        self.discovery.list.set_count(count);
        self.discovery.list.set_selected(value);
      }
      FeedTab::History => {
        self.feed.history_list.set_count(count);
        self.feed.history_list.set_selected(value);
      }
    }
  }

  pub fn set_active_list_offset(&mut self, value: usize) {
    match self.feed.feed_tab {
      FeedTab::Inbox => self.feed.inbox_list.set_offset(value),
      FeedTab::Library => self.feed.library_list.set_offset(value),
      FeedTab::Discoveries => self.discovery.list.set_offset(value),
      FeedTab::History => self.feed.history_list.set_offset(value),
    }
  }

  pub fn reset_active_feed_position(&mut self) {
    self.invalidate_visible_cache();
    self.set_active_selected_index(0);
    self.set_active_list_offset(0);
    self.details_scroll.reset();
    self.details_last_item_url = None;
  }

  pub fn set_notification(&mut self, msg: String) {
    let url = self.selected_item().map(|i| i.url.clone());
    // Sanitize at the chokepoint — set_notification is called from many
    // sites including ones that interpolate reqwest errors / GitHub
    // tree paths / API messages.
    self.notification.message =
      Some(crate::sanitize::sanitize_terminal_text(&msg));
    self.notification.item_id = url;
  }

  pub fn clear_notification(&mut self) {
    self.notification.clear();
  }

  pub fn move_down(&mut self) {
    let len = self.visible_count();
    if len == 0 {
      return;
    }
    let next = (self.active_selected_index() + 1).min(len - 1);
    self.set_active_selected_index(next);
    self.details_scroll.reset();
    self.clear_notification();
  }

  pub fn move_up(&mut self) {
    self.set_active_selected_index(
      self.active_selected_index().saturating_sub(1),
    );
    self.details_scroll.reset();
    self.clear_notification();
  }

  pub fn go_to_top(&mut self) {
    self.set_active_selected_index(0);
    self.details_scroll.reset();
    self.clear_notification();
  }

  pub fn go_to_bottom(&mut self) {
    let len = self.visible_count();
    if len > 0 {
      self.set_active_selected_index(len - 1);
    }
    self.details_scroll.reset();
    self.clear_notification();
  }

  /// Mutator chokepoint for `search_query`. Invokes `f` on the query, then
  /// auto-syncs `search_query_lower` and invalidates every cache that depends
  /// on the query (`visible_cache`, `filtered_history_cache`). All search
  /// query mutations must go through here so the lowercased mirror and the
  /// memoized visible/filtered-history results stay in sync.

  pub fn selected_item(&self) -> Option<&FeedItem> {
    self.visible_get(self.active_selected_index())
  }

  /// Update library_selected_urls from anchor/cursor positions in the visible
  /// item list. Always covers the contiguous range from anchor to cursor.
  /// Used on entry to visual mode (full populate); cursor moves go through
  /// `library_extend_selection` for incremental update.

  pub fn show_quit_popup(&mut self) {
    let kind =
      if self.focus.focused_pane == PaneId::Reader && self.reader.active {
        QuitPopupKind::LeaveReader
      } else if self.discovery.loading || self.is_loading {
        QuitPopupKind::QuitWithProgress
      } else if self.chat.active
        && self.chat.ui.as_ref().map_or(false, |c| !c.input.trim().is_empty())
      {
        QuitPopupKind::QuitWithChat
      } else {
        QuitPopupKind::QuitApp
      };
    self.quit_popup.active = true;
    self.quit_popup.kind = kind;
  }

  pub fn handle_slash_command(&mut self, cmd: String) {
    let parsed = crate::commands::parser::parse_slash_command(&cmd);
    crate::commands::dispatch::dispatch_slash_command(self, parsed);
  }

  pub fn push_chat_assistant_message(&mut self, content: String) {
    if let Some(chat_ui) = self.chat.ui.as_mut() {
      if let Some(session) = chat_ui.active_session.as_mut() {
        session.messages.push(chat::ChatMessage {
          role: chat::Role::Assistant,
          content,
          timestamp: Utc::now(),
        });
        session.updated_at = Utc::now();
        let _ = chat::save_session(session);
        let meta = chat::storage::session_to_meta(session);
        let id = meta.id.clone();
        if let Some(pos) = chat_ui.sessions.iter().position(|s| s.id == id) {
          chat_ui.sessions[pos] = meta;
        }
        let index = chat::ChatIndex {
          sessions: chat_ui.sessions.clone(),
          default_provider: chat_ui.default_provider.clone(),
        };
        let _ = chat::save_index(&index);
        chat_ui.scroll_offset = usize::MAX;
      }
    }
  }

  pub fn clear_chat_messages(&mut self) {
    if let Some(chat_ui) = self.chat.ui.as_mut() {
      if let Some(session) = chat_ui.active_session.as_mut() {
        session.messages.clear();
        session.updated_at = Utc::now();
        let _ = chat::save_session(session);
        let meta = chat::storage::session_to_meta(session);
        let id = meta.id.clone();
        if let Some(pos) = chat_ui.sessions.iter().position(|s| s.id == id) {
          chat_ui.sessions[pos] = meta;
        }
        let index = chat::ChatIndex {
          sessions: chat_ui.sessions.clone(),
          default_provider: chat_ui.default_provider.clone(),
        };
        let _ = chat::save_index(&index);
        chat_ui.scroll_offset = 0;
      }
    }
  }

  // ── Repo viewer ────────────────────────────────────────────────────────
}

pub(super) fn encode_repo_url_path(path: &str) -> String {
  let mut out = String::with_capacity(path.len());
  for &b in path.as_bytes() {
    let safe = b.is_ascii_alphanumeric()
      || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/');
    if safe {
      out.push(b as char);
    } else {
      out.push_str(&format!("%{b:02X}"));
    }
  }
  out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(super) fn toggle_set<T: Eq + std::hash::Hash>(
  set: &mut HashSet<T>,
  value: T,
) {
  if !set.remove(&value) {
    set.insert(value);
  }
}

fn save_discovery_items(_items: &[FeedItem]) {
  crate::store::discovery_cache::save(_items);
}

/// Reject a filename that would write outside `~/Downloads/`. `Path::join`
/// silently accepts absolute paths (replacing the base) and `..` segments
/// (traversing up); both are realistic vectors when the filename comes
/// from a GitHub API response on a hostile or compromised repo.
///
/// The check: `Path::file_name()` extracts the *terminal* component only.
/// If that component differs from the input, the input contained either
/// a path separator or a `..` segment.
pub(super) fn validate_download_name(name: &str) -> Result<(), String> {
  let p = std::path::Path::new(name);
  if p.is_absolute() {
    return Err(format!("absolute path not allowed: {name:?}"));
  }
  // Reject names starting with `.` (matches the chat/notes is_safe_id
  // policy and prevents a malicious GitHub `name = "..bashrc"` from
  // landing as a hidden file in ~/Downloads — audit Sec MED #15).
  if name.starts_with('.') {
    return Err(format!("leading dot in filename not allowed: {name:?}"));
  }
  match p.file_name().and_then(|n| n.to_str()) {
    Some(n) if n == name => Ok(()),
    _ => Err(format!("path separator or traversal segment: {name:?}")),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn needs_redraw_defaults_to_true_so_first_frame_draws() {
    let app = App::new();
    assert!(app.needs_redraw);
  }

  #[test]
  fn check_needs_redraw_reads_and_clears() {
    let mut app = App::new();
    assert!(app.check_needs_redraw(), "first call returns true");
    assert!(
      !app.check_needs_redraw(),
      "second call returns false (flag cleared)"
    );
    app.mark_dirty();
    assert!(app.check_needs_redraw(), "mark_dirty re-arms the flag");
    assert!(!app.check_needs_redraw(), "and clears again on the next read");
  }

  #[test]
  fn mark_dirty_is_idempotent() {
    let mut app = App::new();
    let _ = app.check_needs_redraw(); // clear
    app.mark_dirty();
    app.mark_dirty();
    app.mark_dirty();
    assert!(app.check_needs_redraw(), "still just one redraw needed");
    assert!(!app.check_needs_redraw());
  }

  #[test]
  fn has_active_animation_false_on_idle_app() {
    let mut app = App::new();
    let _ = app.check_needs_redraw();
    // Default App: not loading, not refreshing, no save TTL, no repo ctx,
    // no discovery — should be inert.
    assert!(!app.has_active_animation());
  }

  #[test]
  fn rebuild_indices_maps_every_item() {
    // Post-C9 (ADR-007), this verifies the App-level integration:
    // after seeding items_store via from_items, every URL still maps
    // back to its position. The type-level guarantee lives on
    // ItemStore — see its own #[cfg(test)] block.
    let mut app = App::new();
    app.workspace.items_store =
      crate::data::ItemStore::from_items(mock_items());
    for (idx, item) in app.workspace.items_store.iter().enumerate() {
      assert_eq!(
        app.workspace.items_store.find_index_by_url(&item.url),
        Some(idx),
      );
    }
    for (idx, item) in app.workspace.items_store.iter().enumerate() {
      if let Some(aid) = arxiv_id_from_url(&item.url) {
        assert_eq!(
          app.workspace.items_store.find_index_by_arxiv_id(aid),
          Some(idx),
        );
      }
    }
  }

  #[test]
  fn validate_download_name_accepts_plain_filenames() {
    assert!(super::validate_download_name("foo.zip").is_ok());
    assert!(super::validate_download_name("README.md").is_ok());
    assert!(super::validate_download_name("file_name-1.txt").is_ok());
  }

  #[test]
  fn validate_download_name_rejects_leading_dot() {
    // `..foo`, `.bashrc`, etc. — would land as hidden files in ~/Downloads
    // if the upstream `name` from GitHub is hostile.
    assert!(super::validate_download_name("..foo").is_err());
    assert!(super::validate_download_name(".bashrc").is_err());
    assert!(super::validate_download_name(".hidden").is_err());
  }

  #[test]
  fn validate_download_name_rejects_traversal() {
    assert!(super::validate_download_name("../etc/passwd").is_err());
    assert!(super::validate_download_name("..").is_err());
  }

  #[test]
  fn validate_download_name_rejects_absolute_paths() {
    assert!(super::validate_download_name("/etc/passwd").is_err());
    assert!(super::validate_download_name("/foo.zip").is_err());
  }

  #[test]
  fn validate_download_name_rejects_path_separators() {
    assert!(super::validate_download_name("dir/file").is_err());
    assert!(super::validate_download_name("a/b/c").is_err());
  }

  // Note (C9 / ADR-007): the legacy `rebuild_indices_clears_stale_entries`
  // test asserted that a manual `items.truncate(N)` followed by a
  // `rebuild_indices()` shrank `url_index` in lockstep. Post-C9 there's
  // no public API to make `items_store` carry stale entries — the
  // invariant is type-enforced. The test is superseded by
  // `ItemStore::tests::rebuild_indices_is_idempotent`.

  #[test]
  fn item_counts_breaks_down_workflow_states() {
    let mut app = App::new();
    app.workspace.items_store =
      crate::data::ItemStore::from_items(mock_items());
    let counts = app.item_counts();
    assert_eq!(counts.total, app.workspace.items_store.len());
    // Sum invariant: every item lives in exactly one workflow bucket.
    assert_eq!(
      counts.inbox + counts.queued + counts.deep_read + counts.archived,
      counts.total
    );
    // mock_items() is the source of truth; just spot-check that all four
    // buckets have at least one entry, otherwise the fused-pass match
    // would have a silent fall-through bug.
    assert!(counts.inbox > 0 && counts.queued > 0);
    assert!(counts.deep_read > 0 && counts.archived > 0);
  }

  #[test]
  fn item_counts_queue_preview_caps_at_two() {
    let mut app = App::new();
    app.workspace.items_store =
      crate::data::ItemStore::from_items(mock_items());
    let counts = app.item_counts();
    assert!(counts.queued >= 2, "fixture must have at least 2 queued items");
    assert_eq!(
      counts.queue_preview.len(),
      2,
      "queue_preview must cap at the first 2 queued titles"
    );
  }

  #[test]
  fn invalidate_counts_cache_forces_recompute() {
    let mut app = App::new();
    app.workspace.items_store =
      crate::data::ItemStore::from_items(mock_items());
    // Snapshot the cached total in a scoped block so the Ref drops before
    // we mutate `app.workspace.items` below.
    let before_total = app.item_counts().total;
    // Mutate items directly, bypassing the public mutators that would
    // normally call invalidate_counts_cache. The cache should still hold
    // the stale value until we invalidate by hand.
    app.workspace.items_store.clear();
    let stale_total = app.item_counts().total;
    assert_eq!(stale_total, before_total, "cache survives raw mutation");
    app.invalidate_counts_cache();
    let fresh_total = app.item_counts().total;
    assert_eq!(fresh_total, 0, "post-invalidation, recompute sees empty items");
  }

  #[test]
  fn filter_source_names_caches_until_invalidated() {
    let mut app = App::new();
    app.workspace.items_store =
      crate::data::ItemStore::from_items(mock_items());

    // First call computes and caches.
    let before = app.filter_source_names();
    assert!(!before.is_empty());

    // Mutate items directly without invalidation — cache holds stale value.
    app.workspace.items_store.clear();
    let stale = app.filter_source_names();
    assert_eq!(stale, before, "cache survives raw mutation");

    // Invalidate; next call recomputes (just the seeds, since items empty).
    app.invalidate_filter_source_names_cache();
    let fresh = app.filter_source_names();
    assert!(fresh.contains(&"arxiv".to_string()));
    assert!(fresh.contains(&"hf".to_string()));
    // No source labels beyond the always-included seeds when items is empty.
    assert_eq!(fresh.len(), 2);
  }

  #[test]
  fn reset_items_clears_indices() {
    let mut app = App::new();
    app.workspace.items_store =
      crate::data::ItemStore::from_items(mock_items());
    // Fixture verification: from_items populated both items and indices.
    assert!(!app.workspace.items_store.is_empty());
    assert!(
      app
        .workspace
        .items_store
        .find_index_by_url("https://arxiv.org/abs/1")
        .is_some()
        || app.workspace.items_store.iter().any(|i| !i.url.is_empty()),
      "fixture must produce indexed items",
    );

    app.reset_items();

    // ItemStore::clear collapses items + both indices in lockstep
    // (ADR-007 §S1) — no separate field-level assertions needed.
    assert!(app.workspace.items_store.is_empty());
    assert!(app.workspace.items_store.find_index_by_url("any").is_none());
  }

  #[test]
  fn reset_discovery_items_clears_indices() {
    let mut app = App::new();
    app.discovery.items = mock_items();
    // Manually populate the discovery indices to mirror what
    // merge_discovery_items would do; rebuild_indices targets the primary
    // items vec, not discovery_items.
    for (idx, item) in app.discovery.items.iter().enumerate() {
      app.discovery.url_index.insert(item.url.clone(), idx);
    }
    assert!(!app.discovery.url_index.is_empty());

    app.reset_discovery_items();

    assert!(app.discovery.items.is_empty());
    assert!(app.discovery.url_index.is_empty());
    assert!(app.discovery.arxiv_id_index.is_empty());
  }

  #[test]
  fn has_active_animation_true_when_loading() {
    let mut app = App::new();
    let _ = app.check_needs_redraw();
    app.is_loading = true;
    assert!(app.has_active_animation());
    app.is_loading = false;
    app.is_refreshing = true;
    assert!(app.has_active_animation());
    app.is_refreshing = false;
    app.discovery.loading = true;
    assert!(app.has_active_animation());
  }

  #[allow(dead_code)]
  fn mock_items() -> Vec<FeedItem> {
    vec![
      FeedItem {
        id: "1".into(),
        title: "Attention Is All You Need: Revisited".into(),
        source_platform: SourcePlatform::ArXiv,
        content_type: ContentType::Paper,
        domain_tags: vec!["transformers".into(), "nlp".into()],
        signal: SignalLevel::Primary,
        published_at: "2026-03-15".into(),
        authors: vec!["Vaswani, A.".into(), "Shazeer, N.".into()],
        summary_short: "A retrospective look at the transformer architecture \
        five years on, with ablations on modern hardware."
          .into(),
        workflow_state: WorkflowState::Inbox,
        url: "https://arxiv.org/abs/2603.00001".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "2".into(),
        title: "Mamba-2: State Space Models at Scale".into(),
        source_platform: SourcePlatform::ArXiv,
        content_type: ContentType::Paper,
        domain_tags: vec!["ssm".into(), "efficiency".into()],
        signal: SignalLevel::Primary,
        published_at: "2026-03-14".into(),
        authors: vec!["Gu, A.".into(), "Dao, T.".into()],
        summary_short: "Extends Mamba with structured state space duality \
        enabling better scaling laws."
          .into(),
        workflow_state: WorkflowState::Queued,
        url: "https://arxiv.org/abs/2603.00002".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "3".into(),
        title: "Flash Attention 3 benchmarks on H100".into(),
        source_platform: SourcePlatform::Twitter,
        content_type: ContentType::Thread,
        domain_tags: vec!["cuda".into(), "attention".into()],
        signal: SignalLevel::Secondary,
        published_at: "2026-03-13".into(),
        authors: vec!["tri_dao".into()],
        summary_short: "Thread covering FA3 throughput numbers versus \
        cuDNN on H100 SXM across sequence lengths."
          .into(),
        workflow_state: WorkflowState::Inbox,
        url: "https://twitter.com/tri_dao/status/000001".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "4".into(),
        title: "Building production RAG pipelines without the hype".into(),
        source_platform: SourcePlatform::Blog,
        content_type: ContentType::Article,
        domain_tags: vec!["rag".into(), "production".into()],
        signal: SignalLevel::Secondary,
        published_at: "2026-03-12".into(),
        authors: vec!["Hamel Husain".into()],
        summary_short: "Practical notes on chunking strategies, reranking, \
        and eval harnesses for retrieval-augmented generation."
          .into(),
        workflow_state: WorkflowState::Inbox,
        url: "https://hamel.dev/blog/rag-prod".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "5".into(),
        title: "open-instruct: finetuning LLMs at AllenAI".into(),
        source_platform: SourcePlatform::Blog,
        content_type: ContentType::Repo,
        domain_tags: vec!["finetuning".into(), "rlhf".into()],
        signal: SignalLevel::Primary,
        published_at: "2026-03-11".into(),
        authors: vec!["AllenAI".into()],
        summary_short: "Open-source recipe for instruction tuning and \
        RLHF used in Tulu 3, with full training configs."
          .into(),
        workflow_state: WorkflowState::DeepRead,
        url: "https://github.com/allenai/open-instruct".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "6".into(),
        title: "The Batch — Issue 247: Agents in the wild".into(),
        source_platform: SourcePlatform::Newsletter,
        content_type: ContentType::Digest,
        domain_tags: vec!["agents".into(), "weekly".into()],
        signal: SignalLevel::Tertiary,
        published_at: "2026-03-10".into(),
        authors: vec!["Andrew Ng".into()],
        summary_short: "Weekly digest covering agentic system deployments, \
        tooling updates, and model releases."
          .into(),
        workflow_state: WorkflowState::Archived,
        url: "https://deeplearning.ai/the-batch/issue-247".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "7".into(),
        title: "Constitutional AI: Harmlessness from AI Feedback".into(),
        source_platform: SourcePlatform::ArXiv,
        content_type: ContentType::Paper,
        domain_tags: vec!["alignment".into(), "rlhf".into()],
        signal: SignalLevel::Primary,
        published_at: "2026-03-09".into(),
        authors: vec!["Bai, Y.".into(), "Jones, A.".into()],
        summary_short: "Introduces CAI, a method for training harmless AI \
        assistants using AI-generated feedback without human labels."
          .into(),
        workflow_state: WorkflowState::Queued,
        url: "https://arxiv.org/abs/2212.08073".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "8".into(),
        title: "Why every ML team needs an evals culture".into(),
        source_platform: SourcePlatform::Blog,
        content_type: ContentType::Article,
        domain_tags: vec!["evals".into(), "mlops".into()],
        signal: SignalLevel::Secondary,
        published_at: "2026-03-08".into(),
        authors: vec!["Jason Wei".into()],
        summary_short: "Argues for treating evals as first-class engineering, \
        with examples from production LLM deployments."
          .into(),
        workflow_state: WorkflowState::Inbox,
        url: "https://jasonwei.net/blog/evals-culture".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "9".into(),
        title:
          "vLLM v0.5 release notes — prefix caching and speculative decoding"
            .into(),
        source_platform: SourcePlatform::Blog,
        content_type: ContentType::Repo,
        domain_tags: vec!["inference".into(), "serving".into()],
        signal: SignalLevel::Primary,
        published_at: "2026-03-07".into(),
        authors: vec!["vLLM Team".into()],
        summary_short: "v0.5 ships automatic prefix caching and draft-model \
        speculative decoding, cutting median TTFT by 40%."
          .into(),
        workflow_state: WorkflowState::Inbox,
        url: "https://github.com/vllm-project/vllm/releases/v0.5".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "10".into(),
        title: "Mixture of Experts: a practical guide".into(),
        source_platform: SourcePlatform::Newsletter,
        content_type: ContentType::Digest,
        domain_tags: vec!["moe".into(), "architecture".into()],
        signal: SignalLevel::Secondary,
        published_at: "2026-03-06".into(),
        authors: vec!["Sebastian Raschka".into()],
        summary_short: "Deep-dive into MoE routing strategies, load balancing \
        losses, and differences between Switch, GLaM, and Mixtral."
          .into(),
        workflow_state: WorkflowState::Queued,
        url: "https://magazine.sebastianraschka.com/p/moe-guide".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "11".into(),
        title: "Context length scaling beyond 1M tokens".into(),
        source_platform: SourcePlatform::Twitter,
        content_type: ContentType::Thread,
        domain_tags: vec!["context".into(), "long-range".into()],
        signal: SignalLevel::Secondary,
        published_at: "2026-03-05".into(),
        authors: vec!["Greg Kamradt".into()],
        summary_short: "Empirical thread on attention sink patterns and \
        retrieval degradation at very long context windows."
          .into(),
        workflow_state: WorkflowState::Inbox,
        url: "https://twitter.com/GregKamradt/status/000002".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "12".into(),
        title: "LLM.int8(): 8-bit Matrix Multiplication for Transformers"
          .into(),
        source_platform: SourcePlatform::ArXiv,
        content_type: ContentType::Paper,
        domain_tags: vec!["quantisation".into(), "efficiency".into()],
        signal: SignalLevel::Primary,
        published_at: "2026-03-04".into(),
        authors: vec!["Dettmers, T.".into(), "Lewis, M.".into()],
        summary_short:
          "Introduces mixed-precision decomposition that preserves \
        full model quality at 8-bit with no fine-tuning."
            .into(),
        workflow_state: WorkflowState::DeepRead,
        url: "https://arxiv.org/abs/2208.07339".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "13".into(),
        title: "Toolformer: Language Models Can Teach Themselves to Use Tools"
          .into(),
        source_platform: SourcePlatform::ArXiv,
        content_type: ContentType::Paper,
        domain_tags: vec!["tool-use".into(), "agents".into()],
        signal: SignalLevel::Primary,
        published_at: "2026-03-03".into(),
        authors: vec!["Schick, T.".into()],
        summary_short: "Self-supervised method for teaching LLMs when and how \
        to call APIs, achieving strong performance with few examples."
          .into(),
        workflow_state: WorkflowState::Archived,
        url: "https://arxiv.org/abs/2302.04761".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "14".into(),
        title: "Practical notes on GRPO vs PPO for LLM alignment".into(),
        source_platform: SourcePlatform::Blog,
        content_type: ContentType::Article,
        domain_tags: vec!["rl".into(), "alignment".into()],
        signal: SignalLevel::Secondary,
        published_at: "2026-03-02".into(),
        authors: vec!["Leandro von Werra".into()],
        summary_short: "Side-by-side comparison of GRPO and PPO training \
        dynamics, memory usage, and sample efficiency on code tasks."
          .into(),
        workflow_state: WorkflowState::Inbox,
        url: "https://huggingface.co/blog/grpo-vs-ppo".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
      FeedItem {
        id: "15".into(),
        title: "axolotl: one config to fine-tune them all".into(),
        source_platform: SourcePlatform::Blog,
        content_type: ContentType::Repo,
        domain_tags: vec!["finetuning".into(), "tooling".into()],
        signal: SignalLevel::Tertiary,
        published_at: "2026-03-01".into(),
        authors: vec!["Wing Lian".into()],
        summary_short: "Unified fine-tuning framework supporting LoRA, QLoRA, \
        full-param and FSDP across multiple model families."
          .into(),
        workflow_state: WorkflowState::Inbox,
        url: "https://github.com/OpenAccess-AI-Collective/axolotl".into(),
        upvote_count: 0,
        github_repo: None,
        github_owner: None,
        github_repo_name: None,
        benchmark_results: vec![],
        full_content: None,
        source_name: String::new(),
        title_lower: String::new(),
        authors_lower: Vec::new(),
      },
    ]
  }
}

// ── Reader tab accessors ──────────────────────────────────────────────────────

/// Classify a repo file AND produce its syntect-highlighted lines in a
/// single pass. Previously two separate calls: `classify_repo_file_kind`
/// internally called `highlight_file` to decide Code vs PlainText, then
/// the caller called `highlight_file` AGAIN to get the highlighted
/// result — double work on the UI thread. This version highlights once
/// and returns both the kind and the highlighted output.
///
/// Returned `file_highlighted` is empty for non-Code kinds.
pub(crate) fn classify_and_highlight_repo_file(
  name: &str,
  content: &str,
) -> (RepoFileKind, Vec<Vec<(u8, u8, u8, String)>>) {
  let lower = name.to_ascii_lowercase();
  if lower.ends_with(".md")
    || lower.ends_with(".markdown")
    || lower == "readme"
    || lower.starts_with("readme.")
  {
    return (RepoFileKind::Markdown, Vec::new());
  }

  match crate::syntax::highlight_file(content, name) {
    Some(h) => (RepoFileKind::Code, h),
    None => (RepoFileKind::PlainText, Vec::new()),
  }
}
