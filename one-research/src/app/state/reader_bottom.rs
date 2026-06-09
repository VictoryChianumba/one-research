//! `ReaderBottomState` — the State-3 reader-bottom drawer's UI state.
//!
//! The reader-bottom drawer is the pane that appears below the dual
//! reader in State 3, summoned by `Ldr+f` and dismissed by `q` / `Esc`.
//! It has two view modes — *details* (showing the selected item's
//! summary) and *feed list* (the standard feed view rendered into the
//! drawer's area) — toggled by `Ldr+f` while open.
//!
//! Introduced by [ADR-009](../../../../docs/adr/ADR-009-app-field-grouping.md)
//! as the third cluster after `DebounceState` and `LeaderState`.
//! Replaces five flat `App` fields:
//!
//!   - `reader_bottom_open`         → [`open`]
//!   - `reader_bottom_focused`      → [`focused`]
//!   - `reader_bottom_details`      → [`details`]
//!   - `reader_feed_popup_selected` → [`feed_popup_selected`]
//!   - `reader_bottom_scroll`       → [`scroll`]
//!
//! Unlike [`DebounceState`](super::DebounceState) and
//! [`LeaderState`](super::LeaderState), this cluster ships with **public
//! fields and no protocol methods**.  The drawer's gestures (toggle
//! open, swap mode, move selection) are simple one-off mutations spread
//! across `keys/` and `app/methods/`; encapsulating them as methods
//! today would add a layer without depth.  Matches the convention
//! established by `ReaderPaneModel` / `ReaderInstanceModel` (ADR-002).
//!
//! All defaults preserve byte-equivalent pre-grouping behaviour:
//! drawer starts closed, unfocused, in feed-list mode, selection at 0,
//! scroll at zero.

use crate::primitives::ScrollState;

/// See module-level docs.
#[derive(Default, Debug)]
pub struct ReaderBottomState {
  /// Drawer visibility — true while the pane is shown.
  pub open: bool,
  /// Whether the drawer currently has keyboard focus.  When false the
  /// drawer is visible but key events route to the dual reader above.
  pub focused: bool,
  /// View mode within the drawer: `true` = showing details, `false` =
  /// showing the feed list.  Toggled by `Ldr+f` while open.
  pub details: bool,
  /// Selected index in the bottom feed list.  Only meaningful when
  /// `!details`; in details mode the feed list isn't rendered.
  pub feed_popup_selected: usize,
  /// Scroll offset for both view modes.  Each render path sets `max`
  /// appropriately for its mode (details bounds = paragraph height,
  /// feed-list bounds = item count − viewport).
  pub scroll: ScrollState,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_matches_pre_grouping_initialisers() {
    // Load-bearing: pre-grouping the five flat App fields initialised
    // to (false, false, false, 0, ScrollState::new()).  Deriving Default
    // gives byte-equivalent behaviour — the migration's correctness
    // criterion for ADR-009.
    let s = ReaderBottomState::default();
    assert!(!s.open);
    assert!(!s.focused);
    assert!(!s.details);
    assert_eq!(s.feed_popup_selected, 0);
    assert_eq!(s.scroll.offset(), 0);
    assert_eq!(s.scroll.max(), 0);
  }

  #[test]
  fn fields_are_independently_mutable() {
    // Witness: the cluster is just a grouping, not a state machine.
    // Each field is settable independently — no surprise coupling
    // (e.g., toggling `open` should not zero `feed_popup_selected`).
    let mut s = ReaderBottomState::default();
    s.open = true;
    s.feed_popup_selected = 42;
    assert!(s.open);
    assert_eq!(s.feed_popup_selected, 42);

    s.open = false;
    assert!(!s.open);
    assert_eq!(s.feed_popup_selected, 42, "selection survives close");
  }
}
