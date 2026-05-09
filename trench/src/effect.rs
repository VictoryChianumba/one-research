//! Cross-surface effects emitted by surface action handlers, drained and
//! routed by the orchestrator after the surface returns.
//!
//! Empty in Phase 2 — surfaces still mutate `&mut App` directly during
//! migration. Phase 3 adds the first variants (cache invalidation,
//! workflow state changes, async load completions) and surfaces stop
//! reaching for `&mut App` entirely.
//!
//! The empty enum is intentional: the type exists so signatures can be
//! `fn handle_key(&mut self, ...) -> Vec<Effect>` from Phase 2 onward,
//! avoiding a return-type churn in Phase 3.

#[derive(Debug, Clone)]
pub enum Effect {
  // No variants yet. Phase 3 lands the first.
}
