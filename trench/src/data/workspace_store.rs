//! Authoritative in-memory application model. Items, history, tags,
//! workflow state, and the dedup indices that derive from them.
//!
//! No threads, no sockets, no disk writes here — those go in
//! [`crate::services`]. `Workspace` is pure state that the rest of
//! the app reads / mutates / queries.
//!
//! Phase 3.5 keeps the field set minimal: items + the two dedup
//! indices, history, tags, and persisted workflow states. Filters
//! (library_filter, active_filters) stay on App for now since they
//! are display selections, not data — a later phase may relocate
//! them onto their respective surfaces.

use std::collections::HashMap;

use crate::history::HistoryEntry;
use crate::models::{FeedItem, WorkflowState};
use crate::tags::ItemTags;

#[derive(Default)]
pub struct Workspace {
  /// Item corpus — currently visible feed items, after ingestion +
  /// enrichment. Mutated by ingestion completion routing,
  /// workflow-state changes, and discovery merges.
  pub items: Vec<FeedItem>,

  /// `url → index in self.items`. Maintained by the dedup loop in
  /// process_incoming and rebuilt by `rebuild_indices` after sort.
  pub url_index: HashMap<String, usize>,

  /// `arxiv_id → index in self.items`. Mirror of `url_index` for the
  /// HF/arXiv-collapse path.
  pub arxiv_id_index: HashMap<String, usize>,

  /// Activity log — paper opens and discovery queries. Persisted
  /// across runs.
  pub history: Vec<HistoryEntry>,

  /// Tag store: URL → list of tag names. Persisted to
  /// ~/.config/trench/tags.json.
  pub item_tags: ItemTags,

  /// Workflow state per URL, persisted across runs. Loaded on
  /// startup; written to by `set_workflow_state_for_url` (the
  /// chokepoint that emits Effect::WorkflowStateChanged).
  pub persisted_states: HashMap<String, WorkflowState>,
}

// Construction goes through `Workspace::default()` (derive(Default)).
// The `new()` constructor was removed on 2026-05-16 — no caller had
// taken it up since it landed.
