//! Top-level UI action vocabulary. Translated from key events by
//! [`crate::keys`] before being routed to a surface. Variants accrete as
//! surfaces migrate.
//!
//! Slice 1 shipped only the variants needed by Sources
//! (DismissTopModal, OpenSettings). Slice 2 adds `OpenInReader` —
//! the cross-pane verb that consolidates the "open this paper in a
//! reader" call paths into one place (ADR-002 §S4).
//!
//! `Action` is intentionally not `Clone` or `Debug` — `OpenInReader`
//! carries a `tread::Reader` which is neither. Actions are
//! consumed once at dispatch, not stored or compared, so the missing
//! derives cost nothing.

use crate::app::NotesContext;

/// Which reader surface should receive an `OpenInReader` action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderTarget {
  /// Embedded primary reader pane.
  Primary,
  /// Embedded secondary reader pane (visible when split or dual is active).
  Secondary,
  /// Floating popup reader (`Ldr+Enter`).
  /// Load-bearing vocabulary per CONTEXT.md / ADR-002 — kept while
  /// async-load lifecycle migrates onto `OpenInReader` (Slice 2 follow-up).
  #[allow(dead_code)]
  Popup,
}

pub enum Action {
  /// Top of the modal stack should be dismissed (Esc, q, etc.).
  /// Load-bearing vocabulary per CONTEXT.md / ADR-002 — settings overlay
  /// is partly migrated to this verb; full migration is a Slice 2 follow-up.
  #[allow(dead_code)]
  DismissTopModal,
  /// Generic "open settings view" — used by Sources to leave back to
  /// Settings on Esc/q.
  OpenSettings,
  /// Cross-pane: open a paper in one of the reader surfaces. The
  /// payload (already-constructed `tread::Reader` plus its metadata) is
  /// moved into the orchestrator, which calls `App::reader_open` /
  /// `reader_secondary_open`. Opening replaces the target pane's single
  /// doc — document tabs were removed (ADR-017).
  ///
  /// Popup variant currently routes to the same path as Primary; the
  /// async-load lifecycle is still handled outside this Action (PR 5).
  OpenInReader {
    target: ReaderTarget,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  },
}
