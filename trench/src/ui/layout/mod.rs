use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Direction, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::Paragraph,
};

use super::repo_viewer::draw_repo_viewer;
use crate::app::{App, AppView, FeedTab, FocusedReader};
use crate::models::{
  ContentType, SignalLevel, WorkflowState,
};
use std::collections::HashSet;

mod details;
mod feed;
mod filter;
mod footer;
mod modals;
mod notes;
mod popups;
mod reader;
mod widgets;

pub use popups::HELP_SECTION_COUNT;
use details::draw_details_panel;
use feed::draw_feed_pane;
use filter::draw_filter_panel;
use footer::draw_footer;
use modals::{draw_settings, draw_sources_popup, draw_theme_picker};
use notes::{draw_note_dock, draw_notes_surface};
use reader::{
  chat_context_line, draw_narrow_feed_details_popup, draw_reader_bottom_pane,
  draw_reader_popup, draw_reader_tab_bar, draw_reader_workspace_header, reader_workspace_split,
};
use popups::{
  draw_abstract_popup, draw_help_overlay, draw_quit_popup, draw_tag_picker,
};
use widgets::{
  draw_horiz_split_box, draw_vert_split_box, h_margin,
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


