use ratatui::{
  Frame,
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Block, Borders, Clear, Paragraph},
};

use super::super::widgets::{popup_inner, popup_rect};
use crate::app::App;

pub fn draw_tag_picker(frame: &mut Frame, app: &App) {
  let t = app.theme();
  let area = frame.area();

  let all = crate::tags::all_tags(&app.workspace.item_tags);
  let target_count = app.tag_picker.target_urls.len();
  let target_label = if target_count == 1 {
    "1 item".to_string()
  } else {
    format!("{target_count} items")
  };

  // Find which tags are present on ALL targets.
  let common_on_all: std::collections::HashSet<String> = all
    .iter()
    .filter(|tag| {
      app.tag_picker.target_urls.iter().all(|url| {
        crate::tags::for_url(&app.workspace.item_tags, url)
          .iter()
          .any(|t| t == *tag)
      })
    })
    .cloned()
    .collect();

  // Visible rows: cap to popup body height.
  let body_h = (all.len() as u16 + 5).clamp(8, 20);
  let popup_rect = popup_rect(area, 50, body_h, 50, 8, 70);
  frame.render_widget(Clear, popup_rect);

  let block = Block::default()
    .borders(Borders::ALL)
    // Match the main content boxes (`t.border`) for consistent popup borders.
    .border_style(Style::default().fg(t.border))
    .title(Span::styled(
      format!(" tags · {target_label} "),
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    ));
  let inner = popup_inner(block.inner(popup_rect), 2, 1);
  frame.render_widget(block, popup_rect);
  if inner.height == 0 {
    return;
  }

  let mut lines: Vec<Line> = Vec::new();
  // Input row
  lines.push(Line::from(vec![
    Span::styled("+ ", Style::default().fg(t.accent)),
    Span::styled(
      format!("{}█", app.tag_picker.input),
      Style::default().fg(t.text),
    ),
  ]));
  lines.push(Line::raw(""));

  if all.is_empty() {
    lines.push(Line::from(Span::styled(
      "No tags yet. Type a name and press Enter.",
      Style::default().fg(t.text_dim),
    )));
  } else {
    for (i, tag) in all.iter().enumerate() {
      let is_selected = i == app.tag_picker.selected;
      let active = common_on_all.contains(tag);
      let count = crate::tags::count_for(&app.workspace.item_tags, tag);
      let arrow = if is_selected {
        Span::styled("→ ", Style::default().fg(t.accent))
      } else {
        Span::raw("  ")
      };
      let checkbox = if active { "[x] " } else { "[ ] " };
      let row_style = if is_selected {
        Style::default().fg(t.text).add_modifier(Modifier::BOLD)
      } else {
        Style::default().fg(t.text_dim)
      };
      lines.push(Line::from(vec![
        arrow,
        Span::styled(checkbox, row_style),
        Span::styled(tag.clone(), row_style),
        Span::styled(format!("  ({count})"), Style::default().fg(t.text_dim)),
      ]));
    }
  }

  lines.push(Line::raw(""));
  lines.push(Line::from(Span::styled(
    "↑↓ navigate · Space toggle · Enter add new · Esc close",
    Style::default().fg(t.text_dim),
  )));

  frame.render_widget(Paragraph::new(lines), inner);
}
