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
use crate::app::App;

pub fn draw_settings(frame: &mut Frame, app: &App) {
  let t = app.theme();
  let area = frame.area();
  let popup = settings_modal_rect(area);
  frame.render_widget(Clear, popup);

  let block = settings_card_block(" Settings ", &t);
  let inner = block.inner(popup);
  frame.render_widget(block, popup);

  if inner.width < 44 || inner.height < 14 {
    frame.render_widget(
      Paragraph::new(Span::styled(
        " terminal too small for settings ",
        Style::default().fg(t.text_dim).bg(t.bg_panel),
      ))
      .alignment(Alignment::Center),
      inner,
    );
    return;
  }

  let mask_str = |s: &str| -> String { "*".repeat(s.chars().count()) };

  let secret_status = |s: &str| -> String {
    let n = s.chars().count();
    if n == 0 { "not set".to_string() } else { format!("{n} chars stored") }
  };

  let secret_value = |field: usize, stored: &str| -> String {
    if app.settings.editing && app.settings.field == field {
      format!("{}_", mask_str(&app.settings.edit_buf))
    } else if stored.is_empty() {
      "not set".to_string()
    } else {
      mask_str(stored)
    }
  };

  let selected_style = t.style_selection_text();
  let header_style = Style::default().fg(t.header).add_modifier(Modifier::BOLD);
  let text_style = Style::default().fg(t.text);
  let dim_style = Style::default().fg(t.text_dim);
  let accent_style = Style::default().fg(t.accent);
  let success_style = Style::default().fg(t.success);
  let bg_style = Style::default().bg(t.bg_panel);

  let cats = app.config.sources.arxiv_categories.join(", ");
  let mut active: Vec<String> = app
    .config
    .sources
    .enabled_sources
    .iter()
    .filter(|(_, v)| **v)
    .map(|(k, _)| k.clone())
    .collect();
  active.sort();
  let active_str =
    if active.is_empty() { "none".to_string() } else { active.join(", ") };
  let custom_count = app.config.sources.custom_feeds.len();
  let custom_str = if custom_count == 0 {
    "none".to_string()
  } else {
    custom_count.to_string()
  };

  let body_footer = Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Min(0), Constraint::Length(2)])
    .split(inner);
  let body = body_footer[0];
  let footer_area = body_footer[1];

  let columns = if body.width >= 96 {
    Layout::default()
      .direction(Direction::Horizontal)
      .constraints([Constraint::Length(31), Constraint::Min(0)])
      .split(body)
  } else {
    Layout::default()
      .direction(Direction::Horizontal)
      .constraints([Constraint::Length(0), Constraint::Min(0)])
      .split(body)
  };

  if columns[0].width > 0 {
    let rail = columns[0];
    let rail_rule = "─".repeat(rail.width.saturating_sub(4) as usize);
    let mut rail_lines = vec![
      Line::from(""),
      Line::from(Span::styled("  Configuration", header_style)),
      Line::from(Span::styled(format!("  {rail_rule}"), dim_style)),
      Line::from(""),
      Line::from(vec![
        Span::styled("  GitHub          ", dim_style),
        Span::styled(secret_status(&app.settings.github_token), text_style),
      ]),
      Line::from(vec![
        Span::styled("  Semantic Scholar", dim_style),
        Span::styled(
          format!(" {}", secret_status(&app.settings.s2_key)),
          text_style,
        ),
      ]),
      Line::from(vec![
        Span::styled("  Claude          ", dim_style),
        Span::styled(secret_status(&app.settings.claude_key), text_style),
      ]),
      Line::from(vec![
        Span::styled("  OpenAI          ", dim_style),
        Span::styled(secret_status(&app.settings.openai_key), text_style),
      ]),
      Line::from(""),
      Line::from(Span::styled("  Sources", header_style)),
      Line::from(Span::styled(format!("  {rail_rule}"), dim_style)),
      Line::from(vec![
        Span::styled("  arXiv categories", dim_style),
        Span::styled(format!(" {}", truncate(&cats, 12)), text_style),
      ]),
      Line::from(vec![
        Span::styled("  Active sources  ", dim_style),
        Span::styled(format!(" {}", active.len()), text_style),
      ]),
      Line::from(vec![
        Span::styled("  Custom feeds    ", dim_style),
        Span::styled(format!(" {custom_str}"), text_style),
      ]),
    ];

    if app.settings.save_time.is_some() {
      rail_lines.push(Line::from(""));
      rail_lines.push(Line::from(Span::styled("  Saved.", success_style)));
    }

    frame.render_widget(
      Paragraph::new(rail_lines).wrap(Wrap { trim: false }).style(bg_style),
      rail,
    );
  }

  let settings_area = columns[1];
  let row_width = settings_area.width.saturating_sub(2) as usize;
  let value_width = row_width.saturating_sub(32);
  let row = |field: usize, label: &str, value: String| -> Line<'static> {
    let selected = app.settings.field == field;
    let marker = if selected { ">" } else { " " };
    let style = if selected {
      if app.settings.editing { success_style } else { selected_style }
    } else {
      text_style
    };
    let label = truncate(label, 24);
    let value = truncate(&value, value_width);
    let content = format!(
      " {marker} {label:<24} {value:<value_width$}",
      value_width = value_width
    );
    Line::from(Span::styled(content, style))
  };

  let hint = |field: usize, text: &str| -> Line<'static> {
    let prefix = if app.settings.field == field { "   " } else { "   " };
    Line::from(Span::styled(
      format!("{prefix}{}", truncate(text, row_width.saturating_sub(3))),
      dim_style,
    ))
  };

  let mut lines: Vec<Line> = vec![
    Line::from(""),
    Line::from(Span::styled("  API Keys", header_style)),
    row(0, "GitHub token", secret_value(0, &app.settings.github_token)),
    hint(0, "Repo viewer access"),
    row(1, "Semantic Scholar key", secret_value(1, &app.settings.s2_key)),
    hint(1, "Improves paper metadata"),
    Line::from(""),
    Line::from(Span::styled("  Chat", header_style)),
    row(2, "Claude API key", secret_value(2, &app.settings.claude_key)),
    hint(2, "Used for claude: chat routing"),
    row(3, "OpenAI API key", secret_value(3, &app.settings.openai_key)),
    hint(3, "Used for openai: chat routing"),
    row(4, "Default provider", app.settings.default_chat_provider.clone()),
    hint(4, "Enter toggles provider"),
    Line::from(""),
    Line::from(Span::styled("  Appearance", header_style)),
    row(5, "Theme", app.active_theme_name()),
    hint(5, "Enter opens the theme picker"),
    Line::from(""),
    Line::from(Span::styled("  Sources", header_style)),
    Line::from(vec![
      Span::styled("  arXiv categories  ", dim_style),
      Span::styled(truncate(&cats, row_width.saturating_sub(21)), text_style),
    ]),
    Line::from(vec![
      Span::styled("  Active sources    ", dim_style),
      Span::styled(
        truncate(&active_str, row_width.saturating_sub(21)),
        text_style,
      ),
    ]),
    Line::from(vec![
      Span::styled("  Custom feeds      ", dim_style),
      Span::styled(custom_str, text_style),
    ]),
    Line::from(vec![
      Span::styled("  p", accent_style),
      Span::styled(" manages source subscriptions", dim_style),
    ]),
  ];

  if app.settings.save_time.is_some() {
    lines.push(Line::from(Span::styled("  Saved.", success_style)));
  }

  let selected_line = match app.settings.field {
    0 => 2,
    1 => 4,
    2 => 8,
    3 => 10,
    4 => 12,
    5 => 16,
    _ => 0,
  };
  let viewport_rows = settings_area.height as usize;
  let scroll = if selected_line >= viewport_rows.saturating_sub(2) {
    selected_line.saturating_sub(viewport_rows.saturating_sub(3))
  } else {
    0
  };

  let para = Paragraph::new(lines)
    .wrap(Wrap { trim: false })
    .scroll((scroll as u16, 0))
    .style(bg_style);
  frame.render_widget(para, settings_area);

  let footer_text = if app.settings.editing {
    "  enter apply edit · esc cancel edit"
  } else {
    "  j/k navigate · enter edit/select · s save · p sources · esc/q back"
  };
  draw_card_footer(frame, footer_area, &t, footer_text);
}
