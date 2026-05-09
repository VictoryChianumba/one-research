//! Top-level UI action vocabulary. Translated from key events by
//! [`crate::keys`] before being routed to a surface. Variants accrete as
//! surfaces migrate.
//!
//! Phase 2 ships only the action variants needed by Sources (the first
//! converted overlay). Phase 3 expands the vocabulary as more surfaces
//! convert; Phase 4 hoists render-time state mutations into action
//! handlers so render becomes pure.

#[derive(Debug, Clone)]
pub enum Action {
  /// Top of the modal stack should be dismissed (Esc, q, etc.).
  DismissTopModal,
  /// Generic "open settings view" — used by Sources to leave back to
  /// Settings on Esc/q.
  OpenSettings,
}
