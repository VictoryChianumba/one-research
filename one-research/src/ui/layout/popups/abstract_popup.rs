use ratatui::{
  Frame,
  style::{Modifier, Style},
  text::Line,
  widgets::{Clear, Paragraph, Wrap},
};

use super::super::widgets::{popup_inner, popup_rect, quiet_popup_block};
use crate::app::App;

pub fn draw_abstract_popup(frame: &mut Frame, app: &App) {
  let t = app.theme();
  let area = frame.area();
  let Some(item) = app.selected_item() else { return };

  let popup_w = (area.width * 70 / 100).max(52).min(area.width);
  let content_w = popup_w.saturating_sub(6) as usize;
  let title_wrapped: Vec<Line> = textwrap::wrap(&item.title, content_w)
    .into_iter()
    .take(3)
    .map(|s| {
      Line::styled(
        s.to_string(),
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
      )
    })
    .collect();

  let body_wrapped: Vec<Line> = if item.summary_short.is_empty() {
    vec![Line::styled(
      "No abstract available.",
      Style::default().fg(t.text_dim),
    )]
  } else {
    textwrap::wrap(&item.summary_short, content_w)
      .into_iter()
      // Bright default foreground (white) so the abstract stands out.
      .map(|s| Line::styled(s.to_string(), Style::default()))
      .collect()
  };

  let desired_h = (title_wrapped.len() + body_wrapped.len() + 5)
    .clamp(9, area.height as usize);
  let popup_rect = popup_rect(area, 70, desired_h as u16, 52, 9, 92);

  frame.render_widget(Clear, popup_rect);

  // No box title — section headers / inline hotkey hints were dropped from the
  // design language; Space/Esc dismiss is muscle-memory and in the help overlay.
  let block = quiet_popup_block("", &t);

  let block_inner = block.inner(popup_rect);
  let inner = popup_inner(block_inner, 1, 1);
  frame.render_widget(block, popup_rect);

  if inner.height == 0 {
    return;
  }

  let mut lines: Vec<Line> = Vec::new();
  lines.extend(title_wrapped);
  lines.push(Line::raw(""));
  lines.extend(body_wrapped);

  let para = Paragraph::new(lines).wrap(Wrap { trim: false });
  frame.render_widget(para, inner);
}
