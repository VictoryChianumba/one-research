//! Shared interaction primitives.
//!
//! Each primitive owns a small state machine for a recurring UI pattern:
//! list selection, scroll, text input, palette filtering, async load.
//! Surfaces compose primitives instead of hand-rolling cursors / offsets /
//! buffers, so the invariants (selection-stays-visible, scroll-in-bounds,
//! cursor-in-string) live with the state they belong to.

pub mod list_state;
pub mod scroll_state;
pub mod text_input;

pub use list_state::ListState;
pub use scroll_state::ScrollState;
pub use text_input::TextInputState;
