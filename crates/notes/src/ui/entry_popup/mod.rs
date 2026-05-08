use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Direction, Layout, Rect},
  style::{Color, Style},
  widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use tui_textarea::{CursorMove, TextArea};

use crate::{Note, theme};

use self::tags::{TagsPopup, TagsPopupReturn};

use super::ui_functions::centered_rect_exact_height;

mod tags;

const FOOTER_TEXT: &str = "Enter or <Ctrl-m>: confirm | Esc or <Ctrl-c>: Cancel | Tab: Change focused control | <Ctrl-Space> or <Ctrl-t>: Open tags";
const FOOTER_MARGIN: u16 = 15;

/// Data collected by the note popup on confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotePopupData {
  pub title: String,
  pub tags: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum NotePopupReturn {
  KeepPopup,
  Cancel,
  AddNote(NotePopupData),
  UpdateNote(NotePopupData),
}

pub struct NotePopup<'a> {
  title_txt: TextArea<'a>,
  tags_txt: TextArea<'a>,
  is_edit_note: bool,
  active_txt: ActiveText,
  title_err_msg: String,
  tags_err_msg: String,
  tags_popup: Option<TagsPopup>,
}

#[derive(Debug, PartialEq, Eq)]
enum ActiveText {
  Title,
  Tags,
}

impl NotePopup<'_> {
  pub fn new_note() -> Self {
    Self {
      title_txt: TextArea::default(),
      tags_txt: TextArea::default(),
      is_edit_note: false,
      active_txt: ActiveText::Title,
      title_err_msg: String::default(),
      tags_err_msg: String::default(),
      tags_popup: None,
    }
  }

  pub fn from_note(note: &Note) -> Self {
    let mut title_txt = TextArea::new(vec![note.title.to_owned()]);
    title_txt.move_cursor(CursorMove::End);

    let tags = tags_to_text(&note.tags);
    let mut tags_txt = TextArea::new(vec![tags]);
    tags_txt.move_cursor(CursorMove::End);

    let mut popup = Self {
      title_txt,
      tags_txt,
      is_edit_note: true,
      active_txt: ActiveText::Title,
      title_err_msg: String::default(),
      tags_err_msg: String::default(),
      tags_popup: None,
    };

    popup.validate_all();
    popup
  }

  /// New-note popup pre-filled with a title (e.g. from the selected feed item).
  pub fn with_prefill(title: &str) -> Self {
    let mut title_txt = TextArea::new(vec![title.to_owned()]);
    title_txt.move_cursor(CursorMove::End);

    let mut popup = Self {
      title_txt,
      tags_txt: TextArea::default(),
      is_edit_note: false,
      active_txt: ActiveText::Title,
      title_err_msg: String::default(),
      tags_err_msg: String::default(),
      tags_popup: None,
    };

    popup.validate_all();
    popup
  }

  pub fn render_widget(&mut self, frame: &mut Frame, area: Rect) {
    let mut area = centered_rect_exact_height(70, 12, area);

    const FOOTER_LEN: u16 = FOOTER_TEXT.len() as u16 + FOOTER_MARGIN;
    if area.width < FOOTER_LEN {
      area.height += FOOTER_LEN / area.width;
    }

    let block = Block::default()
      .borders(Borders::ALL)
      .border_style(Style::default().fg(theme::current().border))
      .title(if self.is_edit_note {
        "─── Edit note ───"
      } else {
        "─── Create note ───"
      });

    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
      .direction(Direction::Vertical)
      .horizontal_margin(4)
      .vertical_margin(2)
      .constraints(
        [
          Constraint::Length(3),
          Constraint::Length(3),
          Constraint::Min(1),
        ]
        .as_ref(),
      )
      .split(area);

    self.title_txt.set_cursor_line_style(Style::default());
    self.tags_txt.set_cursor_line_style(Style::default());

    let active_block_style = Style::default().fg(theme::current().accent);
    let reset_style = Style::reset();
    let invalid_block_style = Style::default().fg(Color::Red);
    let active_cursor_style =
      Style::default().fg(theme::current().accent).bg(theme::current().accent);
    let deactivate_cursor_style = Style::default().bg(Color::Reset);
    let invalid_cursor_style = Style::default().fg(Color::Red);

    // Title field
    if self.title_err_msg.is_empty() {
      let (block, cursor) = match self.active_txt {
        ActiveText::Title => (active_block_style, active_cursor_style),
        _ => (reset_style, deactivate_cursor_style),
      };
      self.title_txt.set_style(block);
      self.title_txt.set_cursor_style(cursor);
      self.title_txt.set_block(
        Block::default().borders(Borders::ALL).style(block).title("Title"),
      );
    } else {
      let cursor = if self.active_txt == ActiveText::Title {
        invalid_cursor_style
      } else {
        deactivate_cursor_style
      };
      self.title_txt.set_style(invalid_block_style);
      self.title_txt.set_cursor_style(cursor);
      self.title_txt.set_block(
        Block::default()
          .borders(Borders::ALL)
          .style(invalid_block_style)
          .title(format!("Title : {}", self.title_err_msg)),
      );
    }

    // Tags field
    if self.tags_err_msg.is_empty() {
      let (block, cursor, title) = match self.active_txt {
        ActiveText::Tags => (
          active_block_style,
          active_cursor_style,
          "Tags - A comma-separated list",
        ),
        _ => (reset_style, deactivate_cursor_style, "Tags"),
      };
      self.tags_txt.set_style(block);
      self.tags_txt.set_cursor_style(cursor);
      self.tags_txt.set_block(
        Block::default().borders(Borders::ALL).style(block).title(title),
      );
    } else {
      let cursor = if self.active_txt == ActiveText::Tags {
        invalid_cursor_style
      } else {
        deactivate_cursor_style
      };
      self.tags_txt.set_style(invalid_block_style);
      self.tags_txt.set_cursor_style(cursor);
      self.tags_txt.set_block(
        Block::default()
          .borders(Borders::ALL)
          .style(invalid_block_style)
          .title(format!("Tags : {}", self.tags_err_msg)),
      );
    }

    frame.render_widget(&self.title_txt, chunks[0]);
    frame.render_widget(&self.tags_txt, chunks[1]);

    let footer = Paragraph::new(FOOTER_TEXT)
      .alignment(Alignment::Center)
      .wrap(Wrap { trim: false })
      .block(Block::default().borders(Borders::NONE).style(Style::default()));

    frame.render_widget(footer, chunks[2]);

    if let Some(tags_popup) = self.tags_popup.as_mut() {
      tags_popup.render_widget(frame, area);
    }
  }

  pub fn is_input_valid(&self) -> bool {
    self.title_err_msg.is_empty() && self.tags_err_msg.is_empty()
  }

  pub fn validate_all(&mut self) {
    self.validate_title();
    self.validate_tags();
  }

  fn validate_title(&mut self) {
    let empty = self
      .title_txt
      .lines()
      .first()
      .is_none_or(|line| line.is_empty());
    if empty {
      self.title_err_msg = "Title can't be empty".into();
    } else {
      self.title_err_msg.clear();
    }
  }

  fn validate_tags(&mut self) {
    let tags = text_to_tags(
      self.tags_txt.lines().first().map(|s| s.as_str()).unwrap_or(""),
    );
    if tags.iter().any(|tag| tag.contains(',')) {
      self.tags_err_msg = "Tags are invalid".into();
    } else {
      self.tags_err_msg.clear();
    }
  }

  pub fn handle_input(&mut self, key: KeyEvent) -> NotePopupReturn {
    if self.tags_popup.is_some() {
      self.handle_tags_popup_input(key);
      return NotePopupReturn::KeepPopup;
    }

    let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
      KeyCode::Esc => NotePopupReturn::Cancel,
      KeyCode::Char('c') if has_ctrl => NotePopupReturn::Cancel,
      KeyCode::Enter => self.handle_confirm(),
      KeyCode::Char('m') if has_ctrl => self.handle_confirm(),
      KeyCode::Tab | KeyCode::Down => {
        self.active_txt = match self.active_txt {
          ActiveText::Title => ActiveText::Tags,
          ActiveText::Tags => ActiveText::Title,
        };
        NotePopupReturn::KeepPopup
      }
      KeyCode::Up => {
        self.active_txt = match self.active_txt {
          ActiveText::Title => ActiveText::Tags,
          ActiveText::Tags => ActiveText::Title,
        };
        NotePopupReturn::KeepPopup
      }
      KeyCode::Char(' ') | KeyCode::Char('t') if has_ctrl => {
        debug_assert!(self.tags_popup.is_none());

        let tags_text = self
          .tags_txt
          .lines()
          .first()
          .map(|s| s.as_str())
          .unwrap_or("");

        self.tags_popup = Some(TagsPopup::new(tags_text, Vec::new()));

        NotePopupReturn::KeepPopup
      }
      _ => {
        match self.active_txt {
          ActiveText::Title => {
            if self.title_txt.input(key) {
              self.validate_title();
            }
          }
          ActiveText::Tags => {
            if self.tags_txt.input(key) {
              self.validate_tags();
            }
          }
        }
        NotePopupReturn::KeepPopup
      }
    }
  }

  pub fn handle_tags_popup_input(&mut self, key: KeyEvent) {
    let tags_popup =
      self.tags_popup.as_mut().expect("Tags popup must be some at this point");

    match tags_popup.handle_input(key) {
      TagsPopupReturn::Keep => {}
      TagsPopupReturn::Cancel => self.tags_popup = None,
      TagsPopupReturn::Apply(tags_text) => {
        self.tags_txt = TextArea::new(vec![tags_text]);
        self.tags_txt.move_cursor(CursorMove::End);
        self.active_txt = ActiveText::Tags;
        self.tags_popup = None;
      }
    }
  }

  fn handle_confirm(&mut self) -> NotePopupReturn {
    self.validate_all();
    if !self.is_input_valid() {
      return NotePopupReturn::KeepPopup;
    }

    let title =
      self.title_txt.lines().first().cloned().unwrap_or_default();
    let tags = text_to_tags(
      self.tags_txt.lines().first().map(|s| s.as_str()).unwrap_or(""),
    );

    let data = NotePopupData { title, tags };

    if self.is_edit_note {
      NotePopupReturn::UpdateNote(data)
    } else {
      NotePopupReturn::AddNote(data)
    }
  }
}

pub(super) fn tags_to_text(tags: &[String]) -> String {
  tags.join(", ")
}

pub(super) fn text_to_tags(text: &str) -> Vec<String> {
  text
    .split_terminator(',')
    .map(|tag| String::from(tag.trim()))
    .filter(|tag| !tag.is_empty())
    .collect()
}
