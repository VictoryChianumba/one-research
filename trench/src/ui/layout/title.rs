use ratatui::{
  Frame,
  layout::{Constraint, Direction, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::Paragraph,
};

use std::collections::HashSet;

use super::right_col_width;
use crate::app::{App, FeedTab};
use crate::models::{ContentType, SignalLevel, WorkflowState};

const VERSION: &str = "v0.1.0";

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

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
  let inbox_style = if app.feed.feed_tab == FeedTab::Inbox {
    active_style
  } else {
    inactive_style
  };
  let library_style = if app.feed.feed_tab == FeedTab::Library {
    active_style
  } else {
    inactive_style
  };
  let discoveries_style = if app.feed.feed_tab == FeedTab::Discoveries {
    active_style
  } else {
    inactive_style
  };
  let browse_style = if app.feed.feed_tab == FeedTab::Browse {
    active_style
  } else {
    inactive_style
  };
  let history_style = if app.feed.feed_tab == FeedTab::History {
    active_style
  } else {
    inactive_style
  };
  let discovery_spin = if app.discovery.loading {
    format!(" {}", SPINNER[app.async_jobs.spinner_frame % SPINNER.len()])
  } else {
    String::new()
  };
  const WORDMARK: &[&str] = &[
    "█▀█ █▄ █ █▀  █▀█ █▀ █▀ █▀ █▀█ █▀█ █ █",
    "█ █ █ ▀█ █▀  █▀▄ █▀ ▀█ █▀ █▀█ █▀▄ █▀█",
    "▀▀▀ ▀  ▀ ▀▀  ▀ ▀ ▀▀ ▀▀ ▀▀ ▀ ▀ ▀ ▀ ▀ ▀",
  ];
  // Browse shows the total taxonomy size (static, 155) — communicates
  // "you can browse this many subject categories." See ADR-010 §D2.
  let browse_total = crate::models::arxiv_taxonomy::all_categories().count();
  let logo_style = Style::default().fg(t.accent).add_modifier(Modifier::BOLD);
  let logo_width =
    WORDMARK.iter().map(|line| line.chars().count()).max().unwrap_or(0);
  let logo = Line::from(Span::styled(WORDMARK[0], logo_style));
  frame.render_widget(Paragraph::new(logo), inner[1]);

  // The nav occupies the logo's middle and bottom rows. It keeps full
  // labels and greedily wraps onto a second line — aligned under the first
  // tab, beside the logo's bottom row — when it can't fit on one. This keeps
  // every tab and its count visible instead of clipping the tail off-screen.
  // When the whole nav does fit on one line it is centered, with the version.
  fn nav_item(
    label: &'static str,
    count: String,
    style: Style,
  ) -> (Vec<Span<'static>>, usize) {
    let w = label.chars().count() + count.chars().count();
    (vec![Span::styled(label, style), Span::styled(count, style)], w)
  }
  let disc_count = format!("{}{}", app.discovery.items.len(), discovery_spin);
  let items: Vec<(Vec<Span>, usize)> = vec![
    nav_item("Inbox ", inbox_count.to_string(), inbox_style),
    nav_item("Browse ", browse_total.to_string(), browse_style),
    nav_item("Library ", library_count.to_string(), library_style),
    nav_item("Discoveries ", disc_count, discoveries_style),
    nav_item(
      "History ",
      app.workspace.history.len().to_string(),
      history_style,
    ),
    nav_item("Total ", total.to_string(), inactive_style),
  ];

  // Greedy pack into lines that fit in the width past the logo. A 2-col gap
  // separates tabs on the same line.
  let nav_x = logo_width.saturating_add(3);
  let avail = width.saturating_sub(nav_x);
  let sep_w = 2;
  let mut lines: Vec<(Vec<Span>, usize)> = vec![(Vec::new(), 0)];
  for (spans, w) in items {
    let last = lines.last().unwrap();
    let needs_sep = !last.0.is_empty();
    let overflow = needs_sep && last.1 + sep_w + w > avail;
    if overflow {
      lines.push((spans, w));
    } else {
      let last = lines.last_mut().unwrap();
      if needs_sep {
        last.0.push(Span::raw("  "));
        last.1 += sep_w;
      }
      last.0.extend(spans);
      last.1 += w;
    }
  }

  // Render. WORDMARK[1] anchors the first nav row; WORDMARK[2] the second.
  let logo_rows = [WORDMARK[1], WORDMARK[2]];
  let row_rects = [inner[2], inner[3]];
  if lines.len() == 1 {
    // One line: center the nav past the logo and append the version.
    let (content, nav_width) = lines.pop().unwrap();
    let centered = width.saturating_sub(nav_width) / 2;
    let nav_x = centered.max(logo_width.saturating_add(3));
    let mut spans = vec![
      Span::styled(WORDMARK[1], logo_style),
      Span::raw(" ".repeat(nav_x.saturating_sub(WORDMARK[1].chars().count()))),
    ];
    spans.extend(content);
    let version_gap = width.saturating_sub(nav_x + nav_width + VERSION.len());
    if version_gap >= 1 {
      spans.push(Span::raw(" ".repeat(version_gap)));
      spans.push(Span::styled(VERSION, Style::default().fg(t.text_dim)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner[2]);
    frame.render_widget(
      Paragraph::new(Line::from(Span::styled(WORDMARK[2], logo_style))),
      inner[3],
    );
  } else {
    // Wrapped: each nav line rides its logo row, aligned under the first tab.
    for (i, (content, _)) in lines.into_iter().take(2).enumerate() {
      let logo_row = logo_rows[i];
      let mut spans = vec![
        Span::styled(logo_row, logo_style),
        Span::raw(" ".repeat(nav_x.saturating_sub(logo_row.chars().count()))),
      ];
      spans.extend(content);
      frame.render_widget(Paragraph::new(Line::from(spans)), row_rects[i]);
    }
  }

  // Whitespace gap between header and search row (was a `─` rule).
  // Halloy-style hierarchy: separate sections with space, not horizontal rules.
  frame.render_widget(Paragraph::new(""), inner[4]);
}

// ── Search + filter row ────────────────────────────────────────────────────

pub fn draw_search_row(frame: &mut Frame, app: &App, area: Rect) {
  let t = app.theme();
  // Row 0: content; row 1: separator
  let content_area = Rect { height: 1, ..area };
  let sep_area = Rect { y: area.y + 1, height: 1, ..area };

  let cols = Layout::default()
    .direction(Direction::Horizontal)
    .constraints([
      Constraint::Min(0),
      Constraint::Length(right_col_width(content_area.width)),
    ])
    .split(content_area);

  let search_text = if !app.feed.search_query.is_empty() {
    format!(" / {}", app.feed.search_query)
  } else if app.feed.search_active {
    " / search · author:  cat:  year:  ti:  abs:".to_string()
  } else {
    " / Search items...".to_string()
  };
  let search_style = if app.feed.search_active {
    Style::default().fg(t.text)
  } else {
    Style::default().fg(t.text_dim)
  };
  frame.render_widget(Paragraph::new(search_text).style(search_style), cols[0]);

  let filter_style = if app.feed.filter_focus {
    Style::default().fg(t.accent)
  } else {
    Style::default().fg(t.text_dim)
  };
  frame.render_widget(
    Paragraph::new(format!(" {}", filter_summary(app))).style(filter_style),
    cols[1],
  );

  // Whitespace gap between search row and tab bar (was a `─` rule).
  frame.render_widget(Paragraph::new(""), sep_area);
}

fn filter_summary(app: &App) -> std::cell::Ref<'_, str> {
  // Lazy memoize: the 4 summarize_ordered_set passes + sort + format!
  // ran every frame regardless of whether active_filters changed.
  // Invalidation hooks fire from `toggle_filter_at_cursor` and
  // `clear_filters` in app.rs.
  if app.render_caches.filter_summary.borrow().is_none() {
    let summary = compute_filter_summary(app);
    *app.render_caches.filter_summary.borrow_mut() = Some(summary);
  }
  std::cell::Ref::map(app.render_caches.filter_summary.borrow(), |opt| {
    opt.as_deref().expect("render_caches.filter_summary populated above")
  })
}

fn compute_filter_summary(app: &App) -> String {
  let f = &app.feed.active_filters;
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
