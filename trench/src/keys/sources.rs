use crossterm::event::KeyEvent;

use crate::app::App;
use crate::surfaces::overlays::ActiveModal;

/// Thin dispatch shim. The Sources popup is a real surface in
/// `surfaces/overlays/sources.rs` — this function just consults the
/// modal stack and does the borrow-check dance to call `handle_key`
/// with a `&mut App` while the surface itself lives at
/// `app.sources_popup`.
///
/// The `mem::take` pattern dodges the &mut self / &mut App conflict by
/// detaching the surface from App for the duration of the call. It
/// retires when Phase 3's effect routing eliminates the &mut App
/// argument entirely.
pub(super) fn handle_sources_popup(key: KeyEvent, app: &mut App) -> bool {
  if app.modals.top() != Some(&ActiveModal::Sources) {
    return false;
  }
  let mut surface = std::mem::take(&mut app.sources_popup);
  let _effects = surface.handle_key(key, app);
  app.sources_popup = surface;
  true
}
