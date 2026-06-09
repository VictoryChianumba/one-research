//! Banner notification state. Held by App; rendered into the details
//! panel and the top status bar.
//!
//! `item_id` is the URL of the item that was selected when the
//! notification fired — the renderer compares against the currently
//! selected item's URL so the banner only shows over its originating
//! row, not any subsequent selection.

#[derive(Debug, Default)]
pub struct NotificationState {
  pub message: Option<String>,
  pub item_id: Option<String>,
}

impl NotificationState {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn clear(&mut self) {
    self.message = None;
    self.item_id = None;
  }
}
