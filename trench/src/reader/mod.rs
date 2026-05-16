//! `ReaderPaneModel` — composition-root state owner for the reader pane.
//! `ReaderPopupModel` — sibling Model for the floating popup reader.
//!
//! Slice 2 (ADR-002) lifts the 13 reader+popup fields scattered across
//! `App` into these two Models.  Like Slice 1's `FeedModel`, fields are
//! `pub` during the migration so PR 2 can rewrite `app.reader_tabs`
//! call sites as `app.reader.primary.tabs` with one perl sweep.
//!
//! Contract and rationale: `docs/adr/ADR-002-reader-slice.md`.
//! Vocabulary: `docs/CONTEXT.md`.

use crate::app::{FocusedReader, ReaderTab};

/// One embedded reader instance — primary or secondary.  Owns its tabs
/// and the active-tab index.  Each `ReaderTab` wraps a `tread::Reader`
/// plus the per-tab image cache; this Model owns the *collection*, not
/// the editors themselves.
#[derive(Default)]
pub struct ReaderInstanceModel {
  pub tabs: Vec<ReaderTab>,
  pub active_tab: usize,
}

impl ReaderInstanceModel {
  pub fn new() -> Self {
    Self::default()
  }

  /// Whether this instance currently holds at least one paper.
  pub fn is_loaded(&self) -> bool {
    !self.tabs.is_empty()
  }
}

/// Composition-root model for the reader pane as a region of the
/// layout.  Owns the primary instance, an optional secondary, and the
/// dual/split/focus state machine.
///
/// The historical `reader_active` boolean is derived (`primary.is_loaded()
/// && focus is on reader`), not stored.
pub struct ReaderPaneModel {
  pub primary: ReaderInstanceModel,
  pub secondary: Option<ReaderInstanceModel>,
  /// State 2 of the three-state layout cycle (feed + reader split 40/60).
  pub split_active: bool,
  /// State 3 (dual reader 50/50, primary | secondary).
  pub dual_active: bool,
  /// Which instance currently receives keystrokes when dual is active.
  pub focused: FocusedReader,
}

impl ReaderPaneModel {
  pub fn new() -> Self {
    Self::default()
  }
}

impl Default for ReaderPaneModel {
  /// Hand-written rather than derived because [`FocusedReader`] is from
  /// a sibling module and doesn't impl `Default` (intentionally — its
  /// initial value belongs to the surface that owns it).
  fn default() -> Self {
    Self {
      primary: ReaderInstanceModel::default(),
      secondary: None,
      split_active: false,
      dual_active: false,
      focused: FocusedReader::Primary,
    }
  }
}

/// Floating popup reader (`Ldr+Enter`).  Sibling to `ReaderPaneModel`,
/// not a third `ReaderInstanceModel` — its lifecycle is async-load +
/// dismissible-on-Esc and would not fit `ReaderInstanceModel`'s tab-
/// collection contract.
///
/// Invariant: `active` is true iff `editor.is_some() || rx.is_some()`.
/// PR 5 enforces this via `pre_draw` reconciliation.
#[derive(Default)]
pub struct ReaderPopupModel {
  pub active: bool,
  pub rx: Option<
    std::sync::mpsc::Receiver<Result<tread::PaperData, String>>,
  >,
  pub editor: Option<tread::Reader>,
  pub image_state: tread::ImageState,
  pub burst: tread::BurstTracker,
}

impl ReaderPopupModel {
  pub fn new() -> Self {
    Self::default()
  }

  /// Whether the popup is currently displayed *or* loading.  Equivalent
  /// to the historical `app.reader_popup_active` flag but derives from
  /// the data instead of trusting the caller to keep the flag in sync.
  pub fn is_open(&self) -> bool {
    self.active || self.editor.is_some() || self.rx.is_some()
  }
}

/// Per-frame, read-only context passed into reader-pane renders
/// alongside `&mut ReaderPaneModel` / `&mut ReaderPopupModel`.  Lands
/// properly in PR 5 once the render flip happens; PR 1 ships the
/// shell so call sites have something to import.
pub struct ReaderContext<'a> {
  pub workspace: &'a crate::data::workspace_store::Workspace,
  pub theme: ui_theme::Theme,
  pub viewport: crate::ui::Viewport,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn pane_model_defaults_empty_and_unfocused() {
    let m = ReaderPaneModel::default();
    assert!(!m.primary.is_loaded());
    assert!(m.secondary.is_none());
    assert!(!m.split_active);
    assert!(!m.dual_active);
    assert!(matches!(m.focused, FocusedReader::Primary));
  }

  #[test]
  fn instance_model_is_loaded_tracks_tabs() {
    let m = ReaderInstanceModel::default();
    assert!(!m.is_loaded());
    // Adding a tab would make is_loaded() true.  ReaderTab construction
    // requires a tread::Reader, which we don't build in unit tests
    // (per ADR-002 S5 — tests-without-tread strategy).
  }

  #[test]
  fn popup_defaults_closed() {
    let m = ReaderPopupModel::default();
    assert!(!m.is_open());
    assert!(!m.active);
    assert!(m.editor.is_none());
    assert!(m.rx.is_none());
  }

  #[test]
  fn popup_is_open_when_any_load_signal_present() {
    // is_open() must answer "yes" if EITHER the user-visible flag or
    // the load-in-flight state is true. The invariant (PR 5) ties
    // them together; here we just check the derivation logic.
    let mut m = ReaderPopupModel::default();
    m.active = true;
    assert!(m.is_open());
  }
}
