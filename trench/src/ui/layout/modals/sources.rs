use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Direction, Layout},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Clear, Paragraph, Wrap},
};

use super::super::widgets::{
  draw_card_footer, settings_card_block, settings_modal_rect, truncate,
};
use crate::app::{App, DiscoverResult, SourcesDetectState};

pub fn draw_sources_popup(frame: &mut Frame, app: &App) {
  let t = app.theme();
  let area = frame.area();
  let popup_area = settings_modal_rect(area);

  frame.render_widget(Clear, popup_area);

  let block = settings_card_block(" Manage Sources ", &t);
  let inner = block.inner(popup_area);
  frame.render_widget(block, popup_area);

  if inner.width < 44 || inner.height < 14 {
    frame.render_widget(
      Paragraph::new(Span::styled(
        " terminal too small for sources ",
        Style::default().fg(t.text_dim).bg(t.bg_panel),
      ))
      .alignment(Alignment::Center),
      inner,
    );
    return;
  }

  let chunks = Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Min(0), Constraint::Length(2)])
    .split(inner);
  let body_area = chunks[0];
  let footer_area = chunks[1];

  let columns = if body_area.width >= 96 {
    Layout::default()
      .direction(Direction::Horizontal)
      .constraints([Constraint::Length(29), Constraint::Min(0)])
      .split(body_area)
  } else {
    Layout::default()
      .direction(Direction::Horizontal)
      .constraints([Constraint::Length(0), Constraint::Min(0)])
      .split(body_area)
  };
  let list_area = columns[1];
  let w = list_area.width as usize;
  let hrule = "─".repeat(w.saturating_sub(4));
  let cats = app.sources_popup_arxiv_cats();
  let cats_count = cats.len();
  let sources_count = crate::config::PREDEFINED_SOURCES.len();
  let custom_feeds = &app.config.sources.custom_feeds;
  let cursor = app.sources_popup.cursor;

  let dim_style = Style::default().fg(t.text_dim);
  let text_style = Style::default().fg(t.text);
  let header_style = Style::default().fg(t.header).add_modifier(Modifier::BOLD);
  let accent_style = Style::default().fg(t.accent);
  let bg_style = Style::default().bg(t.bg_panel);
  let selected_style = t.style_selection_text();

  if columns[0].width > 0 {
    let enabled_predefined = crate::config::PREDEFINED_SOURCES
      .iter()
      .filter(|name| {
        app.config.sources.enabled_sources.get(**name).copied().unwrap_or(true)
      })
      .count();
    let rail = columns[0];
    let rail_rule = "─".repeat(rail.width.saturating_sub(4) as usize);
    let rail_lines = vec![
      Line::from(""),
      Line::from(Span::styled("  Source Set", header_style)),
      Line::from(Span::styled(format!("  {rail_rule}"), dim_style)),
      Line::from(""),
      Line::from(vec![
        Span::styled("  arXiv categories ", dim_style),
        Span::styled(
          app.config.sources.arxiv_categories.len().to_string(),
          text_style,
        ),
      ]),
      Line::from(vec![
        Span::styled("  Built-ins        ", dim_style),
        Span::styled(
          format!("{enabled_predefined}/{sources_count}"),
          text_style,
        ),
      ]),
      Line::from(vec![
        Span::styled("  Custom feeds     ", dim_style),
        Span::styled(custom_feeds.len().to_string(), text_style),
      ]),
      Line::from(""),
      Line::from(Span::styled("  Add by URL", header_style)),
      Line::from(Span::styled(format!("  {rail_rule}"), dim_style)),
      Line::from(Span::styled(
        "  Paste RSS, Atom, arXiv category, or supported source URL.",
        dim_style,
      )),
    ];

    frame.render_widget(
      Paragraph::new(rail_lines).wrap(Wrap { trim: false }).style(bg_style),
      rail,
    );
  }

  let mut lines: Vec<Line> = Vec::new();

  lines.push(Line::from(""));
  lines.push(Line::from(Span::styled("  Add source", header_style)));

  let input_active = app.sources_popup.input_active;
  let input_focused = cursor == 0;
  let input_display = if app.sources_popup.input.is_empty() && !input_active {
    "paste a URL...".to_string()
  } else if input_active {
    format!("{}_", app.sources_popup.input)
  } else {
    app.sources_popup.input.clone()
  };
  lines.push(Line::from(vec![
    Span::styled(
      if input_focused { "  > " } else { "    " },
      if input_active {
        Style::default().fg(t.success)
      } else if input_focused {
        accent_style
      } else {
        dim_style
      },
    ),
    Span::styled(
      truncate(&input_display, w.saturating_sub(8)),
      if input_active || input_focused { text_style } else { dim_style },
    ),
  ]));

  let detect_line = match &app.sources_popup.detect_state {
    SourcesDetectState::Idle => {
      if input_focused && !app.sources_popup.input.is_empty() && !input_active {
        Line::from(Span::styled("  Press Enter to detect feed type", dim_style))
      } else {
        Line::from("")
      }
    }
    SourcesDetectState::Detecting => {
      Line::from(Span::styled("  Detecting...", Style::default().fg(t.warning)))
    }
    SourcesDetectState::Result(r) => match r {
      DiscoverResult::ArxivCategory(code) => Line::from(Span::styled(
        format!("  Detected: arXiv category {code} — press Enter to confirm"),
        Style::default().fg(t.success),
      )),
      DiscoverResult::HuggingFaceAlreadyEnabled => Line::from(Span::styled(
        "  Detected: HuggingFace daily papers — already enabled",
        dim_style,
      )),
      DiscoverResult::RssFeed { url, .. } => {
        let display = truncate(url, w.saturating_sub(36));
        Line::from(Span::styled(
          format!("  Detected: RSS feed at {display} — press Enter to confirm"),
          Style::default().fg(t.success),
        ))
      }
      DiscoverResult::Failed(msg) => Line::from(Span::styled(
        format!("  {msg}"),
        Style::default().fg(t.error),
      )),
    },
  };
  lines.push(detect_line);

  lines.push(Line::from(Span::styled("  arXiv categories", header_style)));
  lines.push(Line::from(Span::styled(format!("  {hrule}"), dim_style)));
  for (i, (code, label)) in cats.iter().enumerate() {
    let pos = 1 + i;
    let sel = cursor == pos;
    let enabled = app.config.sources.arxiv_categories.contains(code);
    let cb = if enabled { "[x]" } else { "[ ]" };
    let label_str =
      if label.is_empty() { code.as_str() } else { label.as_str() };
    let text = format!("  {cb} {code:<8} {label_str}");
    let style = if sel {
      selected_style
    } else if enabled {
      accent_style
    } else {
      dim_style
    };
    lines.push(Line::from(Span::styled(text, style)));
  }
  lines.push(Line::from(""));

  lines.push(Line::from(Span::styled("  Sources", header_style)));
  lines.push(Line::from(Span::styled(format!("  {hrule}"), dim_style)));
  for (i, &name) in crate::config::PREDEFINED_SOURCES.iter().enumerate() {
    let pos = 1 + cats_count + i;
    let sel = cursor == pos;
    let enabled =
      app.config.sources.enabled_sources.get(name).copied().unwrap_or(true);
    let cb = if enabled { "[x]" } else { "[ ]" };
    let text = format!("  {cb} {name}");
    let style = if sel {
      selected_style
    } else if enabled {
      accent_style
    } else {
      dim_style
    };
    lines.push(Line::from(Span::styled(text, style)));
  }
  lines.push(Line::from(""));

  lines.push(Line::from(Span::styled("  Custom feeds", header_style)));
  lines.push(Line::from(Span::styled(format!("  {hrule}"), dim_style)));
  if custom_feeds.is_empty() {
    lines.push(Line::from(Span::styled("  none", dim_style)));
  } else {
    for (i, feed) in custom_feeds.iter().enumerate() {
      let pos = 1 + cats_count + sources_count + i;
      let sel = cursor == pos;
      let text = format!("  [x] {}", feed.name);
      let style = if sel { selected_style } else { accent_style };
      lines.push(Line::from(Span::styled(text, style)));
    }
  }

  let selected_line = if cursor == 0 {
    2
  } else if cursor <= cats_count {
    6 + cursor.saturating_sub(1)
  } else if cursor <= cats_count + sources_count {
    9 + cats_count + cursor.saturating_sub(1 + cats_count)
  } else {
    12 + cats_count
      + sources_count
      + cursor.saturating_sub(1 + cats_count + sources_count)
  };
  let viewport_rows = list_area.height as usize;
  let scroll = if selected_line >= viewport_rows.saturating_sub(2) {
    selected_line.saturating_sub(viewport_rows.saturating_sub(3))
  } else {
    0
  };

  let para = Paragraph::new(lines).scroll((scroll as u16, 0)).style(bg_style);
  frame.render_widget(para, list_area);

  draw_card_footer(
    frame,
    footer_area,
    &t,
    "  j/k navigate · space toggle · enter add source · d delete · esc back",
  );
}
