use std::collections::HashSet;

use crate::models::{ContentType, FeedItem, SignalLevel, WorkflowState};

pub struct FilterState {
  pub sources: HashSet<String>,
  pub signals: HashSet<SignalLevel>,
  pub content_types: HashSet<ContentType>,
  pub workflow_states: HashSet<WorkflowState>,
  pub tags: HashSet<String>,
}

impl Default for FilterState {
  fn default() -> Self {
    Self {
      sources: HashSet::new(),
      signals: HashSet::new(),
      content_types: HashSet::new(),
      workflow_states: HashSet::new(),
      tags: HashSet::new(),
    }
  }
}

impl FilterState {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn is_empty(&self) -> bool {
    self.sources.is_empty()
      && self.signals.is_empty()
      && self.content_types.is_empty()
      && self.workflow_states.is_empty()
      && self.tags.is_empty()
  }

  pub fn matches(&self, item: &FeedItem) -> bool {
    (self.sources.is_empty() || {
      let sname = if item.source_name.is_empty() {
        item.source_platform.short_label().to_string()
      } else {
        item.source_name.clone()
      };
      self.sources.contains(&sname)
    }) && (self.signals.is_empty() || self.signals.contains(&item.signal))
      && (self.content_types.is_empty()
        || self.content_types.contains(&item.content_type))
      && (self.workflow_states.is_empty()
        || self.workflow_states.contains(&item.workflow_state))
  }

  pub fn active_count(&self) -> usize {
    self.sources.len()
      + self.signals.len()
      + self.content_types.len()
      + self.workflow_states.len()
      + self.tags.len()
  }
}

/// Memoized aggregate counts derived from `App.items`. Recomputed lazily on
/// the first read after `render_caches.invalidate_counts` clears the cell.
#[derive(Default, Clone)]
pub struct ItemCounts {
  pub inbox: usize,
  pub queued: usize,
  pub deep_read: usize,
  pub archived: usize,
  pub total: usize,

  pub recent_total: usize,
  pub recent_today: usize,
  pub recent_hf: usize,
  pub recent_arxiv: usize,
  pub recent_other: usize,

  /// First two queued paper titles, cloned so the memoized struct doesn't
  /// hold a borrow back into `App.items`.
  pub queue_preview: Vec<String>,
}

#[derive(PartialEq)]
pub enum AppView {
  Feed,
  Settings,
  Sources,
  RepoViewer,
}

#[derive(PartialEq, Clone, Copy)]
pub enum FeedTab {
  Inbox,
  Library,
  Discoveries,
  History,
}

/// Direction for spatial pane navigation.
#[derive(Clone, Copy, Debug)]
pub enum NavDirection {
  Left,
  Right,
  Up,
  Down,
}
