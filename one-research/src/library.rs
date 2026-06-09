use crate::models::WorkflowState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibraryFilter {
  /// Everything decided about, except archived: state ∈ {Queued, DeepRead}.
  #[default]
  All,
  Queue,
  Read,
  Archived,
}

impl LibraryFilter {
  pub fn label(self) -> &'static str {
    match self {
      Self::All => "All",
      Self::Queue => "Queue",
      Self::Read => "Read",
      Self::Archived => "Archived",
    }
  }

  pub const ORDER: [Self; 4] =
    [Self::All, Self::Queue, Self::Read, Self::Archived];

  pub fn next(self) -> Self {
    let idx = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
    Self::ORDER[(idx + 1) % Self::ORDER.len()]
  }

  pub fn prev(self) -> Self {
    let idx = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
    Self::ORDER[(idx + Self::ORDER.len() - 1) % Self::ORDER.len()]
  }

  pub fn matches(self, state: WorkflowState) -> bool {
    match self {
      Self::All => {
        matches!(state, WorkflowState::Queued | WorkflowState::DeepRead)
      }
      Self::Queue => state == WorkflowState::Queued,
      Self::Read => state == WorkflowState::DeepRead,
      Self::Archived => state == WorkflowState::Archived,
    }
  }
}
