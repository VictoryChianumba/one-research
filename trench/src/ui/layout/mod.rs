use ratatui::{
  Frame,
  layout::{Constraint, Direction, Layout},
};

use super::repo_viewer::draw_repo_viewer;
use crate::app::{App, AppView};

mod details;
mod feed;
mod filter;
mod footer;
mod frame_layout;
mod main_row;
mod modals;
mod notes;
mod popups;
mod reader;
mod title;
mod widgets;

pub use frame_layout::{FrameLayout, compute_frame_layout};

use footer::draw_footer;
use main_row::draw_main_row;
use modals::{draw_settings, draw_sources_popup, draw_theme_picker};
pub use popups::HELP_SECTION_COUNT;
use popups::{
  draw_abstract_popup, draw_help_overlay, draw_quit_popup, draw_tag_picker,
};
use reader::{chat_context_line, draw_reader_bottom_pane, draw_reader_popup};
use title::{draw_search_row, draw_title_bar, title_bar_height};
use widgets::h_margin;

pub const RIGHT_COL_WIDTH: u16 = 50;

pub fn draw(frame: &mut Frame, app: &mut App) {
  let t_total = std::time::Instant::now();
  // Pre-draw update phase: any state mutation that conceptually belongs
  // to "react to selection / state change" runs here, so the render
  // path proper stays read-only. Phase 4 — render purification.
  app.pre_draw_update();
  // C6 / ADR-008: post-layout hook.  Layout-derived mutations that
  // `pre_draw_update` can't size (because it runs before layout) live
  // in `apply_frame_layout`, fed by `compute_frame_layout(&App, area)`.
  // The pair eliminates the last `// Intentional render-time mutation`
  // marker in `ui/layout/reader.rs`.
  let frame_layout = compute_frame_layout(app, frame.area());
  app.apply_frame_layout(&frame_layout);
  match app.view {
    AppView::Feed => draw_feed(frame, app),
    AppView::Settings => {
      draw_feed(frame, app);
      draw_settings(frame, app);
    }
    AppView::Sources => {
      draw_feed(frame, app);
      draw_settings(frame, app);
      draw_sources_popup(frame, app);
    }
    AppView::RepoViewer => draw_repo_viewer(frame, app),
  }
  // Abstract popup floats on top of the feed view.
  if app.view_flags.abstract_popup_active {
    draw_abstract_popup(frame, app);
  }
  // Help overlay floats on top of whatever view is rendered.
  if app.help.active {
    draw_help_overlay(frame, app);
  }
  if app.theme_picker.active {
    draw_theme_picker(frame, app);
  }
  if app.tag_picker.active {
    draw_tag_picker(frame, app);
  }
  // Quit popup sits above everything — must be last.
  if app.quit_popup.active {
    draw_quit_popup(frame, app);
  }
  let total_ms = t_total.elapsed().as_millis();
  if total_ms > 8 {
    log::debug!("ui::draw total: {}ms", total_ms);
  }
}

fn draw_feed(frame: &mut Frame, app: &mut App) {
  let area = frame.area();
  let theme = app.theme();
  let margin = area.width / 20;
  let title_h = title_bar_height(area.width);
  let chat_context = chat_context_line(app);

  // Fixed zones: title, search=2, footer=2.  Remaining rows split between
  // main panes and (optionally) chat panel.
  let fixed = title_h + 2 + 2;
  let available = area.height.saturating_sub(fixed);

  // Only allocate a dedicated panel row when the chat conversation is open.
  // Session-list and new-session overlays float over the main layout instead.
  let chat_needs_panel =
    app.chat.active && app.chat.ui.as_ref().map_or(true, |c| c.needs_panel());

  let (main_h, chat_h) = if chat_needs_panel {
    let ch = (available / 2).max(15).min(available.saturating_sub(10));
    let mh = available.saturating_sub(ch);
    (mh, ch)
  } else {
    (available, 0)
  };

  // Build row constraints: title | search | [chat?] | main | [chat?] | footer
  // We place chat above or below main depending on `chat_at_top`.
  if chat_needs_panel && app.chat.at_top {
    let rows = Layout::default()
      .direction(Direction::Vertical)
      .constraints([
        Constraint::Length(title_h), // title bar
        Constraint::Length(2),       // search + filter
        Constraint::Length(chat_h),  // chat panel (top)
        Constraint::Length(main_h),  // main panes
        Constraint::Length(2),       // footer
      ])
      .split(area);

    let t = std::time::Instant::now();
    draw_title_bar(frame, app, rows[0]);
    log::debug!("draw_title_bar: {}ms", t.elapsed().as_millis());

    let t = std::time::Instant::now();
    draw_search_row(frame, app, h_margin(rows[1], margin));
    log::debug!("draw_search_row: {}ms", t.elapsed().as_millis());

    let chat_rect = Some(rows[2]);
    if let Some(chat_ui) = app.chat.ui.as_mut() {
      let t = std::time::Instant::now();
      chat_ui.draw_with_context(
        frame,
        rows[2],
        &theme,
        chat_context.as_deref(),
      );
      log::debug!("chat_ui.draw (top): {}ms", t.elapsed().as_millis());
    }

    let t = std::time::Instant::now();
    let mr = draw_main_row(frame, app, h_margin(rows[3], margin));
    log::debug!("draw_main_row: {}ms", t.elapsed().as_millis());

    app.focus.update_pane_rects(
      mr.feed,
      mr.reader,
      mr.notes,
      mr.details,
      chat_rect,
      mr.secondary_reader,
      mr.secondary_notes,
    );

    let t = std::time::Instant::now();
    draw_footer(frame, app, rows[4]);
    log::debug!("draw_footer: {}ms", t.elapsed().as_millis());
  } else {
    let rows = Layout::default()
      .direction(Direction::Vertical)
      .constraints([
        Constraint::Length(title_h), // title bar
        Constraint::Length(2),       // search + filter
        Constraint::Length(main_h),  // main panes
        Constraint::Length(chat_h), // chat panel (bottom, 0 when inactive or overlay)
        Constraint::Length(2),      // footer
      ])
      .split(area);

    let t = std::time::Instant::now();
    draw_title_bar(frame, app, rows[0]);
    log::debug!("draw_title_bar: {}ms", t.elapsed().as_millis());

    let t = std::time::Instant::now();
    draw_search_row(frame, app, h_margin(rows[1], margin));
    log::debug!("draw_search_row: {}ms", t.elapsed().as_millis());

    let main_rect = h_margin(rows[2], margin);
    let t = std::time::Instant::now();
    let mr = draw_main_row(frame, app, main_rect);
    log::debug!("draw_main_row: {}ms", t.elapsed().as_millis());

    let chat_rect = if chat_needs_panel { Some(rows[3]) } else { None };
    if chat_needs_panel {
      if let Some(chat_ui) = app.chat.ui.as_mut() {
        let t = std::time::Instant::now();
        chat_ui.draw_with_context(
          frame,
          rows[3],
          &theme,
          chat_context.as_deref(),
        );
        log::debug!("chat_ui.draw (bottom): {}ms", t.elapsed().as_millis());
      }
    }
    app.focus.update_pane_rects(
      mr.feed,
      mr.reader,
      mr.notes,
      mr.details,
      chat_rect,
      mr.secondary_reader,
      mr.secondary_notes,
    );

    let t = std::time::Instant::now();
    draw_footer(frame, app, rows[4]);
    log::debug!("draw_footer: {}ms", t.elapsed().as_millis());
  }

  // Session-list / new-session overlay: rendered last so it floats on top.
  if app.chat.active && !chat_needs_panel {
    if let Some(chat_ui) = app.chat.ui.as_mut() {
      chat_ui.draw_overlay(frame, area, &theme);
    }
  }

  // A1 — floating reader popup (Ldr+Enter).
  if app.reader_popup.active {
    draw_reader_popup(frame, app, area);
  }

  // A2 State 3 — bottom pane visible only when summoned (Ldr+f).
  if app.reader.dual_active && app.reader_bottom.open {
    draw_reader_bottom_pane(frame, app, area);
  }
}

// ── Title bar ──────────────────────────────────────────────────────────────
