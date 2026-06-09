use crate::app::{App, FocusedReader, NotesContext, NotesMode};

// App-level convenience wrappers around `NotesPaneModel` accessors.
// These exist for call-site ergonomics — `app.notes_mode_for_side(side)`
// reads cleaner than `app.notes.instance(side).map(|i| i.mode)`.
// PR 3 may push these further into the model.
impl App {
  pub fn notes_mode_for_side(&self, side: FocusedReader) -> NotesMode {
    self.notes.instance(side).map(|i| i.mode).unwrap_or_default()
  }

  pub fn set_notes_mode_for_side(
    &mut self,
    side: FocusedReader,
    mode: NotesMode,
  ) {
    self.notes.instance_or_init_mut(side).mode = mode;
  }

  pub fn notes_context_for_side(
    &self,
    side: FocusedReader,
  ) -> Option<&NotesContext> {
    self.notes.instance(side).and_then(|i| i.context.as_ref())
  }

  pub fn set_notes_context_for_side(
    &mut self,
    side: FocusedReader,
    context: Option<NotesContext>,
  ) {
    self.notes.instance_or_init_mut(side).context = context;
  }
}
