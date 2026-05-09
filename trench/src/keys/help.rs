use crossterm::event::{KeyCode, KeyEvent};

use crate::app::App;

pub(super) fn handle_help_overlay(key: KeyEvent, app: &mut App) -> bool {
  if !app.help.active {
    return false;
  }
  match key.code {
    KeyCode::Tab | KeyCode::Char('l') => {
      app.help.section = (app.help.section + 1) % crate::ui::HELP_SECTION_COUNT;
      app.help.scroll.reset();
    }
    KeyCode::BackTab | KeyCode::Char('h') => {
      app.help.section = app.help.section.saturating_sub(1);
      app.help.scroll.reset();
    }
    KeyCode::Char('j') | KeyCode::Down => {
      app.help.scroll.scroll_down(1);
    }
    KeyCode::Char('k') | KeyCode::Up => {
      app.help.scroll.scroll_up(1);
    }
    KeyCode::Char('q') | KeyCode::Esc => {
      app.help.active = false;
    }
    _ => {}
  }
  true
}
