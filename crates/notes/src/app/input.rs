use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{ActivePopup, App, HandleInputReturn, NotesState};
use crate::{
  editor::NoteEditorAction,
  history::HistoryStack,
  keymap::UICommand,
  ui::{
    PopupReturn,
    entry_popup::NotePopupReturn,
    fuzz_find::FuzzFindReturn,
    help_popup::{HelpInputReturn, KeybindingsTabs},
    msg_box::{MsgBoxActions, MsgBoxInputResult, MsgBoxResult, MsgBoxType},
  },
};

impl App {
  /// State-based input dispatch.
  pub fn handle_input(&mut self, key: KeyEvent) -> HandleInputReturn {
    // Active popup always takes priority.
    if !self.active_popup.is_none() {
      return self.handle_popup_input(key);
    }

    match self.notes_state {
      NotesState::Editor => self.handle_editor_input(key),
      NotesState::List | NotesState::Preview | NotesState::PreviewScroll => {
        self.handle_list_input(key)
      }
    }
  }

  fn handle_editor_input(&mut self, key: KeyEvent) -> HandleInputReturn {
    let action = self.editor.handle_key(key);
    match action {
      NoteEditorAction::Save => {
        if let Err(e) = self.save_current_note_content() {
          self.show_err_msg(format!("Failed to save: {e}"));
        }
        HandleInputReturn::Handled
      }
      NoteEditorAction::Quit => {
        if self.editor.has_unsaved() {
          self.show_msg_box(
            MsgBoxType::Question(
              "Save changes before going back to list?\n(No = discard, Cancel = keep editing)".into(),
            ),
            MsgBoxActions::YesNoCancel,
            UICommand::DiscardChangesNoteContent,
          );
        } else {
          self.go_to_list();
        }
        HandleInputReturn::Handled
      }
      NoteEditorAction::None => HandleInputReturn::Handled,
    }
  }

  fn handle_list_input(&mut self, key: KeyEvent) -> HandleInputReturn {
    let is_preview_scroll = self.notes_state == NotesState::PreviewScroll;

    // In PreviewScroll, j/k scroll the preview rather than the list.
    if is_preview_scroll {
      match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
          self.preview_scroll = self.preview_scroll.saturating_add(1);
          return HandleInputReturn::Handled;
        }
        KeyCode::Char('k') | KeyCode::Up => {
          self.preview_scroll = self.preview_scroll.saturating_sub(1);
          return HandleInputReturn::Handled;
        }
        _ => {}
      }
    }

    match key.code {
      // ── Quit ───────────────────────────────────────────────────────
      KeyCode::Char('q') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
        HandleInputReturn::ExitApp
      }
      KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
        HandleInputReturn::ExitApp
      }

      // ── Esc: dismiss preview or close pane ─────────────────────────
      KeyCode::Esc => match self.notes_state {
        NotesState::Preview | NotesState::PreviewScroll => {
          self.notes_state = NotesState::List;
          HandleInputReturn::Handled
        }
        // In plain List state, Esc signals the caller to hide the pane.
        NotesState::List => HandleInputReturn::ExitApp,
        NotesState::Editor => unreachable!(),
      },

      // ── Tab: cycle focus between list and preview ──────────────────
      KeyCode::Tab => {
        match self.notes_state {
          NotesState::List if self.current_note_id.is_some() => {
            self.notes_state = NotesState::Preview;
            self.preview_scroll = 0;
          }
          NotesState::Preview => {
            self.notes_state = NotesState::PreviewScroll;
          }
          NotesState::PreviewScroll => {
            self.notes_state = NotesState::Preview;
          }
          _ => {}
        }
        HandleInputReturn::Handled
      }

      // ── Enter: open editor ─────────────────────────────────────────
      KeyCode::Enter => {
        if self.current_note_id.is_some() {
          self.go_to_editor();
        }
        HandleInputReturn::Handled
      }

      // ── Navigation ─────────────────────────────────────────────────
      KeyCode::Char('j') | KeyCode::Down => {
        self.select_next_note();
        let note = self.get_current_note().cloned();
        self.editor.load_note(note.as_ref());
        if self.current_note_id.is_some() {
          self.notes_state = NotesState::Preview;
          self.preview_scroll = 0;
        }
        HandleInputReturn::Handled
      }
      KeyCode::Char('k') | KeyCode::Up => {
        self.select_prev_note();
        let note = self.get_current_note().cloned();
        self.editor.load_note(note.as_ref());
        if self.current_note_id.is_some() {
          self.notes_state = NotesState::Preview;
          self.preview_scroll = 0;
        }
        HandleInputReturn::Handled
      }
      KeyCode::Char('g') => {
        self.go_to_top();
        let note = self.get_current_note().cloned();
        self.editor.load_note(note.as_ref());
        HandleInputReturn::Handled
      }
      KeyCode::Char('G') => {
        self.go_to_bottom();
        let note = self.get_current_note().cloned();
        self.editor.load_note(note.as_ref());
        HandleInputReturn::Handled
      }
      KeyCode::PageUp => {
        self.page_up(10);
        let note = self.get_current_note().cloned();
        self.editor.load_note(note.as_ref());
        HandleInputReturn::Handled
      }
      KeyCode::PageDown => {
        self.page_down(10);
        let note = self.get_current_note().cloned();
        self.editor.load_note(note.as_ref());
        HandleInputReturn::Handled
      }

      // ── CRUD & popups ───────────────────────────────────────────────
      KeyCode::Char('n') => {
        self.open_create_note_popup();
        HandleInputReturn::Handled
      }
      KeyCode::Char('f') => {
        self.open_filter_popup();
        HandleInputReturn::Handled
      }
      KeyCode::Char('o') => {
        self.open_sort_popup();
        HandleInputReturn::Handled
      }
      KeyCode::Char('/') => {
        self.open_fuzz_find_popup();
        HandleInputReturn::Handled
      }
      KeyCode::Char('?') => {
        self.open_help_popup(KeybindingsTabs::Global);
        HandleInputReturn::Handled
      }
      KeyCode::Char('d') => {
        if self.current_note_id.is_some() {
          self.show_msg_box(
            MsgBoxType::Question("Delete this note?".into()),
            MsgBoxActions::YesNo,
            UICommand::DeleteCurrentNote,
          );
        }
        HandleInputReturn::Handled
      }
      KeyCode::Char('u') => {
        match self.undo() {
          Ok(Some(id)) => {
            self.set_current_note(Some(id));
          }
          Ok(None) => {}
          Err(e) => {
            self.show_err_msg(format!("Undo failed: {e}"));
          }
        }
        HandleInputReturn::Handled
      }
      KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
        match self.redo() {
          Ok(Some(id)) => {
            self.set_current_note(Some(id));
          }
          Ok(None) => {}
          Err(e) => {
            self.show_err_msg(format!("Redo failed: {e}"));
          }
        }
        HandleInputReturn::Handled
      }

      _ => HandleInputReturn::NotFound,
    }
  }

  fn handle_popup_input(&mut self, key: KeyEvent) -> HandleInputReturn {
    let popup = std::mem::replace(&mut self.active_popup, ActivePopup::None);
    match popup {
      ActivePopup::None => return HandleInputReturn::NotFound,

      ActivePopup::Help(mut p) => match p.handle_input(key) {
        HelpInputReturn::Keep => {
          self.active_popup = ActivePopup::Help(p);
        }
        HelpInputReturn::Close => {}
      },

      ActivePopup::MsgBox(p) => match p.handle_input(key) {
        MsgBoxInputResult::Keep => {
          self.active_popup = ActivePopup::MsgBox(p);
        }
        MsgBoxInputResult::Close(result) => {
          if let Some(cmd) = self.pending_command.take() {
            return self.exec_pending_command(cmd, result);
          }
        }
      },

      ActivePopup::CreateNote(mut p) => match p.handle_input(key) {
        NotePopupReturn::KeepPopup => {
          self.active_popup = ActivePopup::CreateNote(p);
        }
        NotePopupReturn::Cancel => {}
        NotePopupReturn::AddNote(data) => {
          let paper = self.pending_paper.take();
          match self.create_note(data.title, data.tags, paper) {
            Ok(id) => {
              self.set_current_note(Some(id));
            }
            Err(e) => {
              self.show_err_msg(format!("Failed to create note: {e}"));
            }
          }
        }
        NotePopupReturn::UpdateNote(_) => {
          self.active_popup = ActivePopup::CreateNote(p);
        }
      },

      ActivePopup::EditNote(mut p) => {
        match p.handle_input(key) {
          NotePopupReturn::KeepPopup => {
            self.active_popup = ActivePopup::EditNote(p);
          }
          NotePopupReturn::Cancel => {}
          NotePopupReturn::UpdateNote(data) => {
            let linked = self
              .get_current_note()
              .map(|n| n.linked_papers.clone())
              .unwrap_or_default();
            if let Err(e) = self.update_current_note_attributes(
              data.title,
              linked,
              data.tags,
            ) {
              self.show_err_msg(format!("Failed to update note: {e}"));
            } else {
              // Refresh editor title from updated note.
              let note = self.get_current_note().cloned();
              self.editor.load_note(note.as_ref());
            }
          }
          NotePopupReturn::AddNote(_) => {
            self.active_popup = ActivePopup::EditNote(p);
          }
        }
      }

      ActivePopup::Filter(mut p) => match p.handle_input(key) {
        PopupReturn::KeepPopup => {
          self.active_popup = ActivePopup::Filter(p);
        }
        PopupReturn::Cancel => {}
        PopupReturn::Apply(filter) => {
          self.apply_filter(filter);
        }
      },

      ActivePopup::Sort(mut p) => match p.handle_input(key) {
        PopupReturn::KeepPopup => {
          self.active_popup = ActivePopup::Sort(p);
        }
        PopupReturn::Cancel => {}
        PopupReturn::Apply(result) => {
          self.apply_sort(result.applied_criteria, result.order);
        }
      },

      ActivePopup::FuzzyFind(mut p) => match p.handle_input(key) {
        FuzzFindReturn::KeepPopup => {
          self.active_popup = ActivePopup::FuzzyFind(p);
        }
        FuzzFindReturn::Close => {}
        FuzzFindReturn::SelectEntry(id) => {
          if let Some(id) = id {
            self.set_current_note(Some(id));
          }
        }
      },

      ActivePopup::Export(mut p) => {
        match p.handle_input(key) {
          PopupReturn::KeepPopup => {
            self.active_popup = ActivePopup::Export(p);
          }
          PopupReturn::Cancel => {}
          PopupReturn::Apply(_) => {
            // Export not yet implemented — silently close.
          }
        }
      }
    }

    HandleInputReturn::Handled
  }

  fn exec_pending_command(
    &mut self,
    cmd: UICommand,
    result: MsgBoxResult,
  ) -> HandleInputReturn {
    match cmd {
      UICommand::Quit => match result {
        MsgBoxResult::Yes => {
          let _ = self.save_current_note_content();
          return HandleInputReturn::ExitApp;
        }
        MsgBoxResult::No => {
          return HandleInputReturn::ExitApp;
        }
        _ => {}
      },
      UICommand::DeleteCurrentNote => {
        if result == MsgBoxResult::Yes {
          if let Err(e) = self.delete_current_note() {
            self.show_err_msg(format!("Failed to delete: {e}"));
          }
        }
      }
      UICommand::MulSelDeleteNotes => {
        if result == MsgBoxResult::Yes {
          let ids: Vec<String> =
            self.entries_list.selected_notes.drain().collect();
          for id in ids {
            if let Err(e) = self.delete_note_intern(&id, HistoryStack::Undo) {
              log::error!("Failed to delete note {id}: {e}");
            }
          }
          self.entries_list.multi_select_mode = false;
        }
      }
      UICommand::DiscardChangesNoteContent => match result {
        MsgBoxResult::Yes => {
          let _ = self.save_current_note_content();
          self.discard_current_content();
        }
        MsgBoxResult::No => {
          self.discard_current_content();
        }
        _ => {}
      },
      _ => {}
    }
    HandleInputReturn::Handled
  }
}
