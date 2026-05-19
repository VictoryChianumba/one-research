//! `ViewFlags` — booleans tracking which transient popups are currently
//! visible.
//!
//! Three popups today, all keyboard-driven and mutually independent:
//!
//!   - [`tab_window_prompt_active`] — `[1]`/`[2]` prompt asking which
//!     reader window receives a newly-fetched fulltext.  Briefly visible
//!     between the fetch trigger and the user's choice.
//!   - [`abstract_popup_active`] — `Space` to quick-view the selected
//!     item's abstract without entering the reader.
//!   - [`narrow_feed_details_open`] — State-2 description popup over the
//!     reader pane (when the layout is in narrow-feed mode).
//!
//! Introduced by [ADR-009](../../../../docs/adr/ADR-009-app-field-grouping.md)
//! as cluster #4 after `DebounceState`, `LeaderState`, and
//! `ReaderBottomState`.  Public fields, no protocol methods — each
//! popup is opened and dismissed by independent key handlers and there
//! is no shared lifecycle to encapsulate.
//!
//! ## Scope note (corrected from ADR-009's original table)
//!
//! The audit grouped these three with `fulltext_new_tab` and
//! `fulltext_for_secondary` as a single 5-field cluster.  Reading the
//! call sites showed the fulltext-routing pair is *consumed* by the
//! async fetch-resolution path in `main.rs` (~lines 969-980) — they're
//! parameters of the pending fetch, not popup state.  Reclassified into
//! the future `AsyncJobs` cluster which owns `fulltext_rx`,
//! `fulltext_loading`, and `pending_fulltext_context`.

/// See module-level docs.
#[derive(Default, Debug)]
pub struct ViewFlags {
  /// `[1]`/`[2]` prompt asking which reader window receives a newly-
  /// fetched fulltext.
  pub tab_window_prompt_active: bool,
  /// `Space`-triggered abstract quick-view popup.
  pub abstract_popup_active: bool,
  /// State-2 description popup over the reader pane.
  pub narrow_feed_details_open: bool,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_matches_pre_grouping_initialisers() {
    // Load-bearing: pre-grouping the three flat App fields initialised
    // to false.  Deriving Default gives byte-equivalent behaviour.
    let f = ViewFlags::default();
    assert!(!f.tab_window_prompt_active);
    assert!(!f.abstract_popup_active);
    assert!(!f.narrow_feed_details_open);
  }

  #[test]
  fn flags_are_independent() {
    // Witness: each popup opens and closes without touching its
    // siblings — the cluster is just a grouping.  Bug class to avoid:
    // "opening the abstract popup accidentally clears tab_window_prompt."
    let mut f = ViewFlags::default();
    f.tab_window_prompt_active = true;
    f.abstract_popup_active = true;
    f.narrow_feed_details_open = true;

    f.tab_window_prompt_active = false;
    assert!(!f.tab_window_prompt_active);
    assert!(f.abstract_popup_active);
    assert!(f.narrow_feed_details_open);

    f.abstract_popup_active = false;
    assert!(f.narrow_feed_details_open);
  }
}
