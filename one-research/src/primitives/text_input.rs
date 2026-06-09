//! Text input buffer + focus state.
//!
//! Owns the buffer and a focused-flag for surfaces that present a
//! line-edit (sources popup, future search bars). Cursor support is
//! intentionally not yet present; current consumers all append-at-end.
//!
//! Speculative forward-design APIs (`new`, `with_lower_mirror`,
//! `lower`, `len`, `set`, `take`) were dropped on 2026-05-16 — no
//! production caller had taken them up since they landed and the audit
//! flagged them as fiction.  Re-add informed by real usage when Slice 2
//! pulls us in.

#[derive(Debug, Clone, Default)]
pub struct TextInputState {
  buffer: String,
  focused: bool,
}

impl TextInputState {
  pub fn buffer(&self) -> &str {
    &self.buffer
  }

  pub fn is_focused(&self) -> bool {
    self.focused
  }

  pub fn is_empty(&self) -> bool {
    self.buffer.is_empty()
  }

  pub fn focus(&mut self) {
    self.focused = true;
  }

  pub fn blur(&mut self) {
    self.focused = false;
  }

  pub fn push_char(&mut self, c: char) {
    self.buffer.push(c);
  }

  pub fn pop_char(&mut self) -> Option<char> {
    self.buffer.pop()
  }

  pub fn clear(&mut self) {
    self.buffer.clear();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn defaults_empty_unfocused() {
    let s = TextInputState::default();
    assert!(s.is_empty());
    assert!(!s.is_focused());
    assert_eq!(s.buffer(), "");
  }

  #[test]
  fn push_and_pop() {
    let mut s = TextInputState::default();
    s.push_char('a');
    s.push_char('B');
    assert_eq!(s.buffer(), "aB");
    assert_eq!(s.pop_char(), Some('B'));
    assert_eq!(s.buffer(), "a");
  }

  #[test]
  fn pop_on_empty_returns_none() {
    let mut s = TextInputState::default();
    assert_eq!(s.pop_char(), None);
  }

  #[test]
  fn focus_and_blur() {
    let mut s = TextInputState::default();
    assert!(!s.is_focused());
    s.focus();
    assert!(s.is_focused());
    s.blur();
    assert!(!s.is_focused());
  }

  #[test]
  fn clear_resets_buffer() {
    let mut s = TextInputState::default();
    s.push_char('a');
    s.push_char('b');
    s.clear();
    assert!(s.is_empty());
  }
}
