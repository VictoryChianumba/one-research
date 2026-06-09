//! Cursor + offset + viewport for a vertical, single-selection list.
//!
//! Owns the invariant *the selected index is visible in the viewport*.
//! `set_viewport` is called from layout (once per frame) so movement and
//! count changes can adjust `offset` without the caller threading the
//! viewport size through every call.
//!
//! Anchor-based extended selection (visual mode) is **not** part of this
//! primitive — surfaces that need it own the anchor as a separate field
//! and call `set_selected` for direct cursor placement.

// Tested forward-design: `page_down`/`page_up`/`go_to_top`/`go_to_bottom`
// etc. are exercised by the in-file unit tests but not yet wired into
// pane-level key handlers. See audit C11 (2026-05-18).
#![allow(dead_code)]

#[derive(Debug, Clone, Default)]
pub struct ListState {
  count: usize,
  selected: usize,
  offset: usize,
  viewport_size: usize,
}

impl ListState {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn count(&self) -> usize {
    self.count
  }

  pub fn selected(&self) -> usize {
    self.selected
  }

  pub fn offset(&self) -> usize {
    self.offset
  }

  pub fn viewport_size(&self) -> usize {
    self.viewport_size
  }

  /// Update the underlying item count. Clamps `selected` if the count
  /// shrank below it (e.g. after a delete or filter change).
  pub fn set_count(&mut self, count: usize) {
    self.count = count;
    if count == 0 {
      self.selected = 0;
      self.offset = 0;
      return;
    }
    if self.selected >= count {
      self.selected = count - 1;
    }
    self.ensure_visible();
  }

  /// Update the visible row count. Called from layout each frame so
  /// `ensure_visible` can adjust the offset on resize.
  pub fn set_viewport(&mut self, viewport_size: usize) {
    self.viewport_size = viewport_size;
    self.ensure_visible();
  }

  /// Place the cursor at an arbitrary index, clamped to `[0, count)`.
  /// Used by visual-mode extension and other direct-placement callers.
  pub fn set_selected(&mut self, idx: usize) {
    if self.count == 0 {
      self.selected = 0;
    } else {
      self.selected = idx.min(self.count - 1);
    }
    self.ensure_visible();
  }

  pub fn move_down(&mut self) {
    if self.count == 0 {
      return;
    }
    self.selected = (self.selected + 1).min(self.count - 1);
    self.ensure_visible();
  }

  pub fn move_up(&mut self) {
    self.selected = self.selected.saturating_sub(1);
    self.ensure_visible();
  }

  pub fn page_down(&mut self) {
    if self.count == 0 {
      return;
    }
    let step = self.viewport_size.max(1);
    self.selected = (self.selected + step).min(self.count - 1);
    self.ensure_visible();
  }

  pub fn page_up(&mut self) {
    let step = self.viewport_size.max(1);
    self.selected = self.selected.saturating_sub(step);
    self.ensure_visible();
  }

  pub fn go_to_top(&mut self) {
    self.selected = 0;
    self.offset = 0;
  }

  pub fn go_to_bottom(&mut self) {
    if self.count > 0 {
      self.selected = self.count - 1;
    }
    self.ensure_visible();
  }

  /// Reset cursor and offset to zero. Used when the underlying list is
  /// replaced or the user explicitly cycles to a different filter view.
  pub fn reset(&mut self) {
    self.selected = 0;
    self.offset = 0;
  }

  /// Direct offset bypass for transitional draw-time clamps. Phase 4
  /// (update/render separation) will retire this — surfaces will set
  /// the viewport via `set_viewport` and never the offset directly.
  pub fn set_offset(&mut self, offset: usize) {
    self.offset = offset;
  }

  /// Adjust `offset` so `selected` falls within `[offset, offset +
  /// viewport_size)`. No-op when `viewport_size == 0` because layout
  /// hasn't reported a visible window yet.
  fn ensure_visible(&mut self) {
    if self.viewport_size == 0 {
      return;
    }
    if self.selected < self.offset {
      self.offset = self.selected;
    } else if self.selected >= self.offset + self.viewport_size {
      self.offset = self.selected + 1 - self.viewport_size;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn defaults_are_zero() {
    let s = ListState::new();
    assert_eq!(s.selected(), 0);
    assert_eq!(s.offset(), 0);
    assert_eq!(s.count(), 0);
    assert_eq!(s.viewport_size(), 0);
  }

  #[test]
  fn move_down_stops_at_last_index() {
    let mut s = ListState::new();
    s.set_count(3);
    s.set_viewport(10);
    s.move_down();
    s.move_down();
    s.move_down();
    s.move_down();
    assert_eq!(s.selected(), 2);
  }

  #[test]
  fn move_up_saturates_at_zero() {
    let mut s = ListState::new();
    s.set_count(3);
    s.set_viewport(10);
    s.move_up();
    assert_eq!(s.selected(), 0);
  }

  #[test]
  fn page_down_jumps_by_viewport() {
    let mut s = ListState::new();
    s.set_count(100);
    s.set_viewport(10);
    s.page_down();
    assert_eq!(s.selected(), 10);
    s.page_down();
    assert_eq!(s.selected(), 20);
  }

  #[test]
  fn page_up_jumps_by_viewport_saturating() {
    let mut s = ListState::new();
    s.set_count(100);
    s.set_viewport(10);
    s.set_selected(5);
    s.page_up();
    assert_eq!(s.selected(), 0);
  }

  #[test]
  fn ensure_visible_scrolls_offset_down() {
    let mut s = ListState::new();
    s.set_count(100);
    s.set_viewport(5);
    s.set_selected(10);
    // selected=10 with viewport=5 means offset should be 6 (10 - 4)
    assert_eq!(s.offset(), 6);
  }

  #[test]
  fn ensure_visible_scrolls_offset_up() {
    let mut s = ListState::new();
    s.set_count(100);
    s.set_viewport(5);
    s.set_offset(20);
    s.set_selected(2);
    assert_eq!(s.offset(), 2);
  }

  #[test]
  fn set_count_clamps_selected_when_shrinking() {
    let mut s = ListState::new();
    s.set_count(100);
    s.set_viewport(10);
    s.set_selected(99);
    s.set_count(50);
    assert_eq!(s.selected(), 49);
  }

  #[test]
  fn set_count_to_zero_resets_cursor_and_offset() {
    let mut s = ListState::new();
    s.set_count(100);
    s.set_viewport(10);
    s.set_selected(50);
    s.set_count(0);
    assert_eq!(s.selected(), 0);
    assert_eq!(s.offset(), 0);
  }

  #[test]
  fn go_to_top_resets_offset() {
    let mut s = ListState::new();
    s.set_count(100);
    s.set_viewport(10);
    s.set_selected(50);
    s.go_to_top();
    assert_eq!(s.selected(), 0);
    assert_eq!(s.offset(), 0);
  }

  #[test]
  fn go_to_bottom_lands_on_last_with_visible_offset() {
    let mut s = ListState::new();
    s.set_count(100);
    s.set_viewport(10);
    s.go_to_bottom();
    assert_eq!(s.selected(), 99);
    assert_eq!(s.offset(), 90);
  }

  #[test]
  fn reset_zeros_cursor_and_offset_but_keeps_count_and_viewport() {
    let mut s = ListState::new();
    s.set_count(100);
    s.set_viewport(10);
    s.set_selected(50);
    s.reset();
    assert_eq!(s.selected(), 0);
    assert_eq!(s.offset(), 0);
    assert_eq!(s.count(), 100);
    assert_eq!(s.viewport_size(), 10);
  }

  #[test]
  fn set_viewport_runs_ensure_visible_on_shrink() {
    let mut s = ListState::new();
    s.set_count(100);
    s.set_viewport(20);
    s.set_selected(15);
    // selected=15 fits in viewport=20 starting from offset=0
    assert_eq!(s.offset(), 0);
    // shrinking viewport to 5 should bump offset to keep selected visible
    s.set_viewport(5);
    assert_eq!(s.offset(), 11);
  }

  #[test]
  fn move_down_on_empty_list_is_noop() {
    let mut s = ListState::new();
    s.move_down();
    s.move_up();
    s.page_down();
    s.go_to_bottom();
    assert_eq!(s.selected(), 0);
    assert_eq!(s.offset(), 0);
  }
}
