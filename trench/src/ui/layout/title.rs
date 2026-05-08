use ratatui::{
  Frame,
  layout::{Constraint, Direction, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::Paragraph,
};

use std::collections::HashSet;

use super::RIGHT_COL_WIDTH;
use crate::app::{App, FeedTab};
use crate::models::{ContentType, SignalLevel, WorkflowState};

const VERSION: &str = "v0.1.0";

const SPINNER: &[&str] =
  &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn title_bar_height(_width: u16) -> u16 {
  5
}

pub fn draw_title_bar(frame: &mut Frame, app: &App, area: Rect) {
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

pub fn draw_search_row(frame: &mut Frame, app: &App, area: Rect) {
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
