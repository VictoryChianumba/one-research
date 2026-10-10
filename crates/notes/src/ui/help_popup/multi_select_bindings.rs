use ratatui::widgets::TableState;

use super::keybindings_table::{CommandRow, KeybindingsTable};

#[derive(Debug)]
pub struct MultiSelectBindings {
  state: TableState,
  rows: Vec<CommandRow>,
}

impl Default for MultiSelectBindings {
  fn default() -> Self {
    Self::new()
  }
}

impl MultiSelectBindings {
  pub fn new() -> Self {
    Self { state: TableState::default(), rows: Vec::new() }
  }

  pub fn push(&mut self, row: CommandRow) {
    self.rows.push(row);
  }
}

impl KeybindingsTable for MultiSelectBindings {
  fn get_state_mut(&mut self) -> &mut TableState {
    &mut self.state
  }

  fn get_rows(&self) -> &[CommandRow] {
    &self.rows
  }

  fn get_title(&self) -> &str {
    "Multi-Select Mode Keybindings"
  }
}
