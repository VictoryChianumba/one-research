use crossterm::event::{KeyCode, KeyEvent};

use crate::app::App;

pub(super) fn handle_help_overlay(key: KeyEvent, app: &mut App) -> bool {
  if !app.help.active {
    return false;
  }
  match key.code {
    KeyCode::Tab | KeyCode::Char('l') => {
      app.help.section = (app.help.section + 1) % crate::ui::HELP_SECTION_COUNT;
      app.help.scroll = 0;
    }
    KeyCode::BackTab | KeyCode::Char('h') => {
      app.help.section = app.help.section.saturating_sub(1);
      app.help.scroll = 0;
    }
    KeyCode::Char('j') | KeyCode::Down => {
      app.help.scroll = app.help.scroll.saturating_add(1);
    }
    KeyCode::Char('k') | KeyCode::Up => {
      app.help.scroll = app.help.scroll.saturating_sub(1);
    }
    KeyCode::Char('q') | KeyCode::Esc => {
      app.help.active = false;
    }
    _ => {}
  }
  true
}
