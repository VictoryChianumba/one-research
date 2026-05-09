//! Floating UI surfaces with focus-capture lifecycle: dismissible
//! popups, full-screen modals, focus-stealing overlays.
//!
//! No `Modal<S>` generic yet — each overlay is a concrete struct.
//! [`ModalStack`] holds them as variants of [`ActiveModal`]; the top
//! variant intercepts input.

pub mod sources;

pub use sources::SourcesSurface;
