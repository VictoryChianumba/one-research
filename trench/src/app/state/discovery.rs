use std::collections::HashMap;
use std::sync::mpsc::Receiver;

use crate::discovery::DiscoveryMessage;
use crate::models::FeedItem;

/// Discovery pane state — agent results, search bar, palette, multi-turn
/// session. Largest cluster on App; grouped to make ownership explicit.
#[derive(Default)]
pub struct DiscoveryState {
  pub items: Vec<FeedItem>,
  pub url_index: HashMap<String, usize>,
  pub arxiv_id_index: HashMap<String, usize>,
  pub selected_index: usize,
  pub list_offset: usize,
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
  /// Selected row index in the slash-command palette.
  pub palette_selected: usize,
  /// Scroll offset for the palette (when suggestions exceed visible rows).
  pub palette_scroll: usize,
}
