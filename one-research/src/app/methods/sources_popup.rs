use crate::app::App;

impl App {
  /// Total number of cursor-navigable rows in the sources popup.
  ///
  /// ADR-010 PR 3 removed the arXiv-categories section — that management
  /// surface lives in the Subject Browser (`FeedTab::Browse`) now. The
  /// popup retains its input field, predefined-sources toggles, and
  /// custom-feed rows.
  pub fn sources_popup_total_items(&self) -> usize {
    1 // input field
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
