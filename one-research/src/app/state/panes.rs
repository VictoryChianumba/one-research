use ratatui::layout::Rect;

/// Identifies a pane in the pane registry.
/// Discriminants are array indices — must stay contiguous from 0..PANE_COUNT.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum PaneId {
  Feed = 0,
  Reader = 1,
  Notes = 2,
  Details = 3,
  Chat = 4,
  SecondaryReader = 5,
  SecondaryNotes = 6,
}

pub const PANE_COUNT: usize = 7;

/// Which reader pane has focus in dual-reader (State 3) mode.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FocusedReader {
  Primary,
  Secondary,
}

/// Tracks a pane's current screen position and open state.
#[derive(Clone)]
pub struct PaneInfo {
  pub id: PaneId,
  pub rect: Rect,
  pub is_open: bool,
}

impl PaneInfo {
  pub(crate) fn new(id: PaneId) -> Self {
    Self { id, rect: Rect::default(), is_open: false }
  }

  pub(crate) fn is_focusable(&self) -> bool {
    !matches!(self.id, PaneId::Details)
  }
}
