use anyhow::{anyhow, bail};
use arboard::Clipboard;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
  Frame,
  layout::{Constraint, Direction, Layout, Rect},
  prelude::Margin,
  style::{Modifier, Style},
  symbols,
  widgets::{
    Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
  },
};
use tui_textarea::{CursorMove, Scrolling, TextArea};

use crate::{Note, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
  Normal,
  Insert,
  Visual,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NoteEditorAction {
  None,
  Save,
  Quit,
}

pub(crate) enum ClipboardOperation {
  Cut,
  Copy,
  Paste,
}

pub struct NoteEditor<'a> {
  pub title: String,
  text_area: TextArea<'a>,
  mode: EditorMode,
  is_active: bool,
  is_dirty: bool,
  has_unsaved: bool,
  original_content: String,
  /// Mirror yanked text to the OS clipboard via arboard.
  pub sync_os_clipboard: bool,
}

impl<'a> NoteEditor<'a> {
  pub fn new() -> NoteEditor<'a> {
    NoteEditor {
      title: String::new(),
      text_area: TextArea::default(),
      mode: EditorMode::Normal,
      is_active: false,
      is_dirty: false,
      has_unsaved: false,
      original_content: String::new(),
      sync_os_clipboard: false,
    }
  }

  /// Loads a note into the editor, replacing any current content.
  /// Equivalent to `set_current_entry` in tui-journal.
  pub fn load_note(&mut self, note: Option<&Note>) {
    let (title, content) = match note {
      Some(n) => (n.title.clone(), n.content.clone()),
      None => (String::new(), String::new()),
    };

    self.title = title;
    self.original_content = content.clone();
    self.is_dirty = false;

    let text_area = if content.is_empty() {
      TextArea::default()
    } else {
      let lines: Vec<String> = content.lines().map(|l| l.to_owned()).collect();
      let mut ta = TextArea::new(lines);
      ta.move_cursor(CursorMove::Bottom);
      ta.move_cursor(CursorMove::End);
      ta
    };

    self.text_area = text_area;
    self.refresh_has_unsaved();
  }

  /// Replaces the editor content with the given string and marks it dirty.
  /// Equivalent to `set_entry_content` in tui-journal.
  pub fn load_content(&mut self, content: &str) {
    self.is_dirty = true;
    let lines: Vec<String> = content.lines().map(|l| l.to_owned()).collect();
    let mut ta = TextArea::new(lines);
    ta.move_cursor(CursorMove::Bottom);
    ta.move_cursor(CursorMove::End);
    self.text_area = ta;
    self.refresh_has_unsaved();
  }

  #[inline]
  pub fn is_insert_mode(&self) -> bool {
    self.mode == EditorMode::Insert
  }

  #[inline]
  pub fn is_visual_mode(&self) -> bool {
    self.mode == EditorMode::Visual
  }

  #[inline]
  pub fn is_prioritized(&self) -> bool {
    matches!(self.mode, EditorMode::Insert | EditorMode::Visual)
  }

  pub fn set_active(&mut self, active: bool) {
    if !active && self.is_visual_mode() {
      self.set_editor_mode(EditorMode::Normal);
    }
    self.is_active = active;
  }

  pub fn get_editor_mode(&self) -> EditorMode {
    self.mode
  }

  pub fn set_editor_mode(&mut self, mode: EditorMode) {
    match (self.mode, mode) {
      (EditorMode::Normal, EditorMode::Visual) => {
        self.text_area.start_selection();
      }
      (EditorMode::Visual, EditorMode::Normal | EditorMode::Insert) => {
        self.text_area.cancel_selection();
      }
      _ => {}
    }
    self.mode = mode;
  }

  pub fn get_content(&self) -> String {
    self.text_area.lines().to_vec().join("\n")
  }

  pub fn has_unsaved(&self) -> bool {
    self.has_unsaved
  }

  pub fn refresh_has_unsaved(&mut self) {
    self.has_unsaved =
      self.is_dirty && self.get_content() != self.original_content;
  }

  // ── Key handling ─────────────────────────────────────────────────────────

  /// Unified key handler. Returns a [`NoteEditorAction`] indicating what the
  /// caller should do (save, quit, or nothing).
  pub fn handle_key(&mut self, key: KeyEvent) -> NoteEditorAction {
    let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // Global overrides (highest priority)
    if has_ctrl && key.code == KeyCode::Char('s') {
      return NoteEditorAction::Save;
    }
    if key.code == KeyCode::Esc {
      return match self.mode {
        EditorMode::Normal => NoteEditorAction::Quit,
        _ => {
          self.set_editor_mode(EditorMode::Normal);
          NoteEditorAction::None
        }
      };
    }

    match self.mode {
      EditorMode::Insert => {
        self.handle_input_prioritized(key);
      }
      EditorMode::Visual => {
        if !self.handle_input_visual_only(key).unwrap_or(false) {
          let _ = self.handle_vim_motions(key);
        }
        if !self.text_area.is_selecting() {
          self.set_editor_mode(EditorMode::Normal);
        }
      }
      EditorMode::Normal => {
        if is_default_navigation(key) {
          self.text_area.input(key);
        } else {
          let _ = self.handle_vim_motions(key);
        }
        if !self.text_area.is_selecting() && self.mode == EditorMode::Visual {
          self.set_editor_mode(EditorMode::Normal);
        }
      }
    }

    NoteEditorAction::None
  }

  /// Handles key input when in Insert mode. Mirrors `handle_input_prioritized`
  /// from tui-journal.
  fn handle_input_prioritized(&mut self, key: KeyEvent) {
    let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if self.sync_os_clipboard && has_ctrl {
      let handled = match key.code {
        KeyCode::Char('x') => {
          let _ = self.exec_os_clipboard(ClipboardOperation::Cut);
          true
        }
        KeyCode::Char('c') => {
          let _ = self.exec_os_clipboard(ClipboardOperation::Copy);
          true
        }
        KeyCode::Char('y') => {
          let _ = self.exec_os_clipboard(ClipboardOperation::Paste);
          true
        }
        _ => false,
      };

      if handled {
        self.refresh_has_unsaved();
        return;
      }
    }

    if self.text_area.input(key) {
      self.is_dirty = true;
      self.refresh_has_unsaved();
    }
  }

  /// Handles keys that are only meaningful in Visual mode (d / y / c).
  /// Returns `Ok(true)` if the key was consumed.
  fn handle_input_visual_only(
    &mut self,
    key: KeyEvent,
  ) -> anyhow::Result<bool> {
    if !key.modifiers.is_empty() {
      return Ok(false);
    }

    match key.code {
      KeyCode::Char('d') => {
        if self.sync_os_clipboard {
          self.exec_os_clipboard(ClipboardOperation::Cut)?;
        } else {
          self.text_area.cut();
        }
        Ok(true)
      }
      KeyCode::Char('y') => {
        if self.sync_os_clipboard {
          self.exec_os_clipboard(ClipboardOperation::Copy)?;
        } else {
          self.text_area.copy();
        }
        self.set_editor_mode(EditorMode::Normal);
        Ok(true)
      }
      KeyCode::Char('c') => {
        if self.sync_os_clipboard {
          self.exec_os_clipboard(ClipboardOperation::Copy)?;
        } else {
          self.text_area.cut();
        }
        self.set_editor_mode(EditorMode::Insert);
        Ok(true)
      }
      _ => Ok(false),
    }
  }

  fn handle_vim_motions(&mut self, key: KeyEvent) -> anyhow::Result<()> {
    let has_control = key.modifiers.contains(KeyModifiers::CONTROL);

    match (key.code, has_control) {
      (KeyCode::Char('h'), false) => {
        self.text_area.move_cursor(CursorMove::Back);
      }
      (KeyCode::Char('j'), false) => {
        self.text_area.move_cursor(CursorMove::Down);
      }
      (KeyCode::Char('k'), false) => {
        self.text_area.move_cursor(CursorMove::Up);
      }
      (KeyCode::Char('l'), false) => {
        self.text_area.move_cursor(CursorMove::Forward);
      }
      (KeyCode::Char('w'), false) | (KeyCode::Char('e'), false) => {
        self.text_area.move_cursor(CursorMove::WordForward);
      }
      (KeyCode::Char('b'), false) => {
        self.text_area.move_cursor(CursorMove::WordBack);
      }
      (KeyCode::Char('^'), false) => {
        self.text_area.move_cursor(CursorMove::Head);
      }
      (KeyCode::Char('$'), false) => {
        self.text_area.move_cursor(CursorMove::End);
      }
      (KeyCode::Char('D'), false) => {
        self.text_area.delete_line_by_end();
        self.exec_os_clipboard(ClipboardOperation::Copy)?;
        self.is_dirty = true;
      }
      (KeyCode::Char('C'), false) => {
        self.text_area.delete_line_by_end();
        self.exec_os_clipboard(ClipboardOperation::Copy)?;
        self.mode = EditorMode::Insert;
        self.is_dirty = true;
      }
      (KeyCode::Char('p'), false) => {
        if self.sync_os_clipboard {
          self.exec_os_clipboard(ClipboardOperation::Paste)?;
        } else {
          self.text_area.paste();
        }
        self.is_dirty = true;
      }
      (KeyCode::Char('u'), false) => {
        self.text_area.undo();
      }
      (KeyCode::Char('r'), true) => {
        self.text_area.redo();
      }
      (KeyCode::Char('x'), false) => {
        self.text_area.delete_next_char();
        self.exec_os_clipboard(ClipboardOperation::Copy)?;
        self.is_dirty = true;
      }
      (KeyCode::Char('i'), false) => self.mode = EditorMode::Insert,
      (KeyCode::Char('a'), false) => {
        self.text_area.move_cursor(CursorMove::Forward);
        self.mode = EditorMode::Insert;
      }
      (KeyCode::Char('A'), false) => {
        self.text_area.move_cursor(CursorMove::End);
        self.mode = EditorMode::Insert;
      }
      (KeyCode::Char('o'), false) => {
        self.text_area.move_cursor(CursorMove::End);
        self.text_area.insert_newline();
        self.mode = EditorMode::Insert;
      }
      (KeyCode::Char('O'), false) => {
        self.text_area.move_cursor(CursorMove::Head);
        self.text_area.insert_newline();
        self.text_area.move_cursor(CursorMove::Up);
        self.mode = EditorMode::Insert;
      }
      (KeyCode::Char('I'), false) => {
        self.text_area.move_cursor(CursorMove::Head);
        self.mode = EditorMode::Insert;
      }
      (KeyCode::Char('v'), false) => self.set_editor_mode(EditorMode::Visual),
      (KeyCode::Char('d'), true) => {
        self.text_area.scroll(Scrolling::HalfPageDown);
      }
      (KeyCode::Char('u'), true) => {
        self.text_area.scroll(Scrolling::HalfPageUp);
      }
      (KeyCode::Char('f'), true) => {
        self.text_area.scroll(Scrolling::PageDown);
      }
      (KeyCode::Char('b'), true) => {
        self.text_area.scroll(Scrolling::PageUp);
      }
      _ => {}
    }

    Ok(())
  }

  // ── Rendering ────────────────────────────────────────────────────────────

  pub fn render_widget(&mut self, frame: &mut Frame, area: Rect) {
    // Split: 1 row for mode indicator, rest for text area.
    let rows = Layout::default()
      .direction(Direction::Vertical)
      .constraints([Constraint::Length(1), Constraint::Min(0)])
      .split(area);
    let indicator_area = rows[0];
    let text_rect = rows[1];

    // Mode indicator line.
    let (mode_label, mode_color) = if self.is_active {
      match self.mode {
        EditorMode::Normal => ("-- NORMAL --", theme::current().text_dim),
        EditorMode::Insert => ("-- INSERT --", theme::current().warning),
        EditorMode::Visual => ("-- VISUAL --", theme::current().success),
      }
    } else {
      ("", theme::current().text_dim)
    };
    let unsaved = if self.has_unsaved { "  [+]" } else { "" };
    frame.render_widget(
      Paragraph::new(format!("{mode_label}{unsaved}"))
        .style(Style::default().fg(mode_color)),
      indicator_area,
    );

    // Text area — no block border.
    self.text_area.set_block(Block::default());

    let cursor_style = if self.is_active {
      match self.mode {
        EditorMode::Normal => Style::default().add_modifier(Modifier::REVERSED),
        EditorMode::Insert => Style::default(),
        EditorMode::Visual => {
          Style::default().add_modifier(Modifier::UNDERLINED)
        }
      }
    } else {
      Style::reset()
    };
    self.text_area.set_cursor_style(cursor_style);
    self.text_area.set_cursor_line_style(Style::reset());
    self.text_area.set_style(Style::reset());
    self.text_area.set_selection_style(
      Style::default()
        .bg(theme::current().accent)
        .fg(theme::current().text_on_accent),
    );

    frame.render_widget(&self.text_area, text_rect);

    self.render_vertical_scrollbar(frame, text_rect);
    self.render_horizontal_scrollbar(frame, text_rect);
  }

  pub fn render_vertical_scrollbar(&mut self, frame: &mut Frame, area: Rect) {
    let lines_count = self.text_area.lines().len();

    if lines_count as u16 <= area.height {
      return;
    }

    let (row, _) = self.text_area.cursor();

    let mut state =
      ScrollbarState::default().content_length(lines_count).position(row);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
      .begin_symbol(Some("▲"))
      .end_symbol(Some("▼"))
      .track_symbol(Some(symbols::line::VERTICAL))
      .thumb_symbol(symbols::block::FULL);

    let scroll_area = area.inner(Margin { horizontal: 0, vertical: 1 });

    frame.render_stateful_widget(scrollbar, scroll_area, &mut state);
  }

  pub fn render_horizontal_scrollbar(&mut self, frame: &mut Frame, area: Rect) {
    let max_width = self
      .text_area
      .lines()
      .iter()
      .map(|line| line.len())
      .max()
      .unwrap_or_default();

    if max_width as u16 <= area.width {
      return;
    }

    let (_, col) = self.text_area.cursor();

    let mut state =
      ScrollbarState::default().content_length(max_width).position(col);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
      .begin_symbol(Some("◄"))
      .end_symbol(Some("►"))
      .track_symbol(Some(symbols::line::HORIZONTAL))
      .thumb_symbol("🬋");

    let scroll_area = area.inner(Margin { horizontal: 1, vertical: 0 });

    frame.render_stateful_widget(scrollbar, scroll_area, &mut state);
  }

  // ── Clipboard ────────────────────────────────────────────────────────────

  pub(crate) fn exec_os_clipboard(
    &mut self,
    operation: ClipboardOperation,
  ) -> anyhow::Result<()> {
    let mut clipboard = Clipboard::new().map_err(map_clipboard_error)?;

    match operation {
      ClipboardOperation::Copy => {
        self.text_area.copy();
        let selected_text = self.text_area.yank_text();
        clipboard.set_text(selected_text).map_err(map_clipboard_error)?;
      }
      ClipboardOperation::Cut => {
        if self.text_area.cut() {
          self.is_dirty = true;
          self.has_unsaved = true;
        }
        let selected_text = self.text_area.yank_text();
        clipboard.set_text(selected_text).map_err(map_clipboard_error)?;
      }
      ClipboardOperation::Paste => {
        let content = clipboard.get_text().map_err(map_clipboard_error)?;
        if content.is_empty() {
          return Ok(());
        }
        if !self.text_area.insert_str(content) {
          bail!("Text can't be pasted into editor");
        }
        self.is_dirty = true;
        self.has_unsaved = true;
      }
    }

    Ok(())
  }
}

fn is_default_navigation(key: KeyEvent) -> bool {
  let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
  let has_alt = key.modifiers.contains(KeyModifiers::ALT);
  match key.code {
    KeyCode::Left
    | KeyCode::Right
    | KeyCode::Up
    | KeyCode::Down
    | KeyCode::Home
    | KeyCode::End
    | KeyCode::PageUp
    | KeyCode::PageDown => true,
    KeyCode::Char('p') if has_ctrl || has_alt => true,
    KeyCode::Char('n') if has_ctrl || has_alt => true,
    KeyCode::Char('f') if !has_ctrl && has_alt => true,
    KeyCode::Char('b') if !has_ctrl && has_alt => true,
    KeyCode::Char('e') if has_ctrl || has_alt => true,
    KeyCode::Char('a') if has_ctrl || has_alt => true,
    KeyCode::Char('v') if has_ctrl || has_alt => true,
    _ => false,
  }
}

fn map_clipboard_error(err: arboard::Error) -> anyhow::Error {
  anyhow!(
    "Error while communicating with the operation system clipboard.\nError Details: {err}"
  )
}
