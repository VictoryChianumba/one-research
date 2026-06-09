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

use crate::data::ItemStore;
use crate::history::HistoryEntry;
use crate::models::WorkflowState;
use crate::tags::ItemTags;

#[derive(Default)]
pub struct Workspace {
  /// Item corpus + dedup indices, encapsulated by `ItemStore` (ADR-007).
  /// All mutation flows through `ItemStore::push` / `replace_at` /
  /// `sort_by` / `rebuild_indices`; reads use `find_by_url`,
  /// `find_by_arxiv_id`, `iter`, etc. The triple-invariant ("both
  /// indices map into items") lives at the type after C9 PR 2.
  pub items_store: ItemStore,

  /// Activity log — paper opens and discovery queries. Persisted
  /// across runs.
  pub history: Vec<HistoryEntry>,

  /// Tag store: URL → list of tag names. Persisted to
  /// ~/.config/one-research/tags.json.
  pub item_tags: ItemTags,

  /// Workflow state per URL, persisted across runs. Loaded on
  /// startup; written to by `set_workflow_state_for_url` (the
  /// chokepoint that emits Effect::WorkflowStateChanged).
  pub persisted_states: HashMap<String, WorkflowState>,
}

// Construction goes through `Workspace::default()` (derive(Default)).
// The `new()` constructor was removed on 2026-05-16 — no caller had
// taken it up since it landed.
