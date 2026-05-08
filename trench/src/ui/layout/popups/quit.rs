use ratatui::{
  Frame,
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Block, Borders, Clear, Paragraph},
};

use super::super::widgets::{popup_inner, popup_rect};
use crate::app::{App, QuitPopupKind};

pub fn draw_quit_popup(frame: &mut Frame, app: &App) {
  let t = app.theme();
  let area = frame.area();

  let (title, body, action) = match app.quit_popup.kind {
    QuitPopupKind::QuitApp => (
      " quit trench? ",
      &["Feed, progress and sessions are", "saved automatically."][..],
      "quit",
    ),
    QuitPopupKind::QuitWithProgress => (
      " quit trench? ",
      &["Discovery in progress will be", "cancelled."][..],
      "quit",
    ),
    QuitPopupKind::QuitWithChat => (
      " quit trench? ",
      &["You have an unsent message", "in chat."][..],
      "quit",
    ),
    QuitPopupKind::LeaveReader => {
      (" close reader ", &["Your reading position is saved."][..], "close")
    }
  };

  let popup_rect = popup_rect(area, 38, 9, 44, 9, 60);
  frame.render_widget(Clear, popup_rect);

  let block = Block::default()
    .borders(Borders::ALL)
    .border_style(Style::default().fg(t.border_active))
    .title(Span::styled(
      title,
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    ));

  let inner = popup_inner(block.inner(popup_rect), 2, 1);
  frame.render_widget(block, popup_rect);

  if inner.height == 0 {
    return;
  }

  let mut lines: Vec<Line> = Vec::new();
  for &line in body {
    lines.push(Line::styled(line.to_string(), Style::default().fg(t.text)));
  }
  lines.push(Line::raw(""));
  lines.push(Line::from(vec![
    Span::styled(
      "q · Enter  ",
      Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    ),
    Span::styled(format!("{action}     "), Style::default().fg(t.text_dim)),
    Span::styled("Esc  cancel", Style::default().fg(t.text_dim)),
  ]));

  frame.render_widget(Paragraph::new(lines), inner);
}
