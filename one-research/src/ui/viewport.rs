//! `Viewport` — the per-frame layout context passed into `Model::pre_draw`.
//!
//! Carries no ratatui types so models stay layout-toolkit-agnostic. See
//! `docs/adr/ADR-001-render-purification.md` for the contract.

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Viewport {
  pub rows: u16,
  pub cols: u16,
}

impl Viewport {
  pub fn new(cols: u16, rows: u16) -> Self {
    Self { rows, cols }
  }
}
