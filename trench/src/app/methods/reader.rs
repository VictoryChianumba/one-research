use crate::app::{App, FocusedReader, NotesContext, ReaderTab};

impl App {
  pub fn reader_notes_context(
    &self,
    side: FocusedReader,
  ) -> Option<NotesContext> {
    let tab = match side {
      FocusedReader::Primary => self.reader_tabs.get(self.reader_active_tab),
      FocusedReader::Secondary => {
        self.reader_secondary_tabs.get(self.reader_secondary_active_tab)
      }
    }?;
    tab.notes_context.clone()
  }

  /// Active primary reader (mutable).  Returns the tread Reader so
  /// callers can dispatch `handle_event` and read state.  Most call
  /// sites also need the per-tab `ImageState` for `tread::after_draw`
  /// — use `reader_active_tab_mut()` for both at once.
  pub fn reader_editor_mut(&mut self) -> Option<&mut tread::Reader> {
    self.reader_tabs.get_mut(self.reader_active_tab).map(|t| &mut t.reader)
  }

  pub fn reader_secondary_editor_mut(&mut self) -> Option<&mut tread::Reader> {
    self
      .reader_secondary_tabs
      .get_mut(self.reader_secondary_active_tab)
      .map(|t| &mut t.reader)
  }

  /// Active primary tab as a whole — exposes both the Reader and its
  /// ImageState in one borrow.  Call sites that drive `tread::after_draw`
  /// or `tread::clear_images` need both.
  pub fn reader_active_tab_mut(&mut self) -> Option<&mut ReaderTab> {
    self.reader_tabs.get_mut(self.reader_active_tab)
  }

  /// Read-only view of the active primary tab.  Used by layout code
  /// that needs to peek at reader state (e.g. figure count) before
  /// taking the mutable borrow for rendering.
  pub fn reader_active_tab(&self) -> Option<&ReaderTab> {
    self.reader_tabs.get(self.reader_active_tab)
  }

  pub fn reader_secondary_active_tab_mut(&mut self) -> Option<&mut ReaderTab> {
    self.reader_secondary_tabs.get_mut(self.reader_secondary_active_tab)
  }

  pub fn reader_push_tab(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    self.reader_tabs.push(ReaderTab {
      title,
      arxiv_id,
      notes_context,
      reader,
      image_state: tread::ImageState::default(),
      burst: tread::BurstTracker::default(),
      last_resize: None,
      current_figure: None,
    });
    self.reader_active_tab = self.reader_tabs.len() - 1;
    self.reader_active = true;
  }

  pub fn reader_secondary_push_tab(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    self.reader_secondary_tabs.push(ReaderTab {
      title,
      arxiv_id,
      notes_context,
      reader,
      image_state: tread::ImageState::default(),
      burst: tread::BurstTracker::default(),
      last_resize: None,
      current_figure: None,
    });
    self.reader_secondary_active_tab = self.reader_secondary_tabs.len() - 1;
  }

  pub fn reader_replace_active_tab(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    if self.reader_tabs.is_empty() {
      self.reader_push_tab(title, arxiv_id, notes_context, reader);
    } else {
      self.reader_tabs[self.reader_active_tab] = ReaderTab {
        title,
        arxiv_id,
        notes_context,
        reader,
        image_state: tread::ImageState::default(),
        burst: tread::BurstTracker::default(),
        last_resize: None,
        current_figure: None,
      };
      self.reader_active = true;
    }
  }

  pub fn reader_secondary_replace_active_tab(
    &mut self,
    title: String,
    arxiv_id: Option<String>,
    notes_context: Option<NotesContext>,
    reader: tread::Reader,
  ) {
    if self.reader_secondary_tabs.is_empty() {
      self.reader_secondary_push_tab(title, arxiv_id, notes_context, reader);
    } else {
      self.reader_secondary_tabs[self.reader_secondary_active_tab] =
        ReaderTab {
          title,
          arxiv_id,
          notes_context,
          reader,
          image_state: tread::ImageState::default(),
          burst: tread::BurstTracker::default(),
          last_resize: None,
          current_figure: None,
        };
    }
  }

  /// Clear the image cache for every open reader tab + the popup.
  /// Called on `Event::Resize` (placements were anchored to the old
  /// rect coords) and `Event::FocusLost` (tmux pane switch — leftover
  /// kitty placements would bleed into whatever pane sits on top of
  /// us).  Cheap: each ImageState is just a small HashMap.
  pub fn clear_all_reader_image_state(&mut self) {
    for tab in self.reader_tabs.iter_mut() {
      tread::clear_images(&mut tab.image_state);
    }
    for tab in self.reader_secondary_tabs.iter_mut() {
      tread::clear_images(&mut tab.image_state);
    }
    tread::clear_images(&mut self.reader_popup_image_state);
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
    for tab in self.reader_tabs.iter_mut() {
      tab.reader.exit_voice_mode();
    }
    for tab in self.reader_secondary_tabs.iter_mut() {
      tab.reader.exit_voice_mode();
    }
    if let Some(reader) = self.reader_popup_editor.as_mut() {
      reader.exit_voice_mode();
    }
  }

  /// Close the active primary tab. Returns true if the pane is now empty.
  pub fn reader_close_active_tab(&mut self) -> bool {
    if self.reader_tabs.is_empty() {
      return true;
    }
    // Stop voice + clear image cache before drop so audio doesn't
    // outlive the source it was reading and pixel placements the
    // terminal still has cached for this tab's kitty_ids are deleted
    // — otherwise they linger as ghost overlays on whichever tab
    // takes its slot.
    if let Some(tab) = self.reader_tabs.get_mut(self.reader_active_tab) {
      tab.reader.exit_voice_mode();
      tread::clear_images(&mut tab.image_state);
    }
    self.reader_tabs.remove(self.reader_active_tab);
    if self.reader_tabs.is_empty() {
      self.reader_active_tab = 0;
      self.reader_active = false;
      return true;
    }
    self.reader_active_tab = self.reader_active_tab.saturating_sub(1);
    false
  }

  /// Close the active secondary tab. Returns true if the pane is now empty.
  pub fn reader_secondary_close_active_tab(&mut self) -> bool {
    if self.reader_secondary_tabs.is_empty() {
      return true;
    }
    if let Some(tab) =
      self.reader_secondary_tabs.get_mut(self.reader_secondary_active_tab)
    {
      tab.reader.exit_voice_mode();
      tread::clear_images(&mut tab.image_state);
    }
    self.reader_secondary_tabs.remove(self.reader_secondary_active_tab);
    if self.reader_secondary_tabs.is_empty() {
      self.reader_secondary_active_tab = 0;
      return true;
    }
    self.reader_secondary_active_tab =
      self.reader_secondary_active_tab.saturating_sub(1);
    false
  }

  pub fn reader_prev_tab(&mut self) {
    // Voice is tied to the source you were reading.  Switching to a
    // different tab means you're not in that source anymore — stop
    // playback so audio doesn't keep going in the background.
    self.stop_all_reader_voice();
    match self.focused_reader {
      FocusedReader::Primary => {
        let n = self.reader_tabs.len();
        if n > 0 {
          self.reader_active_tab = (self.reader_active_tab + n - 1) % n;
        }
      }
      FocusedReader::Secondary => {
        let n = self.reader_secondary_tabs.len();
        if n > 0 {
          self.reader_secondary_active_tab =
            (self.reader_secondary_active_tab + n - 1) % n;
        }
      }
    }
  }

  pub fn reader_next_tab(&mut self) {
    self.stop_all_reader_voice();
    match self.focused_reader {
      FocusedReader::Primary => {
        let n = self.reader_tabs.len();
        if n > 0 {
          self.reader_active_tab = (self.reader_active_tab + 1) % n;
        }
      }
      FocusedReader::Secondary => {
        let n = self.reader_secondary_tabs.len();
        if n > 0 {
          self.reader_secondary_active_tab =
            (self.reader_secondary_active_tab + 1) % n;
        }
      }
    }
  }
}
