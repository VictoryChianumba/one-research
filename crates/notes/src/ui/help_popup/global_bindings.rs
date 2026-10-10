use ratatui::widgets::TableState;

use super::keybindings_table::{CommandRow, KeybindingsTable};

#[derive(Debug)]
pub struct GlobalBindings {
  state: TableState,
  rows: Vec<CommandRow>,
}

impl Default for GlobalBindings {
  fn default() -> Self {
    Self::new()
  }
}

impl GlobalBindings {
  pub fn new() -> Self {
    Self { state: TableState::default(), rows: Vec::new() }
  }

  pub fn push(&mut self, row: CommandRow) {
    self.rows.push(row);
  }
}

impl KeybindingsTable for GlobalBindings {
  fn get_state_mut(&mut self) -> &mut TableState {
    &mut self.state
  }

  fn get_rows(&self) -> &[CommandRow] {
    &self.rows
  }

  fn get_title(&self) -> &str {
    "Global Keybindings"
  }
}
