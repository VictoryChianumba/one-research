pub mod agent;
pub mod ai_query;
pub mod intent;
pub mod pipeline;
pub mod tools;

use crate::models::FeedItem;

/// Hard ceiling on stored message turns — keeps context tokens bounded.
pub const MAX_SESSION_MESSAGES: usize = 40;

/// Serialisable snapshot of one discovery conversation.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct SessionHistory {
  /// Raw Claude API messages array (user / assistant / tool_result turns).
  pub messages: Vec<serde_json::Value>,
  /// First query that opened this session — used in the UI badge and chat summary.
  pub initial_query: String,
  /// Intent classified from the initial query — preserved across refinements.
  #[serde(default)]
  pub query_intent: intent::QueryIntent,
}

impl SessionHistory {
  pub fn is_empty(&self) -> bool {
    self.messages.is_empty()
  }

  /// Drop oldest turns (after messages[0]) until len ≤ MAX_SESSION_MESSAGES.
  /// messages[0] is always the original user query and is never dropped.
  pub fn truncate_to_limit(&mut self) {
    if self.messages.len() <= MAX_SESSION_MESSAGES {
      return;
    }
    let overage = self.messages.len() - MAX_SESSION_MESSAGES;
    self.messages.drain(1..1 + overage);
  }
}

pub enum DiscoveryMessage {
  /// One-line status update shown in the search bar banner.
  StatusUpdate(String),
  /// Batch of discovered papers to merge into the feed.
  Items(Vec<FeedItem>),
  /// Sent just before Complete — carries the full message log for the next refinement query.
  SessionSnapshot(SessionHistory),
  Complete,
  Error(String),
}
