use crate::app::App;

impl App {
  pub fn sources_popup_arxiv_cats(&self) -> Vec<(String, String)> {
    let mut cats: Vec<(String, String)> = crate::config::KNOWN_ARXIV_CATS
      .iter()
      .map(|(code, label)| (code.to_string(), label.to_string()))
      .collect();
    for cat in &self.config.sources.arxiv_categories {
      if !crate::config::KNOWN_ARXIV_CATS
        .iter()
        .any(|(k, _)| *k == cat.as_str())
      {
        cats.push((cat.clone(), String::new()));
      }
    }
    cats
  }

  /// Total number of cursor-navigable rows in the sources popup.
  pub fn sources_popup_total_items(&self) -> usize {
    1 // input field
      + self.sources_popup_arxiv_cats().len()
      + crate::config::PREDEFINED_SOURCES.len()
      + self.config.sources.custom_feeds.len()
  }

  /// Advance the discovery background thread's load state. Called once
  /// per frame from the event loop. AsyncLoadState handles the actual
  /// state machine internally — Loading → Ready when a value arrives,
  /// Loading → Disconnected if the sender drops without producing one.
  pub fn poll_detect_result(&mut self) {
    self.sources_popup.detect.poll();
  }
}
