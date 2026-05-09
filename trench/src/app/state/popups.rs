//! Quit / tag-picker / sources-popup ephemeral state, plus the source-detect
//! state machine and its DiscoverResult enum (used both here and by the
//! discovery agent).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuitPopupKind {
  #[default]
  QuitApp,
  QuitWithProgress,
  QuitWithChat,
  LeaveReader,
}

/// Quit confirmation popup state.
#[derive(Default)]
pub struct QuitPopupState {
  pub active: bool,
  pub kind: QuitPopupKind,
}

/// Tag picker popup state.
#[derive(Default)]
pub struct TagPickerState {
  pub active: bool,
  pub input: String,
  pub selected: usize,
  pub target_urls: Vec<String>,
}

/// Result from the URL discovery pipeline (used by the sources popup).
/// Defined here because it predates the surfaces/ layout — moving it now
/// would touch every call site that imports `crate::app::DiscoverResult`.
#[derive(Clone)]
pub enum DiscoverResult {
  ArxivCategory(String),
  HuggingFaceAlreadyEnabled,
  RssFeed { url: String, name: String },
  Failed(String),
}
