use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Direction, Layout, Rect},
  style::{Color, Modifier, Style},
  text::{Line, Span},
  widgets::{Clear, Paragraph, Wrap},
};

use super::super::widgets::{
  draw_card_footer, settings_card_block, settings_modal_rect, swatch, truncate,
};
use crate::app::App;
use crate::config::{self, CUSTOM_THEME_ROLES};

pub fn draw_theme_picker(frame: &mut Frame, app: &App) {
  let t = app.theme();
  let area = frame.area();
  let popup = settings_modal_rect(area);

  frame.render_widget(Clear, popup);
  let block = settings_card_block(" Theme ", &t);
  let inner = block.inner(popup);
  frame.render_widget(block, popup);

  if inner.width < 44 || inner.height < 14 {
    frame.render_widget(
      Paragraph::new(Span::styled(
        " terminal too small for theme picker ",
        Style::default().fg(t.text_dim).bg(t.bg_panel),
      ))
      .alignment(Alignment::Center),
      inner,
    );
    return;
  }

  let body_footer = Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Min(0), Constraint::Length(2)])
    .split(inner);
  let body = body_footer[0];
  let footer_area = body_footer[1];

  let columns = if body.width >= 96 {
    Layout::default()
      .direction(Direction::Horizontal)
      .constraints([Constraint::Length(29), Constraint::Min(0)])
      .split(body)
  } else {
    Layout::default()
      .direction(Direction::Horizontal)
      .constraints([Constraint::Length(0), Constraint::Min(0)])
      .split(body)
  };
  let picker_area = columns[1];
  let picker_rows = Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Length(3), Constraint::Min(0)])
    .split(picker_area);
  let header_area = picker_rows[0];
  let list_area = picker_rows[1];
  let list_h = list_area.height;

  let all = ui_theme::ThemeId::all();
  let active_name = app.active_theme_name();

  if columns[0].width > 0 {
    let rail = columns[0];
    let rail_rule = "─".repeat(rail.width.saturating_sub(4) as usize);
    let custom_count = app.config.custom_themes.len();
    let rail_lines = vec![
      Line::from(""),
      Line::from(Span::styled(
        "  Appearance",
        Style::default().fg(t.header).add_modifier(Modifier::BOLD),
      )),
      Line::from(Span::styled(
        format!("  {rail_rule}"),
        Style::default().fg(t.text_dim),
      )),
      Line::from(""),
      Line::from(vec![
        Span::styled("  Active theme  ", Style::default().fg(t.text_dim)),
        Span::styled(
          truncate(&active_name, rail.width.saturating_sub(18) as usize),
          Style::default().fg(t.text),
        ),
      ]),
      Line::from(vec![
        Span::styled("  Presets       ", Style::default().fg(t.text_dim)),
        Span::styled(all.len().to_string(), Style::default().fg(t.text)),
      ]),
      Line::from(vec![
        Span::styled("  Custom        ", Style::default().fg(t.text_dim)),
        Span::styled(custom_count.to_string(), Style::default().fg(t.text)),
      ]),
      Line::from(""),
      Line::from(Span::styled(
        "  Swatches",
        Style::default().fg(t.header).add_modifier(Modifier::BOLD),
      )),
      Line::from(Span::styled(
        format!("  {rail_rule}"),
        Style::default().fg(t.text_dim),
      )),
      Line::from(vec![
        Span::styled("  accent ", Style::default().fg(t.text_dim)),
        swatch(t.accent),
        Span::styled(" header ", Style::default().fg(t.text_dim)),
        swatch(t.header),
      ]),
      Line::from(vec![
        Span::styled("  select ", Style::default().fg(t.text_dim)),
        swatch(t.bg_selection),
        Span::styled(" ok ", Style::default().fg(t.text_dim)),
        swatch(t.success),
      ]),
    ];

    frame.render_widget(
      Paragraph::new(rail_lines)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(t.bg_panel)),
      rail,
    );
  }

  let title_rule = "─".repeat(header_area.width.saturating_sub(2) as usize);
  let header_lines = vec![
    Line::from(vec![
      Span::styled("  ", Style::default().fg(t.text_dim)),
      Span::styled(
        "Theme Library",
        Style::default().fg(t.header).add_modifier(Modifier::BOLD),
      ),
      Span::styled(
        format!(
          "  {}",
          truncate(&active_name, header_area.width.saturating_sub(20) as usize)
        ),
        Style::default().fg(t.text_dim),
      ),
    ]),
    Line::from(Span::styled(
      format!("  {title_rule}"),
      Style::default().fg(t.text_dim),
    )),
    Line::from(""),
  ];
  frame.render_widget(
    Paragraph::new(header_lines).style(Style::default().bg(t.bg_panel)),
    header_area,
  );

  let mut rows: Vec<(Option<usize>, Line)> = Vec::new();
  let mut last_group: Option<ui_theme::ThemeGroup> = None;

  for (idx, id) in all.iter().enumerate() {
    let info = id.info();
    if last_group != Some(info.group) {
      rows.push((
        None,
        Line::from(Span::styled(
          format!("  {}", info.group.label()),
          Style::default().fg(t.header).add_modifier(Modifier::BOLD),
        )),
      ));
      last_group = Some(info.group);
    }

    let theme = id.theme();
    let selected = idx == app.theme_picker_cursor;
    let row_style = if selected {
      t.style_selection_text()
    } else {
      Style::default().fg(t.text)
    };
    let marker = if selected { ">" } else { " " };
    rows.push((
      Some(idx),
      Line::from(vec![
        Span::styled(format!(" {marker} "), row_style),
        Span::styled(format!("{:<20}", info.name), row_style),
        swatch(theme.accent),
        swatch(theme.header),
        swatch(theme.text_dim),
        swatch(theme.bg_selection),
        swatch(theme.success),
        swatch(theme.warning),
        swatch(theme.error),
        Span::styled(format!("  {}", info.id), Style::default().fg(t.text_dim)),
      ]),
    ));
  }

  let custom_start = all.len();
  rows.push((None, Line::from("")));
  rows.push((
    None,
    Line::from(Span::styled(
      "  Custom",
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    )),
  ));

  for (idx, custom) in app.config.custom_themes.iter().enumerate() {
    let row_idx = custom_start + idx;
    let theme = custom.to_theme();
    let selected = row_idx == app.theme_picker_cursor;
    let row_style = if selected {
      t.style_selection_text()
    } else {
      Style::default().fg(t.text)
    };
    let marker = if selected { ">" } else { " " };
    rows.push((
      Some(row_idx),
      Line::from(vec![
        Span::styled(format!(" {marker} "), row_style),
        Span::styled(format!("{:<20}", custom.name), row_style),
        swatch(theme.accent),
        swatch(theme.header),
        swatch(theme.text_dim),
        swatch(theme.bg_selection),
        swatch(theme.success),
        swatch(theme.warning),
        swatch(theme.error),
        Span::styled(
          format!("  based on {}", custom.base.info().name),
          Style::default().fg(t.text_dim),
        ),
      ]),
    ));
  }

  let new_row = custom_start + app.config.custom_themes.len();
  let selected = new_row == app.theme_picker_cursor;
  let row_style = if selected {
    t.style_selection_text()
  } else {
    Style::default().fg(t.text_dim)
  };
  let marker = if selected { ">" } else { " " };
  rows.push((
    Some(new_row),
    Line::from(vec![
      Span::styled(format!(" {marker} "), row_style),
      Span::styled("+ New custom theme", row_style),
    ]),
  ));

  let selected_line = rows
    .iter()
    .position(|(idx, _)| *idx == Some(app.theme_picker_cursor))
    .unwrap_or(0);
  let max_start = rows.len().saturating_sub(list_h as usize);
  let mut start = app.theme_picker_scroll.min(max_start);
  if selected_line < start {
    start = selected_line;
  } else if selected_line >= start + list_h as usize {
    start = selected_line.saturating_sub(list_h as usize - 1);
  }
  start = start.min(max_start);
  let lines: Vec<Line> = rows
    .into_iter()
    .skip(start)
    .take(list_h as usize)
    .map(|(_, line)| line)
    .collect();

  frame.render_widget(
    Paragraph::new(lines).style(Style::default().bg(t.bg_panel)),
    list_area,
  );

  draw_card_footer(
    frame,
    footer_area,
    &t,
    "  j/k preview · enter select/new · e edit · d delete · esc cancel",
  );

  if app.custom_theme_editor.is_some() {
    draw_custom_theme_editor(frame, app);
  }
}

fn draw_custom_theme_editor(frame: &mut Frame, app: &App) {
  let Some(editor) = app.custom_theme_editor.as_ref() else {
    return;
  };
  let t = editor.theme.to_theme();
  let area = frame.area();
  let popup = settings_modal_rect(area);

  frame.render_widget(Clear, popup);
  let title =
    if editor.is_new { " New Custom Theme " } else { " Edit Custom Theme " };
  let block = settings_card_block(title, &t);
  let inner = block.inner(popup);
  frame.render_widget(block, popup);

  if inner.width < 44 || inner.height < 14 {
    frame.render_widget(
      Paragraph::new(Span::styled(
        " terminal too small for custom theme editor ",
        Style::default().fg(t.text_dim).bg(t.bg_panel),
      ))
      .alignment(Alignment::Center),
      inner,
    );
    return;
  }

  let rows = Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Min(0), Constraint::Length(2)])
    .split(inner);
  let body = rows[0];
  let footer = rows[1];

  if editor.mode == crate::app::CustomThemeEditorMode::DeleteConfirm {
    let lines = vec![
      Line::from(""),
      Line::from(vec![
        Span::styled("  Delete ", Style::default().fg(t.warning)),
        Span::styled(
          editor.theme.name.clone(),
          Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled("?", Style::default().fg(t.warning)),
      ]),
      Line::from(""),
      Line::from(Span::styled(
        "  y: delete  n / esc: cancel",
        Style::default().fg(t.text_dim),
      )),
    ];
    frame.render_widget(
      Paragraph::new(lines).style(Style::default().bg(t.bg_panel)),
      body,
    );
    draw_card_footer(frame, footer, &t, "  y delete · n/Esc cancel");
    return;
  }

  let cols = if body.width >= 86 {
    Layout::default()
      .direction(Direction::Horizontal)
      .constraints([Constraint::Length(36), Constraint::Min(34)])
      .split(body)
  } else {
    Layout::default()
      .direction(Direction::Horizontal)
      .constraints([Constraint::Length(0), Constraint::Min(0)])
      .split(body)
  };

  if cols[0].width > 0 {
    draw_custom_theme_roles(frame, cols[0], app);
  }
  draw_custom_theme_palette(frame, cols[1], app);

  let footer_text = match editor.mode {
    crate::app::CustomThemeEditorMode::Name => " enter: save name  esc: cancel",
    crate::app::CustomThemeEditorMode::Hex => " enter: apply hex  esc: cancel",
    _ => {
      "  space apply · h/l hue · [/ ] shade · x hex · n rename · r reset · s/enter save"
    }
  };
  draw_card_footer(frame, footer, &t, footer_text);
}

fn draw_custom_theme_roles(frame: &mut Frame, area: Rect, app: &App) {
  let Some(editor) = app.custom_theme_editor.as_ref() else {
    return;
  };
  let t = editor.theme.to_theme();
  let mut lines = vec![
    Line::from(vec![
      Span::styled("  Name  ", Style::default().fg(t.text_dim)),
      Span::styled(
        editor.theme.name.clone(),
        Style::default().fg(t.text).add_modifier(Modifier::BOLD),
      ),
    ]),
    Line::from(vec![
      Span::styled("  Base  ", Style::default().fg(t.text_dim)),
      Span::styled(
        editor.theme.base.info().name,
        Style::default().fg(t.text_dim),
      ),
    ]),
    Line::from(""),
  ];

  for (idx, role) in CUSTOM_THEME_ROLES.iter().enumerate() {
    let selected = idx == editor.role_cursor;
    let style = if selected {
      t.style_selection_text()
    } else {
      Style::default().fg(t.text)
    };
    let marker = if selected { ">" } else { " " };
    let value = editor.theme.colors.get_role(role.key).unwrap_or("#000000");
    lines.push(Line::from(vec![
      Span::styled(format!(" {marker} "), style),
      Span::styled(format!("{:<16}", role.label), style),
      color_swatch_from_hex(value),
      Span::styled(format!(" {value}"), Style::default().fg(t.text_dim)),
    ]));
  }

  frame.render_widget(
    Paragraph::new(lines).style(Style::default().bg(t.bg_panel)),
    area,
  );
}

fn draw_custom_theme_palette(frame: &mut Frame, area: Rect, app: &App) {
  let Some(editor) = app.custom_theme_editor.as_ref() else {
    return;
  };
  let t = editor.theme.to_theme();
  let role =
    CUSTOM_THEME_ROLES[editor.role_cursor.min(CUSTOM_THEME_ROLES.len() - 1)];
  let current = editor.theme.colors.get_role(role.key).unwrap_or("#000000");
  let mut lines = Vec::new();

  match editor.mode {
    crate::app::CustomThemeEditorMode::Name => {
      lines.push(Line::from(Span::styled(
        "Rename theme",
        Style::default().fg(t.header).add_modifier(Modifier::BOLD),
      )));
      lines.push(Line::from(""));
      lines.push(Line::from(vec![
        Span::styled("  ", Style::default().bg(t.bg_input)),
        Span::styled(
          editor.edit_buf.clone(),
          Style::default().fg(t.text).bg(t.bg_input),
        ),
        Span::styled(" ", Style::default().fg(t.cursor_fg).bg(t.cursor_bg)),
      ]));
    }
    crate::app::CustomThemeEditorMode::Hex => {
      lines.push(Line::from(Span::styled(
        format!("Hex for {}", role.label),
        Style::default().fg(t.header).add_modifier(Modifier::BOLD),
      )));
      lines.push(Line::from(""));
      lines.push(Line::from(vec![
        Span::styled("  ", Style::default().bg(t.bg_input)),
        Span::styled(
          editor.edit_buf.clone(),
          Style::default().fg(t.text).bg(t.bg_input),
        ),
        Span::styled(" ", Style::default().fg(t.cursor_fg).bg(t.cursor_bg)),
      ]));
    }
    _ => {
      lines.push(Line::from(vec![
        Span::styled("Editing ", Style::default().fg(t.text_dim)),
        Span::styled(
          role.label,
          Style::default().fg(t.header).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  current ", Style::default().fg(t.text_dim)),
        color_swatch_from_hex(current),
        Span::styled(format!(" {current}"), Style::default().fg(t.text_dim)),
        Span::styled("  picker ", Style::default().fg(t.text_dim)),
        palette_swatch_from_hex(selected_palette_view_hex(editor), true),
        Span::styled(
          format!(" {}", selected_palette_view_hex(editor)),
          Style::default().fg(t.text_dim),
        ),
      ]));
      lines.push(Line::from(""));
      for (shade_idx, row) in THEME_PALETTE_VIEW.iter().enumerate() {
        let mut spans = Vec::new();
        spans.push(Span::styled("  ", Style::default().fg(t.text_dim)));
        for (hue_idx, hex) in row.iter().enumerate() {
          let selected =
            shade_idx == editor.shade_cursor && hue_idx == editor.hue_cursor;
          spans.push(palette_swatch_from_hex(hex, selected));
        }
        lines.push(Line::from(spans));
      }
      lines.push(Line::from(""));
      lines.push(selection_contrast_line(&editor.theme));
      lines.push(Line::from(""));
      lines.push(Line::from(vec![
        Span::styled("Preview  ", Style::default().fg(t.text_dim)),
        Span::styled(
          "Header",
          Style::default().fg(t.header).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  normal text  ", Style::default().fg(t.text)),
        Span::styled("dim text  ", Style::default().fg(t.text_dim)),
        Span::styled(" selected row ", t.style_selection_text()),
      ]));
      lines.push(Line::from(vec![
        Span::styled("Status   ", Style::default().fg(t.text_dim)),
        Span::styled("v repo  ", Style::default().fg(t.success)),
        Span::styled("warning  ", Style::default().fg(t.warning)),
        Span::styled("error", Style::default().fg(t.error)),
      ]));
    }
  }

  frame.render_widget(
    Paragraph::new(lines)
      .wrap(Wrap { trim: false })
      .style(Style::default().bg(t.bg_panel)),
    area,
  );
}

const THEME_PALETTE_VIEW: &[&[&str]] = &[
  &[
    "#F8FAFC", "#F7FEE7", "#FEFCE8", "#FFFBEB", "#FFF7ED", "#FFF1F2",
    "#FEF2F2", "#FDF2F8", "#FDF4FF", "#FAF5FF", "#F5F3FF", "#EEF2FF",
    "#EFF6FF", "#F0F9FF", "#ECFEFF", "#F0FDFA",
  ],
  &[
    "#E2E8F0", "#ECFCCB", "#FEF9C3", "#FEF3C7", "#FFEDD5", "#FFE4E6",
    "#FEE2E2", "#FCE7F3", "#FAE8FF", "#F3E8FF", "#EDE9FE", "#E0E7FF",
    "#DBEAFE", "#E0F2FE", "#CFFAFE", "#CCFBF1",
  ],
  &[
    "#CBD5E1", "#D9F99D", "#FEF08A", "#FDE68A", "#FED7AA", "#FECDD3",
    "#FECACA", "#FBCFE8", "#F5D0FE", "#E9D5FF", "#DDD6FE", "#C7D2FE",
    "#BFDBFE", "#BAE6FD", "#A5F3FC", "#99F6E4",
  ],
  &[
    "#94A3B8", "#BEF264", "#FDE047", "#FCD34D", "#FDBA74", "#FDA4AF",
    "#FCA5A5", "#F9A8D4", "#F0ABFC", "#D8B4FE", "#C4B5FD", "#A5B4FC",
    "#93C5FD", "#7DD3FC", "#67E8F9", "#5EEAD4",
  ],
  &[
    "#64748B", "#A3E635", "#FACC15", "#F59E0B", "#FB923C", "#FB7185",
    "#F87171", "#F472B6", "#E879F9", "#C084FC", "#A78BFA", "#818CF8",
    "#60A5FA", "#38BDF8", "#22D3EE", "#2DD4BF",
  ],
  &[
    "#475569", "#84CC16", "#EAB308", "#D97706", "#F97316", "#F43F5E",
    "#EF4444", "#EC4899", "#D946EF", "#A855F7", "#8B5CF6", "#6366F1",
    "#3B82F6", "#0EA5E9", "#06B6D4", "#14B8A6",
  ],
  &[
    "#334155", "#65A30D", "#CA8A04", "#B45309", "#EA580C", "#E11D48",
    "#DC2626", "#DB2777", "#C026D3", "#9333EA", "#7C3AED", "#4F46E5",
    "#2563EB", "#0284C7", "#0891B2", "#0D9488",
  ],
  &[
    "#1E293B", "#4D7C0F", "#A16207", "#92400E", "#C2410C", "#BE123C",
    "#B91C1C", "#BE185D", "#A21CAF", "#7E22CE", "#6D28D9", "#4338CA",
    "#1D4ED8", "#0369A1", "#0E7490", "#0F766E",
  ],
  &[
    "#0F172A", "#3F6212", "#854D0E", "#78350F", "#9A3412", "#9F1239",
    "#991B1B", "#9D174D", "#86198F", "#6B21A8", "#5B21B6", "#3730A3",
    "#1E40AF", "#075985", "#155E75", "#115E59",
  ],
  &[
    "#020617", "#365314", "#713F12", "#451A03", "#7C2D12", "#4C0519",
    "#7F1D1D", "#831843", "#701A75", "#581C87", "#4C1D95", "#312E81",
    "#1E3A8A", "#0C4A6E", "#164E63", "#134E4A",
  ],
];

fn color_swatch_from_hex(hex: &str) -> Span<'static> {
  swatch(config::parse_hex_color(hex).unwrap_or(Color::Black))
}

fn palette_swatch_from_hex(hex: &str, selected: bool) -> Span<'static> {
  let color = config::parse_hex_color(hex).unwrap_or(Color::Black);
  if selected {
    Span::styled(
      "[]",
      Style::default()
        .fg(palette_marker_color(color))
        .bg(color)
        .add_modifier(Modifier::BOLD),
    )
  } else {
    Span::styled("██", Style::default().fg(color))
  }
}

fn palette_marker_color(color: Color) -> Color {
  let Color::Rgb(r, g, b) = color else {
    return Color::Black;
  };
  let luma =
    (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0;
  if luma > 0.55 { Color::Black } else { Color::White }
}

fn selected_palette_view_hex(
  editor: &crate::app::CustomThemeEditorState,
) -> &'static str {
  THEME_PALETTE_VIEW[editor.shade_cursor.min(THEME_PALETTE_VIEW.len() - 1)]
    [editor.hue_cursor.min(THEME_PALETTE_VIEW[0].len() - 1)]
}

fn selection_contrast_line(theme: &config::CustomThemeConfig) -> Line<'static> {
  let text = theme.colors.get_role("text").and_then(hex_luma).unwrap_or(1.0);
  let selection =
    theme.colors.get_role("bg_selection").and_then(hex_luma).unwrap_or(0.0);
  let diff = (text - selection).abs();
  let t = theme.to_theme();
  if diff < 0.22 {
    Line::from(Span::styled(
      "Selection contrast is low; text may blend into the selected row.",
      Style::default().fg(t.warning),
    ))
  } else {
    Line::from(Span::styled(
      "Selection contrast looks readable.",
      Style::default().fg(t.success),
    ))
  }
}

fn hex_luma(hex: &str) -> Option<f32> {
  let Color::Rgb(r, g, b) = config::parse_hex_color(hex)? else {
    return None;
  };
  Some((0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0)
}
