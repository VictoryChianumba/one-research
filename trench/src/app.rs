use crate::config::{Config, CustomThemeConfig};
use crate::discovery::DiscoveryMessage;
use crate::ingestion::message::FetchMessage;
use crate::models::*;
use chrono::Utc;
use ratatui::layout::Rect;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Filter state
// ---------------------------------------------------------------------------

pub struct FilterState {
  pub sources: HashSet<String>,
  pub signals: HashSet<SignalLevel>,
  pub content_types: HashSet<ContentType>,
  pub workflow_states: HashSet<WorkflowState>,
  pub tags: HashSet<String>,
}

impl Default for FilterState {
  fn default() -> Self {
    Self {
      sources: HashSet::new(),
      signals: HashSet::new(),
      content_types: HashSet::new(),
      workflow_states: HashSet::new(),
      tags: HashSet::new(),
    }
  }
}

impl FilterState {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn is_empty(&self) -> bool {
    self.sources.is_empty()
      && self.signals.is_empty()
      && self.content_types.is_empty()
      && self.workflow_states.is_empty()
      && self.tags.is_empty()
  }

  pub fn matches(&self, item: &FeedItem) -> bool {
    (self.sources.is_empty() || {
      let sname = if item.source_name.is_empty() {
        item.source_platform.short_label().to_string()
      } else {
        item.source_name.clone()
      };
      self.sources.contains(&sname)
    }) && (self.signals.is_empty() || self.signals.contains(&item.signal))
      && (self.content_types.is_empty()
        || self.content_types.contains(&item.content_type))
      && (self.workflow_states.is_empty()
        || self.workflow_states.contains(&item.workflow_state))
  }

  pub fn active_count(&self) -> usize {
    self.sources.len()
      + self.signals.len()
      + self.content_types.len()
      + self.workflow_states.len()
      + self.tags.len()
  }
}

// ---------------------------------------------------------------------------
// Quit popup
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuitPopupKind {
  #[default]
  QuitApp,
  QuitWithProgress,
  QuitWithChat,
  LeaveReader,
}

// ---------------------------------------------------------------------------
// Repo viewer types
// ---------------------------------------------------------------------------

#[derive(PartialEq)]
pub enum RepoPane {
  Tree,
  File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepoFileKind {
  Markdown,
  Code,
  PlainText,
}

pub struct RepoContext {
  pub owner: String,
  pub repo_name: String,
  pub default_branch: String,
  pub tree_path: String,
  pub tree_nodes: Vec<crate::github::TreeNode>,
  pub tree_cursor: usize,
  pub file_path: Option<String>,
  pub file_name: Option<String>,
  pub raw_file_content: String,
  pub file_kind: RepoFileKind,
  pub file_lines: Vec<String>,
  pub file_highlighted: Vec<Vec<(u8, u8, u8, String)>>,
  pub markdown_cache: Option<crate::ui::repo_markdown::MarkdownRenderCache>,
  pub rendered_line_count: usize,
  pub markdown_has_pannable_lines: bool,
  pub file_scroll: usize,
  pub pane_focus: RepoPane,
  pub status_message: Option<String>,
  pub no_token: bool,
  /// Horizontal character offset for panning (file pane only).
  pub h_offset: usize,
  /// Effective render width (0 = use pane width). +/- keys adjust this.
  pub wrap_width: usize,
  /// Momentum scroll velocity (lines/frame). Positive = down.
  pub scroll_velocity: f32,
}

/// Result from a background repo fetch operation.
pub enum RepoFetchResult {
  /// Initial repo open: default branch + root tree.
  RepoOpened {
    branch: String,
    tree: Result<Vec<crate::github::TreeNode>, String>,
  },
  /// Dir navigation (forward or back): path + tree.
  DirLoaded {
    path: String,
    result: Result<Vec<crate::github::TreeNode>, String>,
  },
  /// File view: path, filename, raw content.
  FileLoaded { path: String, name: String, result: Result<String, String> },
}

/// Result from the URL discovery pipeline.
#[derive(Clone)]
pub enum DiscoverResult {
  ArxivCategory(String),
  HuggingFaceAlreadyEnabled,
  RssFeed { url: String, name: String },
  Failed(String),
}

/// State machine for the "Add source" input in the sources popup.
pub enum SourcesDetectState {
  Idle,
  Detecting,
  Result(DiscoverResult),
}

/// Identifies a pane in the pane registry.
/// Discriminants are array indices — must stay contiguous from 0..PANE_COUNT.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum PaneId {
  Feed = 0,
  Reader = 1,
  Notes = 2,
  Details = 3,
  Chat = 4,
  SecondaryReader = 5,
  SecondaryNotes = 6,
}

const PANE_COUNT: usize = 7;

/// Which reader pane has focus in dual-reader (State 3) mode.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FocusedReader {
  Primary,
  Secondary,
}

/// One open paper inside a reader pane.  Holds a tread Reader plus
/// the per-tab image cache (`ImageState`) and an optional arXiv id
/// the reader was opened with — used by `:reload` to refetch.  When
/// the source isn't an arXiv paper (PDF / EPUB / HTML extract), we
/// pass plain-text lines into `tread::PaperData::from_plain_lines`
/// and `arxiv_id` stays `None`.
pub struct ReaderTab {
  pub title: String,
  pub arxiv_id: Option<String>,
  pub notes_context: Option<NotesContext>,
  pub reader: tread::Reader,
  pub image_state: tread::ImageState,
  /// Last (width, height) we passed to `reader.resize`. Used to skip
  /// no-op resize calls every frame in the steady state — `tread::Reader`
  /// doesn't guarantee its own short-circuit.
  pub last_resize: Option<(u16, u16)>,
}

/// One note document open in the notes pane.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct NotesTab {
  #[serde(alias = "article_id")]
  pub note_id: String,
  pub title: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotesMode {
  PaperNotes,
  Library,
  Capture,
}

#[derive(Clone, Debug)]
pub struct NotesContext {
  pub paper: notes::PaperRef,
  pub source_label: String,
}

impl NotesMode {
  pub fn title(self) -> &'static str {
    match self {
      Self::PaperNotes => "Paper Notes",
      Self::Library => "Notes Library",
      Self::Capture => "Capture",
    }
  }

  pub fn footer_label(self) -> &'static str {
    match self {
      Self::PaperNotes => "paper notes",
      Self::Library => "notes library",
      Self::Capture => "capture",
    }
  }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CustomThemeEditorMode {
  Palette,
  Name,
  Hex,
  DeleteConfirm,
}

#[derive(Clone)]
pub struct CustomThemeEditorState {
  pub theme: CustomThemeConfig,
  pub is_new: bool,
  pub mode: CustomThemeEditorMode,
  pub role_cursor: usize,
  pub hue_cursor: usize,
  pub shade_cursor: usize,
  pub edit_buf: String,
}

/// Tracks a pane's current screen position and open state.
#[derive(Clone)]
pub struct PaneInfo {
  pub id: PaneId,
  pub rect: Rect,
  pub is_open: bool,
}

impl PaneInfo {
  fn new(id: PaneId) -> Self {
    Self { id, rect: Rect::default(), is_open: false }
  }

  fn is_focusable(&self) -> bool {
    !matches!(self.id, PaneId::Details)
  }
}

/// Direction for spatial pane navigation.
#[derive(Clone, Copy, Debug)]
pub enum NavDirection {
  Left,
  Right,
  Up,
  Down,
}

#[derive(PartialEq)]
pub enum AppView {
  Feed,
  Settings,
  Sources,
  RepoViewer,
}

#[derive(PartialEq, Clone, Copy)]
pub enum FeedTab {
  Inbox,
  Library,
  Discoveries,
  History,
}

/// What action should be taken when Enter is pressed in the repo tree pane.
pub enum RepoEnterTarget {
  Dir(String),
  File(String, String), // path, filename
}

/// Memoized aggregate counts derived from `App.items`. Recomputed lazily on
/// the first read after `invalidate_counts_cache` clears the cell. Pair the
/// invalidation hook with every site that mutates a workflow_state, the
/// items length, or a published_at field — otherwise the cache will drift.
/// Replaces the 4 separate full-iter scans inside `draw_details_panel` and
/// the per-chip O(N) closure inside `draw_library_chip_bar`.
#[derive(Default, Clone)]
pub struct ItemCounts {
  pub inbox: usize,
  pub queued: usize,
  pub deep_read: usize,
  pub archived: usize,
  pub total: usize,

  pub recent_total: usize,
  pub recent_today: usize,
  pub recent_hf: usize,
  pub recent_arxiv: usize,
  pub recent_other: usize,

  /// First two queued paper titles, cloned so the memoized struct doesn't
  /// hold a borrow back into `App.items`.
  pub queue_preview: Vec<String>,
}

pub struct App {
  /// True when the UI needs to be redrawn. Set by `mark_dirty()`, cleared by
  /// `check_needs_redraw()`. Defaults to `true` so the first frame always draws.
  pub needs_redraw: bool,

  /// `url → index in self.items`. Maintained by the `process_incoming` dedup
  /// loop and rebuilt by `rebuild_indices` after sort. Replaces the previous
  /// `iter_mut().find(...)` linear scan, which was O(N×M) on every refresh.
  pub url_index: HashMap<String, usize>,
  /// `arxiv_id → index in self.items`. Same role as `url_index` for the
  /// HF/arXiv-collapse path.
  pub arxiv_id_index: HashMap<String, usize>,
  /// Same as `url_index` but for `discovery_items`.
  pub discovery_url_index: HashMap<String, usize>,
  /// Same as `arxiv_id_index` but for `discovery_items`.
  pub discovery_arxiv_id_index: HashMap<String, usize>,

  pub should_quit: bool,
  pub quit_popup_active: bool,
  pub quit_popup_kind: QuitPopupKind,

  pub items: Vec<FeedItem>,
  pub selected_index: usize,
  pub list_offset: usize,
  pub discovery_items: Vec<FeedItem>,
  pub discovery_selected_index: usize,
  pub discovery_list_offset: usize,
  pub discovery_rx: Option<Receiver<DiscoveryMessage>>,
  /// Last status line from the agent ("Searching…", "Found N papers", etc.).
  pub discovery_status: String,
  pub discovery_query: String,
  /// Lowercased mirror of `discovery_query`. Refreshed by the discovery
  /// mutator helpers below. Avoids the per-frame `to_lowercase` heap
  /// allocation in `draw_discovery_palette`.
  pub discovery_query_lower: String,
  /// Whether the persistent search bar at the bottom of Discoveries has focus.
  pub discovery_search_focused: bool,
  pub feed_tab: FeedTab,
  pub discovery_loading: bool,
  /// Accumulated agent message history — enables multi-turn refinement.
  pub discovery_session: crate::discovery::SessionHistory,
  /// Set by Ctrl+N — forces a fresh session even when history exists.
  pub discovery_force_new: bool,
  /// Classified intent of the current/last discovery query.
  pub discovery_intent: crate::discovery::intent::QueryIntent,
  /// When set by a slash command, overrides heuristic classification once.
  pub discovery_forced_intent: Option<crate::discovery::intent::QueryIntent>,
  /// Selected row index in the discovery slash-command palette.
  pub discovery_palette_selected: usize,
  /// Scroll offset for the discovery palette (for when suggestions exceed visible rows).
  pub discovery_palette_scroll: usize,
  /// Activity log — paper opens and discovery queries.
  pub history: Vec<crate::history::HistoryEntry>,
  pub history_filter: crate::history::HistoryFilter,
  pub history_selected_index: usize,
  pub history_list_offset: usize,
  /// Library tab: workflow-state filter chip + per-tab navigation.
  pub library_filter: crate::library::LibraryFilter,
  pub library_selected_index: usize,
  pub library_list_offset: usize,
  /// Library bulk-select state. `library_visual_mode` enables visual selection;
  /// the anchor row is captured at activation; selection always covers the
  /// contiguous range from anchor to current cursor.
  pub library_visual_mode: bool,
  pub library_visual_anchor: usize,
  pub library_selected_urls: HashSet<String>,
  /// Tag store: URL → list of tag names. Persisted to ~/.config/trench/tags.json.
  pub item_tags: crate::tags::ItemTags,
  /// Tag picker popup state.
  pub tag_picker_active: bool,
  pub tag_picker_input: String,
  pub tag_picker_selected: usize,
  pub tag_picker_target_urls: Vec<String>,
  pub search_query: String,
  /// Lowercased mirror of `search_query`. Populated by the search-mutator
  /// helpers (`push_search_char`, `pop_search_char`, `clear_search_query`).
  /// Read by every visible-items / filtered-history filter pass — caching it
  /// here avoids a `to_lowercase` heap alloc on every cache miss.
  pub search_query_lower: String,
  pub search_active: bool,
  pub status_message: Option<String>,
  pub persisted_states: HashMap<String, WorkflowState>,

  // Pane focus

  // Filter panel
  pub filter_focus: bool,
  pub filter_cursor: usize,
  pub active_filters: FilterState,

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
  pub notification: Option<String>,
  pub notification_item_id: Option<String>,
  pub details_scroll: usize,
  pub details_max_scroll: usize,
  /// URL of the item that was selected when details_scroll was last set.
  /// Used to reset scroll when the user moves to a different item.
  pub details_last_item_url: Option<String>,

  // Config (full, persisted)
  pub config: Config,

  // Active theme (mirrors config.theme; applied live each frame)
  pub active_theme: ui_theme::ThemeId,
  pub active_custom_theme_id: Option<String>,

  // Settings screen
  pub settings_field: usize,
  pub settings_editing: bool,
  pub settings_edit_buf: String,
  pub settings_github_token: String,
  pub settings_s2_key: String,
  pub settings_save_time: Option<std::time::Instant>,
  pub theme_picker_active: bool,
  pub theme_picker_cursor: usize,
  pub theme_picker_scroll: usize,
  pub theme_picker_original: Option<(ui_theme::ThemeId, Option<String>)>,
  pub custom_theme_editor: Option<CustomThemeEditorState>,

  // Sources popup
  pub sources_cursor: usize,
  pub sources_input: String,
  pub sources_input_active: bool,
  pub sources_detect_state: SourcesDetectState,
  pub sources_detect_rx: Option<std::sync::mpsc::Receiver<DiscoverResult>>,

  // Embedded notes pane
  pub notes_app: Option<notes::app::App>,
  pub notes_active: bool,
  pub notes_tabs: Vec<NotesTab>,
  pub notes_active_tab: usize,
  pub notes_mode: NotesMode,
  pub notes_context: Option<NotesContext>,
  pub secondary_notes_active: bool,
  pub secondary_notes_tabs: Vec<NotesTab>,
  pub secondary_notes_active_tab: usize,
  pub secondary_notes_mode: NotesMode,
  pub secondary_notes_context: Option<NotesContext>,

  // Embedded chat pane
  pub chat_ui: Option<chat::ChatUi>,
  pub chat_active: bool,
  pub chat_fullscreen: bool,
  pub chat_at_top: bool,

  // Embedded reader (hygg-reader) — tabbed
  pub reader_tabs: Vec<ReaderTab>,
  pub reader_active_tab: usize,
  pub reader_active: bool,

  // Floating reader popup (A1 — Ldr+Enter) — not tabbed, separate slot
  pub reader_popup_active: bool,
  pub reader_popup_rx: Option<Receiver<Result<tread::PaperData, String>>>,
  pub reader_popup_editor: Option<tread::Reader>,
  /// Image cache for the popup reader.  Mirrors the per-tab field on
  /// `ReaderTab`; needed because tread's image escapes are emitted
  /// post-draw, outside ratatui's frame buffer, against host-owned state.
  pub reader_popup_image_state: tread::ImageState,
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

  // Secondary split view (A2 — Ldr+f cycles three reader/feed states)
  // State 1: normal feed (reader_split_active=false, reader_dual_active=false)
  // State 2: feed 40% | reader 60%  (reader_split_active=true)
  // State 3: reader 50% | reader 50% + persistent bottom pane (reader_dual_active=true)
  pub reader_split_active: bool,
  pub reader_dual_active: bool,
  pub reader_secondary_tabs: Vec<ReaderTab>,
  pub reader_secondary_active_tab: usize,
  pub focused_reader: FocusedReader,
  pub fulltext_for_secondary: bool,
  pub fulltext_new_tab: bool,
  // True while waiting for [1]/[2] to choose which reader window gets a new tab.
  pub tab_window_prompt_active: bool,
  // Bottom pane in State 3 (summoned by Ldr+f, dismissed by q/Esc)
  pub reader_bottom_open: bool,    // pane is visible
  pub reader_bottom_focused: bool, // pane has keyboard focus
  pub reader_bottom_details: bool, // showing details (true) or feed list (false)
  pub reader_bottom_scroll: usize, // scroll offset for both feed and details
  pub narrow_feed_details_open: bool, // State 2: description popup over reader
  pub abstract_popup_active: bool, // Space: quick abstract view
  pub reader_feed_popup_selected: usize, // selected item in bottom feed list

  // Settings buffers for chat fields
  pub settings_claude_key: String,
  pub settings_openai_key: String,
  pub settings_default_chat_provider: String,

  // Last opened paper (shown in dashboard "Continue Reading")
  pub last_read: Option<String>,
  pub last_read_source: Option<String>,

  // Background fulltext fetch (article reader)
  pub fulltext_rx: Option<Receiver<Result<tread::PaperData, String>>>,
  pub fulltext_loading: bool,
  pub pending_fulltext_context: Option<NotesContext>,
  // Background repo fetch (repo viewer)
  pub repo_fetch_rx: Option<Receiver<RepoFetchResult>>,

  // Scroll debounce — prevents key-repeat and trackpad inertia flooding
  pub last_scroll_time: Option<Instant>,
  pub scroll_debounce_ms: u64,
  pub last_mouse_scroll_time: Option<Instant>,
  pub mouse_scroll_debounce_ms: u64,

  // Leader key + pane registry
  pub leader_active: bool,
  pub leader_activated_at: Option<Instant>,
  pub leader_timeout_ms: u64,
  pub focused_pane: PaneId,
  pub panes: [PaneInfo; PANE_COUNT],

  // Help overlay
  pub help_active: bool,
  pub help_section: usize,
  pub help_scroll: u16,

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
  /// Memoized `filtered_history` indices into `self.history`. Invalidated
  /// by history mutation, search_query mutation, history_filter mutation,
  /// and active_filters mutation.
  pub filtered_history_cache: RefCell<Option<Vec<usize>>>,
}

// Filter panel cursor positions are computed dynamically in
// `toggle_filter_at_cursor` based on the current source / tag counts. Static
// offsets aren't used anymore.

impl App {
  pub fn new() -> Self {
    Self {
      needs_redraw: true,
      url_index: HashMap::new(),
      arxiv_id_index: HashMap::new(),
      discovery_url_index: HashMap::new(),
      discovery_arxiv_id_index: HashMap::new(),
      should_quit: false,
      quit_popup_active: false,
      quit_popup_kind: QuitPopupKind::default(),
      items: Vec::new(),
      selected_index: 0,
      list_offset: 0,
      discovery_items: crate::store::discovery_cache::load(),
      discovery_selected_index: 0,
      discovery_list_offset: 0,
      discovery_rx: None,
      discovery_status: String::new(),
      discovery_query: String::new(),
      discovery_query_lower: String::new(),
      discovery_search_focused: false,
      feed_tab: FeedTab::Inbox,
      discovery_loading: false,
      discovery_session: crate::store::session::load(),
      discovery_force_new: false,
      discovery_intent: crate::discovery::intent::QueryIntent::default(),
      discovery_forced_intent: None,
      discovery_palette_selected: 0,
      discovery_palette_scroll: 0,
      history: crate::store::history::load(),
      history_filter: crate::history::HistoryFilter::default(),
      history_selected_index: 0,
      history_list_offset: 0,
      library_filter: crate::library::LibraryFilter::default(),
      library_selected_index: 0,
      library_list_offset: 0,
      library_visual_mode: false,
      library_visual_anchor: 0,
      library_selected_urls: HashSet::new(),
      item_tags: crate::store::tags::load(),
      tag_picker_active: false,
      tag_picker_input: String::new(),
      tag_picker_selected: 0,
      tag_picker_target_urls: Vec::new(),
      search_query: String::new(),
      search_query_lower: String::new(),
      search_active: false,
      status_message: None,
      persisted_states: HashMap::new(),
      fetch_rx: None,
      loading_sources: Vec::new(),
      loaded_sources: Vec::new(),
      is_loading: false,
      spinner_frame: 0,
      filter_focus: false,
      filter_cursor: 0,
      active_filters: FilterState::new(),
      view: AppView::Feed,
      repo_context: None,
      github_token: None,
      is_refreshing: false,
      notification: None,
      notification_item_id: None,
      details_scroll: 0,
      details_max_scroll: usize::MAX,
      details_last_item_url: None,
      config: Config::default(),
      active_theme: ui_theme::ThemeId::Dark,
      active_custom_theme_id: None,
      settings_field: 0,
      settings_editing: false,
      settings_edit_buf: String::new(),
      settings_github_token: String::new(),
      settings_s2_key: String::new(),
      settings_save_time: None,
      theme_picker_active: false,
      theme_picker_cursor: 0,
      theme_picker_scroll: 0,
      theme_picker_original: None,
      custom_theme_editor: None,
      sources_cursor: 0,
      sources_input: String::new(),
      sources_input_active: false,
      sources_detect_state: SourcesDetectState::Idle,
      sources_detect_rx: None,
      notes_app: None,
      notes_active: false,
      notes_tabs: Vec::new(),
      notes_active_tab: 0,
      notes_mode: NotesMode::Library,
      notes_context: None,
      secondary_notes_active: false,
      secondary_notes_tabs: Vec::new(),
      secondary_notes_active_tab: 0,
      secondary_notes_mode: NotesMode::Library,
      secondary_notes_context: None,
      chat_ui: None,
      chat_active: false,
      chat_fullscreen: false,
      chat_at_top: false,
      reader_tabs: Vec::new(),
      reader_active_tab: 0,
      reader_active: false,
      reader_popup_active: false,
      reader_popup_rx: None,
      reader_popup_editor: None,
      reader_popup_image_state: tread::ImageState::default(),
      voice_controller: tread::build_voice_controller(),
      kitty_supported: tread::detect_kitty_supported(),
      reader_split_active: false,
      reader_dual_active: false,
      reader_secondary_tabs: Vec::new(),
      reader_secondary_active_tab: 0,
      focused_reader: FocusedReader::Primary,
      fulltext_for_secondary: false,
      fulltext_new_tab: false,
      tab_window_prompt_active: false,
      reader_bottom_open: false,
      reader_bottom_focused: false,
      reader_bottom_details: false,
      narrow_feed_details_open: false,
      abstract_popup_active: false,
      reader_bottom_scroll: 0,
      reader_feed_popup_selected: 0,
      settings_claude_key: String::new(),
      settings_openai_key: String::new(),
      settings_default_chat_provider: "claude".to_string(),
      last_read: None,
      last_read_source: None,
      fulltext_rx: None,
      fulltext_loading: false,
      pending_fulltext_context: None,
      repo_fetch_rx: None,
      last_scroll_time: None,
      scroll_debounce_ms: 50,
      last_mouse_scroll_time: None,
      mouse_scroll_debounce_ms: 80,
      leader_active: false,
      leader_activated_at: None,
      leader_timeout_ms: 1000,
      focused_pane: PaneId::Feed,
      help_active: false,
      help_section: 0,
      help_scroll: 0,
      visible_cache: RefCell::new(None),
      counts_cache: RefCell::new(None),
      filter_source_names_cache: RefCell::new(None),
      filter_summary_cache: RefCell::new(None),
      filtered_history_cache: RefCell::new(None),
      panes: [
        PaneInfo::new(PaneId::Feed),
        PaneInfo::new(PaneId::Reader),
        PaneInfo::new(PaneId::Notes),
        PaneInfo::new(PaneId::Details),
        PaneInfo::new(PaneId::Chat),
        PaneInfo::new(PaneId::SecondaryReader),
        PaneInfo::new(PaneId::SecondaryNotes),
      ],
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

  pub fn pane(&self, id: PaneId) -> &PaneInfo {
    &self.panes[id as usize]
  }

  pub fn pane_mut(&mut self, id: PaneId) -> &mut PaneInfo {
    &mut self.panes[id as usize]
  }

  /// Called from layout every frame with the computed screen rects.
  /// Pass `None` for a pane that is not currently rendered.
  pub fn update_pane_rects(
    &mut self,
    feed: Option<Rect>,
    reader: Option<Rect>,
    notes: Option<Rect>,
    details: Option<Rect>,
    chat: Option<Rect>,
    secondary_reader: Option<Rect>,
    secondary_notes: Option<Rect>,
  ) {
    let updates: [(PaneId, Option<Rect>); PANE_COUNT] = [
      (PaneId::Feed, feed),
      (PaneId::Reader, reader),
      (PaneId::Notes, notes),
      (PaneId::Details, details),
      (PaneId::Chat, chat),
      (PaneId::SecondaryReader, secondary_reader),
      (PaneId::SecondaryNotes, secondary_notes),
    ];
    for (id, opt) in updates {
      let info = self.pane_mut(id);
      info.is_open = opt.is_some();
      if let Some(r) = opt {
        info.rect = r;
      }
    }
  }

  /// Returns the `PaneId` of the nearest open pane in the given direction,
  /// using center-to-center Euclidean distance among directional candidates.
  pub fn find_pane_in_direction(&self, dir: NavDirection) -> Option<PaneId> {
    let current = self.pane(self.focused_pane);
    if !current.is_open {
      return None;
    }
    let cx = current.rect.x as i32 + current.rect.width as i32 / 2;
    let cy = current.rect.y as i32 + current.rect.height as i32 / 2;

    self
      .panes
      .iter()
      .filter(|p| {
        p.id != self.focused_pane
          && p.is_open
          && p.is_focusable()
          && p.rect.width > 0
          && p.rect.height > 0
      })
      .filter(|p| {
        let px = p.rect.x as i32;
        let py = p.rect.y as i32;
        let pw = p.rect.width as i32;
        let ph = p.rect.height as i32;
        match dir {
          NavDirection::Right => px + pw / 2 > cx,
          NavDirection::Left => px + pw / 2 < cx,
          NavDirection::Down => py + ph / 2 > cy,
          NavDirection::Up => py + ph / 2 < cy,
        }
      })
      .min_by_key(|p| {
        let pcx = p.rect.x as i32 + p.rect.width as i32 / 2;
        let pcy = p.rect.y as i32 + p.rect.height as i32 / 2;
        (pcx - cx) * (pcx - cx) + (pcy - cy) * (pcy - cy)
      })
      .map(|p| p.id)
  }

  /// Returns the `PaneId` of the open pane whose rect contains the given
  /// terminal cell, or `None` if no open pane covers that cell.
  pub fn pane_at(&self, col: u16, row: u16) -> Option<PaneId> {
    self
      .panes
      .iter()
      .filter(|p| p.is_open && p.rect.width > 0 && p.rect.height > 0)
      .find(|p| {
        col >= p.rect.x
          && col < p.rect.x + p.rect.width
          && row >= p.rect.y
          && row < p.rect.y + p.rect.height
      })
      .map(|p| p.id)
  }

  /// Returns the focusable open pane whose rect contains the given terminal
  /// cell. Passive panes like Details remain hit-testable via `pane_at` but do
  /// not receive focus.
  pub fn focusable_pane_at(&self, col: u16, row: u16) -> Option<PaneId> {
    self
      .panes
      .iter()
      .filter(|p| {
        p.is_open && p.is_focusable() && p.rect.width > 0 && p.rect.height > 0
      })
      .find(|p| {
        col >= p.rect.x
          && col < p.rect.x + p.rect.width
          && row >= p.rect.y
          && row < p.rect.y + p.rect.height
      })
      .map(|p| p.id)
  }

  /// Returns secondary open panes sorted top-to-bottom then left-to-right.
  pub fn secondary_panes_sorted(&self) -> Vec<PaneId> {
    let primary =
      if self.reader_active { PaneId::Reader } else { PaneId::Feed };
    let mut secondaries: Vec<&PaneInfo> = self
      .panes
      .iter()
      .filter(|p| p.id != primary && p.is_open && p.is_focusable())
      .collect();
    secondaries.sort_by_key(|p| (p.rect.y, p.rect.x));
    secondaries.iter().map(|p| p.id).collect()
  }

  /// Set the redraw flag. Cheap — call from any code path that mutates
  /// state visible to the user. Mirrors `cli-text-reader::Editor::mark_dirty`
  /// so the embedded reader and trench's outer UI use identical semantics.
  pub fn mark_dirty(&mut self) {
    self.needs_redraw = true;
  }

  /// Rebuild the `url_index` and `arxiv_id_index` HashMaps from `self.items`.
  /// Call after any bulk mutation that invalidates positions: cache load,
  /// `items.sort_by`, deletions. The intra-batch dedup in `process_incoming`
  /// maintains the indices incrementally so this rebuild is rare.
  pub fn rebuild_indices(&mut self) {
    self.url_index.clear();
    self.arxiv_id_index.clear();
    self.url_index.reserve(self.items.len());
    for (idx, item) in self.items.iter().enumerate() {
      self.url_index.insert(item.url.clone(), idx);
      if let Some(aid) = arxiv_id_from_url(&item.url) {
        self.arxiv_id_index.insert(aid.to_string(), idx);
      }
    }
  }

  /// Same as `rebuild_indices` but for `discovery_items`.
  pub fn rebuild_discovery_indices(&mut self) {
    self.discovery_url_index.clear();
    self.discovery_arxiv_id_index.clear();
    self.discovery_url_index.reserve(self.discovery_items.len());
    for (idx, item) in self.discovery_items.iter().enumerate() {
      self.discovery_url_index.insert(item.url.clone(), idx);
      if let Some(aid) = arxiv_id_from_url(&item.url) {
        self.discovery_arxiv_id_index.insert(aid.to_string(), idx);
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
    if self.is_loading || self.is_refreshing || self.discovery_loading {
      return true;
    }
    if self.settings_save_time.is_some() {
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
        if tab == self.feed_tab {
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
        if *tab == self.feed_tab {
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
        if tab == self.feed_tab {
          let items = self.items_for_tab();
          return indices.iter().map(|&i| &items[i]).collect();
        }
      }
    }
    let q = self.search_query_lower.as_str();
    let items = self.items_for_tab();
    let indices: Vec<usize> = items
      .iter()
      .enumerate()
      .filter(|(_, item)| {
        // Tab-scoped pre-filter: Inbox shows only Inbox-state items, Library
        // shows whichever workflow chip is active.
        match self.feed_tab {
          FeedTab::Inbox => {
            if item.workflow_state != WorkflowState::Inbox {
              return false;
            }
          }
          FeedTab::Library => {
            if !self.library_filter.matches(item.workflow_state) {
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
        if !self.active_filters.tags.is_empty() {
          let item_tags = crate::tags::for_url(&self.item_tags, &item.url);
          if !item_tags.iter().any(|t| self.active_filters.tags.contains(t)) {
            return false;
          }
        }
        self.active_filters.matches(item)
      })
      .map(|(i, _)| i)
      .collect();
    *self.visible_cache.borrow_mut() = Some((self.feed_tab, indices.clone()));
    indices.iter().map(|&i| &items[i]).collect()
  }

  pub fn visible_window(&self, start: usize, end: usize) -> Vec<&FeedItem> {
    {
      let cache = self.visible_cache.borrow();
      if let Some((tab, indices)) = cache.as_ref() {
        if *tab == self.feed_tab {
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

  pub(crate) fn invalidate_visible_cache(&self) {
    *self.visible_cache.borrow_mut() = None;
  }

  pub(crate) fn invalidate_counts_cache(&self) {
    *self.counts_cache.borrow_mut() = None;
  }

  pub(crate) fn invalidate_filter_source_names_cache(&self) {
    *self.filter_source_names_cache.borrow_mut() = None;
  }

  pub(crate) fn invalidate_filter_summary_cache(&self) {
    *self.filter_summary_cache.borrow_mut() = None;
  }

  pub(crate) fn invalidate_filtered_history_cache(&self) {
    *self.filtered_history_cache.borrow_mut() = None;
  }

  /// Aggregate invalidator for every cache that derives from `app.items`.
  /// Call from any mutation site that changes the items vec or any item's
  /// workflow_state / source_name / published_at fields.
  pub(crate) fn invalidate_items_derived_caches(&self) {
    self.invalidate_counts_cache();
    self.invalidate_filter_source_names_cache();
  }

  /// Reset the primary items vec and every parallel index/cache that mirrors
  /// it. Direct `app.items.clear()` would leave `url_index` /
  /// `arxiv_id_index` populated with stale offsets and panic on the next
  /// `process_incoming` batch.
  pub fn reset_items(&mut self) {
    self.items.clear();
    self.url_index.clear();
    self.arxiv_id_index.clear();
    self.invalidate_visible_cache();
    self.invalidate_items_derived_caches();
  }

  /// Same shape as `reset_items` but for the discovery-side mirrors —
  /// keeps the parallel indexes in sync.
  pub fn reset_discovery_items(&mut self) {
    self.discovery_items.clear();
    self.discovery_url_index.clear();
    self.discovery_arxiv_id_index.clear();
    self.invalidate_visible_cache();
  }

  /// Cheap O(1) read from the memoized count cache. On miss, runs a single
  /// Fused pass over `self.items` that produces every counter the dashboard
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
    for item in &self.items {
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
    match self.feed_tab {
      FeedTab::Inbox => &self.items,
      FeedTab::Library => &self.items,
      FeedTab::Discoveries => &self.discovery_items,
      FeedTab::History => &[],
    }
  }

  fn items_for_tab_mut(&mut self) -> &mut Vec<FeedItem> {
    match self.feed_tab {
      FeedTab::Inbox => &mut self.items,
      FeedTab::Library => &mut self.items,
      FeedTab::Discoveries => &mut self.discovery_items,
      // History doesn't use FeedItem; callers should not dispatch here for this tab.
      FeedTab::History => &mut self.items,
    }
  }

  pub fn active_selected_index(&self) -> usize {
    match self.feed_tab {
      FeedTab::Inbox => self.selected_index,
      FeedTab::Library => self.library_selected_index,
      FeedTab::Discoveries => self.discovery_selected_index,
      FeedTab::History => self.history_selected_index,
    }
  }

  pub fn active_list_offset(&self) -> usize {
    match self.feed_tab {
      FeedTab::Inbox => self.list_offset,
      FeedTab::Library => self.library_list_offset,
      FeedTab::Discoveries => self.discovery_list_offset,
      FeedTab::History => self.history_list_offset,
    }
  }

  pub fn set_active_selected_index(&mut self, value: usize) {
    match self.feed_tab {
      FeedTab::Inbox => self.selected_index = value,
      FeedTab::Library => self.library_selected_index = value,
      FeedTab::Discoveries => self.discovery_selected_index = value,
      FeedTab::History => self.history_selected_index = value,
    }
  }

  pub fn set_active_list_offset(&mut self, value: usize) {
    match self.feed_tab {
      FeedTab::Inbox => self.list_offset = value,
      FeedTab::Library => self.library_list_offset = value,
      FeedTab::Discoveries => self.discovery_list_offset = value,
      FeedTab::History => self.history_list_offset = value,
    }
  }

  pub fn reset_active_feed_position(&mut self) {
    self.invalidate_visible_cache();
    self.set_active_selected_index(0);
    self.set_active_list_offset(0);
    self.details_scroll = 0;
    self.details_last_item_url = None;
  }

  pub fn set_notification(&mut self, msg: String) {
    let url = self.selected_item().map(|i| i.url.clone());
    // Sanitize at the chokepoint — set_notification is called from many
    // sites including ones that interpolate reqwest errors / GitHub
    // tree paths / API messages.
    self.notification = Some(crate::sanitize::sanitize_terminal_text(&msg));
    self.notification_item_id = url;
  }

  pub fn clear_notification(&mut self) {
    self.notification = None;
    self.notification_item_id = None;
  }

  pub fn move_down(&mut self) {
    let len = self.visible_count();
    if len == 0 {
      return;
    }
    let next = (self.active_selected_index() + 1).min(len - 1);
    self.set_active_selected_index(next);
    self.details_scroll = 0;
    self.clear_notification();
  }

  pub fn move_up(&mut self) {
    self.set_active_selected_index(
      self.active_selected_index().saturating_sub(1),
    );
    self.details_scroll = 0;
    self.clear_notification();
  }

  pub fn go_to_top(&mut self) {
    self.set_active_selected_index(0);
    self.details_scroll = 0;
    self.clear_notification();
  }

  pub fn go_to_bottom(&mut self) {
    let len = self.visible_count();
    if len > 0 {
      self.set_active_selected_index(len - 1);
    }
    self.details_scroll = 0;
    self.clear_notification();
  }

  /// Called by the renderer each frame with the computed max scroll for the
  /// details pane. Keeps `details_scroll` bounded without the renderer needing
  /// to mutate scroll state itself.
  pub fn set_details_max_scroll(&mut self, max: usize) {
    self.details_max_scroll = max;
    if self.details_scroll > max {
      self.details_scroll = max;
    }
  }

  pub fn push_search_char(&mut self, c: char) {
    self.search_query.push(c);
    self.search_query_lower = self.search_query.to_lowercase();
    self.invalidate_filtered_history_cache();
    self.reset_active_feed_position();
  }

  pub fn pop_search_char(&mut self) {
    self.search_query.pop();
    self.search_query_lower = self.search_query.to_lowercase();
    self.invalidate_filtered_history_cache();
    self.reset_active_feed_position();
  }

  /// Clear the search bar. Pairs with `push_search_char` / `pop_search_char`
  /// so the lowercased mirror never drifts out of sync with `search_query`.
  /// Also invalidates the visible-items cache so stale filtered results
  /// can't survive across the clear (was a "callers are responsible" footgun).
  pub fn clear_search_query(&mut self) {
    self.search_query.clear();
    self.search_query_lower.clear();
    self.invalidate_visible_cache();
    self.invalidate_filtered_history_cache();
  }

  /// Mirror of `push_search_char` for the discovery palette.
  pub fn push_discovery_char(&mut self, c: char) {
    self.discovery_query.push(c);
    self.discovery_query_lower = self.discovery_query.to_lowercase();
  }

  pub fn pop_discovery_char(&mut self) {
    self.discovery_query.pop();
    self.discovery_query_lower = self.discovery_query.to_lowercase();
  }

  pub fn clear_discovery_query(&mut self) {
    self.discovery_query.clear();
    self.discovery_query_lower.clear();
  }

  /// Set the discovery query to an arbitrary string (used by slash-palette
  /// completion). Refreshes the lowercased mirror.
  pub fn set_discovery_query(&mut self, s: String) {
    self.discovery_query = s;
    self.discovery_query_lower = self.discovery_query.to_lowercase();
  }

  pub fn selected_item(&self) -> Option<&FeedItem> {
    self.visible_get(self.active_selected_index())
  }

  /// Update library_selected_urls from anchor/cursor positions in the visible
  /// item list. Always covers the contiguous range from anchor to cursor.
  /// Used on entry to visual mode (full populate); cursor moves go through
  /// `library_extend_selection` for incremental update.
  pub fn library_recompute_selection(&mut self) {
    if !self.library_visual_mode {
      self.library_selected_urls.clear();
      return;
    }
    let cursor = self.library_selected_index;
    let anchor = self.library_visual_anchor;
    let (lo, hi) =
      if cursor <= anchor { (cursor, anchor) } else { (anchor, cursor) };
    let window = self.visible_window(lo, hi.saturating_add(1));
    self.library_selected_urls =
      window.iter().map(|it| it.url.clone()).collect();
  }

  /// Move the visual-mode cursor to `new_cursor` and update
  /// `library_selected_urls` incrementally — adds/removes only the items at
  /// the boundary between the old and new selection ranges. Single-step j/k
  /// is one HashSet insert or remove per keystroke, instead of cloning every
  /// URL in [lo..=hi]. Falls back to a no-op when not in visual mode.
  pub fn library_extend_selection(&mut self, new_cursor: usize) {
    if !self.library_visual_mode {
      self.library_selected_index = new_cursor;
      return;
    }
    let anchor = self.library_visual_anchor;
    let old_cursor = self.library_selected_index;
    let old_lo = old_cursor.min(anchor);
    let old_hi = old_cursor.max(anchor);
    let new_lo = new_cursor.min(anchor);
    let new_hi = new_cursor.max(anchor);

    // Materialize URL changes into local Vec<String>s so the
    // visible_window borrow (which holds &self) drops before we touch
    // the `library_selected_urls: HashSet<String>` field. For typical
    // single-step j/k moves each Vec has 0 or 1 entries.
    let removed_urls: Vec<String> = {
      let mut out = Vec::new();
      if new_lo > old_lo {
        out.extend(
          self
            .visible_window(old_lo, new_lo)
            .iter()
            .map(|it| it.url.clone()),
        );
      }
      if new_hi < old_hi {
        out.extend(
          self
            .visible_window(new_hi.saturating_add(1), old_hi.saturating_add(1))
            .iter()
            .map(|it| it.url.clone()),
        );
      }
      out
    };
    let added_urls: Vec<String> = {
      let mut out = Vec::new();
      if new_lo < old_lo {
        out.extend(
          self
            .visible_window(new_lo, old_lo)
            .iter()
            .map(|it| it.url.clone()),
        );
      }
      if new_hi > old_hi {
        out.extend(
          self
            .visible_window(old_hi.saturating_add(1), new_hi.saturating_add(1))
            .iter()
            .map(|it| it.url.clone()),
        );
      }
      out
    };

    for url in &removed_urls {
      self.library_selected_urls.remove(url);
    }
    for url in added_urls {
      self.library_selected_urls.insert(url);
    }

    self.library_selected_index = new_cursor;
  }

  pub fn library_exit_visual(&mut self) {
    self.library_visual_mode = false;
    self.library_visual_anchor = 0;
    self.library_selected_urls.clear();
  }

  /// Apply a workflow-state transition to every selected item. Returns the
  /// number of items affected.
  pub fn apply_workflow_to_selection(
    &mut self,
    state: crate::models::WorkflowState,
  ) -> usize {
    if self.library_selected_urls.is_empty() {
      return 0;
    }
    let urls: Vec<String> =
      self.library_selected_urls.iter().cloned().collect();
    let mut count = 0;
    for url in urls {
      for item in self.items.iter_mut() {
        if item.url == url {
          item.workflow_state = state;
          self.persisted_states.insert(url.clone(), state);
          count += 1;
          break;
        }
      }
    }
    crate::store::save(&self.persisted_states);
    self.invalidate_visible_cache();
    self.invalidate_items_derived_caches();
    count
  }

  /// Open the tag picker for a list of target URLs (single item or multi-select).
  pub fn open_tag_picker(&mut self, target_urls: Vec<String>) {
    if target_urls.is_empty() {
      return;
    }
    self.tag_picker_target_urls = target_urls;
    self.tag_picker_input.clear();
    self.tag_picker_selected = 0;
    self.tag_picker_active = true;
  }

  pub fn close_tag_picker(&mut self) {
    self.tag_picker_active = false;
    self.tag_picker_input.clear();
    self.tag_picker_selected = 0;
    self.tag_picker_target_urls.clear();
  }

  /// Toggle a tag on every target URL. If any target lacks the tag, add it to all;
  /// otherwise remove from all (idempotent toggle).
  pub fn toggle_tag_on_targets(&mut self, tag: &str) {
    let tag = crate::tags::normalize(tag);
    if tag.is_empty() {
      return;
    }
    let urls = self.tag_picker_target_urls.clone();
    let any_missing = urls.iter().any(|url| {
      !crate::tags::for_url(&self.item_tags, url).iter().any(|t| t == &tag)
    });
    for url in &urls {
      if any_missing {
        crate::tags::add(&mut self.item_tags, url, tag.clone());
      } else {
        crate::tags::remove(&mut self.item_tags, url, &tag);
      }
    }
    crate::store::tags::save(&self.item_tags);
    self.invalidate_visible_cache();
  }

  pub fn show_quit_popup(&mut self) {
    let kind = if self.focused_pane == PaneId::Reader && self.reader_active {
      QuitPopupKind::LeaveReader
    } else if self.discovery_loading || self.is_loading {
      QuitPopupKind::QuitWithProgress
    } else if self.chat_active
      && self.chat_ui.as_ref().map_or(false, |c| !c.input.trim().is_empty())
    {
      QuitPopupKind::QuitWithChat
    } else {
      QuitPopupKind::QuitApp
    };
    self.quit_popup_active = true;
    self.quit_popup_kind = kind;
  }

  pub fn record_paper_open(&mut self, item: &FeedItem) {
    let meta = crate::history::HistoryPaperMeta {
      authors: item.authors.clone(),
      source_platform: item.source_platform.clone(),
      published_at: item.published_at.clone(),
      summary_short: item.summary_short.clone(),
    };
    let source = if item.source_name.is_empty() {
      item.source_platform.short_label().to_string()
    } else {
      item.source_name.clone()
    };
    crate::history::record_paper(
      &mut self.history,
      item.url.clone(),
      item.title.clone(),
      source,
      meta,
    );
    self.invalidate_filtered_history_cache();
    crate::store::history::save(&self.history);
  }

  pub fn record_discovery_query(
    &mut self,
    topic: &str,
    intent: crate::discovery::intent::QueryIntent,
  ) {
    crate::history::record_query(
      &mut self.history,
      topic.to_string(),
      intent.label(),
    );
    self.invalidate_filtered_history_cache();
    crate::store::history::save(&self.history);
  }

  pub fn filtered_history(&self) -> Vec<&crate::history::HistoryEntry> {
    if self.filtered_history_cache.borrow().is_none() {
      let indices = self.compute_filtered_history_indices();
      *self.filtered_history_cache.borrow_mut() = Some(indices);
    }
    let cache = self.filtered_history_cache.borrow();
    let indices = cache.as_ref().expect("populated above");
    indices.iter().map(|&i| &self.history[i]).collect()
  }

  fn compute_filtered_history_indices(&self) -> Vec<usize> {
    let now = chrono::Utc::now();
    let q = self.search_query_lower.as_str();
    let src_filter = &self.active_filters.sources;
    self
      .history
      .iter()
      .enumerate()
      .filter(|(_, e)| self.history_filter.matches(e, now))
      .filter(|(_, e)| q.is_empty() || e.title_lower.contains(q))
      .filter(|(_, e)| src_filter.is_empty() || src_filter.contains(&e.source))
      .map(|(i, _)| i)
      .collect()
  }

  pub fn handle_slash_command(&mut self, cmd: String) {
    let parsed = crate::commands::parser::parse_slash_command(&cmd);
    crate::commands::dispatch::dispatch_slash_command(self, parsed);
  }

  pub fn push_chat_assistant_message(&mut self, content: String) {
    if let Some(chat_ui) = self.chat_ui.as_mut() {
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
    if let Some(chat_ui) = self.chat_ui.as_mut() {
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

  pub fn close_repo_viewer(&mut self) {
    self.view = AppView::Feed;
    self.repo_context = None;
  }

  pub fn set_repo_status(&mut self, msg: impl Into<String>) {
    if let Some(ctx) = &mut self.repo_context {
      // Sanitize at the setter chokepoint — repo status renders into a
      // styled Span and reqwest error strings include URLs that may
      // carry attacker-supplied bytes via redirects.
      ctx.status_message =
        Some(crate::sanitize::sanitize_terminal_text(&msg.into()));
    }
  }

  /// Returns the action to take when Enter is pressed in the tree pane.
  pub fn repo_enter_target(&self) -> Option<RepoEnterTarget> {
    let ctx = self.repo_context.as_ref()?;
    if ctx.no_token {
      return None;
    }
    let node = ctx.tree_nodes.get(ctx.tree_cursor)?;
    match node.node_type {
      crate::github::NodeType::Dir => {
        Some(RepoEnterTarget::Dir(node.path.clone()))
      }
      crate::github::NodeType::File => {
        Some(RepoEnterTarget::File(node.path.clone(), node.name.clone()))
      }
    }
  }

  /// Returns the parent path for `b` (go up), or None if already at root.
  pub fn repo_back_target(&self) -> Option<String> {
    let ctx = self.repo_context.as_ref()?;
    if ctx.no_token || ctx.tree_path.is_empty() {
      return None;
    }
    let parent = match ctx.tree_path.rfind('/') {
      Some(pos) => ctx.tree_path[..pos].to_string(),
      None => String::new(),
    };
    Some(parent)
  }

  pub fn repo_apply_dir(
    &mut self,
    path: String,
    result: Result<Vec<crate::github::TreeNode>, String>,
  ) {
    let ctx = match self.repo_context.as_mut() {
      Some(c) => c,
      None => return,
    };
    match result {
      Ok(nodes) => {
        ctx.tree_path = path;
        ctx.tree_nodes = nodes;
        ctx.tree_cursor = 0;
        ctx.pane_focus = RepoPane::Tree;
        ctx.status_message = None;
      }
      Err(e) => {
        ctx.status_message = Some(crate::sanitize::sanitize_terminal_text(
          &format!("Error: {e}"),
        ));
      }
    }
  }

  pub fn repo_apply_file(
    &mut self,
    path: String,
    name: String,
    result: Result<String, String>,
  ) {
    let ctx = match self.repo_context.as_mut() {
      Some(c) => c,
      None => return,
    };
    match result {
      Ok(raw_content) => {
        let file_kind = classify_repo_file_kind(&name, &raw_content);
        let highlighted = match file_kind {
          RepoFileKind::Code => {
            crate::syntax::highlight_file(&raw_content, &name)
              .unwrap_or_default()
          }
          _ => Vec::new(),
        };
        let lines: Vec<String> =
          raw_content.lines().map(|l| l.to_string()).collect();
        ctx.file_path = Some(path);
        ctx.file_name = Some(name);
        ctx.raw_file_content = raw_content;
        ctx.file_kind = file_kind;
        ctx.file_lines = lines;
        ctx.file_highlighted = highlighted;
        ctx.markdown_cache = None;
        ctx.rendered_line_count = 0;
        ctx.markdown_has_pannable_lines = false;
        ctx.file_scroll = 0;
        ctx.h_offset = 0;
        ctx.scroll_velocity = 0.0;
        ctx.pane_focus = RepoPane::File;
        ctx.status_message = None;
      }
      Err(e) => {
        ctx.status_message = Some(crate::sanitize::sanitize_terminal_text(
          &format!("Error: {e}"),
        ));
      }
    }
  }

  pub fn repo_switch_pane(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      ctx.pane_focus = match ctx.pane_focus {
        RepoPane::Tree => RepoPane::File,
        RepoPane::File => RepoPane::Tree,
      };
    }
  }

  pub fn repo_nav_down(&mut self, file_visible_h: usize) {
    let _ = file_visible_h;
    if let Some(ctx) = &mut self.repo_context {
      match ctx.pane_focus {
        RepoPane::Tree => {
          let max = ctx.tree_nodes.len().saturating_sub(1);
          ctx.tree_cursor = (ctx.tree_cursor + 1).min(max);
        }
        RepoPane::File => {
          ctx.scroll_velocity += 3.0;
        }
      }
    }
  }

  pub fn repo_nav_up(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      match ctx.pane_focus {
        RepoPane::Tree => {
          ctx.tree_cursor = ctx.tree_cursor.saturating_sub(1);
        }
        RepoPane::File => {
          ctx.scroll_velocity -= 3.0;
        }
      }
    }
  }

  /// Advance momentum scroll by one frame.
  pub fn repo_tick(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      if ctx.scroll_velocity.abs() >= 0.5 {
        let delta = ctx.scroll_velocity.round() as i64;
        let line_count = match ctx.file_kind {
          RepoFileKind::Markdown => ctx.rendered_line_count,
          _ => ctx.file_lines.len(),
        };
        let max = line_count.saturating_sub(1) as i64;
        let next = (ctx.file_scroll as i64 + delta).clamp(0, max) as usize;
        ctx.file_scroll = next;
        ctx.scroll_velocity *= 0.75;
      } else {
        ctx.scroll_velocity = 0.0;
      }
    }
  }

  pub fn repo_pan_left(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      if ctx.file_kind == RepoFileKind::Markdown
        && !ctx.markdown_has_pannable_lines
      {
        return;
      }
      ctx.h_offset = ctx.h_offset.saturating_sub(4);
    }
  }

  pub fn repo_pan_right(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      if ctx.file_kind == RepoFileKind::Markdown
        && !ctx.markdown_has_pannable_lines
      {
        return;
      }
      ctx.h_offset += 4;
    }
  }

  pub fn repo_zoom_in(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      if ctx.file_kind != RepoFileKind::Markdown {
        return;
      }
      if ctx.wrap_width == 0 {
        // start from a sensible default — we don't know pane width here
        ctx.wrap_width = 120;
      }
      ctx.wrap_width = ctx.wrap_width.saturating_sub(10).max(20);
    }
  }

  pub fn repo_zoom_out(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      if ctx.file_kind != RepoFileKind::Markdown {
        return;
      }
      if ctx.wrap_width == 0 {
        ctx.wrap_width = 80;
      }
      ctx.wrap_width = ctx.wrap_width.saturating_add(10).min(200);
    }
  }

  /// Copy the currently selected path to clipboard.
  pub fn repo_copy_path(&mut self) {
    let path = if let Some(ctx) = &self.repo_context {
      match ctx.pane_focus {
        RepoPane::File => {
          ctx.file_path.clone().unwrap_or_else(|| ctx.tree_path.clone())
        }
        RepoPane::Tree => ctx
          .tree_nodes
          .get(ctx.tree_cursor)
          .map(|n| n.path.clone())
          .unwrap_or_default(),
      }
    } else {
      return;
    };

    // Strip any terminal-control bytes embedded in the GitHub-derived path
    // before writing to the OS clipboard. Otherwise a hostile repo could
    // ship ESC bytes that survive into whichever terminal the user later
    // pastes into.
    let safe_path = crate::sanitize::sanitize_terminal_text(&path);
    match arboard::Clipboard::new() {
      Ok(mut cb) => match cb.set_text(&safe_path) {
        Ok(()) => self.set_repo_status(format!("Copied: {safe_path}")),
        Err(e) => self.set_repo_status(format!("Clipboard error: {e}")),
      },
      Err(e) => self.set_repo_status(format!("Clipboard unavailable: {e}")),
    }
  }

  pub fn repo_copy_url(&mut self) {
    let Some(url) = self.repo_current_url() else {
      self.set_repo_status("No repo URL available.".to_string());
      return;
    };

    // Strip any terminal-control bytes from the GitHub-derived URL before
    // writing to the OS clipboard.
    let safe_url = crate::sanitize::sanitize_terminal_text(&url);
    match arboard::Clipboard::new() {
      Ok(mut cb) => match cb.set_text(&safe_url) {
        Ok(()) => self.set_repo_status(format!("Copied URL: {safe_url}")),
        Err(e) => self.set_repo_status(format!("Clipboard error: {e}")),
      },
      Err(e) => self.set_repo_status(format!("Clipboard unavailable: {e}")),
    }
  }

  pub fn repo_current_url(&self) -> Option<String> {
    let ctx = self.repo_context.as_ref()?;
    let base = format!("https://github.com/{}/{}", ctx.owner, ctx.repo_name);
    let branch = if ctx.default_branch.is_empty() {
      "HEAD"
    } else {
      ctx.default_branch.as_str()
    };

    match ctx.pane_focus {
      RepoPane::File => {
        let path = ctx.file_path.as_deref().or(ctx.file_name.as_deref())?;
        Some(format!("{base}/blob/{branch}/{}", encode_repo_url_path(path)))
      }
      RepoPane::Tree => {
        if let Some(node) = ctx.tree_nodes.get(ctx.tree_cursor) {
          let route = match node.node_type {
            crate::github::NodeType::Dir => "tree",
            crate::github::NodeType::File => "blob",
          };
          Some(format!(
            "{base}/{route}/{branch}/{}",
            encode_repo_url_path(&node.path)
          ))
        } else if ctx.tree_path.is_empty() {
          Some(base)
        } else {
          Some(format!(
            "{base}/tree/{branch}/{}",
            encode_repo_url_path(&ctx.tree_path)
          ))
        }
      }
    }
  }

  /// Save the current open file to ~/Downloads/{filename}.
  // (validate_download_name is defined as a free function below.)
  pub fn repo_download_file(&mut self) {
    let (name, content) = if let Some(ctx) = &self.repo_context {
      match (&ctx.file_name, &ctx.file_lines) {
        (Some(name), lines) if !lines.is_empty() => {
          (name.clone(), lines.join("\n"))
        }
        _ => return,
      }
    } else {
      return;
    };

    // Validate the GitHub-supplied filename against path-traversal. The
    // `name` field comes from `ctx.file_name` which originates in the
    // GitHub tree-listing response — a malicious or compromised repo could
    // populate it with `../etc/passwd` or `/etc/passwd`, both of which
    // `Path::join` happily accepts (an absolute join overwrites the base,
    // and `..` segments traverse up). Sec HIGH #4 from the audit.
    if let Err(e) = validate_download_name(&name) {
      self.set_repo_status(format!("Download rejected: {e}"));
      return;
    }

    let dest =
      dirs::download_dir().or_else(dirs::home_dir).map(|p| p.join(&name));

    if let Some(path) = dest {
      match std::fs::write(&path, &content) {
        Ok(()) => self.set_repo_status(format!("Saved to {}", path.display())),
        Err(e) => self.set_repo_status(format!("Download failed: {e}")),
      }
    }
  }

  pub fn repo_status_label(&self) -> Option<String> {
    let ctx = self.repo_context.as_ref()?;
    if ctx.no_token {
      Some("token required".to_string())
    } else {
      ctx.status_message.clone().or_else(|| Some("ready".to_string()))
    }
  }

  pub fn process_incoming(&mut self) {
    use std::sync::mpsc::TryRecvError;

    // Spinner only ticks when something is actually loading. Without this
    // gate, the wrapping increment fires every loop iteration and would
    // perpetually re-set `needs_redraw` even on idle.
    if self.is_loading {
      self.spinner_frame = self.spinner_frame.wrapping_add(1);
      self.mark_dirty();
    }
    self.poll_detect_result();
    self.process_incoming_discovery();

    // Clear "Saved." confirmation after 2 seconds.
    if let Some(t) = self.settings_save_time {
      if t.elapsed().as_secs() >= 2 {
        self.settings_save_time = None;
        self.mark_dirty();
      }
    }

    if self.fetch_rx.is_none() {
      return;
    }

    // Collect pending messages without blocking.
    let mut messages = Vec::new();
    let mut disconnected = false;

    if let Some(rx) = &self.fetch_rx {
      loop {
        match rx.try_recv() {
          Ok(msg) => messages.push(msg),
          Err(TryRecvError::Empty) => break,
          Err(TryRecvError::Disconnected) => {
            disconnected = true;
            break;
          }
        }
      }
    }

    if disconnected {
      self.is_loading = false;
      self.is_refreshing = false;
      self.fetch_rx = None;
    }

    let was_empty = self.items.is_empty();
    let mut had_structural_item_changes = false;
    let mut had_source_updates = false;
    let mut had_enriched_updates = false;

    for msg in messages {
      match msg {
        FetchMessage::Items(new_items) => {
          for mut item in new_items {
            // Apply any persisted workflow state.
            if let Some(state) = self.persisted_states.get(&item.url) {
              item.workflow_state = *state;
            }

            // URL dedup via index — O(1) replaces the prior O(N) linear
            // scan that fired ~50× per refresh × ~2,600 items.
            if let Some(&pos) = self.url_index.get(&item.url) {
              self.items[pos] = item;
              had_source_updates = true;
              continue;
            }

            // ArXiv ID dedup: collapse HF and arXiv entries for the same
            // paper. Keep the arXiv entry as primary. Same O(1) lookup.
            if let Some(aid) = arxiv_id_from_url(&item.url) {
              if let Some(&pos) = self.arxiv_id_index.get(aid) {
                if item.source_platform == SourcePlatform::ArXiv {
                  // Incoming is the canonical arXiv entry — replace HF stub.
                  // Position doesn't change, indices stay valid.
                  let ws = self.items[pos].workflow_state;
                  self.items[pos] = item;
                  self.items[pos].workflow_state = ws;
                  had_source_updates = true;
                }
                // else: existing is already arXiv, drop the HF duplicate.
                continue;
              }
            }

            // New item: push and update indices incrementally so the next
            // iteration of this same loop sees it for intra-batch dedup.
            let new_idx = self.items.len();
            self.url_index.insert(item.url.clone(), new_idx);
            if let Some(aid) = arxiv_id_from_url(&item.url) {
              self.arxiv_id_index.insert(aid.to_string(), new_idx);
            }
            self.items.push(item);
            had_structural_item_changes = true;
            had_source_updates = true;
          }
        }
        FetchMessage::EnrichedItems(new_items) => {
          for mut item in new_items {
            if let Some(state) = self.persisted_states.get(&item.url) {
              item.workflow_state = *state;
            }

            if let Some(&pos) = self.url_index.get(&item.url) {
              let workflow_state = self.items[pos].workflow_state;
              self.items[pos] = item;
              self.items[pos].workflow_state = workflow_state;
              had_enriched_updates = true;
              continue;
            }

            if let Some(aid) = arxiv_id_from_url(&item.url) {
              if let Some(&pos) = self.arxiv_id_index.get(aid) {
                let workflow_state = self.items[pos].workflow_state;
                if self.items[pos].source_platform == SourcePlatform::ArXiv
                  && item.source_platform != SourcePlatform::ArXiv
                {
                  if self.items[pos].github_repo.is_none() {
                    self.items[pos].github_repo = item.github_repo.take();
                    self.items[pos].github_owner = item.github_owner.take();
                    self.items[pos].github_repo_name =
                      item.github_repo_name.take();
                  }
                  if self.items[pos].full_content.is_none() {
                    self.items[pos].full_content = item.full_content.take();
                  }
                  had_enriched_updates = true;
                  continue;
                }
                self.items[pos] = item;
                self.items[pos].workflow_state = workflow_state;
                had_enriched_updates = true;
                continue;
              }
            }

            let new_idx = self.items.len();
            self.url_index.insert(item.url.clone(), new_idx);
            if let Some(aid) = arxiv_id_from_url(&item.url) {
              self.arxiv_id_index.insert(aid.to_string(), new_idx);
            }
            self.items.push(item);
            had_structural_item_changes = true;
          }
        }
        FetchMessage::SourceComplete(name) => {
          self.loading_sources.retain(|s| s != &name);
          self.loaded_sources.push(name);
          // Status bar shows the loading-sources list; without
          // mark_dirty, a phantom in-progress source can sit on
          // screen for ~250ms until any other event ticks the
          // redraw flag.
          self.mark_dirty();
        }
        FetchMessage::SourceError(name, err) => {
          self.status_message = Some(err);
          self.loading_sources.retain(|s| s != &name);
          self.mark_dirty();
        }
        FetchMessage::AllComplete => {
          self.is_loading = false;
          self.is_refreshing = false;
        }
      }
    }

    if had_structural_item_changes {
      self.items.sort_by(|a, b| b.published_at.cmp(&a.published_at));
      // Sort invalidated every position; indices must reflect the new order.
      self.rebuild_indices();
      self.invalidate_visible_cache();
      self.invalidate_counts_cache();
      // Hand off to the background writer — UI thread used to hitch for
      // 100-300 ms here while the 3.8 MB cache.json was serialized + fsynced.
      crate::store::cache::queue_save(self.items.clone());
      if was_empty {
        self.list_offset = 0;
      }
      self.mark_dirty();
    } else if had_source_updates || had_enriched_updates {
      self.invalidate_visible_cache();
      self.invalidate_items_derived_caches();
      crate::store::cache::queue_save(self.items.clone());
      self.mark_dirty();
    }
    if disconnected {
      // Loading-state change is visible in the status bar.
      self.mark_dirty();
    }
  }

  pub fn process_incoming_discovery(&mut self) {
    use std::sync::mpsc::TryRecvError;

    let mut messages = Vec::new();
    let mut disconnected = false;

    if let Some(rx) = &self.discovery_rx {
      loop {
        match rx.try_recv() {
          Ok(msg) => messages.push(msg),
          Err(TryRecvError::Empty) => break,
          Err(TryRecvError::Disconnected) => {
            disconnected = true;
            break;
          }
        }
      }
    }

    if disconnected {
      self.discovery_rx = None;
      self.discovery_loading = false;
    }

    let had_messages = !messages.is_empty();

    for msg in messages {
      match msg {
        DiscoveryMessage::StatusUpdate(s) => {
          self.discovery_status = s;
        }
        DiscoveryMessage::Items(items) => {
          self.merge_discovery_items(items);
          save_discovery_items(&self.discovery_items);
        }
        DiscoveryMessage::SessionSnapshot(snapshot) => {
          self.discovery_session = snapshot;
          crate::store::session::save(&self.discovery_session);
        }
        DiscoveryMessage::Complete => {
          self.discovery_rx = None;
          self.discovery_loading = false;
          let n = self.discovery_items.len();
          self.discovery_status = format!("Found {n} papers");
          self.status_message = Some("Discovery complete".to_string());

          let topic = self.discovery_session.initial_query.clone();
          if !topic.is_empty() {
            let titles: String = self
              .discovery_items
              .iter()
              .take(3)
              .map(|i| format!("• {}", i.title))
              .collect::<Vec<_>>()
              .join("\n");
            let body = if titles.is_empty() {
              String::new()
            } else {
              format!("\n\nTop results:\n{titles}")
            };
            self.push_chat_assistant_message(format!(
              "Discovery complete for \"{topic}\".\nFound {n} papers.{body}"
            ));
          }
        }
        DiscoveryMessage::Error(e) => {
          self.discovery_rx = None;
          self.discovery_loading = false;
          self.discovery_status = format!("Error: {e}");
          self.push_chat_assistant_message(format!("Discovery failed: {e}"));
          self.status_message = Some("Discovery failed".to_string());
        }
      }
    }

    // Any of the above arms mutated discovery state visible to the user.
    if had_messages || disconnected {
      self.mark_dirty();
    }
  }

  fn merge_discovery_items(&mut self, items: Vec<FeedItem>) {
    for mut item in items {
      // Belt-and-suspenders: every current discovery source already
      // sanitizes at ingestion, but the unbarriered injection point
      // is a future-contributor footgun — a new source added without
      // ingestion-time sanitize would silently ship terminal-control
      // bytes to the renderer.
      item.sanitize_in_place();
      if let Some(state) = self.persisted_states.get(&item.url) {
        item.workflow_state = *state;
      }

      // URL dedup via index — O(1).
      if let Some(&pos) = self.discovery_url_index.get(&item.url) {
        self.discovery_items[pos] = item;
        continue;
      }

      // ArXiv ID dedup — O(1).
      if let Some(aid) = arxiv_id_from_url(&item.url) {
        if let Some(&pos) = self.discovery_arxiv_id_index.get(aid) {
          if item.source_platform == SourcePlatform::ArXiv {
            let ws = self.discovery_items[pos].workflow_state;
            self.discovery_items[pos] = item;
            self.discovery_items[pos].workflow_state = ws;
          }
          continue;
        }
      }

      // New item: push and update indices incrementally.
      let new_idx = self.discovery_items.len();
      self.discovery_url_index.insert(item.url.clone(), new_idx);
      if let Some(aid) = arxiv_id_from_url(&item.url) {
        self.discovery_arxiv_id_index.insert(aid.to_string(), new_idx);
      }
      self.discovery_items.push(item);
    }
    self.discovery_items.sort_by(|a, b| b.published_at.cmp(&a.published_at));
    // Sort invalidated positions; rebuild for correctness.
    self.rebuild_discovery_indices();
    self.invalidate_visible_cache();
  }

  pub fn filter_cursor_down(&mut self) {
    let max = self.filter_total_items().saturating_sub(1);
    self.filter_cursor = (self.filter_cursor + 1).min(max);
  }

  pub fn filter_cursor_up(&mut self) {
    self.filter_cursor = self.filter_cursor.saturating_sub(1);
  }

  /// Total number of selectable rows in the filter panel (dynamic source +
  /// tag counts + fixed sections). Workflow-state filtering moved to the
  /// Library tab chips, so the panel covers sources, signals, content types,
  /// tags, and clear-all.
  pub fn filter_total_items(&self) -> usize {
    self.filter_source_names().len()
      + 3
      + 3
      + crate::tags::all_tags(&self.item_tags).len()
      + 1
  }

  /// Sorted unique source label strings derived from loaded items.
  /// Lazy memoize: the BTreeSet build + per-item String clone happens on the
  /// first read after invalidation; every other call (per draw frame, per
  /// j/k keystroke in the filter panel) is an O(1) clone of the cached vec.
  pub fn filter_source_names(&self) -> Vec<String> {
    if let Some(c) = self.filter_source_names_cache.borrow().as_ref() {
      return c.clone();
    }
    let mut names: std::collections::BTreeSet<String> = self
      .items
      .iter()
      .map(|item| {
        if item.source_name.is_empty() {
          item.source_platform.short_label().to_string()
        } else {
          item.source_name.clone()
        }
      })
      .collect();
    // Always include at least "arxiv" and "hf" so the filter is useful
    // even before a fetch completes.
    for seed in &["arxiv", "hf"] {
      names.insert(seed.to_string());
    }
    let computed: Vec<String> = names.into_iter().collect();
    *self.filter_source_names_cache.borrow_mut() = Some(computed.clone());
    computed
  }

  pub fn toggle_filter_at_cursor(&mut self) {
    let source_names = self.filter_source_names();
    let src_count = source_names.len();
    let tag_names = crate::tags::all_tags(&self.item_tags);
    let tag_count = tag_names.len();
    let c = self.filter_cursor;

    // Layout: [sources] [3 signals] [3 content types] [tags] [clear-all]
    let signals_start = src_count;
    let content_start = signals_start + 3;
    let tags_start = content_start + 3;
    let clear_all = tags_start + tag_count;

    if c < src_count {
      let name = source_names[c].clone();
      if !self.active_filters.sources.remove(&name) {
        self.active_filters.sources.insert(name);
      }
    } else if c < content_start {
      match c - signals_start {
        0 => toggle_set(&mut self.active_filters.signals, SignalLevel::Primary),
        1 => {
          toggle_set(&mut self.active_filters.signals, SignalLevel::Secondary)
        }
        _ => {
          toggle_set(&mut self.active_filters.signals, SignalLevel::Tertiary)
        }
      }
    } else if c < tags_start {
      match c - content_start {
        0 => {
          toggle_set(&mut self.active_filters.content_types, ContentType::Paper)
        }
        1 => toggle_set(
          &mut self.active_filters.content_types,
          ContentType::Article,
        ),
        _ => toggle_set(
          &mut self.active_filters.content_types,
          ContentType::Digest,
        ),
      }
    } else if c < clear_all {
      let tag = tag_names[c - tags_start].clone();
      if !self.active_filters.tags.remove(&tag) {
        self.active_filters.tags.insert(tag);
      }
    } else {
      self.active_filters = FilterState::new();
    }
    self.invalidate_filter_summary_cache();
    self.invalidate_filtered_history_cache();
    self.reset_active_feed_position();
  }

  pub fn clear_filters(&mut self) {
    self.active_filters = FilterState::new();
    self.invalidate_filter_summary_cache();
    self.invalidate_filtered_history_cache();
    self.reset_active_feed_position();
  }

  // ── Sources popup ───────────────────────────────────────────────────────

  /// arXiv categories to display in the popup: known list + any user extras.
  pub fn sources_popup_arxiv_cats(&self) -> Vec<(String, String)> {
    let mut cats: Vec<(String, String)> = crate::config::KNOWN_ARXIV_CATS
      .iter()
      .map(|(code, label)| (code.to_string(), label.to_string()))
      .collect();
    for cat in &self.config.sources.arxiv_categories {
      if !crate::config::KNOWN_ARXIV_CATS
        .iter()
        .any(|(k, _)| *k == cat.as_str())
      {
        cats.push((cat.clone(), String::new()));
      }
    }
    cats
  }

  /// Total number of cursor-navigable rows in the sources popup.
  pub fn sources_popup_total_items(&self) -> usize {
    1 // input field
      + self.sources_popup_arxiv_cats().len()
      + crate::config::PREDEFINED_SOURCES.len()
      + self.config.sources.custom_feeds.len()
  }

  /// Poll the discovery background thread and update detect state.
  pub fn poll_detect_result(&mut self) {
    use std::sync::mpsc::TryRecvError;
    let result = if let Some(rx) = &self.sources_detect_rx {
      Some(rx.try_recv())
    } else {
      None
    };
    match result {
      Some(Ok(r)) => {
        self.sources_detect_state = SourcesDetectState::Result(r);
        self.sources_detect_rx = None;
      }
      Some(Err(TryRecvError::Disconnected)) => {
        self.sources_detect_state = SourcesDetectState::Result(
          DiscoverResult::Failed("Detection thread disconnected".to_string()),
        );
        self.sources_detect_rx = None;
      }
      _ => {}
    }
  }

  pub fn set_workflow_state(&mut self, state: WorkflowState) {
    // Collect the URL of the currently selected visible item
    let url = self
      .visible_get(self.active_selected_index())
      .map(|item| item.url.clone());

    if let Some(url) = url {
      if let Some(item) =
        self.items_for_tab_mut().iter_mut().find(|i| i.url == url)
      {
        item.workflow_state = state;
      }
      self.persisted_states.insert(url, state);
      crate::store::save(&self.persisted_states);
      // Unconditional invalidate even when the mutation lands on
      // discovery_items: counts_cache reads only from app.items today,
      // so the call is technically redundant on the Discoveries tab.
      // Keeping it unconditional preserves invariant safety if anyone
      // later folds discovery_items into counts.
      self.invalidate_items_derived_caches();
    }
  }
}

fn encode_repo_url_path(path: &str) -> String {
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

fn toggle_set<T: Eq + std::hash::Hash>(set: &mut HashSet<T>, value: T) {
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
fn validate_download_name(name: &str) -> Result<(), String> {
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
    let mut app = App::new();
    app.items = mock_items();
    let item_count = app.items.len();
    app.rebuild_indices();
    assert_eq!(app.url_index.len(), item_count);
    // Every item's URL should resolve back to its position.
    for (idx, item) in app.items.iter().enumerate() {
      assert_eq!(app.url_index.get(&item.url).copied(), Some(idx));
    }
    // arxiv_id_index covers only items whose URL has an arxiv ID.
    for (idx, item) in app.items.iter().enumerate() {
      if let Some(aid) = arxiv_id_from_url(&item.url) {
        assert_eq!(app.arxiv_id_index.get(aid).copied(), Some(idx));
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

  #[test]
  fn rebuild_indices_clears_stale_entries() {
    let mut app = App::new();
    app.items = mock_items();
    app.rebuild_indices();
    let prior = app.url_index.len();
    // Drop half the items, rebuild — the index should shrink to match.
    app.items.truncate(prior / 2);
    app.rebuild_indices();
    assert_eq!(app.url_index.len(), prior / 2);
  }

  #[test]
  fn item_counts_breaks_down_workflow_states() {
    let mut app = App::new();
    app.items = mock_items();
    let counts = app.item_counts();
    assert_eq!(counts.total, app.items.len());
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
    app.items = mock_items();
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
    app.items = mock_items();
    // Snapshot the cached total in a scoped block so the Ref drops before
    // we mutate `app.items` below.
    let before_total = app.item_counts().total;
    // Mutate items directly, bypassing the public mutators that would
    // normally call invalidate_counts_cache. The cache should still hold
    // the stale value until we invalidate by hand.
    app.items.clear();
    let stale_total = app.item_counts().total;
    assert_eq!(stale_total, before_total, "cache survives raw mutation");
    app.invalidate_counts_cache();
    let fresh_total = app.item_counts().total;
    assert_eq!(fresh_total, 0, "post-invalidation, recompute sees empty items");
  }

  #[test]
  fn filter_source_names_caches_until_invalidated() {
    let mut app = App::new();
    app.items = mock_items();

    // First call computes and caches.
    let before = app.filter_source_names();
    assert!(!before.is_empty());

    // Mutate items directly without invalidation — cache holds stale value.
    app.items.clear();
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
    app.items = mock_items();
    app.rebuild_indices();
    assert!(!app.url_index.is_empty(), "fixture must populate url_index");

    app.reset_items();

    assert!(app.items.is_empty());
    assert!(app.url_index.is_empty(), "url_index must be cleared in lockstep");
    assert!(
      app.arxiv_id_index.is_empty(),
      "arxiv_id_index must be cleared in lockstep"
    );
  }

  #[test]
  fn reset_discovery_items_clears_indices() {
    let mut app = App::new();
    app.discovery_items = mock_items();
    // Manually populate the discovery indices to mirror what
    // merge_discovery_items would do; rebuild_indices targets the primary
    // items vec, not discovery_items.
    for (idx, item) in app.discovery_items.iter().enumerate() {
      app.discovery_url_index.insert(item.url.clone(), idx);
    }
    assert!(!app.discovery_url_index.is_empty());

    app.reset_discovery_items();

    assert!(app.discovery_items.is_empty());
    assert!(app.discovery_url_index.is_empty());
    assert!(app.discovery_arxiv_id_index.is_empty());
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
    app.discovery_loading = true;
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

impl App {
  pub fn notes_mode_for_side(&self, side: FocusedReader) -> NotesMode {
    match side {
      FocusedReader::Primary => self.notes_mode,
      FocusedReader::Secondary => self.secondary_notes_mode,
    }
  }

  pub fn set_notes_mode_for_side(
    &mut self,
    side: FocusedReader,
    mode: NotesMode,
  ) {
    match side {
      FocusedReader::Primary => self.notes_mode = mode,
      FocusedReader::Secondary => self.secondary_notes_mode = mode,
    }
  }

  pub fn notes_context_for_side(
    &self,
    side: FocusedReader,
  ) -> Option<&NotesContext> {
    match side {
      FocusedReader::Primary => self.notes_context.as_ref(),
      FocusedReader::Secondary => self.secondary_notes_context.as_ref(),
    }
  }

  pub fn set_notes_context_for_side(
    &mut self,
    side: FocusedReader,
    context: Option<NotesContext>,
  ) {
    match side {
      FocusedReader::Primary => self.notes_context = context,
      FocusedReader::Secondary => self.secondary_notes_context = context,
    }
  }

  pub fn reader_notes_context(
    &self,
    side: FocusedReader,
  ) -> Option<NotesContext> {
    let tab = match side {
      FocusedReader::Primary => self.reader_tabs.get(self.reader_active_tab),
      FocusedReader::Secondary => {
        self.reader_secondary_tabs.get(self.reader_secondary_active_tab)
      }
    }?;
    tab.notes_context.clone()
  }

  /// Active primary reader (mutable).  Returns the tread Reader so
  /// callers can dispatch `handle_event` and read state.  Most call
  /// sites also need the per-tab `ImageState` for `tread::after_draw`
  /// — use `reader_active_tab_mut()` for both at once.
  pub fn reader_editor_mut(&mut self) -> Option<&mut tread::Reader> {
    self.reader_tabs.get_mut(self.reader_active_tab).map(|t| &mut t.reader)
  }

  pub fn reader_secondary_editor_mut(&mut self) -> Option<&mut tread::Reader> {
    self
      .reader_secondary_tabs
      .get_mut(self.reader_secondary_active_tab)
      .map(|t| &mut t.reader)
  }

  /// Active primary tab as a whole — exposes both the Reader and its
  /// ImageState in one borrow.  Call sites that drive `tread::after_draw`
  /// or `tread::clear_images` need both.
  pub fn reader_active_tab_mut(&mut self) -> Option<&mut ReaderTab> {
    self.reader_tabs.get_mut(self.reader_active_tab)
  }

  pub fn reader_secondary_active_tab_mut(&mut self) -> Option<&mut ReaderTab> {
    self.reader_secondary_tabs.get_mut(self.reader_secondary_active_tab)
  }

  pub fn reader_push_tab(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    self.reader_tabs.push(ReaderTab {
      title,
      arxiv_id,
      notes_context,
      reader,
      image_state: tread::ImageState::default(),
      last_resize: None,
    });
    self.reader_active_tab = self.reader_tabs.len() - 1;
    self.reader_active = true;
  }

  pub fn reader_secondary_push_tab(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    self.reader_secondary_tabs.push(ReaderTab {
      title,
      arxiv_id,
      notes_context,
      reader,
      image_state: tread::ImageState::default(),
      last_resize: None,
    });
    self.reader_secondary_active_tab = self.reader_secondary_tabs.len() - 1;
  }

  pub fn reader_replace_active_tab(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    if self.reader_tabs.is_empty() {
      self.reader_push_tab(title, arxiv_id, notes_context, reader);
    } else {
      self.reader_tabs[self.reader_active_tab] = ReaderTab {
        title,
        arxiv_id,
        notes_context,
        reader,
        image_state: tread::ImageState::default(),
        last_resize: None,
      };
      self.reader_active = true;
    }
  }

  pub fn reader_secondary_replace_active_tab(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    if self.reader_secondary_tabs.is_empty() {
      self.reader_secondary_push_tab(title, arxiv_id, notes_context, reader);
    } else {
      self.reader_secondary_tabs[self.reader_secondary_active_tab] =
        ReaderTab {
          title,
          arxiv_id,
          notes_context,
          reader,
          image_state: tread::ImageState::default(),
          last_resize: None,
        };
    }
  }

  /// Clear the image cache for every open reader tab + the popup.
  /// Called on `Event::Resize` (placements were anchored to the old
  /// rect coords) and `Event::FocusLost` (tmux pane switch — leftover
  /// kitty placements would bleed into whatever pane sits on top of
  /// us).  Cheap: each ImageState is just a small HashMap.
  pub fn clear_all_reader_image_state(&mut self) {
    for tab in self.reader_tabs.iter_mut() {
      tread::clear_images(&mut tab.image_state);
    }
    for tab in self.reader_secondary_tabs.iter_mut() {
      tread::clear_images(&mut tab.image_state);
    }
    tread::clear_images(&mut self.reader_popup_image_state);
  }

  /// Stop voice playback and exit reading mode on every open Reader.
  /// Called when the user navigates away from the source they were
  /// listening to: tab close, tab switch (prev/next), or full reader
  /// exit (Esc back to feed).  Voice should stay tied to the active
  /// reader pane; continuing playback after a transition is
  /// disorienting.  All Readers share one Arc<PlaybackController>, so
  /// vc.stop() may fire multiple times — the controller treats
  /// repeats as no-ops.  Idempotent.
  pub fn stop_all_reader_voice(&mut self) {
    for tab in self.reader_tabs.iter_mut() {
      tab.reader.exit_voice_mode();
    }
    for tab in self.reader_secondary_tabs.iter_mut() {
      tab.reader.exit_voice_mode();
    }
    if let Some(reader) = self.reader_popup_editor.as_mut() {
      reader.exit_voice_mode();
    }
  }

  /// Close the active primary tab. Returns true if the pane is now empty.
  pub fn reader_close_active_tab(&mut self) -> bool {
    if self.reader_tabs.is_empty() {
      return true;
    }
    // Stop voice + clear image cache before drop so audio doesn't
    // outlive the source it was reading and pixel placements the
    // terminal still has cached for this tab's kitty_ids are deleted
    // — otherwise they linger as ghost overlays on whichever tab
    // takes its slot.
    if let Some(tab) = self.reader_tabs.get_mut(self.reader_active_tab) {
      tab.reader.exit_voice_mode();
      tread::clear_images(&mut tab.image_state);
    }
    self.reader_tabs.remove(self.reader_active_tab);
    if self.reader_tabs.is_empty() {
      self.reader_active_tab = 0;
      self.reader_active = false;
      return true;
    }
    self.reader_active_tab = self.reader_active_tab.saturating_sub(1);
    false
  }

  /// Close the active secondary tab. Returns true if the pane is now empty.
  pub fn reader_secondary_close_active_tab(&mut self) -> bool {
    if self.reader_secondary_tabs.is_empty() {
      return true;
    }
    if let Some(tab) =
      self.reader_secondary_tabs.get_mut(self.reader_secondary_active_tab)
    {
      tab.reader.exit_voice_mode();
      tread::clear_images(&mut tab.image_state);
    }
    self.reader_secondary_tabs.remove(self.reader_secondary_active_tab);
    if self.reader_secondary_tabs.is_empty() {
      self.reader_secondary_active_tab = 0;
      return true;
    }
    self.reader_secondary_active_tab =
      self.reader_secondary_active_tab.saturating_sub(1);
    false
  }

  pub fn reader_prev_tab(&mut self) {
    // Voice is tied to the source you were reading.  Switching to a
    // different tab means you're not in that source anymore — stop
    // playback so audio doesn't keep going in the background.
    self.stop_all_reader_voice();
    match self.focused_reader {
      FocusedReader::Primary => {
        let n = self.reader_tabs.len();
        if n > 0 {
          self.reader_active_tab = (self.reader_active_tab + n - 1) % n;
        }
      }
      FocusedReader::Secondary => {
        let n = self.reader_secondary_tabs.len();
        if n > 0 {
          self.reader_secondary_active_tab =
            (self.reader_secondary_active_tab + n - 1) % n;
        }
      }
    }
  }

  pub fn reader_next_tab(&mut self) {
    self.stop_all_reader_voice();
    match self.focused_reader {
      FocusedReader::Primary => {
        let n = self.reader_tabs.len();
        if n > 0 {
          self.reader_active_tab = (self.reader_active_tab + 1) % n;
        }
      }
      FocusedReader::Secondary => {
        let n = self.reader_secondary_tabs.len();
        if n > 0 {
          self.reader_secondary_active_tab =
            (self.reader_secondary_active_tab + 1) % n;
        }
      }
    }
  }
}

fn classify_repo_file_kind(name: &str, content: &str) -> RepoFileKind {
  let lower = name.to_ascii_lowercase();
  if lower.ends_with(".md")
    || lower.ends_with(".markdown")
    || lower == "readme"
    || lower.starts_with("readme.")
  {
    return RepoFileKind::Markdown;
  }

  if crate::syntax::highlight_file(content, name).is_some() {
    RepoFileKind::Code
  } else {
    RepoFileKind::PlainText
  }
}
