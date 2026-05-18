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

impl Default for NotesMode {
  fn default() -> Self {
    Self::Library
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Slice 3 — composition-root models (ADR-003)
//
// Skeletons land in PR 1. PR 2 wires them into `App` and migrates the 11
// scattered `notes_*` / `secondary_notes_*` fields. Tests here exercise
// the empty defaults + the Option<secondary> invariant; gesture tests
// arrive with PR 3 when the methods exist.

/// One notes context (primary or secondary). Owns tabs + active tab +
/// mode + paper anchoring. Pure content — visibility is a pane-level
/// concern and lives on [`NotesPaneModel`].
///
/// Per-instance: primary and secondary can be in different modes and
/// tied to different papers. The `notes_mode_for_side` accessor that
/// currently dispatches on a [`FocusedReader`] discriminator (see
/// `app/methods/notes.rs`) collapses onto `instance.mode` field access
/// in PR 2.
#[derive(Default)]
pub struct NotesInstanceModel {
  pub tabs: Vec<NotesTab>,
  pub active_tab: usize,
  pub mode: NotesMode,
  pub context: Option<NotesContext>,
}

/// Composition-root model for the notes dock. Sibling to
/// `ReaderPaneModel`; lives on `App` as `App.notes`.
///
/// Asymmetric secondary (departure from ADR-002 §S2): `secondary` is
/// `Option<NotesInstanceModel>` rather than always-present, because
/// notes' secondary is a tacked-on extra in real usage, not a
/// first-class layout state like reader's dual/split.
///
/// Visibility flags live here at the pane root, not inside the instance.
/// Layout-level concern: "should this rect render?" Instance stays
/// pure-content. See ADR-003 §S3.
///
/// Invariants:
/// - `secondary_visible` is meaningful only when `secondary.is_some()`.
/// - `app` is `None` until the first notes interaction; lazy-loaded.
#[derive(Default)]
pub struct NotesPaneModel {
  /// Persistence backend, shared across instances. Lazy-loaded.
  pub app: Option<notes::app::App>,

  pub primary: NotesInstanceModel,
  /// Primary pane is on screen. Independent of tab population —
  /// toggling off preserves tabs.
  pub primary_visible: bool,

  /// `None` = never opened. `Some(_)` = ever opened; visibility
  /// (hide-without-unload) controlled by `secondary_visible`.
  pub secondary: Option<NotesInstanceModel>,
  /// Only meaningful when `secondary.is_some()`.
  pub secondary_visible: bool,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn instance_default_is_empty_library() {
    let m = NotesInstanceModel::default();
    assert!(m.tabs.is_empty());
    assert_eq!(m.active_tab, 0);
    assert_eq!(m.mode, NotesMode::Library);
    assert!(m.context.is_none());
  }

  #[test]
  fn pane_default_has_no_backend_no_secondary_nothing_visible() {
    let p = NotesPaneModel::default();
    assert!(p.app.is_none(), "backend lazy-loads, default is None");
    assert!(p.secondary.is_none(), "secondary is Option, defaults to None");
    assert!(!p.primary_visible);
    assert!(!p.secondary_visible);
    // Primary instance is always allocated even on default.
    assert!(p.primary.tabs.is_empty());
  }

  #[test]
  fn secondary_visible_meaningless_without_secondary() {
    // Invariant: secondary_visible only matters when secondary.is_some().
    // The default state shows the degenerate case — visibility false
    // but no instance behind it.
    let p = NotesPaneModel::default();
    assert!(p.secondary.is_none() && !p.secondary_visible);
  }

  #[test]
  fn instance_mode_can_diverge_from_default() {
    let mut m = NotesInstanceModel::default();
    m.mode = NotesMode::Capture;
    // Per-instance mode: tests that different instances can hold
    // different modes (the property that justifies S4 in ADR-003).
    let other = NotesInstanceModel::default();
    assert_ne!(m.mode, other.mode);
    assert_eq!(other.mode, NotesMode::Library);
  }
}
