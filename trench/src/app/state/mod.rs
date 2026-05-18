//! Sub-state types for `App`. Grouped by concern; re-exported flat so that
//! `crate::app::ChatState` (and friends) keep working without `state::` qualifier.

mod chat;
mod discovery;
mod feed;
mod help;
mod notes;
mod panes;
mod popups;
mod reader;
mod repo;
mod settings;
mod theme_picker;

pub use chat::ChatState;
pub use discovery::DiscoveryState;
pub use feed::{AppView, FeedTab, FilterState, ItemCounts, NavDirection};
pub use help::HelpState;
pub use notes::{
  NotesContext, NotesInstanceModel, NotesMode, NotesPaneModel, NotesTab,
};
pub use panes::{FocusedReader, PANE_COUNT, PaneId, PaneInfo};
pub use popups::{
  DiscoverResult, QuitPopupKind, QuitPopupState, TagPickerState,
};
pub use reader::ReaderTab;
pub use repo::{
  RepoContext, RepoEnterTarget, RepoFetchResult, RepoFileFetched,
  RepoFileKind, RepoPane,
};
pub use settings::SettingsEditState;
pub use theme_picker::{
  CustomThemeEditorMode, CustomThemeEditorState, ThemePickerState,
};
