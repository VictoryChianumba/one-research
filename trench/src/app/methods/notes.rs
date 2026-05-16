use crate::app::{App, FocusedReader, NotesContext, NotesMode};

impl App {
  pub fn notes_mode_for_side(&self, side: FocusedReader) -> NotesMode {
    match side {
      FocusedReader::Primary => self.notes_mode,
      FocusedReader::Secondary => self.secondary_notes_mode,
    }
  }

  pub fn set_notes_mode_for_side(
    &mut self,
    side: FocusedReader,
    mode: NotesMode,
  ) {
    match side {
      FocusedReader::Primary => self.notes_mode = mode,
      FocusedReader::Secondary => self.secondary_notes_mode = mode,
    }
  }

  pub fn notes_context_for_side(
    &self,
    side: FocusedReader,
  ) -> Option<&NotesContext> {
    match side {
      FocusedReader::Primary => self.notes_context.as_ref(),
      FocusedReader::Secondary => self.secondary_notes_context.as_ref(),
    }
  }

  pub fn set_notes_context_for_side(
    &mut self,
    side: FocusedReader,
    context: Option<NotesContext>,
  ) {
    match side {
      FocusedReader::Primary => self.notes_context = context,
      FocusedReader::Secondary => self.secondary_notes_context = context,
    }
  }
}
