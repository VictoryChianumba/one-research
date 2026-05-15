//! `FeedModel` — composition-root state owner for the feed pane.
//!
//! Slice 1 PR 1 ships the empty shell. Subsequent PRs migrate feed-related
//! state off `App` (PR 2), wire `Action` routing (PR 3), introduce
//! `pre_draw` + read-only render (PR 4), and add cross-pane `Action`
//! emission (PR 5).
//!
//! Contract and rationale: `docs/adr/ADR-001-render-purification.md`.
//! Vocabulary: `docs/CONTEXT.md`.

use crate::app::FeedTab;

/// Owned state for the feed pane. Renders take `&FeedModel`, never `&mut`.
///
/// Empty in PR 1 — fields migrate from `App` in PR 2.
pub struct FeedModel {
  feed_tab: FeedTab,
}

impl FeedModel {
  /// The currently-selected feed tab.
  pub fn feed_tab(&self) -> FeedTab {
    self.feed_tab
  }
}

impl Default for FeedModel {
  fn default() -> Self {
    Self { feed_tab: FeedTab::Inbox }
  }
}

/// Per-frame, read-only context passed into feed-pane renders alongside
/// `&FeedModel`. Lands properly in PR 4 once the render flip happens —
/// will carry `&Workspace`, the active theme, and the current `Viewport`.
pub struct FeedContext;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_feed_tab_is_inbox() {
    let model = FeedModel::default();
    assert!(model.feed_tab() == FeedTab::Inbox);
  }
}
