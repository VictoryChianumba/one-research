use crossterm::event::{KeyCode, KeyEvent};

use crate::app::App;

pub(super) fn handle_tag_picker(key: KeyEvent, app: &mut App) {
  let all = crate::tags::all_tags(&app.workspace.item_tags);
  match key.code {
    KeyCode::Esc => {
      app.close_tag_picker();
    }
    KeyCode::Enter => {
      let trimmed = app.tag_picker.input.trim().to_string();
      if !trimmed.is_empty() {
        app.toggle_tag_on_targets(&trimmed);
        app.tag_picker.input.clear();
      } else if let Some(tag) = all.get(app.tag_picker.selected) {
        let tag = tag.clone();
        app.toggle_tag_on_targets(&tag);
      }
    }
    KeyCode::Char(' ') => {
      if let Some(tag) = all.get(app.tag_picker.selected) {
        let tag = tag.clone();
        app.toggle_tag_on_targets(&tag);
      }
    }
    KeyCode::Up => {
      app.tag_picker.selected = app.tag_picker.selected.saturating_sub(1);
    }
    KeyCode::Down => {
      if !all.is_empty() {
        app.tag_picker.selected =
          (app.tag_picker.selected + 1).min(all.len() - 1);
      }
    }
    KeyCode::Backspace => {
      app.tag_picker.input.pop();
    }
    KeyCode::Char(c) => {
      app.tag_picker.input.push(c);
    }
    _ => {}
  }
}
