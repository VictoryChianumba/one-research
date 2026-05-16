use std::{env, path::Path, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Direction, Layout, Rect},
  style::{Color, Style},
  widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use tui_textarea::{CursorMove, TextArea};

use crate::{Note, theme};

use super::{PopupReturn, ui_functions::centered_rect_exact_height};

pub type ExportPopupReturn = PopupReturn<(PathBuf, Option<String>)>;

const FOOTER_TEXT: &str = "Enter: confirm | Esc or <Ctrl-c>: Cancel";
const FOOTER_MARGINE: u16 = 8;
const DEFAULT_FILE_NAME: &str = "hygg_notes_export.json";

pub struct ExportPopup<'a> {
  path_txt: TextArea<'a>,
  path_err_msg: String,
  /// `note_id` of the note being exported, or `None` for multi-select.
  note_id: Option<String>,
  paragraph_text: String,
}

impl ExportPopup<'_> {
  pub fn create_note_content(
    note: &Note,
    default_path: Option<&Path>,
  ) -> anyhow::Result<Self> {
    let mut path = if let Some(p) = default_path {
      p.to_path_buf()
    } else {
      env::current_dir()?
    };

    // Add filename if it's not already defined
    if path.extension().is_none() {
      path.push(format!("{}.txt", note.title.as_str()));
    }

    let mut path_txt = TextArea::new(vec![path.to_string_lossy().to_string()]);
    path_txt.move_cursor(CursorMove::End);

    let paragraph_text = format!("Note: {}", note.title.to_owned());

    let mut export_popup = ExportPopup {
      path_txt,
      path_err_msg: String::default(),
      note_id: Some(note.note_id.clone()),
      paragraph_text,
    };

    export_popup.validate_path();

    Ok(export_popup)
  }

  pub fn create_multi_select(
    count: usize,
    default_path: Option<&Path>,
  ) -> anyhow::Result<Self> {
    let mut path = if let Some(p) = default_path {
      p.to_path_buf()
    } else {
      env::current_dir()?
    };

    // Add filename if it's not already defined
    if path.extension().is_none() {
      path.push(DEFAULT_FILE_NAME);
    }

    let mut path_txt = TextArea::new(vec![path.to_string_lossy().to_string()]);
    path_txt.move_cursor(CursorMove::End);

    let paragraph_text = format!("Export the selected {count} notes");

    let mut export_popup = ExportPopup {
      path_txt,
      path_err_msg: String::default(),
      note_id: None,
      paragraph_text,
    };

    export_popup.validate_path();

    Ok(export_popup)
  }

  fn validate_path(&mut self) {
    let path = self
      .path_txt
      .lines()
      .first()
      .expect("Path Textbox should always have one line");

    if path.is_empty() {
      self.path_err_msg = "Path can't be empty".into();
    } else {
      self.path_err_msg.clear();
    }
  }

  fn is_input_valid(&self) -> bool {
    self.path_err_msg.is_empty()
  }

  fn is_multi_select_mode(&self) -> bool {
    self.note_id.is_none()
  }

  pub fn render_widget(&mut self, frame: &mut Frame, area: Rect) {
    let mut area = centered_rect_exact_height(70, 11, area);

    if area.width < FOOTER_TEXT.len() as u16 + FOOTER_MARGINE {
      area.height += 1;
    }

    let block = Block::default()
      .borders(Borders::ALL)
      .border_style(Style::default().fg(theme::current().border))
      .title(if self.is_multi_select_mode() {
        "─── Export notes ───"
      } else {
        "─── Export note ───"
      });

    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
      .direction(Direction::Vertical)
      .horizontal_margin(4)
      .vertical_margin(2)
      .constraints(
        [
          Constraint::Length(2),
          Constraint::Length(3),
          Constraint::Length(1),
          Constraint::Min(1),
        ]
        .as_ref(),
      )
      .split(area);

    let note_paragraph =
      Paragraph::new(self.paragraph_text.as_str()).wrap(Wrap { trim: false });
    frame.render_widget(note_paragraph, chunks[0]);

    if self.path_err_msg.is_empty() {
      let block_style = Style::default().fg(theme::current().accent);
      let cursor_style = Style::default()
        .fg(theme::current().accent)
        .bg(theme::current().accent);
      self.path_txt.set_style(block_style);
      self.path_txt.set_cursor_style(cursor_style);
      self.path_txt.set_block(
        Block::default().borders(Borders::ALL).style(block_style).title("Path"),
      );
    } else {
      let block_style = Style::default().fg(Color::Red);
      let cursor_style = Style::default().fg(Color::Red);
      self.path_txt.set_style(block_style);
      self.path_txt.set_cursor_style(cursor_style);
      self.path_txt.set_block(
        Block::default()
          .borders(Borders::ALL)
          .style(block_style)
          .title(format!("Path : {}", self.path_err_msg)),
      );
    }

    self.path_txt.set_cursor_line_style(Style::default());

    frame.render_widget(&self.path_txt, chunks[1]);

    let footer = Paragraph::new(FOOTER_TEXT)
      .alignment(Alignment::Center)
      .wrap(Wrap { trim: false });

    frame.render_widget(footer, chunks[3]);
  }

  pub fn handle_input(&mut self, key: KeyEvent) -> ExportPopupReturn {
    let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
      KeyCode::Esc => ExportPopupReturn::Cancel,
      KeyCode::Char('c') if has_ctrl => ExportPopupReturn::Cancel,
      KeyCode::Enter => self.handle_confirm(),
      _ => {
        if self.path_txt.input(key) {
          self.validate_path();
        }
        ExportPopupReturn::KeepPopup
      }
    }
  }

  fn handle_confirm(&mut self) -> ExportPopupReturn {
    self.validate_path();
    if !self.is_input_valid() {
      return ExportPopupReturn::KeepPopup;
    }

    let path: PathBuf = self
      .path_txt
      .lines()
      .first()
      .expect("Path Textbox should always have one line")
      .parse()
      .expect("PathBuf from string should never fail");

    ExportPopupReturn::Apply((path, self.note_id.clone()))
  }
}
