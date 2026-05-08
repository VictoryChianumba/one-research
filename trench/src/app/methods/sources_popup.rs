use crate::app::{App, DiscoverResult, SourcesDetectState};

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

  /// Poll the discovery background thread and update detect state.
  pub fn poll_detect_result(&mut self) {
    use std::sync::mpsc::TryRecvError;
    let result = if let Some(rx) = &self.sources_popup.detect_rx {
      Some(rx.try_recv())
    } else {
      None
    };
    match result {
      Some(Ok(r)) => {
        self.sources_popup.detect_state = SourcesDetectState::Result(r);
        self.sources_popup.detect_rx = None;
      }
      Some(Err(TryRecvError::Disconnected)) => {
        self.sources_popup.detect_state = SourcesDetectState::Result(
          DiscoverResult::Failed("Detection thread disconnected".to_string()),
        );
        self.sources_popup.detect_rx = None;
      }
      _ => {}
    }
  }
}
