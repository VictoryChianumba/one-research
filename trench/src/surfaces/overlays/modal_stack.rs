//! Overlay lifecycle: which modals are active, in stack order.
//!
//! Top of the stack intercepts input. Variants are tags — the actual
//! state lives on App fields (e.g. `app.sources_popup` holds
//! `SourcesSurface`). The stack is ordered, supports stacking
//! (modal-over-modal), and is the single source of truth for "which
//! overlay should receive this key event."
//!
//! Phase 2 only converts Sources; other overlays still use their bool
//! `active` flags + `view: AppView` until they migrate. The two
//! representations stay in sync at the call sites that push/pop.

/// Tag for an active overlay. Variants accrete as overlays migrate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveModal {
  /// Sources popup (URL detection + arxiv categories + custom feeds).
  Sources,
}

#[derive(Debug, Default)]
pub struct ModalStack {
  stack: Vec<ActiveModal>,
}

impl ModalStack {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn push(&mut self, modal: ActiveModal) {
    self.stack.push(modal);
  }

  pub fn pop(&mut self) -> Option<ActiveModal> {
    self.stack.pop()
  }

  /// Top of the stack — the modal that should intercept input.
  pub fn top(&self) -> Option<&ActiveModal> {
    self.stack.last()
  }

  pub fn is_empty(&self) -> bool {
    self.stack.is_empty()
  }

  /// Remove a specific modal from the stack regardless of position.
  /// Used when a modal is dismissed by a non-Esc path (e.g. settings
  /// transition) and the stack needs to drop it without affecting
  /// modals stacked above.
  pub fn remove(&mut self, modal: &ActiveModal) {
    self.stack.retain(|m| m != modal);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn empty_by_default() {
    let s = ModalStack::new();
    assert!(s.is_empty());
    assert!(s.top().is_none());
  }

  #[test]
  fn push_pop_lifo() {
    let mut s = ModalStack::new();
    s.push(ActiveModal::Sources);
    assert_eq!(s.top(), Some(&ActiveModal::Sources));
    let popped = s.pop();
    assert_eq!(popped, Some(ActiveModal::Sources));
    assert!(s.is_empty());
  }

  #[test]
  fn remove_by_tag_preserves_others() {
    let mut s = ModalStack::new();
    s.push(ActiveModal::Sources);
    s.push(ActiveModal::Sources);
    s.remove(&ActiveModal::Sources);
    assert!(s.is_empty());
  }
}
