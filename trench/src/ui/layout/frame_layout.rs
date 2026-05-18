//! `FrameLayout` — layout-derived inputs threaded into the per-frame
//! `App::apply_frame_layout` hook.
//!
//! Introduced by [ADR-008](../../../docs/adr/ADR-008-frame-layout.md).
//! PR 1 lands the struct + hook scaffolding without callers; PR 2 wires
//! it at the draw entry point and migrates the last
//! `// Intentional render-time mutation` site (reader-bottom feed-mode
//! auto-scroll, `reader.rs:424-441` pre-C6).

use ratatui::layout::Rect;

use crate::app::App;

/// Layout-derived inputs that `App::apply_frame_layout` needs to size
/// scroll bounds, viewport caps, and other layout-shaped state.
///
/// Each `Option<Rect>` is `Some` only when the corresponding pane is
/// open *and* its area is non-degenerate. `apply_frame_layout` treats
/// `None` as "leave the bound at its `pre_draw_update` default."
///
/// One struct, growing field-by-field. The "every Rect ever computed"
/// version was rejected in ADR-008 §S1 — the audit's framing favours
/// "capture what's consumed, not what's available."
#[derive(Default, Debug, Clone, Copy)]
pub struct FrameLayout {
  /// The reader bottom-drawer *list area* when the drawer is in feed
  /// mode (`reader_bottom_open && !reader_bottom_details`).
  /// `apply_frame_layout` uses `.height` to size
  /// `reader_bottom_scroll.max` for feed-mode auto-scroll.
  pub reader_bottom_feed_list: Option<Rect>,
}

/// Compute `FrameLayout` from `app` state + the full frame area.
///
/// Pure — takes `&App` and never mutates. Called once per frame at the
/// draw entry point; its result is handed to `App::apply_frame_layout`
/// for the post-layout mutation pass, then discarded.
pub fn compute_frame_layout(app: &App, area: Rect) -> FrameLayout {
  let mut layout = FrameLayout::default();

  // Reader bottom drawer is shown only when the dual-reader view is
  // active AND the drawer is open AND it's in feed mode (not details).
  // The Rect math mirrors `draw_reader_bottom_pane` /
  // `draw_bottom_pane_feed`'s shared helpers in `ui/layout/reader.rs`.
  if app.reader.dual_active
    && app.reader_bottom_open
    && !app.reader_bottom_details
  {
    layout.reader_bottom_feed_list =
      super::reader::reader_bottom_feed_list_rect(area);
  }

  layout
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_is_all_none() {
    // Invariant: `FrameLayout::default()` reports "no pane requires a
    // post-layout mutation." `apply_frame_layout(&FrameLayout::default())`
    // is the no-op identity.
    let l = FrameLayout::default();
    assert!(l.reader_bottom_feed_list.is_none());
  }

  #[test]
  fn rect_round_trip_preserves_dimensions() {
    // Light smoke: assigning a Rect to a field round-trips. Sanity
    // check more than logic — guards against accidental Copy/Clone
    // breakage if the struct grows non-trivial fields.
    let r = Rect { x: 1, y: 2, width: 80, height: 24 };
    let l = FrameLayout {
      reader_bottom_feed_list: Some(r),
      ..FrameLayout::default()
    };
    assert_eq!(l.reader_bottom_feed_list.map(|r| r.height), Some(24));
  }

  #[test]
  fn struct_is_copy_for_cheap_threading() {
    // `apply_frame_layout(&FrameLayout)` is fine, but the audit
    // anticipates per-pane code reading values out of a FrameLayout
    // it received by reference and then re-passing.  Copy makes this
    // free.  If FrameLayout grows a non-Copy field later, this test
    // surfaces the regression.
    let l = FrameLayout::default();
    let l2 = l;
    let _l3 = l;
    assert!(l2.reader_bottom_feed_list.is_none());
  }
}
