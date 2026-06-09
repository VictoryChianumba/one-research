//! Scroll offset + max + viewport for a scrollable content surface.
//!
//! Companion to [`ListState`](super::list_state) for surfaces that scroll
//! content (rendered text, paragraph, paginated view) rather than navigating
//! a list of items. Owns the invariant *offset stays within [0, max]*.
//!
//! `set_max` is called from layout once the renderer knows how many lines
//! the content produced; `set_viewport` is called when the visible window
//! resizes. `set_max` clamps `offset` if the content shrank below it.

// Tested forward-design: paging/scroll-to-edge helpers are exercised by
// the in-file unit tests but not yet wired from pane key handlers. See
// audit C11 (2026-05-18).
#![allow(dead_code)]

#[derive(Debug, Clone, Default)]
pub struct ScrollState {
  offset: usize,
  max: usize,
  viewport_size: usize,
}

impl ScrollState {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn offset(&self) -> usize {
    self.offset
  }

  pub fn max(&self) -> usize {
    self.max
  }

  pub fn viewport_size(&self) -> usize {
    self.viewport_size
  }

  /// Update the maximum scrollable offset. Clamps `offset` if the content
  /// shrank below the current cursor (e.g. window resized wider so wrapped
  /// lines collapsed).
  pub fn set_max(&mut self, max: usize) {
    self.max = max;
    if self.offset > max {
      self.offset = max;
    }
  }

  pub fn set_viewport(&mut self, viewport_size: usize) {
    self.viewport_size = viewport_size;
  }

  pub fn scroll_down(&mut self, lines: usize) {
    self.offset = self.offset.saturating_add(lines).min(self.max);
  }

  pub fn scroll_up(&mut self, lines: usize) {
    self.offset = self.offset.saturating_sub(lines);
  }

  pub fn page_down(&mut self) {
    let step = self.viewport_size.max(1);
    self.scroll_down(step);
  }

  pub fn page_up(&mut self) {
    let step = self.viewport_size.max(1);
    self.scroll_up(step);
  }

  pub fn scroll_to_top(&mut self) {
    self.offset = 0;
  }

  pub fn scroll_to_bottom(&mut self) {
    self.offset = self.max;
  }

  pub fn set_offset(&mut self, offset: usize) {
    self.offset = offset.min(self.max);
  }

  /// Reset to top with current max preserved. Used when a tab changes and
  /// new content replaces the old.
  pub fn reset(&mut self) {
    self.offset = 0;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn defaults_are_zero() {
    let s = ScrollState::new();
    assert_eq!(s.offset(), 0);
    assert_eq!(s.max(), 0);
    assert_eq!(s.viewport_size(), 0);
  }

  #[test]
  fn scroll_down_clamps_at_max() {
    let mut s = ScrollState::new();
    s.set_max(10);
    s.scroll_down(15);
    assert_eq!(s.offset(), 10);
  }

  #[test]
  fn scroll_up_saturates_at_zero() {
    let mut s = ScrollState::new();
    s.set_max(10);
    s.scroll_up(5);
    assert_eq!(s.offset(), 0);
  }

  #[test]
  fn page_down_uses_viewport_size() {
    let mut s = ScrollState::new();
    s.set_max(100);
    s.set_viewport(8);
    s.page_down();
    assert_eq!(s.offset(), 8);
    s.page_down();
    assert_eq!(s.offset(), 16);
  }

  #[test]
  fn page_down_with_zero_viewport_steps_one() {
    let mut s = ScrollState::new();
    s.set_max(100);
    s.page_down();
    assert_eq!(s.offset(), 1);
  }

  #[test]
  fn set_max_clamps_offset_when_shrinking() {
    let mut s = ScrollState::new();
    s.set_max(100);
    s.set_offset(50);
    s.set_max(20);
    assert_eq!(s.offset(), 20);
  }

  #[test]
  fn set_offset_clamps_to_max() {
    let mut s = ScrollState::new();
    s.set_max(10);
    s.set_offset(50);
    assert_eq!(s.offset(), 10);
  }

  #[test]
  fn reset_zeros_offset_keeps_max_and_viewport() {
    let mut s = ScrollState::new();
    s.set_max(100);
    s.set_viewport(10);
    s.set_offset(50);
    s.reset();
    assert_eq!(s.offset(), 0);
    assert_eq!(s.max(), 100);
    assert_eq!(s.viewport_size(), 10);
  }

  #[test]
  fn scroll_to_bottom_lands_on_max() {
    let mut s = ScrollState::new();
    s.set_max(100);
    s.scroll_to_bottom();
    assert_eq!(s.offset(), 100);
  }

  #[test]
  fn scroll_down_on_zero_max_is_noop() {
    let mut s = ScrollState::new();
    s.scroll_down(10);
    assert_eq!(s.offset(), 0);
  }
}
