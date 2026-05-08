use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Direction, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span, Text},
  widgets::{
    Cell, Clear, Paragraph, Row, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Table,
  },
};

use super::repo_viewer::draw_repo_viewer;
use crate::app::{App, AppView, FeedTab, FocusedReader};
use crate::models::{
  ContentType, SignalLevel, SourcePlatform, WorkflowState,
};
use std::collections::HashSet;

mod details;
mod filter;
mod footer;
mod modals;
mod notes;
mod popups;
mod reader;
mod widgets;

pub use popups::HELP_SECTION_COUNT;
use details::draw_details_panel;
use filter::draw_filter_panel;
use footer::draw_footer;
use modals::{draw_settings, draw_sources_popup, draw_theme_picker};
use notes::{draw_note_dock, draw_notes_surface};
use reader::{
  chat_context_line, draw_narrow_feed_details_popup, draw_reader_bottom_pane,
  draw_reader_popup, draw_reader_tab_bar, draw_reader_workspace_header,
  drawer_feed_header_line, drawer_feed_row_line, reader_workspace_split,
};
use popups::{
  draw_abstract_popup, draw_help_overlay, draw_quit_popup, draw_tag_picker,
};
use widgets::{
  draw_horiz_split_box, draw_vert_split_box, h_margin, safe_truncate_chars,
  truncate, truncate_str,
};

pub const RIGHT_COL_WIDTH: u16 = 50;

const VERSION: &str = "v0.1.0";

const SPINNER: &[&str] =
  &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw(frame: &mut Frame, app: &mut App) {
  let t_total = std::time::Instant::now();
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
  if app.abstract_popup_active {
    draw_abstract_popup(frame, app);
  }
  // Help overlay floats on top of whatever view is rendered.
  if app.help_active {
    draw_help_overlay(frame, app);
  }
  if app.theme_picker_active {
    draw_theme_picker(frame, app);
  }
  if app.tag_picker_active {
    draw_tag_picker(frame, app);
  }
  // Quit popup sits above everything — must be last.
  if app.quit_popup_active {
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
    app.chat_active && app.chat_ui.as_ref().map_or(true, |c| c.needs_panel());

  let (main_h, chat_h) = if chat_needs_panel {
    let ch = (available / 2).max(15).min(available.saturating_sub(10));
    let mh = available.saturating_sub(ch);
    (mh, ch)
  } else {
    (available, 0)
  };

  // Build row constraints: title | search | [chat?] | main | [chat?] | footer
  // We place chat above or below main depending on `chat_at_top`.
  if chat_needs_panel && app.chat_at_top {
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
    if let Some(chat_ui) = app.chat_ui.as_mut() {
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

    app.update_pane_rects(
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
      if let Some(chat_ui) = app.chat_ui.as_mut() {
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
    app.update_pane_rects(
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
  if app.chat_active && !chat_needs_panel {
    if let Some(chat_ui) = app.chat_ui.as_mut() {
      chat_ui.draw_overlay(frame, area, &theme);
    }
  }

  // A1 — floating reader popup (Ldr+Enter).
  if app.reader_popup_active {
    draw_reader_popup(frame, app, area);
  }

  // A2 State 3 — bottom pane visible only when summoned (Ldr+f).
  if app.reader_dual_active && app.reader_bottom_open {
    draw_reader_bottom_pane(frame, app, area);
  }
}


// ── Title bar ──────────────────────────────────────────────────────────────

fn title_bar_height(_width: u16) -> u16 {
  5
}

fn draw_title_bar(frame: &mut Frame, app: &App, area: Rect) {
  draw_compact_title_bar(frame, app, area);
}

fn draw_compact_title_bar(frame: &mut Frame, app: &App, area: Rect) {
  let t = app.theme();
  let inner = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Length(1),
    ])
    .split(area);

  let width = area.width as usize;
  // Use the memoized counts cache instead of two full O(N) scans per
  // frame for inbox / library counts.
  let counts = app.item_counts();
  let inbox_count = counts.inbox;
  let library_count = counts.total - counts.inbox;
  let total = counts.total;
  let active_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
  let inactive_style = Style::default().fg(t.text_dim);
  let inbox_style =
    if app.feed_tab == FeedTab::Inbox { active_style } else { inactive_style };
  let library_style = if app.feed_tab == FeedTab::Library {
    active_style
  } else {
    inactive_style
  };
  let discoveries_style = if app.feed_tab == FeedTab::Discoveries {
    active_style
  } else {
    inactive_style
  };
  let history_style = if app.feed_tab == FeedTab::History {
    active_style
  } else {
    inactive_style
  };
  let discovery_spin = if app.discovery_loading {
    format!(" {}", SPINNER[app.spinner_frame % SPINNER.len()])
  } else {
    String::new()
  };
  const WORDMARK: &[&str] = &[
    "█▀█ █▄ █ █▀  █▀█ █▀ █▀ █▀ █▀█ █▀█ █ █",
    "█ █ █ ▀█ █▀  █▀▄ █▀ ▀█ █▀ █▀█ █▀▄ █▀█",
    "▀▀▀ ▀  ▀ ▀▀  ▀ ▀ ▀▀ ▀▀ ▀▀ ▀ ▀ ▀ ▀ ▀ ▀",
  ];
  let nav_text = format!(
    "Inbox {inbox_count}  Library {library_count}  Discoveries {}{}  History {}  Total {total}",
    app.discovery_items.len(),
    discovery_spin,
    app.history.len(),
  );
  let logo_style = Style::default().fg(t.accent).add_modifier(Modifier::BOLD);
  let logo_width =
    WORDMARK.iter().map(|line| line.chars().count()).max().unwrap_or(0);
  let logo = Line::from(Span::styled(WORDMARK[0], logo_style));
  frame.render_widget(Paragraph::new(logo), inner[1]);

  let nav_width = nav_text.chars().count();
  let centered_nav_x = width.saturating_sub(nav_width) / 2;
  let nav_x = centered_nav_x.max(logo_width.saturating_add(3));
  let logo_gap = nav_x.saturating_sub(WORDMARK[1].chars().count());
  let version_gap =
    width.saturating_sub(nav_x + nav_width + VERSION.len()).max(1);
  let nav = Line::from(vec![
    Span::styled(WORDMARK[1], logo_style),
    Span::raw(" ".repeat(logo_gap)),
    Span::styled("Inbox ", inbox_style),
    Span::styled(inbox_count.to_string(), inbox_style),
    Span::styled("  Library ", library_style),
    Span::styled(library_count.to_string(), library_style),
    Span::styled("  Discoveries ", discoveries_style),
    Span::styled(
      format!("{}{}", app.discovery_items.len(), discovery_spin),
      discoveries_style,
    ),
    Span::styled("  History ", history_style),
    Span::styled(app.history.len().to_string(), history_style),
    Span::styled("  Total ", inactive_style),
    Span::styled(total.to_string(), inactive_style),
    Span::raw(" ".repeat(version_gap)),
    Span::styled(VERSION, Style::default().fg(t.text_dim)),
  ]);
  frame.render_widget(Paragraph::new(nav), inner[2]);

  frame.render_widget(
    Paragraph::new(Line::from(Span::styled(WORDMARK[2], logo_style))),
    inner[3],
  );

  let sep_str = "─".repeat(area.width as usize);
  let sep = Paragraph::new(sep_str).style(Style::default().fg(t.border));
  frame.render_widget(sep, inner[4]);
}

// ── Search + filter row ────────────────────────────────────────────────────

fn draw_search_row(frame: &mut Frame, app: &App, area: Rect) {
  let t = app.theme();
  // Row 0: content; row 1: separator
  let content_area = Rect { height: 1, ..area };
  let sep_area = Rect { y: area.y + 1, height: 1, ..area };

  let cols = Layout::default()
    .direction(Direction::Horizontal)
    .constraints([Constraint::Min(0), Constraint::Length(RIGHT_COL_WIDTH)])
    .split(content_area);

  let search_text = if app.search_active || !app.search_query.is_empty() {
    format!(" / {}", app.search_query)
  } else {
    " / Search items...".to_string()
  };
  let search_style = if app.search_active {
    Style::default().fg(t.text)
  } else {
    Style::default().fg(t.text_dim)
  };
  frame.render_widget(Paragraph::new(search_text).style(search_style), cols[0]);

  let filter_style = if app.filter_focus {
    Style::default().fg(t.accent)
  } else {
    Style::default().fg(t.text_dim)
  };
  frame.render_widget(
    Paragraph::new(format!(" {}", filter_summary(app))).style(filter_style),
    cols[1],
  );

  let sep = "─".repeat(area.width as usize);
  frame.render_widget(
    Paragraph::new(sep).style(Style::default().fg(t.border)),
    sep_area,
  );
}

fn filter_summary(app: &App) -> std::cell::Ref<'_, str> {
  // Lazy memoize: the 4 summarize_ordered_set passes + sort + format!
  // ran every frame regardless of whether active_filters changed.
  // Invalidation hooks fire from `toggle_filter_at_cursor` and
  // `clear_filters` in app.rs.
  if app.filter_summary_cache.borrow().is_none() {
    let summary = compute_filter_summary(app);
    *app.filter_summary_cache.borrow_mut() = Some(summary);
  }
  std::cell::Ref::map(app.filter_summary_cache.borrow(), |opt| {
    opt.as_deref().expect("filter_summary_cache populated above")
  })
}

fn compute_filter_summary(app: &App) -> String {
  let f = &app.active_filters;
  let source_summary = if f.active_count() == 0 {
    "any".to_string()
  } else {
    summarize_strings(&f.sources)
  };
  format!(
    "source:{}  state:{}  type:{}  signal:{}",
    source_summary,
    summarize_ordered_set(
      &f.workflow_states,
      &[
        (WorkflowState::Inbox, "inbox"),
        (WorkflowState::Queued, "queued"),
        (WorkflowState::DeepRead, "read"),
        (WorkflowState::Archived, "archived"),
      ],
    ),
    summarize_ordered_set(
      &f.content_types,
      &[
        (ContentType::Paper, "paper"),
        (ContentType::Article, "article"),
        (ContentType::Digest, "digest"),
        (ContentType::Thread, "thread"),
        (ContentType::Repo, "repo"),
      ],
    ),
    summarize_ordered_set(
      &f.signals,
      &[
        (SignalLevel::Primary, "primary"),
        (SignalLevel::Secondary, "secondary"),
        (SignalLevel::Tertiary, "tertiary"),
      ],
    ),
  )
}

fn summarize_strings(values: &HashSet<String>) -> String {
  if values.is_empty() {
    return "any".to_string();
  }
  let mut values: Vec<&str> = values.iter().map(String::as_str).collect();
  values.sort_unstable();
  summarize_labels(values)
}

fn summarize_ordered_set<T>(
  values: &HashSet<T>,
  order: &[(T, &'static str)],
) -> String
where
  T: Eq + std::hash::Hash,
{
  if values.is_empty() {
    return "any".to_string();
  }
  let labels: Vec<&str> = order
    .iter()
    .filter_map(|(value, label)| values.contains(value).then_some(*label))
    .collect();
  summarize_labels(labels)
}

fn summarize_labels(labels: Vec<&str>) -> String {
  match labels.as_slice() {
    [] => "any".to_string(),
    [only] => (*only).to_string(),
    [first, second] => format!("{first},{second}"),
    [first, rest @ ..] => format!("{first}+{}", rest.len()),
  }
}

// ── Main row ───────────────────────────────────────────────────────────────

/// Screen rects computed by draw_main_row, passed back to app.update_pane_rects.
struct MainRowRects {
  feed: Option<Rect>,
  reader: Option<Rect>,
  secondary_reader: Option<Rect>,
  notes: Option<Rect>,
  secondary_notes: Option<Rect>,
  details: Option<Rect>,
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


fn draw_main_row(frame: &mut Frame, app: &mut App, area: Rect) -> MainRowRects {
  let theme = app.theme();
  let t = theme;
  // Tread's `Theme` is a sibling type from a separate `ui_theme` crate
  // — we can't pass `&t` directly.  Convert once per draw cycle.
  let tread_theme = app.theme_for_tread();
  // ── A2 State 3: dual-reader (left 50% | right 50%) ──────────────────────
  if app.reader_dual_active && app.reader_active {
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
      let focused = app.focused_reader == FocusedReader::Primary;
      draw_reader_tab_bar(
        frame,
        rows[0],
        &app.reader_tabs,
        app.reader_active_tab,
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
        tread::after_draw(&tab.reader, &mut tab.image_state, rows[1], kitty);
      }
    }
    {
      let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
        .split(right_reader_rect);
      let focused = app.focused_reader == FocusedReader::Secondary;
      draw_reader_tab_bar(
        frame,
        rows[0],
        &app.reader_secondary_tabs,
        app.reader_secondary_active_tab,
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
        tread::after_draw(&tab.reader, &mut tab.image_state, rows[1], kitty);
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
  if app.reader_split_active && app.reader_active {
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
    draw_feed_pane(frame, app, feed_rect);
    {
      let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
        .split(reader_rect);
      draw_reader_tab_bar(
        frame,
        rows[0],
        &app.reader_tabs,
        app.reader_active_tab,
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
        tread::after_draw(&tab.reader, &mut tab.image_state, rows[1], kitty);
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
  if app.reader_active && !app.notes_active {
    let (workspace_area, body_area) = reader_workspace_split(area);
    draw_reader_workspace_header(frame, app, workspace_area, "Reader");
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
      .split(body_area);
    draw_reader_tab_bar(
      frame,
      rows[0],
      &app.reader_tabs,
      app.reader_active_tab,
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
      tread::after_draw(&tab.reader, &mut tab.image_state, rows[1], kitty);
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

  if app.reader_active {
    let (workspace_area, body_area) = reader_workspace_split(area);
    draw_reader_workspace_header(frame, app, workspace_area, "Reader + Notes");
    let (reader_rect, notes_rect) = split_reader_note_dock(body_area);
    {
      let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
        .split(reader_rect);
      draw_reader_tab_bar(
        frame,
        rows[0],
        &app.reader_tabs,
        app.reader_active_tab,
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
        tread::after_draw(&tab.reader, &mut tab.image_state, rows[1], kitty);
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
    } else if app.filter_focus {
      "Filters"
    } else {
      "Details"
    };
    let (feed_rect, bottom_rect) =
      draw_vert_split_box(frame, area, "Feed", bottom_title, &t);

    let t = std::time::Instant::now();
    draw_feed_pane(frame, app, feed_rect);
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
    } else if app.filter_focus {
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
  let inner_w = area.width.saturating_sub(2);
  let right_w = if app.notes_active {
    (inner_w * 40 / 100).max(1)
  } else {
    RIGHT_COL_WIDTH.min(inner_w.saturating_sub(2))
  };
  let right_title = if app.notes_active {
    app.notes_mode.title()
  } else if app.filter_focus {
    "Filters"
  } else {
    "Details"
  };

  let (feed_rect, right_rect) =
    draw_horiz_split_box(frame, area, right_w, "Feed", right_title, &t);

  let t = std::time::Instant::now();
  draw_feed_pane(frame, app, feed_rect);
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
  } else if app.filter_focus {
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

fn draw_feed_pane(frame: &mut Frame, app: &mut App, area: Rect) {
  if area.height == 0 {
    return;
  }
  let content_area = area;

  // Discoveries tab: paper list always shown; persistent search bar pinned at bottom.
  if app.feed_tab == FeedTab::Discoveries {
    draw_discoveries_with_searchbar(frame, app, content_area);
    return;
  }

  // History tab: filter chips + activity log.
  if app.feed_tab == FeedTab::History {
    draw_history_tab(frame, app, content_area);
    return;
  }

  // Library tab: workflow-state filter chips + filtered item list.
  if app.feed_tab == FeedTab::Library {
    draw_library_tab(frame, app, content_area);
    return;
  }

  // Narrow pane: switch to title-only list to avoid squished columns.
  if area.width < 70 {
    draw_narrow_feed(frame, app, content_area);
  } else {
    draw_item_table(frame, app, content_area);
  }
}

/// Discoveries tab: paper list above, persistent search bar below.
fn draw_discoveries_with_searchbar(
  frame: &mut Frame,
  app: &mut App,
  area: Rect,
) {
  const FOOTER_H: u16 = 3; // separator + input + hint
  if area.height <= FOOTER_H {
    draw_discovery_searchbar(frame, app, area);
    return;
  }

  let list_h = area.height - FOOTER_H;
  let list_area =
    Rect { x: area.x, y: area.y, width: area.width, height: list_h };
  let bar_area =
    Rect { x: area.x, y: area.y + list_h, width: area.width, height: FOOTER_H };

  // Paper list
  if area.width < 70 {
    draw_narrow_feed(frame, app, list_area);
  } else {
    draw_item_table(frame, app, list_area);
  }

  draw_discovery_searchbar(frame, app, bar_area);
  draw_discovery_palette(frame, app, list_area);
}

fn draw_discovery_searchbar(frame: &mut Frame, app: &App, area: Rect) {
  let t = app.theme();
  let w = area.width as usize;
  let has_session = !app.discovery_session.is_empty();
  let intent_label = app.discovery_intent.label();

  // Separator line — title shows current status inline rather than a separate row.
  let intent_badge = if intent_label != "papers" {
    format!(" [{}]", intent_label)
  } else {
    String::new()
  };
  let (title_text, title_style) = if app.discovery_loading {
    let short =
      app.discovery_status.trim_end_matches('…').trim_end_matches("...");
    (
      format!("{}…{}", short, intent_badge),
      Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    )
  } else if has_session {
    (
      format!("Discovery ●{}", intent_badge),
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    )
  } else {
    (format!("Discovery{}", intent_badge), Style::default().fg(t.border))
  };
  let sep_fill = "─".repeat(w.saturating_sub(title_text.len() + 8));
  let sep_line = Line::from(vec![
    Span::styled("─── ", Style::default().fg(t.border)),
    Span::styled(title_text, title_style),
    Span::styled(format!(" {sep_fill}"), Style::default().fg(t.border)),
  ]);

  // Input line — prompt only when focused, query dim when unfocused.
  let cursor = if app.discovery_search_focused { "█" } else { "" };
  let (prompt, query_style) = if app.discovery_search_focused {
    (
      Span::styled("  ", Style::default().fg(t.accent)),
      Style::default().fg(t.text),
    )
  } else {
    (
      Span::styled("  ", Style::default().fg(t.text_dim)),
      Style::default().fg(t.text_dim),
    )
  };
  let input_line = Line::from(vec![
    prompt,
    Span::styled(format!("{}{}", app.discovery_query, cursor), query_style),
  ]);

  // Hint line — contextual, always rendered to avoid height jitter.
  let hint_text = if app.discovery_search_focused {
    if app.discovery_query.starts_with('/') {
      "Tab: complete  ↑↓: navigate  Enter: run  Esc: cancel"
    } else if has_session {
      "Enter: refine  Ctrl+N: new search  Esc: unfocus"
    } else {
      "Enter: search  /: commands  Esc: unfocus"
    }
  } else if has_session {
    "Any key to refine  ·  Ctrl+N: new search  ·  / for commands"
  } else {
    "Any key to focus  ·  / for commands"
  };
  let hint_line =
    Line::from(Span::styled(hint_text, Style::default().fg(t.text_dim)));

  frame
    .render_widget(Paragraph::new(vec![sep_line, input_line, hint_line]), area);
}

fn draw_discovery_palette(frame: &mut Frame, app: &App, list_area: Rect) {
  if !app.discovery_search_focused || !app.discovery_query.starts_with('/') {
    return;
  }

  let all_specs = crate::commands::registry::discovery_slash_specs();
  let query_lower = app.discovery_query_lower.as_str();
  let suggestions: Vec<_> = all_specs
    .iter()
    .filter(|s| {
      query_lower == "/" || s.command.starts_with(query_lower)
    })
    .collect();

  if suggestions.is_empty() || list_area.height == 0 {
    return;
  }

  let t = app.theme();
  let w = list_area.width as usize;
  let visible = suggestions.len().min(8);
  let selected = app.discovery_palette_selected.min(suggestions.len() - 1);
  let scroll = app.discovery_palette_scroll;
  let start = scroll;
  let end = (start + visible).min(suggestions.len());

  // separator + rows + count
  let height = (visible as u16 + 2).min(list_area.height);
  let area = Rect {
    x: list_area.x,
    y: list_area.y + list_area.height.saturating_sub(height),
    width: list_area.width,
    height,
  };

  frame.render_widget(Clear, area);

  let name_col = 16usize;
  let badge_col = 7usize;
  let desc_col = w.saturating_sub(name_col + badge_col + 4);

  let sep_fill = "─".repeat(w.saturating_sub(16));
  let mut lines: Vec<Line> = vec![Line::from(Span::styled(
    format!("─── Commands ──{sep_fill}"),
    Style::default().fg(t.border),
  ))];

  for (i, spec) in suggestions.iter().skip(start).take(end - start).enumerate()
  {
    let is_selected = start + i == selected;
    let (arrow, name_style, desc_style) = if is_selected {
      (
        "→ ",
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        Style::default().fg(t.text),
      )
    } else {
      ("  ", Style::default().fg(t.text), Style::default().fg(t.text_dim))
    };

    let name = spec.command.trim_start_matches('/');
    let name_padded = format!("{:<width$}", name, width = name_col);
    let badge = if spec.badge.is_empty() {
      String::new()
    } else {
      format!("[{}]", spec.badge)
    };
    let badge_padded = format!("{:<width$}", badge, width = badge_col);
    let desc: String = spec.description.chars().take(desc_col).collect();

    lines.push(Line::from(vec![
      Span::styled(arrow, Style::default().fg(t.accent)),
      Span::styled(name_padded, name_style),
      Span::styled(badge_padded, Style::default().fg(t.text_dim)),
      Span::styled(desc, desc_style),
    ]));
  }

  let count_str = format!("({}/{})", selected + 1, suggestions.len());
  let padding = w.saturating_sub(count_str.len());
  lines.push(Line::from(Span::styled(
    format!("{}{}", " ".repeat(padding), count_str),
    Style::default().fg(t.text_dim),
  )));

  frame.render_widget(
    Paragraph::new(lines).style(Style::default().bg(t.bg_chat)),
    area,
  );
}

fn draw_library_tab(frame: &mut Frame, app: &mut App, area: Rect) {
  let t = app.theme();
  if area.height == 0 {
    return;
  }

  // ── Filter chip row ───────────────────────────────────────────────────
  let chips_area = Rect { height: 1, ..area };
  let chips_sep_area = Rect { y: area.y + 1, height: 1, ..area };

  // Per-chip count via the memoized aggregate. Extract the three workflow
  // counts into local copies so the Ref<ItemCounts> drops immediately — the
  // rest of the function dispatches to mutating helpers that need &mut app.
  let (count_queued, count_deep_read, count_archived) = {
    let counts = app.item_counts();
    (counts.queued, counts.deep_read, counts.archived)
  };
  let chip_count = |filter: crate::library::LibraryFilter| -> usize {
    match filter {
      crate::library::LibraryFilter::All => count_queued + count_deep_read,
      crate::library::LibraryFilter::Queue => count_queued,
      crate::library::LibraryFilter::Read => count_deep_read,
      crate::library::LibraryFilter::Archived => count_archived,
    }
  };

  let mut chip_spans: Vec<Span> = vec![Span::raw("  ")];
  let mut chip_width: usize = 2;
  for (i, filter) in crate::library::LibraryFilter::ORDER.iter().enumerate() {
    let active = *filter == app.library_filter;
    let style = if active {
      Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
    } else {
      Style::default().fg(t.text_dim)
    };
    let text = format!("[{} {}]", filter.label(), chip_count(*filter));
    chip_width += text.chars().count();
    chip_spans.push(Span::styled(text, style));
    if i + 1 < crate::library::LibraryFilter::ORDER.len() {
      chip_spans.push(Span::raw("  "));
      chip_width += 2;
    }
  }
  let hint = if app.library_visual_mode {
    let n = app.library_selected_urls.len();
    format!("VISUAL · {n} selected · r read · w queue · x archive · Esc cancel")
  } else {
    "[ ] cycle  ·  v select  ·  f filter  ·  / search".to_string()
  };
  let hint_style = if app.library_visual_mode {
    Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
  } else {
    Style::default().fg(t.text_dim)
  };
  let total = area.width as usize;
  if total > chip_width + hint.chars().count() + 4 {
    let pad = total - chip_width - hint.chars().count() - 2;
    chip_spans.push(Span::raw(" ".repeat(pad)));
    chip_spans.push(Span::styled(hint, hint_style));
  }
  frame.render_widget(Paragraph::new(Line::from(chip_spans)), chips_area);

  frame.render_widget(
    Paragraph::new("─".repeat(area.width as usize))
      .style(Style::default().fg(t.border)),
    chips_sep_area,
  );

  // ── Item list (reuse the table renderer) ─────────────────────────────
  let list_area = Rect {
    x: area.x,
    y: area.y + 2,
    width: area.width,
    height: area.height.saturating_sub(2),
  };
  if list_area.height == 0 {
    return;
  }

  if app.visible_count() == 0 {
    let msg = if app.items.is_empty() {
      "No items yet — fetch a feed first."
    } else {
      "No items match this filter."
    };
    frame.render_widget(
      Paragraph::new(Line::from(Span::styled(
        format!("  {msg}"),
        Style::default().fg(t.text_dim),
      ))),
      list_area,
    );
    return;
  }

  if list_area.width < 70 {
    draw_narrow_feed(frame, app, list_area);
  } else {
    draw_item_table(frame, app, list_area);
  }
}

fn draw_history_tab(frame: &mut Frame, app: &App, area: Rect) {
  let t = app.theme();
  if area.height == 0 {
    return;
  }

  // ── Filter chips row ────────────────────────────────────────────────
  let chips_area = Rect { height: 1, ..area };
  let chips_sep_area = Rect { y: area.y + 1, height: 1, ..area };
  let mut chip_spans: Vec<Span> = vec![Span::styled("  ", Style::default())];
  let mut chip_width: usize = 2;
  for (i, filter) in crate::history::HistoryFilter::ORDER.iter().enumerate() {
    let active = *filter == app.history_filter;
    let style = if active {
      Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
    } else {
      Style::default().fg(t.text_dim)
    };
    let text = format!("[{}]", filter.label());
    chip_width += text.chars().count();
    chip_spans.push(Span::styled(text, style));
    if i + 1 < crate::history::HistoryFilter::ORDER.len() {
      chip_spans.push(Span::raw("  "));
      chip_width += 2;
    }
  }
  let hint = "[ ] cycle  ·  f filter  ·  / search";
  let total = area.width as usize;
  if total > chip_width + hint.chars().count() + 4 {
    let pad = total - chip_width - hint.chars().count() - 2;
    chip_spans.push(Span::raw(" ".repeat(pad)));
    chip_spans.push(Span::styled(hint, Style::default().fg(t.text_dim)));
  }
  frame.render_widget(Paragraph::new(Line::from(chip_spans)), chips_area);
  frame.render_widget(
    Paragraph::new("─".repeat(area.width as usize))
      .style(Style::default().fg(t.border)),
    chips_sep_area,
  );

  // ── Activity list ──────────────────────────────────────────────────
  let list_area = Rect {
    x: area.x,
    y: area.y + 2,
    width: area.width,
    height: area.height.saturating_sub(2),
  };
  if list_area.height == 0 {
    return;
  }

  let entries = app.filtered_history();
  if entries.is_empty() {
    let msg = if app.history.is_empty() {
      "No history yet — open a paper or run a search."
    } else {
      "No entries in this time window."
    };
    frame.render_widget(
      Paragraph::new(Line::from(Span::styled(
        format!("  {msg}"),
        Style::default().fg(t.text_dim),
      ))),
      list_area,
    );
    return;
  }

  let header_style = Style::default().fg(t.header).add_modifier(Modifier::BOLD);
  let header = Row::new(vec![
    feed_header_cell("Src", header_style),
    feed_header_cell("Kind", header_style),
    feed_header_cell("Title", header_style),
    feed_header_cell("Date", header_style),
    feed_header_cell("Viewed", header_style),
  ])
  .height(2);

  let inner = Rect {
    y: list_area.y.saturating_add(1),
    height: list_area.height.saturating_sub(1),
    ..list_area
  };
  if inner.height == 0 {
    return;
  }

  let now = chrono::Utc::now();
  let title_w = (inner.width.saturating_sub(7 + 7 + 10 + 10 + 4)) as usize;
  let title_wrap_w = title_w.max(10);
  let viewport_rows = inner.height.saturating_sub(2) as usize;
  if viewport_rows == 0 {
    let table = Table::new(
      Vec::<Row>::new(),
      [
        Constraint::Length(5),
        Constraint::Min(0),
        Constraint::Length(14),
        Constraint::Length(10),
      ],
    )
    .header(header)
    .column_spacing(1)
    .row_highlight_style(Style::default());
    frame.render_widget(table, inner);
    return;
  }
  let total = entries.len();
  let selected = app.history_selected_index.min(total.saturating_sub(1));
  let mut offset =
    app.history_list_offset.min(total.saturating_sub(viewport_rows.min(total)));
  if selected < offset {
    offset = selected;
  } else if selected >= offset + viewport_rows {
    offset = selected + 1 - viewport_rows;
  }

  let end = (offset + viewport_rows + 2).min(total);
  let window = &entries[offset..end];
  // Store raw title strings (not Vec<Line>) — see draw_item_table's
  // window_data shape for the same rationale.
  let window_data: Vec<(u16, Vec<String>)> = window
    .iter()
    .map(|entry| {
      let mut raw_lines = textwrap::wrap(&entry.title, title_wrap_w);
      let row_height = raw_lines.len().min(2).max(1) as u16;
      if raw_lines.len() > 2 {
        raw_lines.truncate(2);
        if let Some(last) = raw_lines.last_mut() {
          let s = last.clone().into_owned();
          let trimmed = safe_truncate_chars(&s, title_wrap_w.saturating_sub(1));
          *last = std::borrow::Cow::Owned(format!("{trimmed}…"));
        }
      }
      let title_lines: Vec<String> =
        raw_lines.into_iter().map(|l| l.into_owned()).collect();
      (row_height, title_lines)
    })
    .collect();

  let rows: Vec<Row> = window
    .iter()
    .enumerate()
    .map(|(i, entry)| {
      let item_idx = offset + i;
      let is_selected = item_idx == selected;
      let (content_height, title_lines) = &window_data[i];
      // O(1) hashmap lookup. Was a per-row O(items + discovery_items)
      // chain+find scan against ~3K items.
      let cached_item = app
        .url_index
        .get(&entry.key)
        .map(|&idx| &app.items[idx])
        .or_else(|| {
          app
            .discovery_url_index
            .get(&entry.key)
            .map(|&idx| &app.discovery_items[idx])
        });
      let row_style =
        if is_selected { t.style_selection() } else { Style::default() };
      let selected_text_style = t.style_selection_text();
      let selected_dim_style = t.style_selection_dim();
      let dim_style = if is_selected {
        selected_dim_style
      } else {
        Style::default().fg(t.text_dim)
      };
      let source = cached_item
        .map(feed_source_label)
        .unwrap_or_else(|| history_source_label(entry));
      let kind = match (entry.kind, cached_item) {
        (crate::history::HistoryKind::Paper, Some(item)) => {
          item.content_type.short_label().to_string()
        }
        (crate::history::HistoryKind::Paper, None) => "paper".to_string(),
        (crate::history::HistoryKind::Query, _) => "query".to_string(),
      };
      let date = cached_item
        .map(|item| item.published_at.as_str())
        .or_else(|| {
          entry.paper_meta.as_ref().map(|meta| meta.published_at.as_str())
        })
        .unwrap_or("");
      let source_style = if is_selected {
        selected_text_style
      } else if entry.kind == crate::history::HistoryKind::Query {
        Style::default().fg(t.accent)
      } else {
        Style::default().fg(t.accent)
      };
      Row::new(vec![
        feed_cell(&source, source_style, is_selected),
        feed_cell(&kind, dim_style, is_selected),
        Cell::from(Text::from({
          let title_style = if is_selected {
            selected_text_style
          } else {
            Style::default()
          };
          let mut lines: Vec<Line<'static>> = title_lines
            .iter()
            .map(|s| Line::from(Span::styled(s.clone(), title_style)))
            .collect();
          lines.push(feed_spacer_line(is_selected));
          lines
        })),
        feed_cell(date, dim_style, is_selected),
        feed_cell(
          &crate::history::format_ago(entry.opened_at, now),
          dim_style,
          is_selected,
        ),
      ])
      .style(row_style)
      .height((content_height + 1).max(3))
    })
    .collect();

  let table = Table::new(
    rows,
    [
      Constraint::Length(7),
      Constraint::Length(7),
      Constraint::Min(0),
      Constraint::Length(10),
      Constraint::Length(10),
    ],
  )
  .header(header)
  .column_spacing(1)
  .row_highlight_style(Style::default());
  frame.render_widget(table, inner);

  if total > 0 {
    let mut scrollbar_state = ScrollbarState::new(total)
      .position(offset)
      .viewport_content_length(viewport_rows);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
      .begin_symbol(None)
      .end_symbol(None);
    frame.render_stateful_widget(scrollbar, inner, &mut scrollbar_state);
  }
}

fn history_source_label(entry: &crate::history::HistoryEntry) -> String {
  match entry.paper_meta.as_ref().map(|meta| &meta.source_platform) {
    Some(SourcePlatform::HuggingFace) => "hf".to_string(),
    Some(SourcePlatform::ArXiv) => "arxiv".to_string(),
    Some(SourcePlatform::Rss) if !entry.source.is_empty() => {
      truncate(&entry.source, 7)
    }
    Some(platform) => platform.short_label().to_string(),
    None => truncate(&entry.source, 7),
  }
}

fn draw_narrow_feed(frame: &mut Frame, app: &mut App, area: Rect) {
  let t = app.theme();
  let rows =
    Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
  let header_area = rows[0];
  let list_area = rows[1];
  if list_area.height == 0 {
    return;
  }
  let viewport_rows = list_area.height as usize;
  let selected = app.active_selected_index();
  let title_w = reader_feed_title_width(list_area.width as usize);

  let mut offset = app.active_list_offset();
  {
    let total = app.visible_count();
    if selected < offset {
      offset = selected;
    } else {
      // Reverse-walk only needs items 0..=selected to find the offset, so
      // grab a bounded window instead of allocating Vec<&FeedItem> for the
      // entire visible set every redraw.
      let visible = app.visible_window(0, selected.saturating_add(1));
      let vc = count_reader_feed_visible_items(
        &visible,
        offset,
        viewport_rows,
        title_w,
      );
      if selected >= offset + vc {
        let mut rows_used = 0usize;
        offset = selected;
        for i in (0..=selected).rev() {
          let h = reader_feed_row_height(visible[i], title_w);
          if rows_used + h > viewport_rows {
            break;
          }
          rows_used += h;
          offset = i;
        }
      }
    }
    offset = offset.min(total.saturating_sub(1));
  }
  app.set_active_list_offset(offset);

  frame.render_widget(
    Paragraph::new(drawer_feed_header_line(list_area.width as usize, &t)),
    header_area,
  );

  // Each visible row consumes at least 1 terminal row, so capping the
  // window at viewport_rows is a safe upper bound for what gets drawn.
  let visible =
    app.visible_window(offset, offset.saturating_add(viewport_rows));
  // Pre-wrap titles once per item; `reader_feed_row_lines` previously
  // re-ran textwrap on each call, doubling work against the same
  // textwrap done for row-height counting.
  let pre_wrapped: Vec<Vec<String>> = visible
    .iter()
    .map(|item| reader_feed_title_lines(&item.title, title_w))
    .collect();
  let mut y = list_area.y;
  for (rel_i, item) in visible.iter().enumerate() {
    if y >= list_area.y + list_area.height {
      break;
    }
    let abs_i = offset + rel_i;
    let is_selected = abs_i == selected;
    let row_lines = reader_feed_row_lines_with_wrapped(
      item,
      &pre_wrapped[rel_i],
      list_area.width as usize,
      is_selected,
      &t,
    );
    for line in row_lines {
      if y >= list_area.y + list_area.height {
        break;
      }
      let row_rect =
        Rect { x: list_area.x, y, width: list_area.width, height: 1 };
      if is_selected {
        frame.render_widget(
          Paragraph::new(line).style(t.style_selection()),
          row_rect,
        );
      } else {
        frame.render_widget(Paragraph::new(line), row_rect);
      }
      y += 1;
    }
    if y < list_area.y + list_area.height {
      let spacer_rect =
        Rect { x: list_area.x, y, width: list_area.width, height: 1 };
      if is_selected {
        frame.render_widget(
          Paragraph::new(Line::from(" ")).style(t.style_selection()),
          spacer_rect,
        );
      }
      y += 1;
    }
  }
}

fn reader_feed_title_width(width: usize) -> usize {
  if width < 34 {
    width.max(8)
  } else {
    let source_w = 7usize;
    let kind_w = 6usize;
    let date_w = 10usize;
    let gap_w = 3usize;
    width.saturating_sub(source_w + kind_w + date_w + gap_w).max(8)
  }
}

fn count_reader_feed_visible_items(
  items: &[&crate::models::FeedItem],
  list_offset: usize,
  viewport_rows: usize,
  title_w: usize,
) -> usize {
  let mut rows_used = 0usize;
  let mut count = 0usize;
  for item in items.iter().skip(list_offset) {
    let item_height = reader_feed_row_height(item, title_w);
    if rows_used + item_height > viewport_rows {
      break;
    }
    rows_used += item_height;
    count += 1;
  }
  count.max(1)
}

fn reader_feed_row_height(
  item: &crate::models::FeedItem,
  title_w: usize,
) -> usize {
  reader_feed_title_lines(&item.title, title_w).len() + 1
}

fn reader_feed_title_lines(title: &str, title_w: usize) -> Vec<String> {
  let mut raw_lines =
    textwrap::wrap(title, title_w).into_iter().collect::<Vec<_>>();
  if raw_lines.is_empty() {
    return vec![String::new()];
  }
  if raw_lines.len() > 2 {
    raw_lines.truncate(2);
    if let Some(last) = raw_lines.last_mut() {
      let s = last.clone().into_owned();
      let trimmed = safe_truncate_chars(&s, title_w.saturating_sub(1));
      *last = std::borrow::Cow::Owned(format!("{trimmed}…"));
    }
  }
  raw_lines.into_iter().map(|line| line.into_owned()).collect()
}

/// Build the per-row Line<'static>s for a single visible item in the
/// narrow-feed table. Takes pre-wrapped title lines so the caller can
/// share the textwrap output with height-counting.
fn reader_feed_row_lines_with_wrapped(
  item: &crate::models::FeedItem,
  title_lines: &[String],
  width: usize,
  selected: bool,
  t: &crate::theme::Theme,
) -> Vec<Line<'static>> {
  if width < 34 {
    return vec![drawer_feed_row_line(item, width, selected, t)];
  }

  let source_w = 7usize;
  let kind_w = 6usize;
  let date_w = 10usize;
  let title_w = reader_feed_title_width(width);
  let source = truncate_str(&feed_source_label(item), source_w);
  let kind = truncate_str(item.content_type.short_label(), kind_w);
  let date = truncate_str(&item.published_at, date_w);

  title_lines
    .iter()
    .enumerate()
    .map(|(idx, title)| {
      let source_text = if idx == 0 {
        format!("{source:<source_w$}")
      } else {
        " ".repeat(source_w)
      };
      let kind_text =
        if idx == 0 { format!("{kind:<kind_w$}") } else { " ".repeat(kind_w) };
      let date_text =
        if idx == 0 { format!("{date:<date_w$}") } else { " ".repeat(date_w) };

      if selected {
        let row =
          format!("{source_text} {kind_text} {title:<title_w$} {date_text}");
        return Line::from(Span::styled(row, t.style_selection_text()));
      }

      Line::from(vec![
        Span::styled(source_text, Style::default().fg(t.accent)),
        Span::raw(" "),
        Span::styled(kind_text, Style::default().fg(t.text_dim)),
        Span::raw(" "),
        Span::styled(format!("{title:<title_w$}"), Style::default().fg(t.text)),
        Span::raw(" "),
        Span::styled(date_text, Style::default().fg(t.text_dim)),
      ])
    })
    .collect()
}


fn draw_item_table(frame: &mut Frame, app: &mut App, area: Rect) {
  let t = app.theme();
  let t_item_table = std::time::Instant::now();
  let header_style = Style::default().fg(t.header).add_modifier(Modifier::BOLD);

  let header = Row::new(vec![
    feed_header_cell(" ", header_style),
    feed_header_cell("Src", header_style),
    feed_header_cell("Kind", header_style),
    feed_header_cell("Title", header_style),
    feed_header_cell("Author", header_style),
    feed_header_cell("Date", header_style),
    feed_header_cell("State", header_style),
  ])
  .height(2);

  // Inner area: leave one quiet row below the pane title before table headers.
  let inner = Rect {
    y: area.y.saturating_add(1),
    height: area.height.saturating_sub(1),
    ..area
  };
  if inner.height == 0 {
    return;
  }

  // Available width for title column: total inner width minus fixed cols.
  // sig(1) + source(7) + kind(5) + author(14) + date(10) + state(8) + spacing(6)
  let title_col_w =
    (inner.width.saturating_sub(1 + 7 + 5 + 14 + 10 + 8 + 6)) as usize;
  let title_wrap_w = title_col_w.max(10);

  // Viewport height in rows (inner height minus 2 header rows).
  let viewport_rows = inner.height.saturating_sub(2) as usize;

  // ── Auto scroll tracking — item-count-based ───────────────────────────────
  // Count and visible_count computed in a scoped borrow so list_offset can be
  // mutated afterwards without a live reference into app.items.
  let total_items_pre = app.visible_count();
  let visible_count = count_visible_items_from_app(
    app,
    app.active_list_offset(),
    viewport_rows,
    title_wrap_w,
  );

  let mut list_offset = app.active_list_offset();
  let selected_index = app.active_selected_index();

  if selected_index < list_offset {
    // Selection moved above the window — scroll up.
    list_offset = selected_index;
  } else if visible_count >= 2
    && selected_index >= list_offset + visible_count.saturating_sub(2)
  {
    // Selection is within 2 items of the bottom edge — scroll down.
    list_offset = (selected_index + 2).saturating_sub(visible_count);
  }
  list_offset = list_offset.min(total_items_pre.saturating_sub(1));
  app.set_active_list_offset(list_offset);

  // Now get the full visible slice for rendering.
  let total_items = total_items_pre;

  // ── Slice to visible window — trust app.list_offset as first visible item ─
  // Take viewport_rows + 2 extra so the last row is never clipped even when
  // an item spans 2 rows.
  let start = app.active_list_offset().min(total_items.saturating_sub(1));
  let end = (start + viewport_rows + 2).min(total_items);
  let window = app.visible_window(start, end);

  // ── Single textwrap pass over visible window only ─────────────────────────
  // Produces (row_height, title_lines) together — no second wrap call needed.
  let t_heights = std::time::Instant::now();
  // Store raw title strings (not pre-wrapped Lines). Building the styled
  // Lines directly at row time saves a Vec<Line>::clone + Span content
  // into_owned re-conversion per row.
  let window_data: Vec<(u16, Vec<String>)> = window
    .iter()
    .map(|item| {
      let mut raw_lines = textwrap::wrap(&item.title, title_wrap_w);
      let row_height = raw_lines.len().min(2).max(1) as u16;
      if raw_lines.len() > 2 {
        raw_lines.truncate(2);
        if let Some(last) = raw_lines.last_mut() {
          let s = last.clone().into_owned();
          let max_chars = title_wrap_w.saturating_sub(1);
          let trimmed = safe_truncate_chars(&s, max_chars);
          *last = std::borrow::Cow::Owned(format!("{trimmed}…"));
        }
      }
      let title_lines: Vec<String> =
        raw_lines.into_iter().map(|l| l.into_owned()).collect();
      (row_height, title_lines)
    })
    .collect();
  log::debug!(
    "window textwrap ({} items): {}ms",
    window.len(),
    t_heights.elapsed().as_millis()
  );

  // ── Build rows for visible window only ────────────────────────────────────
  let t_rows = std::time::Instant::now();
  let visual_mode = app.feed_tab == FeedTab::Library && app.library_visual_mode;
  let rows: Vec<Row> = window
    .iter()
    .enumerate()
    .map(|(i, item)| {
      let item_idx = start + i;
      let is_cursor = item_idx == app.active_selected_index();
      let in_visual =
        visual_mode && app.library_selected_urls.contains(&item.url);
      let is_selected = is_cursor || in_visual;
      let (content_height, title_lines) = &window_data[i];

      let signal_style = match item.signal {
        crate::models::SignalLevel::Primary => Style::default().fg(t.accent),
        crate::models::SignalLevel::Secondary => {
          Style::default().fg(t.text_dim)
        }
        crate::models::SignalLevel::Tertiary => Style::default().fg(t.border),
      };

      let row_style =
        if is_selected { t.style_selection() } else { Style::default() };
      let selected_text_style = t.style_selection_text();
      let selected_dim_style = t.style_selection_dim();

      let author =
        truncate(item.authors.first().map(|s| s.as_str()).unwrap_or(""), 13);

      let row_height = content_height + 1;

      Row::new(vec![
        feed_cell(
          item.signal.indicator(),
          if is_selected { selected_text_style } else { signal_style },
          is_selected,
        ),
        feed_cell(
          &feed_source_label(item),
          if is_selected {
            selected_text_style
          } else {
            Style::default().fg(t.accent)
          },
          is_selected,
        ),
        feed_cell(
          item.content_type.short_label(),
          if is_selected {
            selected_dim_style
          } else {
            Style::default().fg(t.text_dim)
          },
          is_selected,
        ),
        Cell::from(Text::from({
          let title_style = if is_selected {
            selected_text_style
          } else {
            Style::default()
          };
          let mut lines: Vec<Line<'static>> = title_lines
            .iter()
            .map(|s| Line::from(Span::styled(s.clone(), title_style)))
            .collect();
          lines.push(feed_spacer_line(is_selected));
          lines
        })),
        feed_cell(
          &author,
          if is_selected {
            selected_dim_style
          } else {
            Style::default().fg(t.text_dim)
          },
          is_selected,
        ),
        feed_cell(
          item.published_at.as_str(),
          if is_selected {
            selected_dim_style
          } else {
            Style::default().fg(t.text_dim)
          },
          is_selected,
        ),
        feed_cell(
          item.workflow_state.short_label(),
          if is_selected {
            selected_dim_style
          } else {
            Style::default().fg(t.text_dim)
          },
          is_selected,
        ),
      ])
      .style(row_style)
      .height(row_height)
    })
    .collect();
  log::debug!(
    "rows build ({} window items): {}ms",
    window.len(),
    t_rows.elapsed().as_millis()
  );

  let table = Table::new(
    rows,
    [
      Constraint::Length(1),
      Constraint::Length(7),
      Constraint::Length(5),
      Constraint::Min(0),
      Constraint::Length(14),
      Constraint::Length(10),
      Constraint::Length(8),
    ],
  )
  .header(header)
  .column_spacing(1)
  .row_highlight_style(Style::default());

  let t_render = std::time::Instant::now();
  frame.render_widget(table, inner);
  log::debug!(
    "frame.render_widget(table): {}ms",
    t_render.elapsed().as_millis()
  );

  // Scrollbar uses item indices for proportions — no full-list row count needed.
  if total_items > 0 {
    let mut scrollbar_state = ScrollbarState::new(total_items)
      .position(start)
      .viewport_content_length(viewport_rows);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
      .begin_symbol(None)
      .end_symbol(None);
    frame.render_stateful_widget(scrollbar, inner, &mut scrollbar_state);
  }
  log::debug!(
    "draw_item_table total: {}ms ({} total items, {} in window)",
    t_item_table.elapsed().as_millis(),
    total_items,
    window.len()
  );
}

pub(super) fn feed_source_label(item: &crate::models::FeedItem) -> String {
  match item.source_platform {
    SourcePlatform::HuggingFace => "hf".to_string(),
    SourcePlatform::ArXiv => "arxiv".to_string(),
    SourcePlatform::Rss if !item.source_name.is_empty() => {
      truncate(&item.source_name, 7)
    }
    _ if !item.source_name.is_empty() => truncate(&item.source_name, 7),
    _ => item.source_platform.short_label().to_string(),
  }
}

fn feed_header_cell(label: &'static str, style: Style) -> Cell<'static> {
  Cell::from(Text::from(vec![
    Line::from(Span::styled(label, style)),
    Line::from(""),
  ]))
}

fn feed_cell(value: &str, style: Style, selected: bool) -> Cell<'static> {
  let mut lines = Vec::new();
  lines.push(Line::from(Span::styled(value.to_string(), style)));
  lines.push(feed_spacer_line(selected));
  Cell::from(Text::from(lines))
}

fn feed_spacer_line(selected: bool) -> Line<'static> {
  if selected {
    Line::from(Span::styled(" ", Style::default()))
  } else {
    Line::from("")
  }
}


// ── Helpers ────────────────────────────────────────────────────────────────

/// Count how many items (starting from `list_offset`) fit in `viewport_rows`
/// screen rows, including one spacer row between feed items.
fn count_visible_items_from_app(
  app: &App,
  list_offset: usize,
  viewport_rows: usize,
  title_wrap_w: usize,
) -> usize {
  let mut rows_used = 0usize;
  let mut count = 0usize;
  for idx in list_offset..app.visible_count() {
    let Some(item) = app.visible_get(idx) else { break };
    let item_height = if item.title.len() > title_wrap_w { 3 } else { 2 };
    if rows_used + item_height > viewport_rows {
      break;
    }
    rows_used += item_height;
    count += 1;
  }
  count.max(1)
}

