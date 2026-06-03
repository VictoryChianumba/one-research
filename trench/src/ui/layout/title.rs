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

/// Rendered width of a nav line: the sum of its tab widths plus a
/// `sep`-column gap between adjacent tabs.
fn nav_line_width(widths: &[usize], sep: usize) -> usize {
  match widths.len() {
    0 => 0,
    n => widths.iter().sum::<usize>() + sep * (n - 1),
  }
}

/// Choose the split index `k` (line 1 = tabs `[..k]`, line 2 = `[k..]`)
/// that balances the nav across two lines — minimising the wider line —
/// preferring splits where both lines fit `avail`. Order is preserved, so
/// only contiguous splits are considered. Falls back to `1` when nothing
/// fits (a single over-wide tab), keeping at least one tab per line.
fn balanced_nav_split(widths: &[usize], avail: usize, sep: usize) -> usize {
  let mut best_k = 1;
  let mut best_max = usize::MAX;
  for k in 1..widths.len() {
    let w1 = nav_line_width(&widths[..k], sep);
    let w2 = nav_line_width(&widths[k..], sep);
    let wider = w1.max(w2);
    if w1 <= avail && w2 <= avail && wider < best_max {
      best_max = wider;
      best_k = k;
    }
  }
  best_k
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

  // The nav occupies the logo's middle and bottom rows. It keeps full labels
  // and wraps onto a balanced second line — aligned under the first tab,
  // beside the logo's bottom row — when it can't fit on one. This keeps every
  // tab and its count visible instead of clipping the tail off-screen. When
  // the whole nav fits on one line it is centred, with the version appended.
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

  // Lay the nav out past the logo. If every tab fits on one line, centre it
  // and show the version (wide terminals). Otherwise split into two balanced
  // lines — minimising the wider line — so the wrap doesn't dump five tabs on
  // row one and a lone tab on row two. Line two aligns under the first tab.
  let nav_x = logo_width.saturating_add(3);
  let avail = width.saturating_sub(nav_x);
  let sep_w = 2;
  let widths: Vec<usize> = items.iter().map(|(_, w)| *w).collect();
  let total = nav_line_width(&widths, sep_w);
  // Join tab items into a single span run, 2-col gap between tabs.
  let join = |items: Vec<(Vec<Span<'static>>, usize)>| -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, (item, _)) in items.into_iter().enumerate() {
      if i > 0 {
        spans.push(Span::raw("  "));
      }
      spans.extend(item);
    }
    spans
  };
  // A nav row: logo row + gap to `x` + content.
  let row = |logo_row: &'static str, x: usize, content: Vec<Span<'static>>| {
    let mut spans = vec![
      Span::styled(logo_row, logo_style),
      Span::raw(" ".repeat(x.saturating_sub(logo_row.chars().count()))),
    ];
    spans.extend(content);
    Line::from(spans)
  };

  if total <= avail {
    // One line: centre past the logo, append the version.
    let x = (width.saturating_sub(total) / 2).max(nav_x);
    let mut spans = row(WORDMARK[1], x, join(items)).spans;
    let version_gap = width.saturating_sub(x + total + VERSION.len());
    if version_gap >= 1 {
      spans.push(Span::raw(" ".repeat(version_gap)));
      spans.push(Span::styled(VERSION, Style::default().fg(t.text_dim)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner[2]);
    frame.render_widget(
      Paragraph::new(row(WORDMARK[2], nav_x, Vec::new())),
      inner[3],
    );
  } else {
    // Two balanced lines: the split that minimises the wider line.
    let k = balanced_nav_split(&widths, avail, sep_w);
    let mut it = items.into_iter();
    let line1: Vec<_> = it.by_ref().take(k).collect();
    let line2: Vec<_> = it.collect();
    frame.render_widget(
      Paragraph::new(row(WORDMARK[1], nav_x, join(line1))),
      inner[2],
    );
    frame.render_widget(
      Paragraph::new(row(WORDMARK[2], nav_x, join(line2))),
      inner[3],
    );
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

#[cfg(test)]
mod tests {
  use super::{balanced_nav_split, nav_line_width};

  // Real tab widths: Inbox 1351 / Browse 155 / Library 3 /
  // Discoveries 21 / History 8 / Total 1516, with a 2-col gap.
  const NAV: [usize; 6] = [10, 10, 9, 14, 9, 10];

  #[test]
  fn nav_line_width_sums_tabs_and_gaps() {
    assert_eq!(nav_line_width(&[], 2), 0);
    assert_eq!(nav_line_width(&[10], 2), 10);
    assert_eq!(nav_line_width(&[10, 9], 2), 21);
  }

  #[test]
  fn balanced_split_evens_the_two_lines() {
    // At ~half-screen avail the greedy fill would be 4+2 (line widths
    // 49 / 21); the balanced split is 3+3 (33 / 37). We want the latter
    // so the wrap doesn't look lopsided.
    let k = balanced_nav_split(&NAV, 57, 2);
    assert_eq!(k, 3, "expected a 3+3 split, not greedy 4+2");
    assert!(nav_line_width(&NAV[..k], 2) <= 57);
    assert!(nav_line_width(&NAV[k..], 2) <= 57);
  }

  #[test]
  fn balanced_split_minimises_the_wider_line() {
    // No other valid split yields a smaller wider-line than the one chosen.
    let k = balanced_nav_split(&NAV, 57, 2);
    let chosen = nav_line_width(&NAV[..k], 2).max(nav_line_width(&NAV[k..], 2));
    for alt in 1..NAV.len() {
      let w1 = nav_line_width(&NAV[..alt], 2);
      let w2 = nav_line_width(&NAV[alt..], 2);
      if w1 <= 57 && w2 <= 57 {
        assert!(chosen <= w1.max(w2));
      }
    }
  }
}
