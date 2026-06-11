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

/// One embedded reader instance — primary or secondary.  Holds the
/// single open paper.  The `ReaderTab` wraps a `tread::Reader` plus the
/// per-document image cache.  Document tabs were removed (ADR-017): one
/// paper at a time, opening another replaces it.
#[derive(Default)]
pub struct ReaderInstanceModel {
  /// The open paper, or `None` when the pane holds nothing.
  pub doc: Option<ReaderTab>,
}

impl ReaderInstanceModel {
  /// Slice 2 stub: callers use `ReaderInstanceModel::default()` today;
  /// `new()` lives here for symmetry with sibling models and future use.
  #[allow(dead_code)]
  pub fn new() -> Self {
    Self::default()
  }

  /// Whether this instance currently holds a paper.
  pub fn is_loaded(&self) -> bool {
    self.doc.is_some()
  }

  /// Layout-derived reconciliation that runs once per frame, before
  /// the render hook calls `tread::draw`.  Compares the supplied
  /// viewport against `last_resize` on the open doc and emits the
  /// `tread::Reader::resize` call only when the size actually changed.
  ///
  /// Without this, the render path would call `resize()` every frame
  /// — `tread::Reader` doesn't guarantee its own short-circuit and
  /// the resize triggers re-flow + image re-placement, both expensive.
  pub fn pre_draw(&mut self, viewport: crate::ui::Viewport) {
    let Some(tab) = self.doc.as_mut() else {
      return;
    };
    let new_size = (viewport.cols, viewport.rows);
    if tab.last_resize != Some(new_size) {
      tab.reader.resize(new_size.0, new_size.1);
      tab.last_resize = Some(new_size);
    }
  }

  /// Close the open paper. Returns `true` — the pane is now empty
  /// (callers may want to dismiss it). Kept boolean-returning to match
  /// the dual→single collapse call sites.
  pub fn close_active_tab(&mut self) -> bool {
    self.doc = None;
    true
  }
}

/// Composition-root model for the reader pane as a region of the
/// layout.  Owns the primary instance, an optional secondary, and the
/// dual/split/focus state machine.
///
/// Note: `active` is *view-state* — "the user is currently in reader
/// mode" — not the same as `primary.is_loaded()`. ADR-002 originally
/// proposed collapsing them; PR 2 found the semantics differ (the
/// user navigates into reader before any paper loads, and back to feed
/// without unloading tabs). The two flags stay separate; future work
/// may reconsider once usage is clearer.
pub struct ReaderPaneModel {
  pub primary: ReaderInstanceModel,
  /// Secondary reader instance.  Always present; "the secondary is
  /// active" is encoded by `split_active || dual_active`, not by
  /// `Option`-presence.  A `None` `doc` is the no-content signal the
  /// layout already uses.
  pub secondary: ReaderInstanceModel,
  /// View-state: the user is currently inside reader mode.
  pub active: bool,
  /// State 2 of the three-state layout cycle (feed + reader split 40/60).
  pub split_active: bool,
  /// State 3 (dual reader 50/50, primary | secondary).
  pub dual_active: bool,
  /// Which instance currently receives keystrokes when dual is active.
  pub focused: FocusedReader,
}

impl ReaderPaneModel {
  /// Slice 2 stub — see [`ReaderInstanceModel::new`] for rationale.
  #[allow(dead_code)]
  pub fn new() -> Self {
    Self::default()
  }

  /// Reference to whichever instance currently has focus.
  /// Slice 2 stub: focus dispatch is still inline in render today;
  /// these helpers will replace those when the dual-reader gestures land.
  #[allow(dead_code)]
  pub fn focused_instance(&self) -> &ReaderInstanceModel {
    match self.focused {
      FocusedReader::Primary => &self.primary,
      FocusedReader::Secondary => &self.secondary,
    }
  }

  /// Mutable variant of [`focused_instance`].
  /// Stub like its siblings since the tab-nav caller was removed (ADR-017);
  /// will be used when the dual-reader gestures land.
  #[allow(dead_code)]
  pub fn focused_instance_mut(&mut self) -> &mut ReaderInstanceModel {
    match self.focused {
      FocusedReader::Primary => &mut self.primary,
      FocusedReader::Secondary => &mut self.secondary,
    }
  }

  /// Set focus to `target`.  Returns the previous focus so callers can
  /// branch on "actually changed" without re-reading.
  /// Slice 2 stub — see [`focused_instance`] for rationale.
  #[allow(dead_code)]
  pub fn set_focus(&mut self, target: FocusedReader) -> FocusedReader {
    let prev = self.focused;
    self.focused = target;
    prev
  }

  /// Swap focus between primary and secondary.  Returns the new focus.
  /// Slice 2 stub — see [`focused_instance`] for rationale.
  #[allow(dead_code)]
  pub fn toggle_focus(&mut self) -> FocusedReader {
    self.focused = match self.focused {
      FocusedReader::Primary => FocusedReader::Secondary,
      FocusedReader::Secondary => FocusedReader::Primary,
    };
    self.focused
  }
}

impl Default for ReaderPaneModel {
  /// Hand-written rather than derived because [`FocusedReader`] is from
  /// a sibling module and doesn't impl `Default` (intentionally — its
  /// initial value belongs to the surface that owns it).
  fn default() -> Self {
    Self {
      primary: ReaderInstanceModel::default(),
      secondary: ReaderInstanceModel::default(),
      active: false,
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
  pub rx: Option<std::sync::mpsc::Receiver<Result<tread::PaperData, String>>>,
  pub editor: Option<tread::Reader>,
  pub image_state: tread::ImageState,
  pub burst: tread::BurstTracker,
  /// Last viewport size we called `tread::Reader::resize` with. Mirrors
  /// `ReaderTab::last_resize` (see [`ReaderInstanceModel::pre_draw`]) so
  /// the popup's per-frame `pre_draw` can short-circuit when the
  /// viewport hasn't changed. Reset to `None` whenever `editor` is
  /// replaced so the first `pre_draw` against a new editor always
  /// resizes.
  pub last_resize: Option<(u16, u16)>,
}

impl ReaderPopupModel {
  /// Slice 2 stub — see [`ReaderInstanceModel::new`] for rationale.
  #[allow(dead_code)]
  pub fn new() -> Self {
    Self::default()
  }

  /// Whether the popup is currently displayed *or* loading.  Equivalent
  /// to the historical `app.reader_popup_active` flag but derives from
  /// the data instead of trusting the caller to keep the flag in sync.
  /// Slice 2 stub: callers still read `self.active` directly; this
  /// derives-from-data helper takes over when the flag is removed.
  #[allow(dead_code)]
  pub fn is_open(&self) -> bool {
    self.active || self.editor.is_some() || self.rx.is_some()
  }

  /// Same shape as [`ReaderInstanceModel::pre_draw`] — resize the
  /// editor only when the viewport size actually changed. Without the
  /// `last_resize` short-circuit the popup paid for a full reflow +
  /// image re-placement on every frame it was open (the previous
  /// comment here documented the gap; this commit closes it).
  pub fn pre_draw(&mut self, viewport: crate::ui::Viewport) {
    let Some(editor) = self.editor.as_mut() else {
      return;
    };
    let new_size = (viewport.cols, viewport.rows);
    if self.last_resize != Some(new_size) {
      editor.resize(new_size.0, new_size.1);
      self.last_resize = Some(new_size);
    }
  }
}

/// Per-frame, read-only context passed into reader-pane renders
/// alongside `&mut ReaderPaneModel` / `&mut ReaderPopupModel`.  Lands
/// properly in PR 5 once the render flip happens; PR 1 ships the
/// shell so call sites have something to import.
/// Load-bearing vocabulary per CONTEXT.md; constructed by PR 5.
#[allow(dead_code)]
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
    assert!(!m.secondary.is_loaded());
    assert!(!m.active);
    assert!(!m.split_active);
    assert!(!m.dual_active);
    assert!(matches!(m.focused, FocusedReader::Primary));
  }

  #[test]
  fn instance_model_default_is_unloaded() {
    let m = ReaderInstanceModel::default();
    assert!(!m.is_loaded());
    assert!(m.doc.is_none());
    // Setting `doc` would make is_loaded() true. ReaderTab construction
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
  fn toggle_focus_swaps_primary_and_secondary() {
    let mut m = ReaderPaneModel::default();
    assert!(matches!(m.focused, FocusedReader::Primary));
    let new_focus = m.toggle_focus();
    assert!(matches!(new_focus, FocusedReader::Secondary));
    assert!(matches!(m.focused, FocusedReader::Secondary));
    m.toggle_focus();
    assert!(matches!(m.focused, FocusedReader::Primary));
  }

  #[test]
  fn set_focus_returns_previous_value() {
    let mut m = ReaderPaneModel::default();
    let prev = m.set_focus(FocusedReader::Secondary);
    assert!(matches!(prev, FocusedReader::Primary));
    assert!(matches!(m.focused, FocusedReader::Secondary));
  }

  #[test]
  fn close_clears_the_doc_and_reports_empty() {
    let mut inst = ReaderInstanceModel::default();
    // Already empty: closing is a no-op that reports empty.
    assert!(inst.close_active_tab());
    assert!(inst.doc.is_none());
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
