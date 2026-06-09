//! `DebounceState` — input-rate gates for keyboard and mouse scrolls.
//!
//! Two independent cooldowns: keyboard scrolls (key-repeat) and mouse
//! scrolls (trackpad inertia). Each gate stores its last-accepted time
//! and a per-gate cooldown.  Callers ask `try_kbd_scroll()` /
//! `try_mouse_scroll()`; a `true` means "go ahead, your scroll counts"
//! and the gate's timer is updated atomically.
//!
//! Introduced by [ADR-009](../../../../docs/adr/ADR-009-app-field-grouping.md)
//! as the pilot cluster for the App-field grouping pass. Before this
//! module the same logic lived as four flat fields on `App` plus two
//! open-coded helpers in `main.rs`.
//!
//! The two cooldown defaults — 50ms keyboard, 80ms mouse — preserve the
//! pre-grouping behaviour byte-for-byte. The mouse cooldown is higher
//! because trackpad inertia events fire thicker than key-repeats.

use std::time::Instant;

/// See module-level docs.
#[derive(Debug)]
pub struct DebounceState {
  last_kbd: Option<Instant>,
  kbd_cooldown_ms: u64,
  last_mouse: Option<Instant>,
  mouse_cooldown_ms: u64,
}

impl Default for DebounceState {
  fn default() -> Self {
    Self {
      last_kbd: None,
      kbd_cooldown_ms: 50,
      last_mouse: None,
      mouse_cooldown_ms: 80,
    }
  }
}

impl DebounceState {
  /// Returns `true` if a keyboard scroll motion should be accepted now,
  /// and updates the timer. Returns `false` if the cooldown hasn't
  /// elapsed since the last accepted scroll.
  pub fn try_kbd_scroll(&mut self) -> bool {
    let now = Instant::now();
    if let Some(last) = self.last_kbd
      && last.elapsed().as_millis() < self.kbd_cooldown_ms as u128
    {
      return false;
    }
    self.last_kbd = Some(now);
    true
  }

  /// Mirror of [`try_kbd_scroll`] for mouse-wheel events. Higher
  /// cooldown by default to tame trackpad inertia.
  pub fn try_mouse_scroll(&mut self) -> bool {
    let now = Instant::now();
    if let Some(last) = self.last_mouse
      && last.elapsed().as_millis() < self.mouse_cooldown_ms as u128
    {
      return false;
    }
    self.last_mouse = Some(now);
    true
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::thread::sleep;
  use std::time::Duration;

  #[test]
  fn default_matches_pre_grouping_constants() {
    // Load-bearing: 50ms / 80ms were the constants the four flat
    // App fields shipped with before ADR-009.  Byte-equivalent
    // behaviour is the migration's correctness criterion.
    let d = DebounceState::default();
    assert_eq!(d.kbd_cooldown_ms, 50);
    assert_eq!(d.mouse_cooldown_ms, 80);
    assert!(d.last_kbd.is_none());
    assert!(d.last_mouse.is_none());
  }

  #[test]
  fn first_kbd_scroll_is_always_accepted() {
    let mut d = DebounceState::default();
    assert!(d.try_kbd_scroll(), "first scroll must pass — last_kbd is None");
    assert!(d.last_kbd.is_some(), "timer must be set after acceptance");
  }

  #[test]
  fn immediate_second_kbd_scroll_is_rejected() {
    // Witness for the cooldown gate: two scrolls back-to-back means
    // the second one falls inside the 50ms window and must be rejected.
    let mut d = DebounceState::default();
    assert!(d.try_kbd_scroll());
    assert!(!d.try_kbd_scroll(), "second within cooldown must be rejected");
  }

  #[test]
  fn kbd_scroll_accepted_after_cooldown_elapses() {
    // Real-time test: sleep past the cooldown then retry. Uses a small
    // local cooldown override to keep the test under 20ms.
    let mut d =
      DebounceState { kbd_cooldown_ms: 5, ..DebounceState::default() };
    assert!(d.try_kbd_scroll());
    sleep(Duration::from_millis(10));
    assert!(d.try_kbd_scroll(), "scroll after cooldown must be accepted");
  }

  #[test]
  fn kbd_and_mouse_gates_are_independent() {
    // Witness that the two cooldowns don't share state — a kbd scroll
    // shouldn't suppress a mouse scroll, even within the kbd cooldown.
    let mut d = DebounceState::default();
    assert!(d.try_kbd_scroll());
    assert!(d.try_mouse_scroll(), "mouse gate must be independent of kbd gate");
  }
}
