//! Text input buffer + focus state.
//!
//! Owns the buffer, focused-flag, and (optional) lowercase mirror used by
//! search-style consumers that do case-insensitive matching against every
//! item on every cache miss. The mirror is opt-in via [`with_lower_mirror`]
//! — consumers that don't need it pay nothing.
//!
//! Cursor support is intentionally not yet present; current consumers all
//! append-at-end. When the chat input or notes editor migrate, this
//! primitive will grow `cursor: usize` (byte offset) + cursor-aware
//! `insert` / `backspace` methods.

#[derive(Debug, Clone, Default)]
pub struct TextInputState {
  buffer: String,
  focused: bool,
  /// Lowercase mirror of `buffer`, kept in sync by every mutator. `None`
  /// means the consumer opted out (no mirror cost).
  lower_mirror: Option<String>,
}

impl TextInputState {
  pub fn new() -> Self {
    Self::default()
  }

  /// Opt into a lowercase mirror, refreshed by every mutator. Used by
  /// search-style consumers to avoid `to_lowercase` on every match pass.
  pub fn with_lower_mirror() -> Self {
    Self { lower_mirror: Some(String::new()), ..Self::default() }
  }

  pub fn buffer(&self) -> &str {
    &self.buffer
  }

  /// Lowercase mirror, or `""` if the consumer didn't opt in. Always safe
  /// to call — returns the empty string rather than panicking on opt-out.
  pub fn lower(&self) -> &str {
    self.lower_mirror.as_deref().unwrap_or("")
  }

  pub fn is_focused(&self) -> bool {
    self.focused
  }

  pub fn is_empty(&self) -> bool {
    self.buffer.is_empty()
  }

  pub fn len(&self) -> usize {
    self.buffer.len()
  }

  pub fn focus(&mut self) {
    self.focused = true;
  }

  pub fn blur(&mut self) {
    self.focused = false;
  }

  pub fn push_char(&mut self, c: char) {
    self.buffer.push(c);
    self.refresh_lower();
  }

  pub fn pop_char(&mut self) -> Option<char> {
    let popped = self.buffer.pop();
    if popped.is_some() {
      self.refresh_lower();
    }
    popped
  }

  pub fn clear(&mut self) {
    self.buffer.clear();
    self.refresh_lower();
  }

  /// Replace the buffer with `value`. Used by completion paths (slash
  /// command palette) that overwrite the input wholesale.
  pub fn set(&mut self, value: String) {
    self.buffer = value;
    self.refresh_lower();
  }

  /// Take the buffer contents, leaving the input empty. Useful for
  /// committing input on Enter without an extra clone.
  pub fn take(&mut self) -> String {
    let out = std::mem::take(&mut self.buffer);
    self.refresh_lower();
    out
  }

  fn refresh_lower(&mut self) {
    if let Some(mirror) = self.lower_mirror.as_mut() {
      mirror.clear();
      mirror.extend(self.buffer.chars().flat_map(|c| c.to_lowercase()));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn defaults_empty_unfocused_no_mirror() {
    let s = TextInputState::new();
    assert!(s.is_empty());
    assert!(!s.is_focused());
    assert_eq!(s.buffer(), "");
    assert_eq!(s.lower(), "");
  }

  #[test]
  fn push_and_pop() {
    let mut s = TextInputState::new();
    s.push_char('a');
    s.push_char('B');
    assert_eq!(s.buffer(), "aB");
    assert_eq!(s.pop_char(), Some('B'));
    assert_eq!(s.buffer(), "a");
  }

  #[test]
  fn pop_on_empty_returns_none() {
    let mut s = TextInputState::new();
    assert_eq!(s.pop_char(), None);
  }

  #[test]
  fn clear_resets_buffer_and_mirror() {
    let mut s = TextInputState::with_lower_mirror();
    s.push_char('A');
    s.push_char('b');
    assert_eq!(s.lower(), "ab");
    s.clear();
    assert!(s.is_empty());
    assert_eq!(s.lower(), "");
  }

  #[test]
  fn set_overwrites_buffer_and_mirror() {
    let mut s = TextInputState::with_lower_mirror();
    s.push_char('a');
    s.set("HELLO".to_string());
    assert_eq!(s.buffer(), "HELLO");
    assert_eq!(s.lower(), "hello");
  }

  #[test]
  fn take_returns_buffer_and_clears() {
    let mut s = TextInputState::new();
    s.push_char('h');
    s.push_char('i');
    let taken = s.take();
    assert_eq!(taken, "hi");
    assert!(s.is_empty());
  }

  #[test]
  fn focus_and_blur() {
    let mut s = TextInputState::new();
    assert!(!s.is_focused());
    s.focus();
    assert!(s.is_focused());
    s.blur();
    assert!(!s.is_focused());
  }

  #[test]
  fn lower_mirror_kept_in_sync() {
    let mut s = TextInputState::with_lower_mirror();
    s.push_char('H');
    assert_eq!(s.lower(), "h");
    s.push_char('I');
    assert_eq!(s.lower(), "hi");
    s.pop_char();
    assert_eq!(s.lower(), "h");
  }

  #[test]
  fn no_lower_mirror_means_empty_string() {
    let mut s = TextInputState::new();
    s.push_char('H');
    s.push_char('I');
    assert_eq!(s.lower(), "");
  }

  #[test]
  fn lower_mirror_handles_multibyte() {
    let mut s = TextInputState::with_lower_mirror();
    s.push_char('Ä');
    assert_eq!(s.lower(), "ä");
  }
}
