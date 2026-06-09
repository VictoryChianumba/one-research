use std::collections::HashMap;
use std::sync::mpsc::Receiver;

use crate::discovery::DiscoveryMessage;
use crate::models::FeedItem;

/// Composition-root model for the discovery sub-pane. Owns the agent
/// search bar, the discovered-items list + dedup indices, the multi-turn
/// session history, intent classification, the slash-command palette,
/// and the background agent's `Receiver<DiscoveryMessage>`. Sibling to
/// `FeedModel`, `ReaderPaneModel`, and `NotesPaneModel`.
///
/// Lives at `App.discovery`. Introduced by slice 5 (`ADR-005`); was
/// `DiscoveryState` pre-rename and `App.feed.discovery` pre-PR 2.
#[derive(Default)]
pub struct DiscoveryModel {
  pub items: Vec<FeedItem>,
  pub url_index: HashMap<String, usize>,
  pub arxiv_id_index: HashMap<String, usize>,
  /// Cursor + offset for the Discoveries items list. Migrated from
  /// raw scalars in Migration #3. Counterpart to `palette` below.
  pub list: crate::primitives::ListState,
  pub rx: Option<Receiver<DiscoveryMessage>>,
  /// Last status line from the agent ("Searching…", "Found N papers", etc.).
  pub status: String,
  pub query: String,
  /// Lowercased mirror of `query`. Refreshed by mutator helpers.
  /// Avoids the per-frame `to_lowercase` heap allocation in the palette draw.
  pub query_lower: String,
  /// Whether the persistent search bar at the bottom of Discoveries has focus.
  pub search_focused: bool,
  pub loading: bool,
  /// Accumulated agent message history — enables multi-turn refinement.
  pub session: crate::discovery::SessionHistory,
  /// Set by Ctrl+N — forces a fresh session even when history exists.
  pub force_new: bool,
  /// Classified intent of the current/last discovery query.
  pub intent: crate::discovery::intent::QueryIntent,
  /// When set by a slash command, overrides heuristic classification once.
  pub forced_intent: Option<crate::discovery::intent::QueryIntent>,
  /// Slash-command palette selection + scroll. Count is set each frame
  /// from `discovery_palette_count(query)`.
  pub palette: crate::primitives::ListState,
}

impl DiscoveryModel {
  // ── Search-bar gestures ───────────────────────────────────────────────
  //
  // The four query mutators below all keep `query_lower` in lock-step with
  // `query` so the per-frame palette filter pass (drawn at 60Hz) can do an
  // ASCII compare without re-running `to_lowercase()` over a heap-allocated
  // copy of the query each frame. Lifted from the `App` wrappers in
  // `app/caches.rs` in C7 PR 3 (ADR-005 §S5).

  /// Append a character to the search query.
  pub fn push_char(&mut self, c: char) {
    self.query.push(c);
    self.query_lower = self.query.to_lowercase();
  }

  /// Pop the trailing character from the search query (no-op when empty).
  pub fn pop_char(&mut self) {
    self.query.pop();
    self.query_lower = self.query.to_lowercase();
  }

  /// Clear the search query and its lowercase mirror.
  pub fn clear_query(&mut self) {
    self.query.clear();
    self.query_lower.clear();
  }

  /// Replace the search query (used by slash-palette tab-completion).
  pub fn set_query(&mut self, s: String) {
    self.query = s;
    self.query_lower = self.query.to_lowercase();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::discovery::intent::QueryIntent;

  #[test]
  fn default_model_is_empty_and_unfocused() {
    let m = DiscoveryModel::default();
    assert!(m.items.is_empty());
    assert!(m.query.is_empty());
    assert!(m.query_lower.is_empty());
    assert!(!m.search_focused);
    assert!(!m.loading);
    assert!(!m.force_new);
    assert!(m.rx.is_none());
    assert!(m.status.is_empty());
    assert!(m.session.is_empty());
    assert!(m.forced_intent.is_none());
  }

  #[test]
  fn indices_default_empty() {
    // Invariant: both dedup indices start empty alongside an empty items vec.
    // After PR 2, render paths read `app.discovery.url_index` and
    // `app.discovery.arxiv_id_index`; a non-empty index with an empty
    // items list would be a torn state.
    let m = DiscoveryModel::default();
    assert!(m.url_index.is_empty());
    assert!(m.arxiv_id_index.is_empty());
  }

  #[test]
  fn default_intent_is_default_classifier_state() {
    // QueryIntent::default() represents "not yet classified" — PR 3 will
    // grow gesture methods that set this on intent classification.
    let m = DiscoveryModel::default();
    assert_eq!(m.intent, QueryIntent::default());
  }

  #[test]
  fn list_and_palette_are_separate_cursors() {
    // The discovery items list and the slash-command palette have
    // independent cursors. Default state confirms they start at 0.
    let m = DiscoveryModel::default();
    assert_eq!(m.list.selected(), 0);
    assert_eq!(m.palette.selected(), 0);
  }

  #[test]
  fn push_char_appends_and_mirrors_lowercase() {
    // Invariant: `query_lower` always tracks `query`. Render hot path
    // skips the per-frame `to_lowercase` heap alloc by reading the mirror.
    let mut m = DiscoveryModel::default();
    m.push_char('A');
    m.push_char('B');
    assert_eq!(m.query, "AB");
    assert_eq!(m.query_lower, "ab");
  }

  #[test]
  fn pop_char_shortens_and_mirrors() {
    let mut m = DiscoveryModel::default();
    m.set_query("Hello".to_string());
    m.pop_char();
    assert_eq!(m.query, "Hell");
    assert_eq!(m.query_lower, "hell");
  }

  #[test]
  fn pop_char_on_empty_is_a_noop() {
    // Regression guard: pop on empty must not panic or desync the mirror.
    let mut m = DiscoveryModel::default();
    m.pop_char();
    assert!(m.query.is_empty());
    assert!(m.query_lower.is_empty());
  }

  #[test]
  fn clear_query_resets_both_fields() {
    let mut m = DiscoveryModel::default();
    m.set_query("HasContent".to_string());
    m.clear_query();
    assert!(m.query.is_empty());
    assert!(m.query_lower.is_empty());
  }

  #[test]
  fn set_query_replaces_existing_content() {
    // Tab-completion path: arbitrary string replaces whatever was typed.
    let mut m = DiscoveryModel::default();
    m.set_query("first".to_string());
    m.set_query("/search".to_string());
    assert_eq!(m.query, "/search");
    assert_eq!(m.query_lower, "/search");
  }
}
