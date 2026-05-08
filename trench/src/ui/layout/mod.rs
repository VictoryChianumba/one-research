use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Direction, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span, Text},
  widgets::{
    Block, Borders, Cell, Clear, Paragraph, Row, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Table, Wrap,
  },
};

use super::repo_viewer::draw_repo_viewer;
use crate::app::{
  App, AppView, FeedTab, FocusedReader, NotesMode, NotesTab, PaneId, ReaderTab,
};
use crate::models::{
  ContentType, FeedItem, SignalLevel, SourcePlatform, WorkflowState,
};
use std::collections::HashSet;

mod footer;
mod modals;
mod popups;
mod widgets;

pub use popups::HELP_SECTION_COUNT;
use footer::draw_footer;
use modals::{draw_settings, draw_sources_popup, draw_theme_picker};
use popups::{
  draw_abstract_popup, draw_help_overlay, draw_quit_popup, draw_tag_picker,
};
use widgets::{
  draw_horiz_split_box, draw_vert_split_box, h_margin, popup_inner,
  popup_rect, quiet_popup_block, safe_truncate_chars, truncate, truncate_str,
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

fn chat_context_line(app: &App) -> Option<String> {
  if app.reader_active {
    let title = match app.focused_reader {
      FocusedReader::Secondary if app.reader_dual_active => app
        .reader_secondary_tabs
        .get(app.reader_secondary_active_tab)
        .map(|tab| tab.title.as_str()),
      _ => {
        app.reader_tabs.get(app.reader_active_tab).map(|tab| tab.title.as_str())
      }
    };
    if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
      return Some(format!("active reader · {}", truncate(title, 96)));
    }
  }

  app.selected_item().map(|item| {
    let source = if item.source_name.is_empty() {
      item.source_platform.short_label()
    } else {
      item.source_name.as_str()
    };
    format!("selected item · {} · {}", source, truncate(&item.title, 96))
  })
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

fn reader_workspace_split(area: Rect) -> (Rect, Rect) {
  if area.height <= 1 {
    return (area, Rect { x: area.x, y: area.y, width: area.width, height: 0 });
  }
  let rows =
    Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
  (rows[0], rows[1])
}

fn reader_tab_title(tabs: &[ReaderTab], active: usize) -> &str {
  tabs.get(active).map(|tab| tab.title.as_str()).unwrap_or("no paper")
}

fn draw_reader_workspace_header(
  frame: &mut Frame,
  app: &App,
  area: Rect,
  label: &str,
) {
  if area.height == 0 || area.width == 0 {
    return;
  }
  if label == "Dual Reader" {
    draw_dual_reader_workspace_header(frame, app, area);
    return;
  }
  let t = app.theme();
  let primary = reader_tab_title(&app.reader_tabs, app.reader_active_tab);
  let context = primary.to_string();
  let label_style =
    Style::default().fg(t.accent).bg(t.bg_panel).add_modifier(Modifier::BOLD);
  let dim_style = Style::default().fg(t.text_dim).bg(t.bg_panel);
  let text_style = Style::default().fg(t.text).bg(t.bg_panel);
  let prefix = format!(" {label} ");
  let sep = "· ";
  let max_context = (area.width as usize)
    .saturating_sub(prefix.chars().count() + sep.chars().count() + 1);
  let context = truncate(&context, max_context);
  let line = Line::from(vec![
    Span::styled(prefix, label_style),
    Span::styled(sep, dim_style),
    Span::styled(context, text_style),
  ]);
  frame.render_widget(
    Paragraph::new(line).style(Style::default().bg(t.bg_panel)),
    area,
  );
}

fn draw_dual_reader_workspace_header(frame: &mut Frame, app: &App, area: Rect) {
  let t = app.theme();
  let label_w = 14.min(area.width);
  let label_rect = Rect { x: area.x, y: area.y, width: label_w, height: 1 };
  let title_area = Rect {
    x: area.x + label_w,
    y: area.y,
    width: area.width.saturating_sub(label_w),
    height: 1,
  };
  let label_style =
    Style::default().fg(t.accent).bg(t.bg_panel).add_modifier(Modifier::BOLD);
  frame.render_widget(
    Paragraph::new(Span::styled(" Dual Reader ", label_style))
      .style(Style::default().bg(t.bg_panel)),
    label_rect,
  );

  let halves = Layout::horizontal([
    Constraint::Percentage(50),
    Constraint::Percentage(50),
  ])
  .split(title_area);
  draw_reader_header_title(
    frame,
    halves[0],
    "primary",
    reader_tab_title(&app.reader_tabs, app.reader_active_tab),
    &t,
  );
  draw_reader_header_title(
    frame,
    halves[1],
    "secondary",
    reader_tab_title(
      &app.reader_secondary_tabs,
      app.reader_secondary_active_tab,
    ),
    &t,
  );
}

fn draw_reader_header_title(
  frame: &mut Frame,
  area: Rect,
  label: &str,
  title: &str,
  t: &crate::theme::Theme,
) {
  if area.width == 0 {
    return;
  }
  let prefix = format!("{label}: ");
  let title_w =
    (area.width as usize).saturating_sub(prefix.chars().count() + 2);
  let line = Line::from(vec![
    Span::styled(" · ", Style::default().fg(t.text_dim).bg(t.bg_panel)),
    Span::styled(prefix, Style::default().fg(t.text_dim).bg(t.bg_panel)),
    Span::styled(
      truncate(title, title_w),
      Style::default().fg(t.text).bg(t.bg_panel),
    ),
  ]);
  frame.render_widget(
    Paragraph::new(line).style(Style::default().bg(t.bg_panel)),
    area,
  );
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


fn note_pane_for_side(side: FocusedReader) -> PaneId {
  match side {
    FocusedReader::Primary => PaneId::Notes,
    FocusedReader::Secondary => PaneId::SecondaryNotes,
  }
}

fn notes_browser_visible<'a>(
  app: &'a App,
  side: FocusedReader,
) -> Vec<&'a notes::Note> {
  let Some(notes_app) = app.notes_app.as_ref() else {
    return Vec::new();
  };
  match app.notes_mode_for_side(side) {
    NotesMode::Capture => Vec::new(),
    NotesMode::Library => notes_app.get_active_notes().collect(),
    NotesMode::PaperNotes => {
      let Some(context) = app.notes_context_for_side(side) else {
        return Vec::new();
      };
      notes_app
        .get_active_notes()
        .filter(|note| {
          note.linked_papers.iter().any(|paper| paper.id == context.paper.id)
        })
        .collect()
    }
  }
}

fn notes_browser_selected_index(
  app: &App,
  visible: &[&notes::Note],
) -> Option<usize> {
  let current_id = app.notes_app.as_ref()?.current_note_id.as_ref()?;
  visible.iter().position(|note| &note.note_id == current_id)
}

fn notes_browser_selected_note<'a>(
  app: &App,
  visible: &[&'a notes::Note],
) -> Option<&'a notes::Note> {
  let idx = notes_browser_selected_index(app, visible)?;
  visible.get(idx).copied()
}

fn draw_notes_mode_switcher(
  frame: &mut Frame,
  area: Rect,
  mode: NotesMode,
  focused: bool,
  t: &crate::theme::Theme,
) {
  let ordinary = Style::default().fg(t.text_dim);
  let active = if focused {
    Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
  } else {
    Style::default().fg(t.header).add_modifier(Modifier::BOLD)
  };
  let mut spans = Vec::new();
  for (idx, candidate) in
    [NotesMode::PaperNotes, NotesMode::Library, NotesMode::Capture]
      .into_iter()
      .enumerate()
  {
    if idx > 0 {
      spans.push(Span::styled("  ·  ", ordinary));
    }
    let label = match candidate {
      NotesMode::PaperNotes => "Paper Notes",
      NotesMode::Library => "Library",
      NotesMode::Capture => "Capture",
    };
    spans.push(Span::styled(
      label,
      if candidate == mode { active } else { ordinary },
    ));
  }
  frame.render_widget(
    Paragraph::new(Line::from(spans)).style(Style::default().fg(t.text_dim)),
    area,
  );
}

fn build_notes_summary_line(
  app: &App,
  side: FocusedReader,
  width: u16,
  t: &crate::theme::Theme,
) -> Line<'static> {
  let mode = app.notes_mode_for_side(side);
  let context = app.notes_context_for_side(side);
  // Compute visible once; previously notes_browser_visible ran twice here
  // (once for count, once inside notes_browser_selected_note) — audit
  // Perf CRIT C2.
  let visible = notes_browser_visible(app, side);
  let visible_count = visible.len();
  let selected_note = notes_browser_selected_note(app, &visible);
  let summary = match mode {
    NotesMode::Library => {
      if let Some(note) = selected_note {
        format!(
          "{} notes  ·  selected {}",
          visible_count,
          truncate(&note.title, width.saturating_sub(24) as usize)
        )
      } else {
        format!("{visible_count} notes in library")
      }
    }
    NotesMode::PaperNotes => {
      if let Some(ctx) = context {
        format!(
          "{}  ·  {}  ·  {} linked",
          truncate(&ctx.paper.title, width.saturating_sub(28) as usize),
          ctx.source_label,
          visible_count
        )
      } else {
        "No paper context".to_string()
      }
    }
    NotesMode::Capture => {
      if let Some(ctx) = context {
        format!(
          "{}  ·  {}  ·  ready to capture",
          truncate(&ctx.paper.title, width.saturating_sub(36) as usize),
          ctx.source_label
        )
      } else {
        "No paper context".to_string()
      }
    }
  };
  Line::from(Span::styled(
    truncate(&summary, width as usize),
    Style::default().fg(t.text_dim),
  ))
}

fn draw_notes_empty_state(
  frame: &mut Frame,
  area: Rect,
  mode: NotesMode,
  t: &crate::theme::Theme,
) {
  let lines = match mode {
    NotesMode::PaperNotes => vec![
      Line::from(Span::styled(
        "No notes linked to this paper.",
        Style::default().fg(t.text).add_modifier(Modifier::BOLD),
      )),
      Line::from(""),
      Line::from(Span::styled(
        "Press ] to switch to Capture, then n or Enter to create one.",
        Style::default().fg(t.text_dim),
      )),
    ],
    NotesMode::Library => vec![
      Line::from(Span::styled(
        "No notes yet.",
        Style::default().fg(t.text).add_modifier(Modifier::BOLD),
      )),
      Line::from(""),
      Line::from(Span::styled(
        "Press n to create a note, or open notes from a paper with Ldr+n.",
        Style::default().fg(t.text_dim),
      )),
    ],
    NotesMode::Capture => vec![
      Line::from(Span::styled(
        "Capture a linked note.",
        Style::default().fg(t.text).add_modifier(Modifier::BOLD),
      )),
      Line::from(""),
      Line::from(Span::styled(
        "Press n or Enter to open the prefilled composer.",
        Style::default().fg(t.text_dim),
      )),
    ],
  };
  let chunks = Layout::vertical([
    Constraint::Percentage(24),
    Constraint::Length((lines.len() as u16).saturating_add(1)),
    Constraint::Min(0),
  ])
  .split(area);
  frame.render_widget(
    Paragraph::new(lines)
      .alignment(Alignment::Center)
      .wrap(Wrap { trim: false })
      .style(Style::default().fg(t.text_dim)),
    chunks[1],
  );
}

fn note_list_meta_summary(note: &notes::Note) -> String {
  let mut parts = Vec::new();
  if !note.tags.is_empty() {
    let noun = if note.tags.len() == 1 { "tag" } else { "tags" };
    parts.push(format!("{} {noun}", note.tags.len()));
  }
  let noun = if note.linked_papers.len() == 1 { "paper" } else { "papers" };
  parts.push(format!("{} {noun}", note.linked_papers.len()));
  parts.push(note.updated_at.format("%Y-%m-%d").to_string());
  parts.join("  ·  ")
}

fn note_preview_meta_summary(note: &notes::Note) -> String {
  let mut parts = Vec::new();
  let noun = if note.linked_papers.len() == 1 { "paper" } else { "papers" };
  parts.push(format!("{} {noun}", note.linked_papers.len()));
  parts.push(note.updated_at.format("%Y-%m-%d").to_string());
  if !note.tags.is_empty() {
    parts
      .push(note.tags.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
  }
  parts.join("  ·  ")
}

fn draw_notes_browser_list(
  frame: &mut Frame,
  area: Rect,
  notes: &[&notes::Note],
  selected_idx: Option<usize>,
  focused: bool,
  t: &crate::theme::Theme,
) {
  if area.height == 0 || area.width == 0 {
    return;
  }
  if notes.is_empty() {
    draw_notes_empty_state(frame, area, NotesMode::Library, t);
    return;
  }

  let slots = (area.height as usize / 2).max(1);
  let selected = selected_idx.unwrap_or(0).min(notes.len().saturating_sub(1));
  let start =
    selected.saturating_sub(slots / 2).min(notes.len().saturating_sub(slots));
  let end = (start + slots).min(notes.len());
  let selection_style = if focused {
    Style::default().bg(t.bg_selection).fg(t.text)
  } else {
    Style::default().bg(t.bg_panel).fg(t.text)
  };

  let mut y = area.y;
  for (idx, note) in notes[start..end].iter().enumerate() {
    let note_index = start + idx;
    let row_area = Rect {
      x: area.x,
      y,
      width: area.width,
      height: 2.min(area.y + area.height - y),
    };
    let is_selected = note_index == selected;
    let title = truncate(&note.title, area.width.saturating_sub(3) as usize);
    let meta = truncate(
      &note_list_meta_summary(note),
      area.width.saturating_sub(3) as usize,
    );
    let lines = vec![
      Line::from(vec![
        Span::styled(
          if is_selected { "› " } else { "  " },
          if is_selected {
            selection_style
          } else {
            Style::default().fg(t.text_dim)
          },
        ),
        Span::styled(
          title,
          if is_selected {
            selection_style.add_modifier(Modifier::BOLD)
          } else {
            Style::default().fg(t.text)
          },
        ),
      ]),
      Line::from(vec![
        Span::styled(
          "  ",
          if is_selected { selection_style } else { Style::default() },
        ),
        Span::styled(
          meta,
          if is_selected {
            selection_style.remove_modifier(Modifier::BOLD)
          } else {
            Style::default().fg(t.text_dim)
          },
        ),
      ]),
    ];
    frame.render_widget(
      Paragraph::new(lines)
        .style(if is_selected { selection_style } else { Style::default() })
        .wrap(Wrap { trim: false }),
      row_area,
    );
    y = y.saturating_add(2);
    if y >= area.y + area.height {
      break;
    }
  }
}

fn draw_notes_browser_preview(
  frame: &mut Frame,
  area: Rect,
  note: Option<&notes::Note>,
  t: &crate::theme::Theme,
) {
  let inner = Rect {
    x: area.x.saturating_add(1),
    y: area.y,
    width: area.width.saturating_sub(1),
    height: area.height,
  };
  let Some(note) = note else {
    frame.render_widget(
      Paragraph::new("No note selected")
        .style(Style::default().fg(t.text_dim))
        .alignment(Alignment::Center),
      inner,
    );
    return;
  };
  let mut lines = vec![
    Line::from(""),
    Line::from(Span::styled(
      truncate(&note.title, inner.width as usize),
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    )),
  ];
  lines.push(Line::from(Span::styled(
    truncate(&note_preview_meta_summary(note), inner.width as usize),
    Style::default().fg(t.text_dim),
  )));
  lines.push(Line::from(""));
  if note.content.trim().is_empty() {
    lines.push(Line::from(Span::styled(
      "Empty note",
      Style::default().fg(t.text_dim),
    )));
  } else {
    for line in textwrap::wrap(&note.content, area.width as usize).into_iter() {
      lines.push(Line::from(Span::styled(
        line.into_owned(),
        Style::default().fg(t.text),
      )));
    }
  }
  frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_notes_surface(
  frame: &mut Frame,
  app: &mut App,
  area: Rect,
  side: FocusedReader,
  preview_when_unfocused: bool,
  theme: &crate::theme::Theme,
) {
  if area.height == 0 || area.width == 0 {
    return;
  }
  let is_focused = app.focused_pane == note_pane_for_side(side);
  // Inline the field access so Rust's split-borrow rules can keep the
  // `tabs` borrow disjoint from the later `app.notes_app.as_mut()` —
  // saves a per-draw `Vec<NotesTab>` clone.
  let (tabs, active) = match side {
    FocusedReader::Primary => (&app.notes_tabs, app.notes_active_tab),
    FocusedReader::Secondary => {
      (&app.secondary_notes_tabs, app.secondary_notes_active_tab)
    }
  };
  let show_tabs = tabs.len() > 1;
  let rows = Layout::vertical(if show_tabs {
    vec![
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Min(0),
    ]
  } else {
    vec![
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Min(0),
    ]
  })
  .split(area);
  draw_note_dock_rule(
    frame,
    rows[0],
    app.notes_mode_for_side(side).title(),
    is_focused,
    theme,
  );
  draw_notes_mode_switcher(
    frame,
    rows[1],
    app.notes_mode_for_side(side),
    is_focused,
    theme,
  );
  frame.render_widget(
    Paragraph::new(build_notes_summary_line(app, side, rows[2].width, theme))
      .wrap(Wrap { trim: false }),
    rows[2],
  );
  let content_row = if show_tabs {
    draw_notes_tab_bar(frame, rows[3], &tabs, active, is_focused, theme);
    4
  } else {
    3
  };
  let content_area = rows[content_row];

  let editor_active = app.notes_app.as_ref().is_some_and(|notes_app| {
    notes_app.notes_state == notes::app::NotesState::Editor
  });
  let popup_active = app
    .notes_app
    .as_ref()
    .is_some_and(|notes_app| !notes_app.active_popup.is_none());

  if editor_active {
    if let Some(notes_app) = app.notes_app.as_mut() {
      notes_app.draw_editor_surface(frame, content_area);
      if popup_active {
        notes_app.draw_popup_overlay(frame, content_area);
      }
    }
    return;
  }

  if preview_when_unfocused && !is_focused {
    draw_note_preview(frame, app, content_area, &tabs, active, theme);
    if popup_active {
      if let Some(notes_app) = app.notes_app.as_mut() {
        notes_app.draw_popup_overlay(frame, content_area);
      }
    }
    return;
  }

  match app.notes_mode_for_side(side) {
    NotesMode::Capture => {
      draw_notes_empty_state(frame, content_area, NotesMode::Capture, theme);
    }
    NotesMode::PaperNotes | NotesMode::Library => {
      let visible = notes_browser_visible(app, side);
      if visible.is_empty() {
        draw_notes_empty_state(
          frame,
          content_area,
          app.notes_mode_for_side(side),
          theme,
        );
      } else if content_area.width >= 72 {
        let chunks = Layout::horizontal([
          Constraint::Percentage(44),
          Constraint::Length(1),
          Constraint::Percentage(56),
        ])
        .split(content_area);
        draw_notes_browser_list(
          frame,
          chunks[0],
          &visible,
          notes_browser_selected_index(app, &visible),
          is_focused,
          theme,
        );
        frame.render_widget(
          Paragraph::new("│").style(Style::default().fg(theme.border)),
          chunks[1],
        );
        draw_notes_browser_preview(
          frame,
          chunks[2],
          notes_browser_selected_note(app, &visible),
          theme,
        );
      } else {
        let chunks = Layout::vertical([
          Constraint::Percentage(48),
          Constraint::Length(1),
          Constraint::Percentage(52),
        ])
        .split(content_area);
        draw_notes_browser_list(
          frame,
          chunks[0],
          &visible,
          notes_browser_selected_index(app, &visible),
          is_focused,
          theme,
        );
        frame.render_widget(
          Paragraph::new("─".repeat(chunks[1].width as usize))
            .style(Style::default().fg(theme.border)),
          chunks[1],
        );
        draw_notes_browser_preview(
          frame,
          chunks[2],
          notes_browser_selected_note(app, &visible),
          theme,
        );
      }
    }
  }

  if popup_active {
    if let Some(notes_app) = app.notes_app.as_mut() {
      notes_app.draw_popup_overlay(frame, content_area);
    }
  }
}

fn draw_note_dock(
  frame: &mut Frame,
  app: &mut App,
  area: Rect,
  side: FocusedReader,
  theme: &crate::theme::Theme,
) {
  draw_notes_surface(frame, app, area, side, true, theme);
}

fn draw_note_dock_rule(
  frame: &mut Frame,
  area: Rect,
  title: &str,
  focused: bool,
  t: &crate::theme::Theme,
) {
  let w = area.width as usize;
  let style = if focused {
    Style::default().fg(t.border_active)
  } else {
    Style::default().fg(t.border)
  };
  let title_style = if focused {
    Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
  } else {
    Style::default().fg(t.header).add_modifier(Modifier::BOLD)
  };
  let left = "── ";
  let right = " ";
  let fill =
    "─".repeat(w.saturating_sub(left.len() + title.len() + right.len()));
  let line = Line::from(vec![
    Span::styled(left, style),
    Span::styled(title.to_string(), title_style),
    Span::styled(format!("{right}{fill}"), style),
  ]);
  frame.render_widget(Paragraph::new(line), area);
}

fn draw_note_preview(
  frame: &mut Frame,
  app: &App,
  area: Rect,
  tabs: &[NotesTab],
  active: usize,
  t: &crate::theme::Theme,
) {
  let selected = app
    .notes_app
    .as_ref()
    .and_then(|notes_app| notes_app.get_current_note())
    .or_else(|| {
      tabs.get(active).and_then(|tab| {
        app.notes_app.as_ref().and_then(|na| na.get_note(&tab.note_id))
      })
    });
  draw_notes_browser_preview(frame, area, selected, t);
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

fn draw_reader_tab_bar(
  frame: &mut Frame,
  area: Rect,
  tabs: &[ReaderTab],
  active: usize,
  focused: bool,
  t: &crate::theme::Theme,
) {
  if tabs.is_empty() {
    return;
  }
  let max_title =
    (area.width as usize).saturating_sub(4).max(8) / tabs.len().max(1);
  let spans: Vec<Span> = tabs
    .iter()
    .enumerate()
    .flat_map(|(i, tab)| {
      let title: String =
        tab.title.chars().take(max_title.saturating_sub(5)).collect();
      let label = format!("[{}: {}]", i + 1, title);
      let style = if i == active {
        if focused {
          Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
          Style::default().fg(t.text).add_modifier(Modifier::BOLD)
        }
      } else {
        Style::default().fg(t.text_dim)
      };
      let sep = Span::raw("  ");
      vec![Span::styled(label, style), sep]
    })
    .collect();
  frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_notes_tab_bar(
  frame: &mut Frame,
  area: Rect,
  tabs: &[NotesTab],
  active: usize,
  focused: bool,
  t: &crate::theme::Theme,
) {
  if tabs.is_empty() {
    return;
  }
  let max_title =
    (area.width as usize).saturating_sub(4).max(8) / tabs.len().max(1);
  let spans: Vec<Span> = tabs
    .iter()
    .enumerate()
    .flat_map(|(i, tab)| {
      let title: String =
        tab.title.chars().take(max_title.saturating_sub(5)).collect();
      let label = format!("[{}: {}]", i + 1, title);
      let style = if i == active {
        if focused {
          Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
          Style::default().fg(t.text).add_modifier(Modifier::BOLD)
        }
      } else {
        Style::default().fg(t.text_dim)
      };
      vec![Span::styled(label, style), Span::raw("  ")]
    })
    .collect();
  frame.render_widget(Paragraph::new(Line::from(spans)), area);
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

fn feed_source_label(item: &crate::models::FeedItem) -> String {
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

fn draw_filter_panel(frame: &mut Frame, app: &App, area: Rect) {
  let t = app.theme();
  let inner = area;
  let focused = app.filter_focus;

  let f = &app.active_filters;
  let c = app.filter_cursor;
  let mut s: usize = 0;
  let mut lines: Vec<Line> = Vec::new();
  let mut cursor_line: usize = 0;

  let hrule = "\u{2500}".repeat(inner.width as usize);

  lines.push(filter_header("Source", &t));
  for name in app.filter_source_names() {
    let active = f.sources.contains(&name);
    let cursor = focused && s == c;
    if cursor {
      cursor_line = lines.len();
    }
    let checkbox = if active { "[x]" } else { "[ ]" };
    let line = if cursor {
      let hl = t.style_selection_text();
      Line::from(vec![
        Span::styled("  ", hl),
        Span::styled(checkbox, hl),
        Span::styled(" ", hl),
        Span::styled(name, hl),
      ])
    } else if active {
      Line::from(vec![
        Span::raw("  "),
        Span::styled(checkbox, Style::default().fg(t.text)),
        Span::raw(" "),
        Span::styled(name, Style::default().fg(t.text)),
      ])
    } else {
      Line::from(vec![
        Span::raw("  "),
        Span::styled(checkbox, Style::default().fg(t.text_dim)),
        Span::raw(" "),
        Span::raw(name),
      ])
    };
    lines.push(line);
    s += 1;
  }
  lines.push(Line::from(""));

  lines.push(filter_header("Signal", &t));
  if focused && s == c {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "primary",
    f.signals.contains(&SignalLevel::Primary),
    focused && s == c,
    &t,
  ));
  s += 1;
  if focused && s == c {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "secondary",
    f.signals.contains(&SignalLevel::Secondary),
    focused && s == c,
    &t,
  ));
  s += 1;
  if focused && s == c {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "tertiary",
    f.signals.contains(&SignalLevel::Tertiary),
    focused && s == c,
    &t,
  ));
  s += 1;
  lines.push(Line::from(""));

  lines.push(filter_header("Type", &t));
  if focused && s == c {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "paper",
    f.content_types.contains(&ContentType::Paper),
    focused && s == c,
    &t,
  ));
  s += 1;
  if focused && s == c {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "article",
    f.content_types.contains(&ContentType::Article),
    focused && s == c,
    &t,
  ));
  s += 1;
  if focused && s == c {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "digest",
    f.content_types.contains(&ContentType::Digest),
    focused && s == c,
    &t,
  ));
  s += 1;
  lines.push(Line::from(""));

  // Workflow state filtering moved to the Library tab chips — the panel only
  // covers source / signal / content_type / tags now.

  let tag_names = crate::tags::all_tags(&app.item_tags);
  if !tag_names.is_empty() {
    lines.push(filter_header("Tags", &t));
    for name in tag_names {
      let active = f.tags.contains(&name);
      let cursor = focused && s == c;
      if cursor {
        cursor_line = lines.len();
      }
      lines.push(filter_row_owned(name, active, cursor, &t));
      s += 1;
    }
    lines.push(Line::from(""));
  }

  lines.push(Line::from(Span::styled(hrule, Style::default().fg(t.border))));

  let clear_hl = focused && s == c;
  if clear_hl {
    cursor_line = lines.len();
  }
  let clear_style = if clear_hl {
    t.style_selection_text()
  } else {
    Style::default().fg(t.text_dim)
  };
  lines
    .push(Line::from(Span::styled("[c] clear all".to_string(), clear_style)));

  let total_lines = lines.len();
  let visible_height = inner.height as usize;

  let scroll_offset = if cursor_line < visible_height {
    0
  } else {
    cursor_line.saturating_sub(visible_height.saturating_sub(2))
  };

  let para = Paragraph::new(lines).scroll((scroll_offset as u16, 0));
  frame.render_widget(para, inner);

  if total_lines > visible_height {
    let mut sb_state = ScrollbarState::new(total_lines)
      .position(scroll_offset)
      .viewport_content_length(visible_height);
    let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
      .begin_symbol(None)
      .end_symbol(None);
    frame.render_stateful_widget(sb, inner, &mut sb_state);
  }
}

fn draw_details_panel(frame: &mut Frame, app: &mut App, area: Rect) {
  let t = app.theme();
  let t_details = std::time::Instant::now();

  let bottom_h = area.height * 38 / 100;
  let top_h = area.height.saturating_sub(bottom_h + 1);
  let div_y = area.y + top_h;

  let top_area = Rect { height: top_h, ..area };
  let bottom_area = Rect { y: div_y + 1, height: bottom_h, ..area };

  // Divider between selected-paper detail and the activity summary.
  let sb = Style::default().fg(t.border);
  let activity_title = " Activity ";
  let activity_w = activity_title.chars().count();
  let left_rule_w = (area.width as usize).saturating_sub(activity_w) / 2;
  let right_rule_w =
    (area.width as usize).saturating_sub(activity_w + left_rule_w);
  frame.render_widget(
    Paragraph::new(Line::from(vec![
      Span::styled("─".repeat(left_rule_w), sb),
      Span::styled(
        activity_title,
        Style::default().fg(t.header).add_modifier(Modifier::BOLD),
      ),
      Span::styled("─".repeat(right_rule_w), sb),
    ])),
    Rect { x: area.x, y: div_y, width: area.width, height: 1 },
  );
  frame.render_widget(
    Paragraph::new(Span::styled("├", sb)),
    Rect { x: area.x.saturating_sub(1), y: div_y, width: 1, height: 1 },
  );
  frame.render_widget(
    Paragraph::new(Span::styled("┤", sb)),
    Rect { x: area.x + area.width, y: div_y, width: 1, height: 1 },
  );

  // ── Dashboard (bottom pane) ───────────────────────────────────────────────
  {
    let dash_inner = Rect {
      x: bottom_area.x + 1,
      width: bottom_area.width.saturating_sub(2),
      ..bottom_area
    };
    let w = dash_inner.width as usize;

    // Single memoized read replaces 4 workflow-state scans, the queue-titles
    // scan, and the recent-48h fused pass that previously ran on every draw.
    let counts = app.item_counts();
    let queued = counts.queued;
    let read = counts.deep_read;
    let archived = counts.archived;
    let total = counts.total;
    let recent_count = counts.recent_total;
    let today_count = counts.recent_today;
    let recent_hf = counts.recent_hf;
    let recent_arxiv = counts.recent_arxiv;
    let recent_other = counts.recent_other;

    let activity_label_style =
      Style::default().fg(t.text_dim).add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(t.text_dim);
    let val_style = Style::default().fg(t.text);

    // Continue Reading
    let continue_title =
      app.last_read.as_deref().unwrap_or("─ nothing opened yet ─");
    let continue_source = app.last_read_source.as_deref().unwrap_or("");

    let label_w = 11;
    let value_w = w.saturating_sub(label_w).max(1);
    let queue_summary =
      format!("{queued} queued item{}", if queued == 1 { "" } else { "s" });
    let fresh_summary = format!("{today_count} today   {recent_count} in 48h");
    let source_summary =
      format!("HF {recent_hf}   arXiv {recent_arxiv}   Other {recent_other}");

    let mut lines: Vec<Line> = vec![Line::from("")];
    push_activity_wrapped(
      &mut lines,
      "Last read",
      continue_title,
      activity_label_style,
      val_style,
      value_w,
      2,
    );
    if !continue_source.is_empty() {
      push_activity_continuation(
        &mut lines,
        continue_source,
        label_style,
        value_w,
        1,
      );
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
      Span::styled("Queue      ", activity_label_style),
      Span::styled(queue_summary, val_style),
    ]));
    if counts.queue_preview.is_empty() {
      lines.push(Line::from(vec![
        Span::styled("           ", label_style),
        Span::styled("─ empty ─", label_style),
      ]));
    } else {
      for title in counts.queue_preview.iter() {
        push_activity_continuation(&mut lines, title, val_style, value_w, 2);
      }
    }
    lines.extend([
      Line::from(""),
      Line::from(vec![
        Span::styled("Fresh      ", activity_label_style),
        Span::styled(truncate(&fresh_summary, value_w), val_style),
      ]),
      Line::from(vec![
        Span::styled("           ", label_style),
        Span::styled(truncate(&source_summary, value_w), label_style),
      ]),
      Line::from(""),
      Line::from(vec![
        Span::styled("Library    ", activity_label_style),
        Span::styled(
          truncate(&format!("Queue {queued}   Read {read}"), value_w),
          val_style,
        ),
      ]),
      Line::from(vec![
        Span::styled("           ", label_style),
        Span::styled(
          truncate(&format!("Archived {archived}   Total {total}"), value_w),
          val_style,
        ),
      ]),
    ]);

    frame.render_widget(Paragraph::new(lines), dash_inner);
  }

  // Add margin so text doesn't abut the divider.
  let inner = Rect {
    x: top_area.x + 2,
    y: top_area.y.saturating_add(1),
    width: top_area.width.saturating_sub(3),
    height: top_area.height.saturating_sub(1),
    ..top_area
  };

  // Reset scroll when the selected item changes, before borrowing item data.
  // The filtered_history call is scoped so the borrow drops before the
  // mutable access below; we'll re-borrow for the render path.
  {
    let history = app.filtered_history();
    let current_key = details_subject_key(app, &history);
    if current_key != app.details_last_item_url {
      // Note: actual mutation happens after the scope ends; capture state.
      drop(history);
      app.details_scroll = 0;
      app.details_last_item_url = current_key;
    }
  }

  // Re-borrow filtered_history for the render path. The cache memoizes
  // on App so this is cheap; without the memo this filter would run
  // multiple times per History-tab frame (draw_history_tab +
  // details_subject + details_subject_key).
  let history = app.filtered_history();
  if let Some(subject) = details_subject(app, &history) {
    let title_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
    let meta_style = Style::default().fg(t.text_dim);
    let label_style =
      Style::default().fg(t.text_dim).add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(t.text_dim);
    let value_style = Style::default().fg(t.text);
    let accent_style = Style::default().fg(t.accent);
    let detail_w = inner.width.max(1) as usize;
    let mut lines = render_details_subject(
      subject,
      app,
      DetailStyles {
        title_style,
        header_style: Style::default()
          .fg(t.header)
          .add_modifier(Modifier::BOLD),
        meta_style,
        label_style,
        dim_style,
        value_style,
        accent_style,
        success_style: Style::default().fg(t.success),
      },
      detail_w,
      inner.width as usize,
      inner.height as usize,
    );

    if lines.len() > inner.height as usize {
      lines.truncate(inner.height as usize);
    }

    let para = Paragraph::new(lines);
    let t_para = std::time::Instant::now();
    frame.render_widget(para, inner);
    app.set_details_max_scroll(0);
    log::debug!("details Paragraph render: {}ms", t_para.elapsed().as_millis());
  } else {
    let hint = Paragraph::new("Select an item from the feed or history")
      .style(Style::default().fg(t.text_dim));
    frame.render_widget(hint, inner);
  }

  log::debug!(
    "draw_details_panel total: {}ms",
    t_details.elapsed().as_millis()
  );
}

enum DetailsSubject<'a> {
  FeedItem(&'a FeedItem),
  HistoryPaper {
    entry: &'a crate::history::HistoryEntry,
    item: Option<&'a FeedItem>,
    meta: Option<&'a crate::history::HistoryPaperMeta>,
  },
  HistoryQuery(&'a crate::history::HistoryEntry),
}

#[derive(Clone, Copy)]
struct DetailStyles {
  title_style: Style,
  header_style: Style,
  meta_style: Style,
  label_style: Style,
  dim_style: Style,
  value_style: Style,
  accent_style: Style,
  success_style: Style,
}

fn details_subject<'a>(
  app: &'a App,
  history: &[&'a crate::history::HistoryEntry],
) -> Option<DetailsSubject<'a>> {
  if app.feed_tab == FeedTab::History {
    let entry = *history.get(app.history_selected_index)?;
    return match entry.kind {
      crate::history::HistoryKind::Paper => {
        // O(1) hashmap lookup vs the prior O(N) chain+find scan that
        // ran per row × per frame on the History tab.
        let item = app
          .url_index
          .get(&entry.key)
          .map(|&i| &app.items[i])
          .or_else(|| {
            app
              .discovery_url_index
              .get(&entry.key)
              .map(|&i| &app.discovery_items[i])
          });
        Some(DetailsSubject::HistoryPaper {
          entry,
          item,
          meta: entry.paper_meta.as_ref(),
        })
      }
      crate::history::HistoryKind::Query => {
        Some(DetailsSubject::HistoryQuery(entry))
      }
    };
  }
  app.selected_item().map(DetailsSubject::FeedItem)
}

fn details_subject_key(
  app: &App,
  history: &[&crate::history::HistoryEntry],
) -> Option<String> {
  match details_subject(app, history)? {
    DetailsSubject::FeedItem(item) => Some(item.url.clone()),
    DetailsSubject::HistoryPaper { entry, .. } => Some(entry.key.clone()),
    DetailsSubject::HistoryQuery(entry) => Some(format!("query:{}", entry.key)),
  }
}

fn render_details_subject<'a>(
  subject: DetailsSubject<'a>,
  app: &'a App,
  s: DetailStyles,
  detail_w: usize,
  inner_w: usize,
  visible_height: usize,
) -> Vec<Line<'a>> {
  match subject {
    DetailsSubject::FeedItem(item) => {
      render_feed_item_details(item, app, s, detail_w, inner_w, visible_height)
    }
    DetailsSubject::HistoryPaper { entry, item: Some(item), .. } => {
      let mut lines = render_feed_item_details(
        item,
        app,
        s,
        detail_w,
        inner_w,
        visible_height,
      );
      lines.insert(
        2.min(lines.len()),
        Line::from(Span::styled(
          format!(
            "Viewed {}",
            crate::history::format_ago(entry.opened_at, chrono::Utc::now())
          ),
          s.meta_style,
        )),
      );
      lines
    }
    DetailsSubject::HistoryPaper { entry, item: None, meta } => {
      render_history_paper_details(
        entry,
        meta,
        app,
        s,
        detail_w,
        visible_height,
      )
    }
    DetailsSubject::HistoryQuery(entry) => {
      render_history_query_details(entry, s, detail_w)
    }
  }
}

fn render_feed_item_details<'a>(
  item: &'a FeedItem,
  app: &'a App,
  s: DetailStyles,
  detail_w: usize,
  inner_w: usize,
  visible_height: usize,
) -> Vec<Line<'a>> {
  let tags = item.domain_tags.join(", ");
  let authors = item.authors.join(", ");
  let source_label = if item.source_name.is_empty() {
    item.source_platform.short_label().to_string()
  } else {
    item.source_name.clone()
  };
  let mut lines = detail_title_lines(&item.title, detail_w, s.title_style);
  let meta_parts = [
    source_label.as_str(),
    item.content_type.short_label(),
    item.published_at.as_str(),
    item.workflow_state.short_label(),
  ];
  lines.push(Line::from(Span::styled(
    truncate(&meta_parts.join(" · "), detail_w),
    s.meta_style,
  )));
  lines.push(Line::from(""));
  push_detail_field(
    &mut lines,
    "Authors",
    &authors,
    s.label_style,
    s.value_style,
    detail_w,
    3,
  );
  lines.push(Line::from(""));
  let mut source_spans = vec![
    Span::styled("Source   ", s.label_style),
    Span::styled(truncate(&source_label, 16), s.accent_style),
    Span::styled("  ", s.dim_style),
    Span::styled(item.content_type.short_label(), s.accent_style),
  ];
  if item.source_platform == SourcePlatform::HuggingFace
    && item.upvote_count > 0
  {
    source_spans.extend([
      Span::styled("  votes ", s.dim_style),
      Span::styled(item.upvote_count.to_string(), s.value_style),
    ]);
  }
  lines.push(Line::from(source_spans));
  if let Some(ref repo) = item.github_repo {
    let display = repo.strip_prefix("https://").unwrap_or(repo.as_str());
    push_detail_field(
      &mut lines,
      "Repo",
      display,
      s.label_style,
      s.accent_style,
      detail_w,
      1,
    );
  }
  if !tags.is_empty() {
    push_detail_field(
      &mut lines,
      "Topics",
      &tags,
      s.label_style,
      s.value_style,
      detail_w,
      2,
    );
  }
  let user_tags = crate::tags::for_url(&app.item_tags, &item.url);
  if !user_tags.is_empty() {
    let formatted =
      user_tags.iter().map(|t| format!("[{t}]")).collect::<Vec<_>>().join("  ");
    push_detail_field(
      &mut lines,
      "Tags",
      &formatted,
      s.label_style,
      s.accent_style,
      detail_w,
      2,
    );
  }
  if !item.benchmark_results.is_empty() {
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
      "Benchmarks",
      s.header_style.add_modifier(Modifier::UNDERLINED),
    )));
    for b in item.benchmark_results.iter().take(3) {
      lines.push(Line::from(Span::styled(
        truncate(
          &format!("  {}/{}: {} ({})", b.task, b.dataset, b.score, b.metric),
          inner_w,
        ),
        s.dim_style,
      )));
    }
  }
  let notif = app
    .notification
    .as_deref()
    .filter(|_| app.notification_item_id.as_deref() == Some(item.url.as_str()));
  let footer_lines =
    3 + usize::from(
      item.github_owner.is_some() && item.github_repo_name.is_some(),
    ) + notif.map_or(0, |_| 2);
  let summary_lines = visible_height
    .saturating_sub(lines.len().saturating_add(footer_lines + 2))
    .min(7);
  push_summary_and_actions(
    &mut lines,
    &item.summary_short,
    &item.url,
    item.github_owner.is_some() && item.github_repo_name.is_some(),
    notif,
    summary_lines,
    s,
    detail_w,
  );
  lines
}

fn render_history_paper_details<'a>(
  entry: &'a crate::history::HistoryEntry,
  meta: Option<&'a crate::history::HistoryPaperMeta>,
  app: &'a App,
  s: DetailStyles,
  detail_w: usize,
  visible_height: usize,
) -> Vec<Line<'a>> {
  let mut lines = detail_title_lines(&entry.title, detail_w, s.title_style);
  let now = chrono::Utc::now();
  let viewed = crate::history::format_ago(entry.opened_at, now);
  let published = meta.map(|m| m.published_at.as_str()).unwrap_or("");
  let source = meta
    .map(|m| m.source_platform.short_label())
    .unwrap_or(entry.source.as_str());
  let meta_line = if published.is_empty() {
    format!("History · {source} · viewed {viewed}")
  } else {
    format!("History · {source} · {published} · viewed {viewed}")
  };
  lines.push(Line::from(Span::styled(
    truncate(&meta_line, detail_w),
    s.meta_style,
  )));
  lines.push(Line::from(""));
  if let Some(meta) = meta {
    let authors = meta.authors.join(", ");
    push_detail_field(
      &mut lines,
      "Authors",
      &authors,
      s.label_style,
      s.value_style,
      detail_w,
      3,
    );
  }
  lines.push(Line::from(""));
  lines.push(Line::from(vec![
    Span::styled("Source   ", s.label_style),
    Span::styled(truncate(&entry.source, 16), s.accent_style),
  ]));
  let summary = meta.map(|m| m.summary_short.as_str()).unwrap_or("");
  let notif = app.notification.as_deref().filter(|_| {
    app.notification_item_id.as_deref() == Some(entry.key.as_str())
  });
  let footer_lines = 3 + notif.map_or(0, |_| 2);
  let summary_lines = visible_height
    .saturating_sub(lines.len().saturating_add(footer_lines + 2))
    .min(7);
  push_summary_and_actions(
    &mut lines,
    summary,
    &entry.key,
    false,
    notif,
    summary_lines,
    s,
    detail_w,
  );
  lines
}

fn render_history_query_details<'a>(
  entry: &'a crate::history::HistoryEntry,
  s: DetailStyles,
  detail_w: usize,
) -> Vec<Line<'a>> {
  let now = chrono::Utc::now();
  let mut lines = detail_title_lines(&entry.title, detail_w, s.title_style);
  lines.push(Line::from(Span::styled(
    truncate(
      &format!(
        "History · query · {}",
        crate::history::format_ago(entry.opened_at, now)
      ),
      detail_w,
    ),
    s.meta_style,
  )));
  lines.push(Line::from(""));
  push_detail_field(
    &mut lines,
    "Intent",
    &entry.source,
    s.label_style,
    s.value_style,
    detail_w,
    1,
  );
  push_detail_field(
    &mut lines,
    "Query",
    &entry.key,
    s.label_style,
    s.value_style,
    detail_w,
    4,
  );
  lines.push(Line::from(""));
  lines.push(Line::from(vec![
    Span::styled("Action   ", s.label_style),
    Span::styled("Enter", s.accent_style),
    Span::styled(" rerun discovery", s.dim_style),
  ]));
  lines
}

fn detail_title_lines<'a>(
  title: &str,
  detail_w: usize,
  title_style: Style,
) -> Vec<Line<'a>> {
  textwrap::wrap(title, detail_w)
    .into_iter()
    .take(2)
    .map(|line| Line::from(Span::styled(line.into_owned(), title_style)))
    .collect()
}

fn push_summary_and_actions<'a>(
  lines: &mut Vec<Line<'a>>,
  summary: &str,
  url: &str,
  has_repo: bool,
  notification: Option<&str>,
  summary_lines: usize,
  s: DetailStyles,
  detail_w: usize,
) {
  if !summary.is_empty() && summary_lines > 0 {
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Summary", s.header_style)));
    lines.extend(wrap_limited_ellipsis(
      summary,
      detail_w,
      summary_lines,
      s.value_style,
    ));
  }
  let url_w = detail_w.saturating_sub(9);
  lines.push(Line::from(""));
  lines.push(Line::from(vec![
    Span::styled("URL      ", s.label_style),
    Span::styled(truncate(url, url_w), s.dim_style),
  ]));
  lines.push(Line::from(vec![
    Span::styled("Action   ", s.label_style),
    Span::styled("o", s.accent_style),
    Span::styled(" open URL", s.dim_style),
  ]));
  if has_repo {
    lines.push(Line::from(vec![
      Span::styled("         ", s.label_style),
      Span::styled("v", s.success_style),
      Span::styled(" view repo", s.dim_style),
    ]));
  }
  if let Some(notification) = notification {
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(notification.to_string(), s.dim_style)));
  }
}

fn filter_header(
  label: &'static str,
  t: &crate::theme::Theme,
) -> Line<'static> {
  Line::from(Span::styled(
    label,
    Style::default().fg(t.header).add_modifier(Modifier::BOLD),
  ))
}

fn filter_row_owned(
  label: String,
  active: bool,
  cursor: bool,
  t: &crate::theme::Theme,
) -> Line<'static> {
  let checkbox = if active { "[x]" } else { "[ ]" };
  if cursor {
    let hl = t.style_selection_text();
    Line::from(vec![
      Span::styled("  ", hl),
      Span::styled(checkbox, hl),
      Span::styled(" ", hl),
      Span::styled(label, hl),
    ])
  } else if active {
    Line::from(vec![
      Span::raw("  "),
      Span::styled(checkbox, Style::default().fg(t.text)),
      Span::raw(" "),
      Span::styled(label, Style::default().fg(t.text)),
    ])
  } else {
    Line::from(vec![
      Span::raw("  "),
      Span::styled(checkbox, Style::default().fg(t.text_dim)),
      Span::raw(" "),
      Span::styled(label, Style::default().fg(t.text_dim)),
    ])
  }
}

fn filter_row(
  label: &'static str,
  active: bool,
  cursor: bool,
  t: &crate::theme::Theme,
) -> Line<'static> {
  let checkbox = if active { "[x]" } else { "[ ]" };
  if cursor {
    let hl = t.style_selection_text();
    Line::from(vec![
      Span::styled("  ", hl),
      Span::styled(checkbox, hl),
      Span::styled(" ", hl),
      Span::styled(label, hl),
    ])
  } else if active {
    Line::from(vec![
      Span::raw("  "),
      Span::styled(checkbox, Style::default().fg(t.text)),
      Span::raw(" "),
      Span::styled(label, Style::default().fg(t.text)),
    ])
  } else {
    Line::from(vec![
      Span::raw("  "),
      Span::styled(checkbox, Style::default().fg(t.text_dim)),
      Span::raw(" "),
      Span::raw(label),
    ])
  }
}

fn push_activity_wrapped<'a>(
  lines: &mut Vec<Line<'a>>,
  label: &'static str,
  value: &str,
  label_style: Style,
  value_style: Style,
  value_width: usize,
  max_lines: usize,
) {
  let mut wrapped: Vec<String> = textwrap::wrap(value, value_width)
    .into_iter()
    .take(max_lines)
    .map(|line| line.into_owned())
    .collect();
  if wrapped.is_empty() {
    wrapped.push(String::new());
  }
  let label_text = format!("{label:<11}");
  for (idx, value_line) in wrapped.into_iter().enumerate() {
    let prefix = if idx == 0 { label_text.as_str() } else { "           " };
    lines.push(Line::from(vec![
      Span::styled(prefix.to_string(), label_style),
      Span::styled(value_line, value_style),
    ]));
  }
}

fn push_activity_continuation<'a>(
  lines: &mut Vec<Line<'a>>,
  value: &str,
  value_style: Style,
  value_width: usize,
  max_lines: usize,
) {
  for value_line in
    textwrap::wrap(value, value_width).into_iter().take(max_lines)
  {
    lines.push(Line::from(vec![
      Span::raw("           "),
      Span::styled(value_line.into_owned(), value_style),
    ]));
  }
}

fn push_detail_field<'a>(
  lines: &mut Vec<Line<'a>>,
  label: &'static str,
  value: &str,
  label_style: Style,
  value_style: Style,
  total_width: usize,
  max_lines: usize,
) {
  let label_w = 9;
  let value_w = total_width.saturating_sub(label_w).max(1);
  let label_text = format!("{label:<9}");
  let wrapped: Vec<String> = textwrap::wrap(value, value_w)
    .into_iter()
    .take(max_lines)
    .map(|line| line.into_owned())
    .collect();
  if wrapped.is_empty() {
    lines.push(Line::from(vec![
      Span::styled(label_text, label_style),
      Span::raw(""),
    ]));
    return;
  }
  for (idx, value_line) in wrapped.into_iter().enumerate() {
    let prefix = if idx == 0 { label_text.as_str() } else { "         " };
    lines.push(Line::from(vec![
      Span::styled(prefix.to_string(), label_style),
      Span::styled(value_line, value_style),
    ]));
  }
}

fn wrap_limited_ellipsis<'a>(
  value: &str,
  width: usize,
  max_lines: usize,
  style: Style,
) -> Vec<Line<'a>> {
  if max_lines == 0 {
    return Vec::new();
  }
  let width = width.max(1);
  let wrapped: Vec<String> = textwrap::wrap(value, width)
    .into_iter()
    .map(|line| line.into_owned())
    .collect();
  if wrapped.is_empty() {
    return vec![Line::from(Span::styled("", style))];
  }
  let overflowed = wrapped.len() > max_lines;
  let mut lines: Vec<String> = wrapped.into_iter().take(max_lines).collect();
  if overflowed {
    if let Some(last) = lines.last_mut() {
      let keep = width.saturating_sub(1);
      let trimmed = safe_truncate_chars(last.trim_end(), keep);
      *last = format!("{trimmed}…");
    }
  }
  lines.into_iter().map(|line| Line::from(Span::styled(line, style))).collect()
}


// ── Popup helpers ──────────────────────────────────────────────────────────



// ── A1: floating reader popup (Ldr+Enter) ─────────────────────────────────

fn draw_reader_popup(frame: &mut Frame, app: &mut App, area: Rect) {
  let t = app.theme();
  let desired_h = (area.height as u32 * 58 / 100) as u16;
  let popup_rect = popup_rect(area, 70, desired_h, 64, 14, 88);

  frame.render_widget(Clear, popup_rect);

  let block = quiet_popup_block(" Reader · Esc close ", &t);

  let block_inner = block.inner(popup_rect);
  let inner = popup_inner(block_inner, 1, 1);
  frame.render_widget(block, popup_rect);

  let tread_theme = app.theme_for_tread();
  let kitty = app.kitty_supported;
  // Split-borrow so the reader and its image state can both be borrowed —
  // they live on different App fields.
  let App { reader_popup_editor, reader_popup_image_state, .. } = app;
  if let Some(editor) = reader_popup_editor.as_mut() {
    editor.resize(inner.width, inner.height);
    tread::draw(frame, inner, editor, &tread_theme);
    tread::after_draw(editor, reader_popup_image_state, inner, kitty);
  }
}


fn draw_narrow_feed_details_popup(frame: &mut Frame, app: &App, area: Rect) {
  let t = app.theme();
  let popup_h = (area.height * 40 / 100).max(6).min(area.height);
  let popup_w = area.width.saturating_sub(4).max(20);
  let popup_x = area.x + (area.width.saturating_sub(popup_w)) / 2;
  let popup_y = area.y + area.height.saturating_sub(popup_h);
  let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);

  frame.render_widget(Clear, popup_rect);

  let block = Block::default()
    .borders(Borders::ALL)
    .border_style(Style::default().fg(t.border_active))
    .title(Span::styled(
      " Details · j/k: scroll  d/Esc: close ",
      Style::default().fg(t.accent),
    ));

  let inner = block.inner(popup_rect);
  frame.render_widget(block, popup_rect);

  if inner.height == 0 {
    return;
  }

  let items = app.items_for_tab();
  let sel = app.active_selected_index();
  let Some(item) = items.get(sel) else { return };

  let text = format!("{}\n\n{}", item.title, item.summary_short);
  let scroll = app.details_scroll;
  let para = Paragraph::new(text)
    .wrap(ratatui::widgets::Wrap { trim: false })
    .scroll((scroll as u16, 0))
    .style(Style::default().fg(t.text));
  frame.render_widget(para, inner);
}

// ── A2 State 3: persistent Feed Drawer (feed list or details) ──────────────

fn draw_reader_bottom_pane(frame: &mut Frame, app: &mut App, area: Rect) {
  let t = app.theme();
  const POPUP_H: u16 = 11; // border(2) + hint row(1) + sep(1) + content(7)
  let popup_w = (area.width as u32 * 60 / 100) as u16;
  let popup_x = area.x + (area.width.saturating_sub(popup_w)) / 2;
  let popup_y = area.y + area.height.saturating_sub(POPUP_H);
  let popup_rect = Rect::new(popup_x, popup_y, popup_w, POPUP_H);

  frame.render_widget(Clear, popup_rect);

  let focused = app.reader_bottom_focused;
  let border_color = if focused { t.border_active } else { t.border };

  let title_str = if app.reader_bottom_details {
    " Feed Drawer Details · d: back  j/k: scroll  Esc: back "
  } else if app.search_active || !app.search_query.is_empty() {
    " Feed Drawer · / search active  Enter: open  Esc: clear  q: close "
  } else {
    " Feed Drawer · j/k: navigate  /: search  Enter: open  d: details  q: close "
  };
  let block = Block::default()
    .borders(Borders::ALL)
    .border_style(Style::default().fg(border_color))
    .title(Span::styled(title_str, Style::default().fg(t.accent)));

  let inner = block.inner(popup_rect);
  frame.render_widget(block, popup_rect);

  if inner.height == 0 {
    return;
  }

  if app.reader_bottom_details {
    draw_bottom_pane_details(frame, app, inner);
  } else {
    draw_bottom_pane_feed(frame, app, inner);
  }
}

fn draw_bottom_pane_details(frame: &mut Frame, app: &App, area: Rect) {
  let t = app.theme();
  let sel = app.reader_feed_popup_selected;
  let Some(item) = app.visible_get(sel) else { return };

  let rows =
    Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);
  let title = Line::from(Span::styled(
    item.title.clone(),
    Style::default().fg(t.text).add_modifier(Modifier::BOLD),
  ));
  let meta = Line::from(vec![
    Span::styled(
      feed_source_label(item),
      Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    ),
    Span::styled("  ", Style::default().fg(t.text_dim)),
    Span::styled(
      item.content_type.short_label(),
      Style::default().fg(t.text_dim),
    ),
    Span::styled("  ", Style::default().fg(t.text_dim)),
    Span::styled(item.published_at.clone(), Style::default().fg(t.text_dim)),
  ]);
  frame.render_widget(
    Paragraph::new(vec![title, Line::from(""), meta])
      .wrap(ratatui::widgets::Wrap { trim: false }),
    rows[0],
  );

  let scroll = app.reader_bottom_scroll;
  let para = Paragraph::new(item.summary_short.clone())
    .wrap(ratatui::widgets::Wrap { trim: false })
    .scroll((scroll as u16, 0))
    .style(Style::default().fg(t.text));
  frame.render_widget(para, rows[1]);
}

fn draw_bottom_pane_feed(frame: &mut Frame, app: &mut App, area: Rect) {
  let t = app.theme();
  let sel = app.reader_feed_popup_selected;
  let rows =
    Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
  let header_area = rows[0];
  let list_area = rows[1];
  let viewport_rows = list_area.height as usize;
  if viewport_rows == 0 {
    return;
  }

  // Auto-scroll offset to keep selection visible.
  let mut offset = app.reader_bottom_scroll;
  let total = app.visible_count();
  if sel < offset {
    offset = sel;
  } else if sel >= offset.saturating_add(viewport_rows) {
    offset = sel + 1 - viewport_rows;
  }
  offset = offset.min(total.saturating_sub(1));
  app.reader_bottom_scroll = offset;

  frame.render_widget(
    Paragraph::new(drawer_feed_header_line(list_area.width as usize, &t)),
    header_area,
  );

  if total == 0 {
    let empty =
      if app.search_query.is_empty() { "No items" } else { "No matches" };
    frame.render_widget(
      Paragraph::new(empty)
        .alignment(Alignment::Center)
        .style(Style::default().fg(t.text_dim)),
      list_area,
    );
    return;
  }

  let window = app.visible_window(offset, offset.saturating_add(viewport_rows));
  for (rel_i, item) in window.iter().enumerate() {
    let i = offset + rel_i;
    let is_selected = i == sel;
    let row_y = list_area.y + rel_i as u16;
    let row_rect = Rect::new(list_area.x, row_y, list_area.width, 1);
    let row =
      drawer_feed_row_line(item, list_area.width as usize, is_selected, &t);
    if is_selected {
      frame.render_widget(
        Paragraph::new(row).style(t.style_selection()),
        row_rect,
      );
    } else {
      frame.render_widget(Paragraph::new(row), row_rect);
    }
  }
}

fn drawer_feed_header_line(
  width: usize,
  t: &crate::theme::Theme,
) -> Line<'static> {
  if width < 34 {
    return Line::from(Span::styled(
      "Title",
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    ));
  }

  let source_w = 7usize;
  let kind_w = 6usize;
  let date_w = 10usize;
  let gap_w = 3usize;
  let title_w = width.saturating_sub(source_w + kind_w + date_w + gap_w).max(8);

  Line::from(vec![
    Span::styled(
      format!("{:<source_w$}", "Src"),
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    ),
    Span::raw(" "),
    Span::styled(
      format!("{:<kind_w$}", "Kind"),
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    ),
    Span::raw(" "),
    Span::styled(
      format!("{:<title_w$}", "Title"),
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    ),
    Span::raw(" "),
    Span::styled(
      format!("{:<date_w$}", "Date"),
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    ),
  ])
}

fn drawer_feed_row_line(
  item: &crate::models::FeedItem,
  width: usize,
  selected: bool,
  t: &crate::theme::Theme,
) -> Line<'static> {
  if width < 34 {
    let title = truncate_str(&item.title, width);
    let style = if selected {
      t.style_selection_text()
    } else {
      Style::default().fg(t.text)
    };
    return Line::from(Span::styled(title, style));
  }

  let source_w = 7usize;
  let kind_w = 6usize;
  let date_w = 10usize;
  let gap_w = 3usize;
  let title_w = width.saturating_sub(source_w + kind_w + date_w + gap_w).max(8);

  let source = truncate_str(&feed_source_label(item), source_w);
  let kind = truncate_str(item.content_type.short_label(), kind_w);
  let title = truncate_str(&item.title, title_w);
  let date = truncate_str(&item.published_at, date_w);

  if selected {
    let row = format!(
      "{source:<source_w$} {kind:<kind_w$} {title:<title_w$} {date:<date_w$}"
    );
    return Line::from(Span::styled(row, t.style_selection_text()));
  }

  Line::from(vec![
    Span::styled(format!("{source:<source_w$}"), Style::default().fg(t.accent)),
    Span::raw(" "),
    Span::styled(format!("{kind:<kind_w$}"), Style::default().fg(t.text_dim)),
    Span::raw(" "),
    Span::styled(format!("{title:<title_w$}"), Style::default().fg(t.text)),
    Span::raw(" "),
    Span::styled(format!("{date:<date_w$}"), Style::default().fg(t.text_dim)),
  ])
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

