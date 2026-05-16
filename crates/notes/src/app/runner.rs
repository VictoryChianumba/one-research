use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};
use ui_theme::Theme;

use super::{App, HandleInputReturn, theme};

/// Render the notes UI into `area` within the caller's frame.
pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, t: &Theme) {
  theme::set_current(t);
  app.draw(frame, area);
}

/// Forward a key event to the notes app.
/// Returns `true` when notes wants to quit (caller should hide the pane).
pub fn handle_key(key: KeyEvent, app: &mut App) -> bool {
  matches!(app.handle_input(key), HandleInputReturn::ExitApp)
}
