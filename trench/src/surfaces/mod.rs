//! Surface-oriented organization. Each surface owns its state, action
//! handling, and rendering entry point.
//!
//! No `Surface` trait yet — concrete dispatch lives in
//! [`crate::app::shell`]. The trait is delayed until enough surfaces
//! land to prove the shape of `handle_key` / `render` is uniform.

pub mod overlays;
