//! Sources popup — first overlay surface conversion. Owns its cursor,
//! URL detection input, and async detect state.
//!
//! Phase 2 milestone: this struct + its `handle_key` method replace
//! the `app.sources_popup: SourcesPopupState` field and the
//! `keys/sources.rs::handle_sources_popup` free function. Behavior is
//! preserved exactly — the diff is one of *ownership*, not semantics.

use crate::app::DiscoverResult;
use crate::primitives::{AsyncLoadState, TextInputState};

/// Sources popup surface state. Was `SourcesPopupState` in app/state/popups.rs.
#[derive(Default)]
pub struct SourcesSurface {
  /// Selected row in the sources list (input field is row 0).
  pub cursor: usize,
  /// URL detection input. `input.is_focused()` mirrors the prior
  /// `input_active: bool` companion field.
  pub input: TextInputState,
  /// URL detection async state machine. Idle / Loading(rx) / Ready(result)
  /// / Disconnected, replacing the prior `SourcesDetectState` enum +
  /// `detect_rx` pair.
  pub detect: AsyncLoadState<DiscoverResult>,
}

impl SourcesSurface {
  pub fn new() -> Self {
    Self::default()
  }
}
