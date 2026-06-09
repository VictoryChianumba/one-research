//! `LeaderState` — Ctrl+T leader-key arming + auto-expire.
//!
//! Tentative's keybindings use `Ctrl+T` as a global leader; once armed,
//! the next keystroke is interpreted as a leader-prefixed binding.  If
//! the user doesn't follow up within `timeout_ms`, the leader silently
//! disarms.
//!
//! The protocol has four operations:
//!   - [`LeaderState::activate`] — Ctrl+T pressed, arm the leader.
//!   - [`LeaderState::deactivate`] — explicit disarm (e.g. after the
//!     leader-prefixed key was consumed, or `?` opened the help screen).
//!   - [`LeaderState::expire_if_timed_out`] — called before every leader
//!     check; disarms if the timeout has elapsed.
//!   - [`LeaderState::is_active`] — read-only check, no side effects.
//!
//! Separating *expire* from *is_active* keeps the read pure: footer
//! renders and debug-log reads don't accidentally mutate the gate.
//!
//! Introduced by [ADR-009](../../../../docs/adr/ADR-009-app-field-grouping.md)
//! as the second cluster after `DebounceState`. Replaces three flat
//! `App` fields (`leader_active`, `leader_activated_at`, `leader_timeout_ms`).
//!
//! Default `timeout_ms = 1000` preserves the pre-grouping constant.

use std::time::Instant;

/// See module-level docs.
#[derive(Debug)]
pub struct LeaderState {
  active: bool,
  activated_at: Option<Instant>,
  timeout_ms: u64,
}

impl Default for LeaderState {
  fn default() -> Self {
    Self { active: false, activated_at: None, timeout_ms: 1000 }
  }
}

impl LeaderState {
  /// Arm the leader.  Records the current `Instant` so a later
  /// [`expire_if_timed_out`] knows whether the window has elapsed.
  pub fn activate(&mut self) {
    self.active = true;
    self.activated_at = Some(Instant::now());
  }

  /// Disarm the leader.  Called when a leader-prefixed key was consumed,
  /// when the user pressed `?` to open help, etc.
  pub fn deactivate(&mut self) {
    self.active = false;
  }

  /// Disarm if active and the timeout has elapsed.  No-op otherwise.
  /// Call before any `is_active` check that should respect the timeout.
  pub fn expire_if_timed_out(&mut self) {
    if self.active
      && self
        .activated_at
        .is_some_and(|t| t.elapsed().as_millis() > self.timeout_ms as u128)
    {
      self.active = false;
    }
  }

  /// Read-only: whether the leader is currently armed.  Does *not*
  /// auto-expire; pair with [`expire_if_timed_out`] when the call site
  /// needs the timeout-respecting view.
  pub fn is_active(&self) -> bool {
    self.active
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::thread::sleep;
  use std::time::Duration;

  #[test]
  fn default_matches_pre_grouping_constants() {
    // Load-bearing: 1000ms was the constant the flat App field shipped
    // with pre-grouping.  Byte-equivalent behaviour is the correctness
    // criterion for ADR-009 migrations.
    let l = LeaderState::default();
    assert!(!l.is_active());
    assert!(l.activated_at.is_none());
    assert_eq!(l.timeout_ms, 1000);
  }

  #[test]
  fn activate_sets_active_and_timestamp() {
    let mut l = LeaderState::default();
    l.activate();
    assert!(l.is_active());
    assert!(l.activated_at.is_some());
  }

  #[test]
  fn deactivate_clears_active() {
    let mut l = LeaderState::default();
    l.activate();
    l.deactivate();
    assert!(!l.is_active());
  }

  #[test]
  fn expire_is_noop_within_timeout() {
    // Witness: activate, immediately expire-check, leader is still on.
    // The pre-grouping bug class would be "expire fires too eagerly."
    let mut l = LeaderState::default();
    l.activate();
    l.expire_if_timed_out();
    assert!(l.is_active(), "leader must survive within the timeout window");
  }

  #[test]
  fn expire_disarms_after_timeout_elapses() {
    // Real-time test with a short cooldown override so it runs fast.
    let mut l = LeaderState { timeout_ms: 5, ..LeaderState::default() };
    l.activate();
    sleep(Duration::from_millis(10));
    l.expire_if_timed_out();
    assert!(!l.is_active(), "leader must disarm after timeout elapsed");
  }

  #[test]
  fn expire_is_noop_when_inactive() {
    // Witness: expiring an already-disarmed leader does nothing.  The
    // gate should not, e.g., reset `activated_at` to something stale or
    // set `active` to anything other than false.
    let mut l = LeaderState::default();
    l.expire_if_timed_out();
    assert!(!l.is_active());
    assert!(l.activated_at.is_none());
  }

  #[test]
  fn is_active_does_not_mutate() {
    // Read-purity: calling is_active many times after the timeout has
    // elapsed but before expire_if_timed_out runs still returns true.
    // This is intentional — separating read from side effect lets the
    // footer renderer ask "is leader on?" without mutating state.
    let mut l = LeaderState { timeout_ms: 5, ..LeaderState::default() };
    l.activate();
    sleep(Duration::from_millis(10));
    assert!(l.is_active(), "is_active must not auto-expire");
    assert!(l.is_active(), "is_active must remain pure across calls");
  }
}
