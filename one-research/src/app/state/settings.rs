/// Settings screen edit-time state. Grouped from `settings_*` fields.
/// `default_chat_provider` defaults to "claude" (not empty); custom Default impl.
pub struct SettingsEditState {
  pub field: usize,
  pub editing: bool,
  pub edit_buf: String,
  pub github_token: String,
  pub s2_key: String,
  pub claude_key: String,
  pub openai_key: String,
  pub default_chat_provider: String,
  pub save_time: Option<std::time::Instant>,
}

impl Default for SettingsEditState {
  fn default() -> Self {
    Self {
      field: 0,
      editing: false,
      edit_buf: String::new(),
      github_token: String::new(),
      s2_key: String::new(),
      claude_key: String::new(),
      openai_key: String::new(),
      default_chat_provider: "claude".to_string(),
      save_time: None,
    }
  }
}
