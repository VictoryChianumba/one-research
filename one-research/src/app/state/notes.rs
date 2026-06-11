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
  /// Cycle order for `Ldr+[` / `Ldr+]` mode rotation in the notes pane.
  /// Order is load-bearing — PaperNotes first matches the user-tested
  /// "default focus" when a paper context exists.
  pub const CYCLE_ORDER: [NotesMode; 3] =
    [NotesMode::PaperNotes, NotesMode::Library, NotesMode::Capture];

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

/// One notes context (primary or secondary). Owns mode + paper anchoring
/// + the browser selection. Pure content — visibility is a pane-level
/// concern and lives on [`NotesPaneModel`].
///
/// Per-instance: primary and secondary can be in different modes and
/// tied to different papers, accessed via `app.notes.<side>` (ADR-003).
///
/// Owns the browser selection (`selected`) as of ADR-016 — the dock is
/// the single owner of "which note is selected", not the backend's
/// `current_note_id`. Document tabs were removed (ADR-017): one note is
/// open at a time, addressed by `selected`.
#[derive(Default)]
pub struct NotesInstanceModel {
  pub mode: NotesMode,
  pub context: Option<NotesContext>,
  /// `note_id` selected in the browser list for this instance (ADR-016
  /// §S1). Selection identity, not a list index, so it survives
  /// re-sort / filter / deletion of other notes. `None` = nothing
  /// selected (empty list, or Capture mode). The dock owns this; the
  /// backend is seeded from it at the editor handoff.
  pub selected: Option<String>,
}

impl NotesInstanceModel {
  /// The `note_id` currently selected in this instance's browser, if any.
  pub fn selected_note_id(&self) -> Option<&str> {
    self.selected.as_deref()
  }

  /// Set the browser selection to a specific `note_id`, or clear it.
  pub fn select(&mut self, note_id: Option<String>) {
    self.selected = note_id;
  }

  /// Reconcile the selection against the current visible set: keep it if
  /// still present, otherwise fall back to the first visible note (or
  /// clear when nothing is visible). Returns the resulting selection.
  ///
  /// Pure — the visible set (mode + paper-link filtering) is computed by
  /// the orchestrator and passed in; the backend never enters here. This
  /// replaces `ensure_notes_browser_selection` (`keys/mod.rs:332`) when
  /// the dock is wired over in PR 2.
  pub fn reconcile_selection(&mut self, visible_ids: &[String]) -> Option<&str> {
    if visible_ids.is_empty() {
      self.selected = None;
    } else if !self
      .selected
      .as_ref()
      .is_some_and(|id| visible_ids.iter().any(|v| v == id))
    {
      self.selected = visible_ids.first().cloned();
    }
    self.selected.as_deref()
  }

  /// Move the selection within the visible set. `delta` is the step
  /// direction (the current callers pass ±1); `page` scales multi-row
  /// jumps; `absolute` overrides to a fixed index (`g` → `Some(0)`,
  /// `G` → `Some(usize::MAX)`). Clamps at both edges. Returns the
  /// resulting selection. Wired into the dock in ADR-016 PR3.
  ///
  /// NOTE: carries forward verbatim the quirk in the dock's original
  /// `move_notes_browser_selection` — `page` only applies on the
  /// `|delta| > 1` branch, which the callers never take, so PageUp /
  /// PageDown effectively step by one. Preserved deliberately for
  /// behaviour parity (ADR-016 §S5); a deliberate fix is deferred.
  pub fn move_selection(
    &mut self,
    visible_ids: &[String],
    delta: isize,
    page: usize,
    absolute: Option<usize>,
  ) -> Option<&str> {
    if visible_ids.is_empty() {
      self.selected = None;
      return None;
    }
    let last = visible_ids.len() - 1;
    let target = if let Some(absolute) = absolute {
      absolute.min(last)
    } else {
      let current = self
        .selected
        .as_ref()
        .and_then(|id| visible_ids.iter().position(|v| v == id))
        .unwrap_or(0);
      if delta == 0 {
        current
      } else if delta > 1 || delta < -1 {
        (current as isize + delta * page as isize).clamp(0, last as isize)
          as usize
      } else {
        (current as isize + delta).clamp(0, last as isize) as usize
      }
    };
    self.selected = visible_ids.get(target).cloned();
    self.selected.as_deref()
  }
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
  /// Primary pane is on screen. Independent of content — toggling off
  /// preserves the instance's selection/mode/context.
  pub primary_visible: bool,

  /// `None` = never opened. `Some(_)` = ever opened; visibility
  /// (hide-without-unload) controlled by `secondary_visible`.
  pub secondary: Option<NotesInstanceModel>,
  /// Only meaningful when `secondary.is_some()`.
  pub secondary_visible: bool,
}

impl NotesPaneModel {
  /// Reference to the instance at `side`. Primary always exists;
  /// secondary returns `None` when it's never been opened. Collapses
  /// the `app.notes_mode_for_side` style accessor onto the data.
  pub fn instance(
    &self,
    side: super::FocusedReader,
  ) -> Option<&NotesInstanceModel> {
    match side {
      super::FocusedReader::Primary => Some(&self.primary),
      super::FocusedReader::Secondary => self.secondary.as_ref(),
    }
  }

  /// Mutable counterpart of [`instance`]. Same Option-shape for
  /// secondary.
  pub fn instance_mut(
    &mut self,
    side: super::FocusedReader,
  ) -> Option<&mut NotesInstanceModel> {
    match side {
      super::FocusedReader::Primary => Some(&mut self.primary),
      super::FocusedReader::Secondary => self.secondary.as_mut(),
    }
  }

  /// Lazily-allocate secondary and return a mutable reference. Use when
  /// a gesture wants to open secondary notes for the first time —
  /// avoids the Option dance at call sites.
  pub fn instance_or_init_mut(
    &mut self,
    side: super::FocusedReader,
  ) -> &mut NotesInstanceModel {
    match side {
      super::FocusedReader::Primary => &mut self.primary,
      super::FocusedReader::Secondary => {
        self.secondary.get_or_insert_with(NotesInstanceModel::default)
      }
    }
  }

  /// Visibility flag for the given side. Centralizes the
  /// `notes_visible / secondary_notes_visible` dispatch so call sites
  /// don't `match` on side.
  pub fn is_visible(&self, side: super::FocusedReader) -> bool {
    match side {
      super::FocusedReader::Primary => self.primary_visible,
      super::FocusedReader::Secondary => {
        self.secondary.is_some() && self.secondary_visible
      }
    }
  }

  // ── Gesture surface (ADR-003 §S6) ───────────────────────────────────
  //
  // Pure-model gestures. They mutate notes pane state but never touch
  // the persistence backend (`self.app`) and never touch focus / pane
  // routing. Where a gesture might cascade into backend sync (e.g.,
  // tab switch → `notes::app::App::focus_note`), the method returns
  // the note_id and the caller does the sync.

  /// Set the visibility flag for `side`. Centralizes the
  /// `primary_visible` / `secondary_visible` dispatch. Mirrors
  /// [`is_visible`] on the write side.
  pub fn set_visible(&mut self, side: super::FocusedReader, visible: bool) {
    match side {
      super::FocusedReader::Primary => self.primary_visible = visible,
      super::FocusedReader::Secondary => self.secondary_visible = visible,
    }
  }

  /// True if either side of the notes pane is currently visible. Used
  /// by leader-key handlers that need a "should this Ldr+w route to
  /// notes?" gate.
  pub fn any_visible(&self) -> bool {
    self.primary_visible || self.secondary_visible
  }

  /// Hide both sides without dropping content. Used when switching to
  /// chat / closing all readers — visibility is layout, not lifecycle.
  pub fn hide_all(&mut self) {
    self.primary_visible = false;
    self.secondary_visible = false;
  }

  /// Compute the next mode in cycle order from `side`'s current mode.
  /// `direction` is `+1` for `Ldr+]`, `-1` for `Ldr+[`. Pure: returns
  /// the new mode without applying it — the caller (`activate_notes_mode`)
  /// applies it together with backend-side effects.
  ///
  /// When `side` has no instance (secondary not yet opened), starts
  /// from the default mode index 0.
  pub fn next_mode(
    &self,
    side: super::FocusedReader,
    direction: isize,
  ) -> NotesMode {
    let order = NotesMode::CYCLE_ORDER;
    let current_mode = self.instance(side).map(|i| i.mode).unwrap_or_default();
    let current =
      order.iter().position(|mode| *mode == current_mode).unwrap_or(0) as isize;
    let next = (current + direction).rem_euclid(order.len() as isize);
    order[next as usize]
  }

  /// Promote secondary to primary: `primary_visible` ← `secondary_visible`,
  /// secondary's content (mode + context + selection) replaces primary's,
  /// then secondary unloads and `secondary_visible` clears. Used when the
  /// reader transitions from dual → single.
  ///
  /// No-op (content-wise) when secondary is None — visibility flags still
  /// update since `secondary_visible` may have been true without an
  /// instance (defensive against invariant breaks).
  pub fn collapse_secondary_into_primary(&mut self) {
    self.primary_visible = self.secondary_visible;
    if let Some(sec) = self.secondary.take() {
      self.primary = sec;
    }
    self.secondary_visible = false;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn instance_default_is_empty_library() {
    let m = NotesInstanceModel::default();
    assert_eq!(m.mode, NotesMode::Library);
    assert!(m.context.is_none());
    assert!(m.selected.is_none(), "selection defaults to None");
  }

  // ── Selection ownership (ADR-016 §S1) ────────────────────────────
  //
  // These exercise the model's selection logic against a caller-supplied
  // visible set — no `notes::app::App` backend. They encode *why* the
  // selection survives list churn: a `note_id` identity, reconciled
  // against the current visible set, is what lets selection outlive
  // re-sort / filter / deletion.

  fn ids(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
  }

  #[test]
  fn select_then_read_round_trips() {
    let mut m = NotesInstanceModel::default();
    m.select(Some("n1".into()));
    assert_eq!(m.selected_note_id(), Some("n1"));
    m.select(None);
    assert_eq!(m.selected_note_id(), None);
  }

  #[test]
  fn reconcile_empty_visible_clears_selection() {
    let mut m = NotesInstanceModel::default();
    m.select(Some("n1".into()));
    // Nothing visible (e.g. Capture mode, or a paper with no links):
    // the selection must clear so render never points at a hidden note.
    assert_eq!(m.reconcile_selection(&ids(&[])), None);
    assert!(m.selected.is_none());
  }

  #[test]
  fn reconcile_keeps_still_visible_selection() {
    let mut m = NotesInstanceModel::default();
    m.select(Some("n2".into()));
    // n2 is still in the visible set after a re-sort — selection is
    // stable, not reset to the top. This is the property index-based
    // selection couldn't give us.
    assert_eq!(m.reconcile_selection(&ids(&["n3", "n2", "n1"])), Some("n2"));
  }

  #[test]
  fn reconcile_falls_back_to_first_when_selection_gone() {
    let mut m = NotesInstanceModel::default();
    m.select(Some("deleted".into()));
    // The selected note was deleted / filtered out: fall back to the
    // first visible note rather than dangling on a missing id.
    assert_eq!(m.reconcile_selection(&ids(&["n1", "n2"])), Some("n1"));
    assert_eq!(m.selected_note_id(), Some("n1"));
  }

  #[test]
  fn reconcile_with_no_selection_defaults_to_first() {
    let mut m = NotesInstanceModel::default();
    assert_eq!(m.reconcile_selection(&ids(&["a", "b"])), Some("a"));
  }

  #[test]
  fn move_selection_steps_and_clamps_at_edges() {
    let mut m = NotesInstanceModel::default();
    let v = ids(&["a", "b", "c"]);
    m.select(Some("a".into()));
    assert_eq!(m.move_selection(&v, 1, 1, None), Some("b")); // j
    assert_eq!(m.move_selection(&v, 1, 1, None), Some("c"));
    assert_eq!(m.move_selection(&v, 1, 1, None), Some("c")); // clamp at last
    assert_eq!(m.move_selection(&v, -1, 1, None), Some("b")); // k
  }

  #[test]
  fn move_selection_absolute_g_and_capital_g() {
    let mut m = NotesInstanceModel::default();
    let v = ids(&["a", "b", "c"]);
    m.select(Some("b".into()));
    // `G` passes usize::MAX and must land on the last note, not panic.
    assert_eq!(m.move_selection(&v, 0, 1, Some(usize::MAX)), Some("c"));
    // `g` passes 0.
    assert_eq!(m.move_selection(&v, 0, 1, Some(0)), Some("a"));
  }

  #[test]
  fn move_selection_empty_clears() {
    let mut m = NotesInstanceModel::default();
    m.select(Some("a".into()));
    assert_eq!(m.move_selection(&ids(&[]), 1, 1, None), None);
    assert!(m.selected.is_none());
  }

  #[test]
  fn move_selection_page_jump_quirk_is_preserved() {
    // Locks ADR-016 §S5: PageUp/PageDown pass delta=±1, page=8, but the
    // dock's original logic only multiplies by `page` on the |delta|>1
    // branch — so page keys step by ONE, not eight. We carry this
    // forward verbatim for behaviour parity; this test fails loudly if a
    // future change "fixes" it without intending to.
    let mut m = NotesInstanceModel::default();
    let v = ids(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
    m.select(Some("a".into()));
    assert_eq!(
      m.move_selection(&v, 1, 8, None),
      Some("b"),
      "page key steps by one, matching the preserved dock quirk"
    );
  }

  #[test]
  fn pane_default_has_no_backend_no_secondary_nothing_visible() {
    let p = NotesPaneModel::default();
    assert!(p.app.is_none(), "backend lazy-loads, default is None");
    assert!(p.secondary.is_none(), "secondary is Option, defaults to None");
    assert!(!p.primary_visible);
    assert!(!p.secondary_visible);
    // Primary instance is always allocated even on default.
    assert!(p.primary.selected.is_none());
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

  // ── Gesture tests (ADR-003 §S6) ──────────────────────────────────
  //
  // These exercise the model methods without instantiating the
  // `notes::app::App` persistence backend.

  use super::super::FocusedReader;

  #[test]
  fn next_mode_cycles_forward_through_order() {
    let mut p = NotesPaneModel::default();
    // Default instance starts at NotesMode::Library.
    assert_eq!(p.primary.mode, NotesMode::Library);
    // Cycle order is [PaperNotes, Library, Capture]; from Library
    // forward lands on Capture.
    let next = p.next_mode(FocusedReader::Primary, 1);
    assert_eq!(next, NotesMode::Capture);
    p.primary.mode = next;
    // Capture forward wraps to PaperNotes.
    assert_eq!(p.next_mode(FocusedReader::Primary, 1), NotesMode::PaperNotes);
  }

  #[test]
  fn next_mode_cycles_backward_through_order() {
    let mut p = NotesPaneModel::default();
    // Library backward → PaperNotes (one step left in the order).
    let prev = p.next_mode(FocusedReader::Primary, -1);
    assert_eq!(prev, NotesMode::PaperNotes);
    p.primary.mode = prev;
    // PaperNotes backward wraps to Capture.
    assert_eq!(p.next_mode(FocusedReader::Primary, -1), NotesMode::Capture);
  }

  #[test]
  fn next_mode_on_unopened_secondary_uses_default_mode() {
    let p = NotesPaneModel::default();
    // Secondary is None; next_mode falls back to the *default mode*
    // (`Library`), which sits at index 1 of the cycle. Going +1
    // lands on `Capture`. Important: this must not panic on a None
    // secondary — that was the latent risk before the Option-aware
    // accessor was in place.
    assert_eq!(p.next_mode(FocusedReader::Secondary, 1), NotesMode::Capture);
  }

  #[test]
  fn set_visible_and_any_visible_track_per_side_flags() {
    let mut p = NotesPaneModel::default();
    assert!(!p.any_visible());
    p.set_visible(FocusedReader::Primary, true);
    assert!(p.any_visible());
    assert!(p.primary_visible);
    p.set_visible(FocusedReader::Primary, false);
    p.set_visible(FocusedReader::Secondary, true);
    assert!(p.any_visible());
  }

  #[test]
  fn hide_all_clears_both_flags_without_dropping_content() {
    let mut p = NotesPaneModel::default();
    p.primary.select(Some("a".into()));
    p.secondary = Some(NotesInstanceModel::default());
    p.primary_visible = true;
    p.secondary_visible = true;
    p.hide_all();
    assert!(!p.primary_visible);
    assert!(!p.secondary_visible);
    // Content survives visibility toggle — visibility is layout, not
    // lifecycle (ADR-003 §S3).
    assert_eq!(p.primary.selected_note_id(), Some("a"));
    assert!(p.secondary.is_some());
  }

  #[test]
  fn collapse_secondary_into_primary_promotes_content_and_visibility() {
    let mut p = NotesPaneModel::default();
    p.primary.select(Some("old".into()));
    p.primary_visible = false;
    let mut sec = NotesInstanceModel::default();
    sec.mode = NotesMode::PaperNotes;
    sec.select(Some("new".into()));
    p.secondary = Some(sec);
    p.secondary_visible = true;
    p.collapse_secondary_into_primary();
    // Visibility followed secondary.
    assert!(p.primary_visible);
    assert!(!p.secondary_visible);
    // Secondary's content (mode + selection) is now primary's.
    assert_eq!(p.primary.selected_note_id(), Some("new"));
    assert_eq!(p.primary.mode, NotesMode::PaperNotes);
    // Secondary unloaded.
    assert!(p.secondary.is_none());
  }

  #[test]
  fn collapse_with_no_secondary_still_clears_flags() {
    let mut p = NotesPaneModel::default();
    p.primary.select(Some("a".into()));
    p.primary_visible = true;
    p.secondary_visible = true;
    // Degenerate case: secondary_visible was true without an instance
    // (invariant break). Collapse should still clear flags and not panic.
    p.collapse_secondary_into_primary();
    assert!(p.primary_visible);
    assert!(!p.secondary_visible);
    assert!(p.secondary.is_none());
    // Primary content untouched.
    assert_eq!(p.primary.selected_note_id(), Some("a"));
  }
}
