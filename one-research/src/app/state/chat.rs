/// Chat pane state. Grouped from the previous flat fields on App
/// (`chat_ui`, `chat_active`, `chat_fullscreen`, `chat_at_top`).
#[derive(Default)]
pub struct ChatState {
  pub ui: Option<chat::ChatUi>,
  pub active: bool,
  pub fullscreen: bool,
  pub at_top: bool,
}
