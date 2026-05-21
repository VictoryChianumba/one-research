use ratatui::{
  Frame,
  layout::Rect,
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use super::widgets::truncate;
use crate::app::App;
use crate::models::{ContentType, SignalLevel};

enum FilterRowKind {
  Toggle,
  Radio,
  Action,
}

pub fn draw_filter_panel(frame: &mut Frame, app: &App, area: Rect) {
  let t = app.theme();
  let inner = area;
  let focused = app.feed.filter_focus;
  let width = inner.width as usize;

  let f = &app.feed.active_filters;
  let c = app.feed.filter_cursor;
  let mut s: usize = 0;
  let mut lines: Vec<Line> = Vec::new();
  let mut cursor_line: usize = 0;

  let source_names = app.filter_source_names();
  let tag_names = crate::tags::all_tags(&app.workspace.item_tags);
  lines.push(panel_status_line(
    f.active_count(),
    app.feed.sort_mode.label(),
    app.feed.subject_follow,
    focused,
    width,
    &t,
  ));
  lines.push(help_line(
    "Space toggles  c clears filters  Esc closes",
    width,
    &t,
  ));
  lines.push(Line::from(""));

  lines.push(section_header(
    "Sources",
    f.sources.len(),
    source_names.len(),
    width,
    &t,
  ));
  for name in source_names {
    let active = f.sources.contains(&name);
    let cursor = focused && s == c;
    if cursor {
      cursor_line = lines.len();
    }
    lines.push(filter_row_owned(
      name,
      active,
      cursor,
      FilterRowKind::Toggle,
      width,
      &t,
    ));
    s += 1;
  }
  lines.push(Line::from(""));

  lines.push(section_header("Signal", f.signals.len(), 3, width, &t));
  if focused && s == c {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "Primary",
    f.signals.contains(&SignalLevel::Primary),
    focused && s == c,
    FilterRowKind::Toggle,
    width,
    &t,
  ));
  s += 1;
  if focused && s == c {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "Secondary",
    f.signals.contains(&SignalLevel::Secondary),
    focused && s == c,
    FilterRowKind::Toggle,
    width,
    &t,
  ));
  s += 1;
  if focused && s == c {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "Tertiary",
    f.signals.contains(&SignalLevel::Tertiary),
    focused && s == c,
    FilterRowKind::Toggle,
    width,
    &t,
  ));
  s += 1;
  lines.push(Line::from(""));

  lines.push(section_header("Content", f.content_types.len(), 3, width, &t));
  if focused && s == c {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "Paper",
    f.content_types.contains(&ContentType::Paper),
    focused && s == c,
    FilterRowKind::Toggle,
    width,
    &t,
  ));
  s += 1;
  if focused && s == c {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "Article",
    f.content_types.contains(&ContentType::Article),
    focused && s == c,
    FilterRowKind::Toggle,
    width,
    &t,
  ));
  s += 1;
  if focused && s == c {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "Digest",
    f.content_types.contains(&ContentType::Digest),
    focused && s == c,
    FilterRowKind::Toggle,
    width,
    &t,
  ));
  s += 1;
  lines.push(Line::from(""));

  // Workflow state filtering moved to the Library tab chips — the panel only
  // covers source / signal / content_type / tags now.

  if !tag_names.is_empty() {
    lines.push(section_header(
      "Tags",
      f.tags.len(),
      tag_names.len(),
      width,
      &t,
    ));
    for name in tag_names {
      let active = f.tags.contains(&name);
      let cursor = focused && s == c;
      if cursor {
        cursor_line = lines.len();
      }
      lines.push(filter_row_owned(
        name,
        active,
        cursor,
        FilterRowKind::Toggle,
        width,
        &t,
      ));
      s += 1;
    }
    lines.push(Line::from(""));
  }

  // ADR-011 §E3 — sort modes. Mutually exclusive: [x] on the active
  // mode; Space on any row sets that mode. Random re-shuffles the
  // session seed each time it's selected, so re-Spacing Random gives
  // a fresh order.
  lines.push(section_header("Sort", 1, 4, width, &t));
  for mode in [
    crate::feed::FeedSortMode::Dated,
    crate::feed::FeedSortMode::Random,
    crate::feed::FeedSortMode::Popular,
    crate::feed::FeedSortMode::Trending,
  ] {
    let active = app.feed.sort_mode == mode;
    let cursor = focused && s == c;
    if cursor {
      cursor_line = lines.len();
    }
    lines.push(filter_row(
      mode.label(),
      active,
      cursor,
      FilterRowKind::Radio,
      width,
      &t,
    ));
    s += 1;
  }
  lines.push(Line::from(""));

  // ADR-011 §E4 — subject-follow toggle. When ON, the Browse rail's
  // current drill point narrows the visible items.
  lines.push(section_header(
    "Browse",
    usize::from(app.feed.subject_follow),
    1,
    width,
    &t,
  ));
  let cursor = focused && s == c;
  if cursor {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "Follow rail subject",
    app.feed.subject_follow,
    cursor,
    FilterRowKind::Toggle,
    width,
    &t,
  ));
  s += 1;
  lines.push(Line::from(""));

  let clear_hl = focused && s == c;
  if clear_hl {
    cursor_line = lines.len();
  }
  lines.push(filter_row(
    "Clear filters",
    false,
    clear_hl,
    FilterRowKind::Action,
    width,
    &t,
  ));

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

fn panel_status_line(
  active_filters: usize,
  sort_label: &'static str,
  subject_follow: bool,
  focused: bool,
  width: usize,
  t: &crate::theme::Theme,
) -> Line<'static> {
  let status = if active_filters == 0 {
    "No filters".to_string()
  } else if active_filters == 1 {
    "1 filter".to_string()
  } else {
    format!("{active_filters} filters")
  };
  let focus = if focused { "editing" } else { "preview" };
  let follow = if subject_follow { "follow on" } else { "follow off" };
  let label = format!("{status}  ·  {sort_label}  ·  {follow}  ·  {focus}");
  let line = fit_line(&label, width);
  Line::from(Span::styled(
    line,
    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
  ))
}

fn help_line(
  label: &'static str,
  width: usize,
  t: &crate::theme::Theme,
) -> Line<'static> {
  Line::from(Span::styled(
    fit_line(label, width),
    Style::default().fg(t.text_dim),
  ))
}

fn section_header(
  label: &'static str,
  active: usize,
  total: usize,
  width: usize,
  t: &crate::theme::Theme,
) -> Line<'static> {
  let count =
    if total == 0 { String::new() } else { format!(" {active}/{total}") };
  let text = format!("{label}{count}");
  Line::from(Span::styled(
    fit_line(&text, width),
    Style::default().fg(t.header).add_modifier(Modifier::BOLD),
  ))
}

fn filter_row_owned(
  label: String,
  active: bool,
  cursor: bool,
  kind: FilterRowKind,
  width: usize,
  t: &crate::theme::Theme,
) -> Line<'static> {
  filter_row_text(label, active, cursor, kind, width, t)
}

fn filter_row(
  label: &'static str,
  active: bool,
  cursor: bool,
  kind: FilterRowKind,
  width: usize,
  t: &crate::theme::Theme,
) -> Line<'static> {
  filter_row_text(label.to_string(), active, cursor, kind, width, t)
}

fn filter_row_text(
  label: String,
  active: bool,
  cursor: bool,
  kind: FilterRowKind,
  width: usize,
  t: &crate::theme::Theme,
) -> Line<'static> {
  let marker = match kind {
    FilterRowKind::Toggle if active => "[x]",
    FilterRowKind::Toggle => "[ ]",
    FilterRowKind::Radio if active => "(*)",
    FilterRowKind::Radio => "( )",
    FilterRowKind::Action => "[c]",
  };
  let prefix = if cursor { "> " } else { "  " };
  let max_label = width.saturating_sub(prefix.len() + marker.len() + 2);
  let label = truncate(&label, max_label);
  let text = fit_line(&format!("{prefix}{marker} {label}"), width);
  if cursor {
    let hl = t.style_selection_text();
    Line::from(Span::styled(text, hl))
  } else if active {
    Line::from(Span::styled(text, Style::default().fg(t.text)))
  } else if matches!(kind, FilterRowKind::Action) {
    Line::from(Span::styled(text, Style::default().fg(t.text_dim)))
  } else {
    Line::from(Span::styled(text, Style::default().fg(t.text_dim)))
  }
}

fn fit_line(s: &str, width: usize) -> String {
  let mut text = truncate(s, width);
  let current = text.chars().count();
  if current < width {
    text.push_str(&" ".repeat(width - current));
  }
  text
}
