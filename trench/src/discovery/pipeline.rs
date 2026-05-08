use std::sync::mpsc;

use serde_json::Value;

use crate::config::Config;
use crate::discovery::intent::QueryIntent;
use crate::discovery::{DiscoveryMessage, agent, ai_query};

/// Spawn a discovery thread. Uses the ReAct agent when a Claude API key is
/// available; falls back to the single-shot arXiv plan otherwise.
pub fn spawn_discovery(
  topic: String,
  config: Config,
  tx: mpsc::Sender<DiscoveryMessage>,
  prior_history: Option<Vec<Value>>,
  intent: QueryIntent,
) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      if config
        .claude_api_key
        .as_deref()
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
      {
        agent::run(&topic, &config, &tx, prior_history, intent);
      } else {
        run_fallback(&topic, &config, &tx);
      }
    }));
    if let Err(payload) = result {
      let msg = crate::panic_msg(payload);
      log::error!("discovery::spawn_discovery: thread panicked — {msg}");
      let _ = tx_panic.send(DiscoveryMessage::Error(format!(
        "discovery thread panicked: {msg}"
      )));
    }
  });
}

/// Single-shot arXiv search via the old ai_query path (used when no Claude key).
fn run_fallback(
  topic: &str,
  config: &Config,
  tx: &mpsc::Sender<DiscoveryMessage>,
) {
  let _ = tx.send(DiscoveryMessage::StatusUpdate(
    "Starting discovery (single-shot mode)…".to_string(),
  ));

  let plan = match ai_query::run_ai_query(topic, config) {
    Ok(p) => p,
    Err(e) => {
      let _ = tx.send(DiscoveryMessage::Error(e));
      return;
    }
  };

  let _ = tx.send(DiscoveryMessage::StatusUpdate(format!(
    "Searching arXiv for: {}",
    plan.search_terms.join(", ")
  )));

  for term in &plan.search_terms {
    match crate::ingestion::arxiv::search_query(term, 20) {
      Ok(items) if !items.is_empty() => {
        let _ = tx.send(DiscoveryMessage::Items(items));
      }
      _ => {}
    }
  }

  let new_categories: Vec<String> = plan
    .arxiv_categories
    .iter()
    .filter(|cat| !config.sources.arxiv_categories.contains(*cat))
    .cloned()
    .collect();
  if !new_categories.is_empty() {
    if let Ok(items) = crate::ingestion::arxiv::fetch(&new_categories) {
      if !items.is_empty() {
        let _ = tx.send(DiscoveryMessage::Items(items));
      }
    }
  }

  if !plan.paper_ids.is_empty() {
    if let Ok(items) = crate::ingestion::arxiv::fetch_by_ids(&plan.paper_ids) {
      if !items.is_empty() {
        let _ = tx.send(DiscoveryMessage::Items(items));
      }
    }
  }

  let _ = tx.send(DiscoveryMessage::Complete);
}
