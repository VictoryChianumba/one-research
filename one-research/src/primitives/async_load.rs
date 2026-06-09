//! Async-load state machine: `Idle → Loading → Ready` (or `Disconnected`).
//!
//! Wraps an `mpsc::Receiver<T>` together with the consumer-facing state
//! enum so consumers don't need to keep two fields in sync. Each frame
//! the consumer calls [`poll`] which advances the state if the worker
//! thread has produced a value or dropped its sender.
//!
//! "Error" semantics are deliberately consumer-defined: if `T` is a
//! Result-like enum (e.g. `DiscoverResult` with a `Failed` variant), the
//! Ready state carries it. The primitive only distinguishes
//! "Disconnected" from "Ready" because the former never produced a value.

// Tested forward-design: methods like `is_idle`, `is_ready`, `result` are
// exercised by the in-file unit tests but not yet wired from main render
// paths. See audit C11 (downgrade to "nuance", 2026-05-18).
#![allow(dead_code)]

use std::sync::mpsc;

/// Async load state machine. Default is `Idle`.
pub enum AsyncLoadState<T> {
  Idle,
  Loading(mpsc::Receiver<T>),
  Ready(T),
  /// The worker dropped its sender without producing a value. Consumer
  /// must explicitly `reset()` to return to Idle.
  Disconnected,
}

impl<T> Default for AsyncLoadState<T> {
  fn default() -> Self {
    Self::Idle
  }
}

impl<T> AsyncLoadState<T> {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn is_idle(&self) -> bool {
    matches!(self, Self::Idle)
  }

  pub fn is_loading(&self) -> bool {
    matches!(self, Self::Loading(_))
  }

  pub fn is_ready(&self) -> bool {
    matches!(self, Self::Ready(_))
  }

  pub fn is_disconnected(&self) -> bool {
    matches!(self, Self::Disconnected)
  }

  /// Begin a load with the given receiver. Replaces any prior state —
  /// callers should reset() first if they want explicit transition.
  pub fn start(&mut self, rx: mpsc::Receiver<T>) {
    *self = Self::Loading(rx);
  }

  /// Try to advance Loading → Ready or Loading → Disconnected. No-op
  /// if not Loading or if the receiver hasn't received a value yet.
  /// Call once per frame from the event loop.
  pub fn poll(&mut self) {
    let owned = std::mem::replace(self, Self::Idle);
    *self = match owned {
      Self::Loading(rx) => match rx.try_recv() {
        Ok(value) => Self::Ready(value),
        Err(mpsc::TryRecvError::Empty) => Self::Loading(rx),
        Err(mpsc::TryRecvError::Disconnected) => Self::Disconnected,
      },
      other => other,
    };
  }

  /// If Ready, return the loaded value and reset to Idle. Otherwise
  /// return None and leave state unchanged.
  pub fn take(&mut self) -> Option<T> {
    let owned = std::mem::replace(self, Self::Idle);
    match owned {
      Self::Ready(value) => Some(value),
      other => {
        *self = other;
        None
      }
    }
  }

  /// Read-only access to the loaded value. Returns None unless Ready.
  pub fn result(&self) -> Option<&T> {
    match self {
      Self::Ready(value) => Some(value),
      _ => None,
    }
  }

  /// Reset to Idle. Drops the receiver if currently Loading. Use to
  /// cancel in-flight loads or clear a Ready/Disconnected terminal
  /// state.
  pub fn reset(&mut self) {
    *self = Self::Idle;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn defaults_to_idle() {
    let s: AsyncLoadState<i32> = AsyncLoadState::new();
    assert!(s.is_idle());
    assert!(!s.is_loading());
    assert!(!s.is_ready());
  }

  #[test]
  fn start_transitions_to_loading() {
    let (_tx, rx) = mpsc::channel::<i32>();
    let mut s = AsyncLoadState::new();
    s.start(rx);
    assert!(s.is_loading());
  }

  #[test]
  fn poll_with_value_transitions_to_ready() {
    let (tx, rx) = mpsc::channel();
    let mut s = AsyncLoadState::new();
    s.start(rx);
    tx.send(42).unwrap();
    s.poll();
    assert!(s.is_ready());
    assert_eq!(s.result(), Some(&42));
  }

  #[test]
  fn poll_without_value_stays_loading() {
    let (_tx, rx) = mpsc::channel::<i32>();
    let mut s = AsyncLoadState::new();
    s.start(rx);
    s.poll();
    assert!(s.is_loading());
  }

  #[test]
  fn poll_after_sender_dropped_transitions_to_disconnected() {
    let (tx, rx) = mpsc::channel::<i32>();
    let mut s = AsyncLoadState::new();
    s.start(rx);
    drop(tx);
    s.poll();
    assert!(s.is_disconnected());
  }

  #[test]
  fn take_returns_value_and_resets() {
    let (tx, rx) = mpsc::channel();
    let mut s = AsyncLoadState::new();
    s.start(rx);
    tx.send("hi".to_string()).unwrap();
    s.poll();
    assert_eq!(s.take(), Some("hi".to_string()));
    assert!(s.is_idle());
  }

  #[test]
  fn take_when_not_ready_returns_none_and_preserves_state() {
    let mut s: AsyncLoadState<i32> = AsyncLoadState::new();
    assert_eq!(s.take(), None);
    assert!(s.is_idle());
  }

  #[test]
  fn poll_on_idle_is_noop() {
    let mut s: AsyncLoadState<i32> = AsyncLoadState::new();
    s.poll();
    assert!(s.is_idle());
  }

  #[test]
  fn reset_drops_receiver_and_returns_idle() {
    let (_tx, rx) = mpsc::channel::<i32>();
    let mut s = AsyncLoadState::new();
    s.start(rx);
    s.reset();
    assert!(s.is_idle());
  }

  #[test]
  fn reset_after_ready_returns_idle() {
    let (tx, rx) = mpsc::channel();
    let mut s = AsyncLoadState::new();
    s.start(rx);
    tx.send(1).unwrap();
    s.poll();
    s.reset();
    assert!(s.is_idle());
  }
}
