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
  /// Cycle order for `Ldr+[` / `Ldr+]` mode rotation in the notes pane.
  /// Order is load-bearing — PaperNotes first matches the user-tested
  /// "default focus" when a paper context exists.
  pub const CYCLE_ORDER: [NotesMode; 3] =
    [NotesMode::PaperNotes, NotesMode::Library, NotesMode::Capture];

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

/// Outcome of [`NotesPaneModel::close_active_tab`]. Forces callers to
/// handle the two structurally different post-conditions explicitly: the
/// pane just emptied (caller should hide it) vs. another tab took over
/// (caller should sync the persistence backend to the new note_id).
pub enum CloseTabOutcome {
  /// Side had no tabs, or `secondary` was `None`. Nothing was done.
  NoOp,
  /// Removed the active tab; another tab is now active. Carries its
  /// `note_id` so the caller can call `notes::app::App::focus_note`.
  Switched(String),
  /// Removed the last tab on this side. Caller should set visibility
  /// off and shift focus away — the model has done its part.
  BecameEmpty,
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

  /// Hide both sides without dropping tabs. Used when switching to
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

  /// Advance to the next tab on `side`, saturating at the last index.
  /// Returns the `note_id` of the newly active tab so the caller can
  /// re-focus the persistence backend.
  pub fn next_tab(&mut self, side: super::FocusedReader) -> Option<String> {
    let inst = self.instance_mut(side)?;
    if inst.tabs.is_empty() {
      return None;
    }
    let len = inst.tabs.len();
    inst.active_tab = (inst.active_tab + 1).min(len - 1);
    inst.tabs.get(inst.active_tab).map(|t| t.note_id.clone())
  }

  /// Step back to the previous tab on `side`, saturating at 0.
  pub fn prev_tab(&mut self, side: super::FocusedReader) -> Option<String> {
    let inst = self.instance_mut(side)?;
    if inst.tabs.is_empty() {
      return None;
    }
    inst.active_tab = inst.active_tab.saturating_sub(1);
    inst.tabs.get(inst.active_tab).map(|t| t.note_id.clone())
  }

  /// Remove the active tab on `side`. The returned [`CloseTabOutcome`]
  /// makes "pane is now empty" vs "tab switched" explicit at the call
  /// site — the previous inline form bundled the two via a boolean and
  /// a second `instance_mut` borrow.
  pub fn close_active_tab(
    &mut self,
    side: super::FocusedReader,
  ) -> CloseTabOutcome {
    let Some(inst) = self.instance_mut(side) else {
      return CloseTabOutcome::NoOp;
    };
    if inst.tabs.is_empty() {
      return CloseTabOutcome::NoOp;
    }
    inst.active_tab = inst.active_tab.min(inst.tabs.len() - 1);
    inst.tabs.remove(inst.active_tab);
    if inst.tabs.is_empty() {
      return CloseTabOutcome::BecameEmpty;
    }
    inst.active_tab = inst.active_tab.min(inst.tabs.len() - 1);
    let note_id = inst
      .tabs
      .get(inst.active_tab)
      .map(|t| t.note_id.clone())
      .unwrap_or_default();
    CloseTabOutcome::Switched(note_id)
  }

  /// Add `tab` to `side` if not already present, then make it active.
  /// Lazily initialises secondary. Used when a freshly created note
  /// surfaces from the persistence backend.
  pub fn add_or_focus_tab(
    &mut self,
    side: super::FocusedReader,
    tab: NotesTab,
  ) {
    let inst = self.instance_or_init_mut(side);
    if let Some(idx) = inst.tabs.iter().position(|t| t.note_id == tab.note_id) {
      inst.active_tab = idx;
    } else {
      inst.tabs.push(tab);
      inst.active_tab = inst.tabs.len() - 1;
    }
  }

  /// Drop tabs on both sides whose `note_id` no longer satisfies
  /// `exists`. Clamps `active_tab` to remain in range. Used at notes
  /// pane open to prune stale `ui.json` entries against the live
  /// notes-backend index.
  pub fn prune_tabs<F: Fn(&str) -> bool>(&mut self, exists: F) {
    self.primary.tabs.retain(|t| exists(&t.note_id));
    self.primary.active_tab =
      self.primary.active_tab.min(self.primary.tabs.len().saturating_sub(1));
    if let Some(sec) = self.secondary.as_mut() {
      sec.tabs.retain(|t| exists(&t.note_id));
      sec.active_tab = sec.active_tab.min(sec.tabs.len().saturating_sub(1));
    }
  }

  /// Promote secondary to primary: `primary_visible` ← `secondary_visible`,
  /// secondary's tabs + active_tab replace primary's, then secondary
  /// unloads and `secondary_visible` clears. Used when the reader
  /// transitions from dual → single (reader.rs's "primary ran out of
  /// tabs in dual mode" pattern).
  ///
  /// No-op when secondary is None — visibility flags still update
  /// since `secondary_visible` may have been true without an instance
  /// (defensive against invariant breaks).
  pub fn collapse_secondary_into_primary(&mut self) {
    self.primary_visible = self.secondary_visible;
    if let Some(sec) = self.secondary.take() {
      self.primary.tabs = sec.tabs;
      self.primary.active_tab = sec.active_tab;
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

  // ── Gesture tests (PR 3, ADR-003 §S6) ────────────────────────────
  //
  // These exercise the model methods without instantiating the
  // `notes::app::App` persistence backend. The synthetic tabs use
  // throwaway note_ids; we don't care whether they exist in any real
  // notes store — we only care about the model transitions.

  use super::super::FocusedReader;

  fn tab(id: &str) -> NotesTab {
    NotesTab { note_id: id.to_string(), title: format!("title-{id}") }
  }

  fn pane_with_primary_tabs(ids: &[&str]) -> NotesPaneModel {
    let mut p = NotesPaneModel::default();
    p.primary.tabs = ids.iter().map(|id| tab(id)).collect();
    p
  }

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
  fn next_tab_advances_then_saturates() {
    let mut p = pane_with_primary_tabs(&["a", "b", "c"]);
    assert_eq!(p.next_tab(FocusedReader::Primary), Some("b".into()));
    assert_eq!(p.primary.active_tab, 1);
    assert_eq!(p.next_tab(FocusedReader::Primary), Some("c".into()));
    // Saturates at last index — does not wrap. Behavior carried over
    // from the original `notes_next_tab` (mod.rs:1172).
    assert_eq!(p.next_tab(FocusedReader::Primary), Some("c".into()));
    assert_eq!(p.primary.active_tab, 2);
  }

  #[test]
  fn prev_tab_steps_back_then_saturates_at_zero() {
    let mut p = pane_with_primary_tabs(&["a", "b", "c"]);
    p.primary.active_tab = 2;
    assert_eq!(p.prev_tab(FocusedReader::Primary), Some("b".into()));
    assert_eq!(p.prev_tab(FocusedReader::Primary), Some("a".into()));
    // saturating_sub means we sit at 0, not panic.
    assert_eq!(p.prev_tab(FocusedReader::Primary), Some("a".into()));
    assert_eq!(p.primary.active_tab, 0);
  }

  #[test]
  fn next_and_prev_tab_no_op_on_empty() {
    let mut p = NotesPaneModel::default();
    assert_eq!(p.next_tab(FocusedReader::Primary), None);
    assert_eq!(p.prev_tab(FocusedReader::Primary), None);
    // Secondary not allocated either.
    assert_eq!(p.next_tab(FocusedReader::Secondary), None);
  }

  #[test]
  fn close_active_tab_reports_switch_when_more_remain() {
    let mut p = pane_with_primary_tabs(&["a", "b", "c"]);
    p.primary.active_tab = 1;
    let out = p.close_active_tab(FocusedReader::Primary);
    match out {
      CloseTabOutcome::Switched(id) => assert_eq!(id, "c"),
      _ => panic!("expected Switched"),
    }
    assert_eq!(
      p.primary.tabs.iter().map(|t| t.note_id.as_str()).collect::<Vec<_>>(),
      vec!["a", "c"]
    );
    assert_eq!(p.primary.active_tab, 1);
  }

  #[test]
  fn close_active_tab_reports_became_empty_on_last_tab() {
    let mut p = pane_with_primary_tabs(&["a"]);
    let out = p.close_active_tab(FocusedReader::Primary);
    assert!(matches!(out, CloseTabOutcome::BecameEmpty));
    assert!(p.primary.tabs.is_empty());
  }

  #[test]
  fn close_active_tab_no_op_when_secondary_unopened() {
    let mut p = NotesPaneModel::default();
    assert!(matches!(
      p.close_active_tab(FocusedReader::Secondary),
      CloseTabOutcome::NoOp
    ));
  }

  #[test]
  fn add_or_focus_tab_pushes_new_and_focuses_existing() {
    let mut p = NotesPaneModel::default();
    p.add_or_focus_tab(FocusedReader::Primary, tab("a"));
    p.add_or_focus_tab(FocusedReader::Primary, tab("b"));
    assert_eq!(p.primary.tabs.len(), 2);
    assert_eq!(p.primary.active_tab, 1);
    // Re-adding "a" focuses the existing tab, doesn't duplicate.
    p.add_or_focus_tab(FocusedReader::Primary, tab("a"));
    assert_eq!(p.primary.tabs.len(), 2);
    assert_eq!(p.primary.active_tab, 0);
  }

  #[test]
  fn add_or_focus_tab_lazily_initialises_secondary() {
    let mut p = NotesPaneModel::default();
    assert!(p.secondary.is_none());
    p.add_or_focus_tab(FocusedReader::Secondary, tab("x"));
    // Invariant: secondary instance exists now; visibility unchanged
    // (still false — visibility is a separate concern per ADR-003 §S3).
    assert!(p.secondary.is_some());
    assert!(!p.secondary_visible);
    let sec = p.secondary.as_ref().unwrap();
    assert_eq!(sec.tabs.len(), 1);
    assert_eq!(sec.active_tab, 0);
  }

  #[test]
  fn prune_tabs_drops_unknown_ids_and_clamps_active() {
    let mut p = pane_with_primary_tabs(&["a", "b", "c"]);
    p.primary.active_tab = 2;
    p.add_or_focus_tab(FocusedReader::Secondary, tab("x"));
    p.add_or_focus_tab(FocusedReader::Secondary, tab("y"));
    // Only "a" survives on primary; "y" survives on secondary.
    let kept: std::collections::HashSet<&str> =
      ["a", "y"].into_iter().collect();
    p.prune_tabs(|id| kept.contains(id));
    assert_eq!(
      p.primary.tabs.iter().map(|t| t.note_id.as_str()).collect::<Vec<_>>(),
      vec!["a"]
    );
    assert_eq!(p.primary.active_tab, 0);
    let sec = p.secondary.as_ref().unwrap();
    assert_eq!(
      sec.tabs.iter().map(|t| t.note_id.as_str()).collect::<Vec<_>>(),
      vec!["y"]
    );
    assert_eq!(sec.active_tab, 0);
  }

  #[test]
  fn prune_tabs_handles_empty_primary_via_saturating_sub() {
    let mut p = pane_with_primary_tabs(&["a"]);
    p.prune_tabs(|_| false);
    assert!(p.primary.tabs.is_empty());
    // active_tab clamps to 0 (saturating_sub on len()=0).
    assert_eq!(p.primary.active_tab, 0);
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
  fn hide_all_clears_both_flags_without_dropping_tabs() {
    let mut p = pane_with_primary_tabs(&["a", "b"]);
    p.add_or_focus_tab(FocusedReader::Secondary, tab("x"));
    p.primary_visible = true;
    p.secondary_visible = true;
    p.hide_all();
    assert!(!p.primary_visible);
    assert!(!p.secondary_visible);
    // Tabs survive visibility toggle — that's the whole point of
    // separating visibility from lifecycle (ADR-003 §S3).
    assert_eq!(p.primary.tabs.len(), 2);
    assert!(p.secondary.is_some());
  }

  #[test]
  fn collapse_secondary_into_primary_promotes_tabs_and_visibility() {
    let mut p = pane_with_primary_tabs(&["old1", "old2"]);
    p.primary_visible = false;
    p.add_or_focus_tab(FocusedReader::Secondary, tab("new1"));
    p.add_or_focus_tab(FocusedReader::Secondary, tab("new2"));
    p.secondary.as_mut().unwrap().active_tab = 1;
    p.secondary_visible = true;
    p.collapse_secondary_into_primary();
    // Visibility followed secondary.
    assert!(p.primary_visible);
    assert!(!p.secondary_visible);
    // Tabs replaced — secondary's content is now primary's.
    assert_eq!(
      p.primary.tabs.iter().map(|t| t.note_id.as_str()).collect::<Vec<_>>(),
      vec!["new1", "new2"]
    );
    assert_eq!(p.primary.active_tab, 1);
    // Secondary unloaded.
    assert!(p.secondary.is_none());
  }

  #[test]
  fn collapse_with_no_secondary_still_clears_flags() {
    let mut p = pane_with_primary_tabs(&["a"]);
    p.primary_visible = true;
    p.secondary_visible = true;
    // Degenerate case: secondary_visible was true without an instance
    // (invariant break). Collapse should still clear flags and not panic.
    p.collapse_secondary_into_primary();
    assert!(p.primary_visible);
    assert!(!p.secondary_visible);
    assert!(p.secondary.is_none());
    // Primary tabs untouched.
    assert_eq!(p.primary.tabs.len(), 1);
  }
}
