use ratatui::{
  Frame,
  layout::Rect,
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::app::App;
use crate::models::{ContentType, SignalLevel};

pub fn draw_filter_panel(frame: &mut Frame, app: &App, area: Rect) {
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
