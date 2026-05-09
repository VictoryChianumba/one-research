//! Shared interaction primitives.
//!
//! Each primitive owns a small state machine for a recurring UI pattern:
//! list selection, scroll, text input, palette filtering, async load.
//! Surfaces compose primitives instead of hand-rolling cursors / offsets /
//! buffers, so the invariants (selection-stays-visible, scroll-in-bounds,
//! cursor-in-string) live with the state they belong to.

pub mod list_state;

pub use list_state::ListState;
