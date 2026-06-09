use crate::app::{App, FeedTab};

pub fn start(app: &mut App, topic: String) {
  app.feed.feed_tab = FeedTab::Discoveries;
  app.reset_active_feed_position();
  app.status_message = Some(format!("Discovering: {topic}"));
  crate::spawn_ai_discovery(topic, app.config.clone(), app);
}
