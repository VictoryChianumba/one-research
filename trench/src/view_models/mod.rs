//! Display-shaped data derived from domain types. View models exist
//! where presentation logic was repeated across multiple render sites
//! (feed rows, history rows, status bar assembly) — not as a uniform
//! adapter layer over every domain type.
//!
//! Per the architectural principles: only create a VM when there's
//! real duplication or width-keyed derivation. Domain types that map
//! 1:1 to render output stay raw; render code reads them directly.

pub mod feed_row;

pub use feed_row::{FeedRowVm, feed_source_label};
