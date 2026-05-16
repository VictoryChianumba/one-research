use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Layout, Rect},
  style::Style,
  widgets::Paragraph,
};

use super::RIGHT_COL_WIDTH;
use super::details::draw_details_panel;
use super::feed::draw_feed_pane;
use super::filter::draw_filter_panel;
use super::notes::{draw_note_dock, draw_notes_surface};
use super::reader::{
  draw_narrow_feed_details_popup, draw_reader_tab_bar,
  draw_reader_workspace_header, reader_workspace_split,
};
use super::widgets::{draw_horiz_split_box, draw_vert_split_box};
use crate::app::{App, FocusedReader};

/// Orchestrator helper: builds a `FeedContext` snapshot from `App` and
/// dispatches to the (renderer-pure) `draw_feed_pane`. Lives here at
/// the orchestrator level because it crosses fields of `App` — `feed.rs`
/// itself is forbidden to take `&mut App` post-PR-4c (ADR-001 D4).
///
/// The local owned values (`visible`, `history`, `counts`) anchor the
/// FeedContext's lifetimes — once they're constructed the `&app` borrow
/// from the method calls is released and `&app.workspace` / `&mut
/// app.feed` can split-borrow cleanly.
fn dispatch_feed_pane(frame: &mut Frame, app: &mut App, area: Rect) {
  let theme = app.theme();
  let counts: crate::app::ItemCounts = (*app.item_counts()).clone();
  // Both helpers take field-scoped borrows so the resulting owned
  // values (Vec<usize>, Vec<&HistoryEntry>) carry no live borrow on
  // `app` beyond `app.workspace` — leaving `&mut app.feed` free below.
  let visible_indices =
    crate::feed::visible_indices_for(&app.workspace, &app.feed, &app.config);
  let filtered_history =
    crate::feed::filtered_history_for(&app.workspace, &app.feed);
  let ctx = crate::feed::FeedContext {
    workspace: &app.workspace,
    theme,
    visible_indices,
    filtered_history,
    item_counts: counts,
  };
  draw_feed_pane(frame, &mut app.feed, &ctx, area);
}

/// Screen rects computed by draw_main_row, passed back to app.update_pane_rects.
pub(super) struct MainRowRects {
  pub feed: Option<Rect>,
  pub reader: Option<Rect>,
  pub secondary_reader: Option<Rect>,
  pub notes: Option<Rect>,
  pub secondary_notes: Option<Rect>,
  pub details: Option<Rect>,
}

fn split_reader_note_dock(area: Rect) -> (Rect, Rect) {
  if area.height < 16 {
    return (
      Rect { x: area.x, y: area.y, width: area.width, height: area.height },
      Rect { x: area.x, y: area.y + area.height, width: area.width, height: 0 },
    );
  }
  let dock_h = (area.height * 34 / 100).clamp(7, 16);
  let rows = Layout::vertical([Constraint::Min(8), Constraint::Length(dock_h)])
    .split(area);
  (rows[0], rows[1])
}

pub fn draw_main_row(
  frame: &mut Frame,
  app: &mut App,
  area: Rect,
) -> MainRowRects {
  let theme = app.theme();
  let t = theme;
  // Tread's `Theme` is a sibling type from a separate `ui_theme` crate
  // — we can't pass `&t` directly.  Convert once per draw cycle.
  let tread_theme = app.theme_for_tread();
  // ── A2 State 3: dual-reader (left 50% | right 50%) ──────────────────────
  if app.reader.dual_active && app.reader.active {
    let (workspace_area, body_area) = reader_workspace_split(area);
    draw_reader_workspace_header(frame, app, workspace_area, "Dual Reader");
    let inner_w = body_area.width.saturating_sub(2);
    let right_w = (inner_w / 2).max(1);
    let (left_rect, right_rect) = draw_horiz_split_box(
      frame,
      body_area,
      right_w,
      "Primary",
      "Secondary",
      &t,
    );
    let (left_reader_rect, left_notes_rect) = if app.notes_active {
      let (reader, notes) = split_reader_note_dock(left_rect);
      (reader, Some(notes))
    } else {
      (left_rect, None)
    };
    let (right_reader_rect, right_notes_rect) = if app.secondary_notes_active {
      let (reader, notes) = split_reader_note_dock(right_rect);
      (reader, Some(notes))
    } else {
      (right_rect, None)
    };
    {
      let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
        .split(left_reader_rect);
      let focused = app.reader.focused == FocusedReader::Primary;
      draw_reader_tab_bar(
        frame,
        rows[0],
        &app.reader.primary.tabs,
        app.reader.primary.active_tab,
        focused,
        &t,
      );
      let kitty = app.kitty_supported;
      if let Some(tab) = app.reader_active_tab_mut() {
        let new_size = (rows[1].width, rows[1].height);
        if tab.last_resize != Some(new_size) {
          tab.reader.resize(new_size.0, new_size.1);
          tab.last_resize = Some(new_size);
        }
        tread::draw(frame, rows[1], &tab.reader, &tread_theme);
        let burst = tab.burst.in_burst();
        tread::after_draw_guarded(
          &tab.reader,
          &mut tab.image_state,
          rows[1],
          kitty,
          burst,
        );
      }
    }
    {
      let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
        .split(right_reader_rect);
      let focused = app.reader.focused == FocusedReader::Secondary;
      draw_reader_tab_bar(
        frame,
        rows[0],
        &app.reader.secondary.tabs,
        app.reader.secondary.active_tab,
        focused,
        &t,
      );
      let kitty = app.kitty_supported;
      if let Some(tab) = app.reader_secondary_active_tab_mut() {
        let new_size = (rows[1].width, rows[1].height);
        if tab.last_resize != Some(new_size) {
          tab.reader.resize(new_size.0, new_size.1);
          tab.last_resize = Some(new_size);
        }
        tread::draw(frame, rows[1], &tab.reader, &tread_theme);
        let burst = tab.burst.in_burst();
        tread::after_draw_guarded(
          &tab.reader,
          &mut tab.image_state,
          rows[1],
          kitty,
          burst,
        );
      } else {
        let hint = Paragraph::new(
          "No paper loaded\n\nLdr+f → open feed · Enter to load",
        )
        .alignment(Alignment::Center)
        .style(Style::default().fg(t.text_dim));
        frame.render_widget(hint, rows[1]);
      }
    }
    if let Some(notes_rect) = left_notes_rect {
      draw_note_dock(frame, app, notes_rect, FocusedReader::Primary, &theme);
    }
    if let Some(notes_rect) = right_notes_rect {
      draw_note_dock(frame, app, notes_rect, FocusedReader::Secondary, &theme);
    }
    return MainRowRects {
      feed: None,
      reader: Some(left_reader_rect),
      secondary_reader: Some(right_reader_rect),
      notes: left_notes_rect,
      secondary_notes: right_notes_rect,
      details: None,
    };
  }

  // ── A2 State 2: feed (40%) | reader (60%) ────────────────────────────────
  if app.reader.split_active && app.reader.active {
    let (workspace_area, body_area) = reader_workspace_split(area);
    draw_reader_workspace_header(frame, app, workspace_area, "Reader + Feed");
    let inner_w = body_area.width.saturating_sub(2);
    let reader_w = (inner_w * 60 / 100).max(1);
    let (feed_rect, reader_rect) = draw_horiz_split_box(
      frame,
      body_area,
      reader_w,
      "Reader Feed",
      "Reader",
      &t,
    );
    dispatch_feed_pane(frame, app, feed_rect);
    {
      let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
        .split(reader_rect);
      draw_reader_tab_bar(
        frame,
        rows[0],
        &app.reader.primary.tabs,
        app.reader.primary.active_tab,
        true,
        &t,
      );
      let kitty = app.kitty_supported;
      if let Some(tab) = app.reader_active_tab_mut() {
        let new_size = (rows[1].width, rows[1].height);
        if tab.last_resize != Some(new_size) {
          tab.reader.resize(new_size.0, new_size.1);
          tab.last_resize = Some(new_size);
        }
        tread::draw(frame, rows[1], &tab.reader, &tread_theme);
        let burst = tab.burst.in_burst();
        tread::after_draw_guarded(
          &tab.reader,
          &mut tab.image_state,
          rows[1],
          kitty,
          burst,
        );
      }
    }
    if app.narrow_feed_details_open {
      draw_narrow_feed_details_popup(frame, app, reader_rect);
    }
    return MainRowRects {
      feed: Some(feed_rect),
      reader: Some(reader_rect),
      secondary_reader: None,
      notes: None,
      secondary_notes: None,
      details: None,
    };
  }

  // ── Reader: always full-width or 60/40 split, regardless of terminal width ─
  if app.reader.active && !app.notes_active {
    let (workspace_area, body_area) = reader_workspace_split(area);
    draw_reader_workspace_header(frame, app, workspace_area, "Reader");
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
      .split(body_area);
    draw_reader_tab_bar(
      frame,
      rows[0],
      &app.reader.primary.tabs,
      app.reader.primary.active_tab,
      true,
      &t,
    );
    let kitty = app.kitty_supported;
    if let Some(tab) = app.reader_active_tab_mut() {
      let elapsed = std::time::Instant::now();
      let new_size = (rows[1].width, rows[1].height);
      if tab.last_resize != Some(new_size) {
        tab.reader.resize(new_size.0, new_size.1);
        tab.last_resize = Some(new_size);
      }
      tread::draw(frame, rows[1], &tab.reader, &tread_theme);
      let burst = tab.burst.in_burst();
      tread::after_draw_guarded(
        &tab.reader,
        &mut tab.image_state,
        rows[1],
        kitty,
        burst,
      );
      log::debug!(
        "draw_editor (full-width): {}ms",
        elapsed.elapsed().as_millis()
      );
    }
    return MainRowRects {
      feed: None,
      reader: Some(body_area),
      secondary_reader: None,
      notes: None,
      secondary_notes: None,
      details: None,
    };
  }

  if app.reader.active {
    let (workspace_area, body_area) = reader_workspace_split(area);
    draw_reader_workspace_header(frame, app, workspace_area, "Reader + Notes");
    let (reader_rect, notes_rect) = split_reader_note_dock(body_area);
    {
      let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
        .split(reader_rect);
      draw_reader_tab_bar(
        frame,
        rows[0],
        &app.reader.primary.tabs,
        app.reader.primary.active_tab,
        true,
        &t,
      );
      let kitty = app.kitty_supported;
      if let Some(tab) = app.reader_active_tab_mut() {
        let elapsed = std::time::Instant::now();
        let new_size = (rows[1].width, rows[1].height);
        if tab.last_resize != Some(new_size) {
          tab.reader.resize(new_size.0, new_size.1);
          tab.last_resize = Some(new_size);
        }
        tread::draw(frame, rows[1], &tab.reader, &tread_theme);
        let burst = tab.burst.in_burst();
        tread::after_draw_guarded(
          &tab.reader,
          &mut tab.image_state,
          rows[1],
          kitty,
          burst,
        );
        log::debug!("draw_editor (split): {}ms", elapsed.elapsed().as_millis());
      }
    }
    draw_note_dock(frame, app, notes_rect, FocusedReader::Primary, &theme);
    return MainRowRects {
      feed: None,
      reader: Some(reader_rect),
      secondary_reader: None,
      notes: Some(notes_rect),
      secondary_notes: None,
      details: None,
    };
  }

  // ── Narrow mode (< 100 cols): vertical stack — feed top, details/notes bottom ──
  if area.width < 100 {
    let bottom_title = if app.notes_active {
      app.notes_mode.title()
    } else if app.feed.filter_focus {
      "Filters"
    } else {
      ""
    };
    // Inset by 1 row top + 1 row bottom for breathing room around the frame.
    let area = Rect {
      y: area.y.saturating_add(1),
      height: area.height.saturating_sub(2),
      ..area
    };
    let (feed_rect, bottom_rect) =
      draw_vert_split_box(frame, area, "", bottom_title, &t);

    let t = std::time::Instant::now();
    dispatch_feed_pane(frame, app, feed_rect);
    log::debug!("draw_item_table (narrow): {}ms", t.elapsed().as_millis());

    let mut details_rect: Option<Rect> = None;
    if app.notes_active {
      let t = std::time::Instant::now();
      draw_notes_surface(
        frame,
        app,
        bottom_rect,
        FocusedReader::Primary,
        false,
        &theme,
      );
      log::debug!("notes::draw (narrow): {}ms", t.elapsed().as_millis());
    } else if app.feed.filter_focus {
      let t = std::time::Instant::now();
      draw_filter_panel(frame, app, bottom_rect);
      log::debug!("draw_filter_panel (narrow): {}ms", t.elapsed().as_millis());
    } else {
      details_rect = Some(bottom_rect);
      let t = std::time::Instant::now();
      draw_details_panel(frame, app, bottom_rect);
      log::debug!("draw_details_panel (narrow): {}ms", t.elapsed().as_millis());
    }

    return MainRowRects {
      feed: Some(feed_rect),
      reader: None,
      secondary_reader: None,
      notes: if app.notes_active { Some(bottom_rect) } else { None },
      secondary_notes: None,
      details: details_rect,
    };
  }

  // ── Wide mode (>= 100 cols): single outer border, feed left, right panel ──
  // Inset by 1 row top + 1 row bottom so the rounded frame has breathing
  // room from the search bar above and the footer below — the same separation
  // halloy gets from its OS chrome.
  let area = Rect {
    y: area.y.saturating_add(1),
    height: area.height.saturating_sub(2),
    ..area
  };
  let inner_w = area.width.saturating_sub(2);
  let right_w = if app.notes_active {
    (inner_w * 40 / 100).max(1)
  } else {
    RIGHT_COL_WIDTH.min(inner_w.saturating_sub(2))
  };
  let right_title = if app.notes_active {
    app.notes_mode.title()
  } else if app.feed.filter_focus {
    "Filters"
  } else {
    ""
  };

  let (feed_rect, right_rect) =
    draw_horiz_split_box(frame, area, right_w, "", right_title, &t);

  let t = std::time::Instant::now();
  dispatch_feed_pane(frame, app, feed_rect);
  log::debug!("draw_item_table: {}ms", t.elapsed().as_millis());

  let mut details_rect: Option<Rect> = None;
  if app.notes_active {
    let t = std::time::Instant::now();
    draw_notes_surface(
      frame,
      app,
      right_rect,
      FocusedReader::Primary,
      false,
      &theme,
    );
    log::debug!("notes::draw: {}ms", t.elapsed().as_millis());
  } else if app.feed.filter_focus {
    let t = std::time::Instant::now();
    draw_filter_panel(frame, app, right_rect);
    log::debug!("draw_filter_panel: {}ms", t.elapsed().as_millis());
  } else {
    details_rect = Some(right_rect);
    let t = std::time::Instant::now();
    draw_details_panel(frame, app, right_rect);
    log::debug!("draw_details_panel: {}ms", t.elapsed().as_millis());
  }

  MainRowRects {
    feed: Some(feed_rect),
    reader: None,
    secondary_reader: None,
    notes: if app.notes_active { Some(right_rect) } else { None },
    secondary_notes: None,
    details: details_rect,
  }
}
