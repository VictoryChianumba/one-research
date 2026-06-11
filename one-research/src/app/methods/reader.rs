use crate::action::{Action, ReaderTarget};
use crate::app::{App, FocusedReader, NotesContext, PaneId, ReaderTab};

fn make_tab(
  title: String,
  arxiv_id: Option<String>,
  notes_context: Option<NotesContext>,
  reader: tread::Reader,
) -> ReaderTab {
  ReaderTab {
    title,
    arxiv_id,
    notes_context,
    reader,
    image_state: tread::ImageState::default(),
    burst: tread::BurstTracker::default(),
    last_resize: None,
  }
}

impl App {
  /// Apply an `Action::OpenInReader` by routing to the appropriate
  /// reader-pane method based on `target`. The single chokepoint for
  /// "open this paper in a reader surface" — no caller should set
  /// `app.reader.active = true` directly (ADR-002 §S4).
  ///
  /// Document tabs were removed (ADR-017): opening always replaces the
  /// pane's single doc.
  ///
  /// `Action` non-variant panics: this method panics if handed a non-
  /// `OpenInReader` variant.  Other actions route through their own
  /// surface-specific handlers.
  pub fn apply_open_in_reader(&mut self, action: Action) {
    let Action::OpenInReader { target, title, arxiv_id, notes_context, reader } =
      action
    else {
      panic!("apply_open_in_reader called with non-OpenInReader variant");
    };
    match target {
      ReaderTarget::Secondary => {
        self.reader_secondary_open(title, arxiv_id, notes_context, reader);
        self.reader.focused = FocusedReader::Secondary;
        self.focus.focused_pane = PaneId::SecondaryReader;
      }
      // Popup async-load lifecycle is handled outside this Action for now
      // (PR 5 will fold it in via ReaderPopupModel::pre_draw); falls
      // through to the primary path until then so the variant isn't dead.
      ReaderTarget::Primary | ReaderTarget::Popup => {
        self.reader_open(title, arxiv_id, notes_context, reader);
        self.focus.focused_pane = PaneId::Reader;
      }
    }
  }

  pub fn reader_notes_context(
    &self,
    side: FocusedReader,
  ) -> Option<NotesContext> {
    let tab = match side {
      FocusedReader::Primary => self.reader.primary.doc.as_ref(),
      FocusedReader::Secondary => self.reader.secondary.doc.as_ref(),
    }?;
    tab.notes_context.clone()
  }

  /// The open primary reader (mutable).  Returns the tread Reader so
  /// callers can dispatch `handle_event` and read state.  Most call
  /// sites also need the per-doc `ImageState` for `tread::after_draw`
  /// — use `reader_active_tab_mut()` for both at once.
  pub fn reader_editor_mut(&mut self) -> Option<&mut tread::Reader> {
    self.reader.primary.doc.as_mut().map(|t| &mut t.reader)
  }

  pub fn reader_secondary_editor_mut(&mut self) -> Option<&mut tread::Reader> {
    self.reader.secondary.doc.as_mut().map(|t| &mut t.reader)
  }

  /// The open primary doc as a whole — exposes both the Reader and its
  /// ImageState in one borrow.  Call sites that drive `tread::after_draw`
  /// or `tread::clear_images` need both.
  pub fn reader_active_tab_mut(&mut self) -> Option<&mut ReaderTab> {
    self.reader.primary.doc.as_mut()
  }

  pub fn reader_secondary_active_tab_mut(&mut self) -> Option<&mut ReaderTab> {
    self.reader.secondary.doc.as_mut()
  }

  /// Open a paper in the primary reader, replacing whatever was open.
  pub fn reader_open(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    self.reader.primary.doc =
      Some(make_tab(title, arxiv_id, notes_context, reader));
    self.reader.active = true;
  }

  /// Open a paper in the secondary reader, replacing whatever was open.
  pub fn reader_secondary_open(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    self.reader.secondary.doc =
      Some(make_tab(title, arxiv_id, notes_context, reader));
  }

  /// Clear the image cache for both reader docs + the popup.  Called on
  /// `Event::Resize` (placements were anchored to the old rect coords)
  /// and `Event::FocusLost` (tmux pane switch — leftover kitty
  /// placements would bleed into whatever pane sits on top of us).
  /// Cheap: each ImageState is just a small HashMap.
  pub fn clear_all_reader_image_state(&mut self) {
    if let Some(tab) = self.reader.primary.doc.as_mut() {
      tread::clear_images(&mut tab.image_state);
    }
    if let Some(tab) = self.reader.secondary.doc.as_mut() {
      tread::clear_images(&mut tab.image_state);
    }
    tread::clear_images(&mut self.reader_popup.image_state);
  }

  /// Stop voice playback and exit reading mode on every open Reader.
  /// Called when the user navigates away from the source they were
  /// listening to (reader exit, dual teardown).  All Readers share one
  /// Arc<PlaybackController>, so vc.stop() may fire multiple times — the
  /// controller treats repeats as no-ops.  Idempotent.
  pub fn stop_all_reader_voice(&mut self) {
    if let Some(tab) = self.reader.primary.doc.as_mut() {
      tab.reader.exit_voice_mode();
    }
    if let Some(tab) = self.reader.secondary.doc.as_mut() {
      tab.reader.exit_voice_mode();
    }
    if let Some(reader) = self.reader_popup.editor.as_mut() {
      reader.exit_voice_mode();
    }
  }

  /// Close the primary reader doc. Returns true if the pane is now empty.
  pub fn reader_close_active_tab(&mut self) -> bool {
    // Pre-close cleanup: stop voice + clear image cache before drop so
    // audio doesn't outlive the source and pixel placements the terminal
    // still has cached for this doc's kitty_ids are deleted.  tread side
    // effects stay here at the App level — they're external to the
    // Model's pure-state contract.
    if let Some(tab) = self.reader.primary.doc.as_mut() {
      tab.reader.exit_voice_mode();
      tread::clear_images(&mut tab.image_state);
    }
    let now_empty = self.reader.primary.close_active_tab();
    if now_empty {
      self.reader.active = false;
    }
    now_empty
  }

  /// Close the secondary reader doc. Returns true if the pane is now empty.
  pub fn reader_secondary_close_active_tab(&mut self) -> bool {
    if let Some(tab) = self.reader.secondary.doc.as_mut() {
      tab.reader.exit_voice_mode();
      tread::clear_images(&mut tab.image_state);
    }
    self.reader.secondary.close_active_tab()
  }
}
