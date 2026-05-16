use crate::action::{Action, OpenMode, ReaderTarget};
use crate::app::{App, FocusedReader, NotesContext, PaneId, ReaderTab};

impl App {
  /// Apply an `Action::OpenInReader` by routing to the appropriate
  /// reader-pane method based on `target` + `mode`. The single chokepoint
  /// for "open this paper in a reader surface" — no caller should set
  /// `app.reader.active = true` directly (ADR-002 §S4).
  ///
  /// `Action` non-variant panics: this method panics if handed a non-
  /// `OpenInReader` variant.  Other actions route through their own
  /// surface-specific handlers.
  pub fn apply_open_in_reader(&mut self, action: Action) {
    let Action::OpenInReader {
      target,
      mode,
      title,
      arxiv_id,
      notes_context,
      reader,
    } = action
    else {
      panic!("apply_open_in_reader called with non-OpenInReader variant");
    };
    match (target, mode) {
      (ReaderTarget::Primary, OpenMode::NewTab) => {
        self.reader_push_tab(title, arxiv_id, notes_context, reader);
        self.focus.focused_pane = PaneId::Reader;
      }
      (ReaderTarget::Primary, OpenMode::ReplaceActive) => {
        self.reader_replace_active_tab(title, arxiv_id, notes_context, reader);
        self.focus.focused_pane = PaneId::Reader;
      }
      (ReaderTarget::Secondary, OpenMode::NewTab) => {
        self.reader_secondary_push_tab(title, arxiv_id, notes_context, reader);
        self.reader.focused = FocusedReader::Secondary;
        self.focus.focused_pane = PaneId::SecondaryReader;
      }
      (ReaderTarget::Secondary, OpenMode::ReplaceActive) => {
        self.reader_secondary_replace_active_tab(
          title,
          arxiv_id,
          notes_context,
          reader,
        );
        self.reader.focused = FocusedReader::Secondary;
        self.focus.focused_pane = PaneId::SecondaryReader;
      }
      (ReaderTarget::Popup, _) => {
        // Popup async-load lifecycle is handled outside this Action for
        // now (PR 5 will fold it in via ReaderPopupModel::pre_draw).
        // Falls through to the primary path until then so the variant
        // isn't dead.
        self.reader_push_tab(title, arxiv_id, notes_context, reader);
        self.focus.focused_pane = PaneId::Reader;
      }
    }
  }
  pub fn reader_notes_context(
    &self,
    side: FocusedReader,
  ) -> Option<NotesContext> {
    let tab = match side {
      FocusedReader::Primary => self.reader.primary.tabs.get(self.reader.primary.active_tab),
      FocusedReader::Secondary => {
        self.reader.secondary.tabs.get(self.reader.secondary.active_tab)
      }
    }?;
    tab.notes_context.clone()
  }

  /// Active primary reader (mutable).  Returns the tread Reader so
  /// callers can dispatch `handle_event` and read state.  Most call
  /// sites also need the per-tab `ImageState` for `tread::after_draw`
  /// — use `reader_active_tab_mut()` for both at once.
  pub fn reader_editor_mut(&mut self) -> Option<&mut tread::Reader> {
    self.reader.primary.tabs.get_mut(self.reader.primary.active_tab).map(|t| &mut t.reader)
  }

  pub fn reader_secondary_editor_mut(&mut self) -> Option<&mut tread::Reader> {
    self
      .reader.secondary.tabs
      .get_mut(self.reader.secondary.active_tab)
      .map(|t| &mut t.reader)
  }

  /// Active primary tab as a whole — exposes both the Reader and its
  /// ImageState in one borrow.  Call sites that drive `tread::after_draw`
  /// or `tread::clear_images` need both.
  pub fn reader_active_tab_mut(&mut self) -> Option<&mut ReaderTab> {
    self.reader.primary.tabs.get_mut(self.reader.primary.active_tab)
  }

  pub fn reader_secondary_active_tab_mut(&mut self) -> Option<&mut ReaderTab> {
    self.reader.secondary.tabs.get_mut(self.reader.secondary.active_tab)
  }

  pub fn reader_push_tab(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    self.reader.primary.tabs.push(ReaderTab {
      title,
      arxiv_id,
      notes_context,
      reader,
      image_state: tread::ImageState::default(),
      burst: tread::BurstTracker::default(),
      last_resize: None,
    });
    self.reader.primary.active_tab = self.reader.primary.tabs.len() - 1;
    self.reader.active = true;
  }

  pub fn reader_secondary_push_tab(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    self.reader.secondary.tabs.push(ReaderTab {
      title,
      arxiv_id,
      notes_context,
      reader,
      image_state: tread::ImageState::default(),
      burst: tread::BurstTracker::default(),
      last_resize: None,
    });
    self.reader.secondary.active_tab = self.reader.secondary.tabs.len() - 1;
  }

  pub fn reader_replace_active_tab(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    if self.reader.primary.tabs.is_empty() {
      self.reader_push_tab(title, arxiv_id, notes_context, reader);
    } else {
      self.reader.primary.tabs[self.reader.primary.active_tab] = ReaderTab {
        title,
        arxiv_id,
        notes_context,
        reader,
        image_state: tread::ImageState::default(),
        burst: tread::BurstTracker::default(),
        last_resize: None,
      };
      self.reader.active = true;
    }
  }

  pub fn reader_secondary_replace_active_tab(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    if self.reader.secondary.tabs.is_empty() {
      self.reader_secondary_push_tab(title, arxiv_id, notes_context, reader);
    } else {
      self.reader.secondary.tabs[self.reader.secondary.active_tab] =
        ReaderTab {
          title,
          arxiv_id,
          notes_context,
          reader,
          image_state: tread::ImageState::default(),
          burst: tread::BurstTracker::default(),
          last_resize: None,
        };
    }
  }

  /// Clear the image cache for every open reader tab + the popup.
  /// Called on `Event::Resize` (placements were anchored to the old
  /// rect coords) and `Event::FocusLost` (tmux pane switch — leftover
  /// kitty placements would bleed into whatever pane sits on top of
  /// us).  Cheap: each ImageState is just a small HashMap.
  pub fn clear_all_reader_image_state(&mut self) {
    for tab in self.reader.primary.tabs.iter_mut() {
      tread::clear_images(&mut tab.image_state);
    }
    for tab in self.reader.secondary.tabs.iter_mut() {
      tread::clear_images(&mut tab.image_state);
    }
    tread::clear_images(&mut self.reader_popup.image_state);
  }

  /// Stop voice playback and exit reading mode on every open Reader.
  /// Called when the user navigates away from the source they were
  /// listening to: tab close, tab switch (prev/next), or full reader
  /// exit (Esc back to feed).  Voice should stay tied to the active
  /// reader pane; continuing playback after a transition is
  /// disorienting.  All Readers share one Arc<PlaybackController>, so
  /// vc.stop() may fire multiple times — the controller treats
  /// repeats as no-ops.  Idempotent.
  pub fn stop_all_reader_voice(&mut self) {
    for tab in self.reader.primary.tabs.iter_mut() {
      tab.reader.exit_voice_mode();
    }
    for tab in self.reader.secondary.tabs.iter_mut() {
      tab.reader.exit_voice_mode();
    }
    if let Some(reader) = self.reader_popup.editor.as_mut() {
      reader.exit_voice_mode();
    }
  }

  /// Close the active primary tab. Returns true if the pane is now empty.
  pub fn reader_close_active_tab(&mut self) -> bool {
    // Pre-close cleanup: stop voice + clear image cache before drop so
    // audio doesn't outlive the source and pixel placements the
    // terminal still has cached for this tab's kitty_ids are deleted.
    // tread side effects stay here at the App level — they're external
    // to the Model's pure-state contract.
    if let Some(tab) =
      self.reader.primary.tabs.get_mut(self.reader.primary.active_tab)
    {
      tab.reader.exit_voice_mode();
      tread::clear_images(&mut tab.image_state);
    }
    let now_empty = self.reader.primary.close_active_tab();
    if now_empty {
      self.reader.active = false;
    }
    now_empty
  }

  /// Close the active secondary tab. Returns true if the pane is now empty.
  pub fn reader_secondary_close_active_tab(&mut self) -> bool {
    if let Some(tab) =
      self.reader.secondary.tabs.get_mut(self.reader.secondary.active_tab)
    {
      tab.reader.exit_voice_mode();
      tread::clear_images(&mut tab.image_state);
    }
    self.reader.secondary.close_active_tab()
  }

  pub fn reader_prev_tab(&mut self) {
    // Voice is tied to the source you were reading. Switching to a
    // different tab means you're not in that source anymore — stop
    // playback so audio doesn't keep going in the background.
    self.stop_all_reader_voice();
    self.reader.focused_instance_mut().prev_tab();
  }

  pub fn reader_next_tab(&mut self) {
    self.stop_all_reader_voice();
    self.reader.focused_instance_mut().next_tab();
  }
}
