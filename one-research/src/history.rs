use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::models::SourcePlatform;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoryKind {
  Paper,
  Query,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryPaperMeta {
  pub authors: Vec<String>,
  pub source_platform: SourcePlatform,
  #[serde(default)]
  pub published_at: String,
  #[serde(default)]
  pub summary_short: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
  pub kind: HistoryKind,
  pub key: String,
  pub title: String,
  pub source: String,
  #[serde(default)]
  pub paper_meta: Option<HistoryPaperMeta>,
  pub opened_at: DateTime<Utc>,
  pub visit_count: u32,
  /// Lowercased title cached for the history search filter. `#[serde(skip)]`
  /// keeps `history.json` unchanged; populated by the constructors below
  /// and backfilled in `store::history::load`. Mirror of `FeedItem.title_lower`.
  #[serde(skip)]
  pub title_lower: String,
}

#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize,
)]
pub enum HistoryFilter {
  #[default]
  All,
  Today,
  Last24h,
  Last48h,
  Week,
  Month,
}

impl HistoryFilter {
  pub fn label(self) -> &'static str {
    match self {
      Self::All => "All",
      Self::Today => "Today",
      Self::Last24h => "24h",
      Self::Last48h => "48h",
      Self::Week => "Week",
      Self::Month => "Month",
    }
  }

  pub const ORDER: [Self; 6] = [
    Self::All,
    Self::Today,
    Self::Last24h,
    Self::Last48h,
    Self::Week,
    Self::Month,
  ];

  pub fn next(self) -> Self {
    let idx = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
    Self::ORDER[(idx + 1) % Self::ORDER.len()]
  }

  pub fn prev(self) -> Self {
    let idx = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
    Self::ORDER[(idx + Self::ORDER.len() - 1) % Self::ORDER.len()]
  }

  pub fn matches(self, entry: &HistoryEntry, now: DateTime<Utc>) -> bool {
    self.matches_time(entry.opened_at, now)
  }

  /// Variant that operates on a raw timestamp — used by Library smart filters
  /// where the timestamp comes from a paper's most-recent open in history.
  pub fn matches_time(
    self,
    opened_at: DateTime<Utc>,
    now: DateTime<Utc>,
  ) -> bool {
    match self {
      Self::All => true,
      Self::Today => opened_at.date_naive() == now.date_naive(),
      Self::Last24h => {
        now.signed_duration_since(opened_at) <= Duration::hours(24)
      }
      Self::Last48h => {
        now.signed_duration_since(opened_at) <= Duration::hours(48)
      }
      Self::Week => now.signed_duration_since(opened_at) <= Duration::days(7),
      Self::Month => now.signed_duration_since(opened_at) <= Duration::days(30),
    }
  }
}

pub const HISTORY_CAP: usize = 500;
const REFINEMENT_DEDUP_WINDOW_SECS: i64 = 60;

/// Record a paper open. Dedupes by URL.
pub fn record_paper(
  history: &mut Vec<HistoryEntry>,
  url: String,
  title: String,
  source: String,
  meta: HistoryPaperMeta,
) {
  let now = Utc::now();
  if let Some(pos) =
    history.iter().position(|e| e.kind == HistoryKind::Paper && e.key == url)
  {
    let mut entry = history.remove(pos);
    entry.opened_at = now;
    entry.visit_count = entry.visit_count.saturating_add(1);
    entry.title_lower = title.to_lowercase();
    entry.title = title;
    entry.source = source;
    entry.paper_meta = Some(meta);
    history.insert(0, entry);
  } else {
    let title_lower = title.to_lowercase();
    history.insert(
      0,
      HistoryEntry {
        kind: HistoryKind::Paper,
        key: url,
        title,
        title_lower,
        source,
        paper_meta: Some(meta),
        opened_at: now,
        visit_count: 1,
      },
    );
  }
  history.truncate(HISTORY_CAP);
}

/// Record a discovery query. Dedupes the head if the same topic was logged < 60s ago.
pub fn record_query(
  history: &mut Vec<HistoryEntry>,
  topic: String,
  intent_label: &str,
) {
  let now = Utc::now();
  if let Some(head) = history.first_mut() {
    if head.kind == HistoryKind::Query
      && head.key == topic
      && now.signed_duration_since(head.opened_at).num_seconds()
        < REFINEMENT_DEDUP_WINDOW_SECS
    {
      head.opened_at = now;
      head.visit_count = head.visit_count.saturating_add(1);
      head.source = intent_label.to_string();
      return;
    }
  }
  if let Some(pos) =
    history.iter().position(|e| e.kind == HistoryKind::Query && e.key == topic)
  {
    let mut entry = history.remove(pos);
    entry.opened_at = now;
    entry.visit_count = entry.visit_count.saturating_add(1);
    entry.source = intent_label.to_string();
    history.insert(0, entry);
  } else {
    let title_lower = topic.to_lowercase();
    history.insert(
      0,
      HistoryEntry {
        kind: HistoryKind::Query,
        key: topic.clone(),
        title: topic,
        title_lower,
        source: intent_label.to_string(),
        paper_meta: None,
        opened_at: now,
        visit_count: 1,
      },
    );
  }
  history.truncate(HISTORY_CAP);
}

/// Format a timestamp as "2h ago", "3d ago", etc.
pub fn format_ago(opened_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
  let delta = now.signed_duration_since(opened_at);
  let secs = delta.num_seconds().max(0);
  if secs < 60 {
    "now".to_string()
  } else if secs < 3600 {
    format!("{}m ago", secs / 60)
  } else if secs < 86400 {
    format!("{}h ago", secs / 3600)
  } else if secs < 86400 * 7 {
    format!("{}d ago", secs / 86400)
  } else if secs < 86400 * 30 {
    format!("{}w ago", secs / (86400 * 7))
  } else {
    format!("{}mo ago", secs / (86400 * 30))
  }
}
