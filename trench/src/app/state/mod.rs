//! Sub-state types for `App`. Grouped by concern; re-exported flat so that
//! `crate::app::ChatState` (and friends) keep working without `state::` qualifier.

mod async_jobs;
mod browse;
mod chat;
mod debounce;
mod discovery;
mod feed;
mod help;
mod leader;
mod notes;
mod panes;
mod popups;
mod reader;
mod reader_bottom;
mod render_caches;
mod repo;
mod settings;
mod theme_picker;
mod view_flags;

pub use async_jobs::AsyncJobs;
pub use browse::BrowseModel;
pub use chat::ChatState;
pub use debounce::DebounceState;
pub use discovery::DiscoveryModel;
pub use feed::{AppView, FeedTab, FilterState, ItemCounts, NavDirection};
pub use help::HelpState;
pub use leader::LeaderState;
pub use notes::{
  CloseTabOutcome, NotesContext, NotesInstanceModel, NotesMode, NotesPaneModel,
  NotesTab,
};
pub use panes::{FocusedReader, PANE_COUNT, PaneId, PaneInfo};
pub use popups::{
  DiscoverResult, QuitPopupKind, QuitPopupState, TagPickerState,
};
pub use reader::ReaderTab;
pub use reader_bottom::ReaderBottomState;
pub use render_caches::RenderCaches;
pub use repo::{
  RepoContext, RepoEnterTarget, RepoFetchResult, RepoFileFetched, RepoFileKind,
  RepoPane,
};
pub use settings::SettingsEditState;
pub use theme_picker::{
  CustomThemeEditorMode, CustomThemeEditorState, ThemePickerState,
};
pub use view_flags::ViewFlags;
