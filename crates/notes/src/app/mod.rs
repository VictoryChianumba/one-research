use std::collections::{BTreeSet, HashSet};

use chrono::Utc;
use ratatui::{
  Frame,
  layout::Rect,
  style::Style,
  text::Text,
  widgets::{Block, Borders, Paragraph, Wrap},
};
use rayon::prelude::*;

use crate::{
  Note, PaperRef,
  colored_tags::{ColoredTagsManager, TagColors},
  editor::{EditorMode, NoteEditor},
  entries_list::EntriesList,
  filter::{Filter, FilterCriterion, criterion::TagFilterOption},
  history::{Change, HistoryManager, HistoryStack},
  keymap::{UICommand, get_global_keymaps, get_multi_select_keymaps},
  sorter::{SortCriteria, SortOrder, Sorter},
  storage, theme,
  ui::{
    entry_popup::NotePopup,
    export_popup::ExportPopup,
    filter_popup::FilterPopup,
    fuzz_find::FuzzFindPopup,
    help_popup::{CommandRow, HelpPopup, KeybindingsTabs},
    msg_box::{MsgBox, MsgBoxActions, MsgBoxType},
    sort_popup::SortPopup,
  },
};

mod input;
pub mod runner;
pub mod state;
use state::AppState;

#[derive(Debug, PartialEq, Eq)]
pub enum HandleInputReturn {
  Handled,
  NotFound,
  ExitApp,
  Ignore,
}

const DEFAULT_HISTORY_LIMIT: usize = 50;

/// Which control currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlType {
  EntriesList,
  EditorPane,
}

/// State machine for the notes pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotesState {
  /// List fills the full pane; no preview visible.
  List,
  /// List visible, preview popup overlaid at the bottom; focus in list.
  Preview,
  /// Like Preview but keyboard focus is inside the preview for scrolling.
  PreviewScroll,
  /// Full-pane editor.
  Editor,
}

/// Which popup (if any) is currently shown.
pub enum ActivePopup {
  None,
  CreateNote(Box<NotePopup<'static>>),
  EditNote(Box<NotePopup<'static>>),
  Help(HelpPopup),
  Filter(Box<FilterPopup<'static>>),
  Sort(SortPopup),
  FuzzyFind(Box<FuzzFindPopup<'static>>),
  Export(Box<ExportPopup<'static>>),
  MsgBox(MsgBox),
}

impl Default for ActivePopup {
  fn default() -> Self {
    ActivePopup::None
  }
}

impl ActivePopup {
  pub fn is_none(&self) -> bool {
    matches!(self, ActivePopup::None)
  }
}

pub struct App {
  pub notes: Vec<Note>,
  pub current_note_id: Option<String>,
  pub filtered_out_notes: HashSet<String>,
  pub filter: Option<Filter>,
  pub full_screen: bool,
  pub editor: NoteEditor<'static>,
  pub entries_list: EntriesList,
  pub active_popup: ActivePopup,
  pub active_control: ControlType,
  pub notes_state: NotesState,
  pub preview_scroll: usize,
  sorter: Sorter,
  history: HistoryManager,
  colored_tags: ColoredTagsManager,
  /// Command waiting for a MsgBox confirmation before executing.
  pending_command: Option<UICommand>,
  /// Set by `focus_article` before `run()` so the runner selects the right note.
  initial_focus_id: Option<String>,
  initial_focus_title: String,
  initial_focus_url: String,
  /// Paper context held from `focus_article` until the CreateNote popup confirms.
  pending_paper: Option<PaperRef>,
  /// Set after note creation so trench can add the new tab.
  pub last_created_note_id: Option<String>,
}

impl App {
  pub fn new() -> Self {
    let mut s = Self {
      notes: Vec::new(),
      current_note_id: None,
      filtered_out_notes: HashSet::new(),
      filter: None,
      full_screen: false,
      editor: NoteEditor::new(),
      entries_list: EntriesList::new(),
      active_popup: ActivePopup::None,
      active_control: ControlType::EntriesList,
      notes_state: NotesState::List,
      preview_scroll: 0,
      sorter: Sorter::default(),
      history: HistoryManager::new(DEFAULT_HISTORY_LIMIT),
      colored_tags: ColoredTagsManager::new(),
      pending_command: None,
      initial_focus_id: None,
      initial_focus_title: String::new(),
      initial_focus_url: String::new(),
      pending_paper: None,
      last_created_note_id: None,
    };
    s.entries_list.set_active(true);
    s
  }

  /// The note ID currently visible (note selected or pending initial focus).
  pub fn focused_note_id(&self) -> Option<&str> {
    self.current_note_id.as_deref().or(self.initial_focus_id.as_deref())
  }

  /// Returns note_ids whose linked_papers contains this paper_id.
  pub fn find_notes_for_paper(&self, paper_id: &str) -> Vec<String> {
    self
      .notes
      .iter()
      .filter(|n| n.linked_papers.iter().any(|p| p.id == paper_id))
      .map(|n| n.note_id.clone())
      .collect()
  }

  /// Focus a specific note by note_id (used by tab switching).
  pub fn focus_note(&mut self, note_id: &str) {
    let pos = self.get_active_notes().position(|n| n.note_id == note_id);
    if let Some(pos) = pos {
      self.entries_list.state.select(Some(pos));
      self.sync_current_note_id();
      let note = self.get_active_notes().nth(pos).cloned();
      self.editor.load_note(note.as_ref());
      self.notes_state = NotesState::Preview;
      self.preview_scroll = 0;
    }
  }

  /// Get a note's title by note_id (used by trench to build tab label).
  pub fn get_note_title(&self, note_id: &str) -> Option<String> {
    self.notes.iter().find(|n| n.note_id == note_id).map(|n| n.title.clone())
  }

  /// Store article info so the runner can focus the right note after loading.
  pub fn focus_article(&mut self, id: &str, title: &str, url: &str) {
    self.initial_focus_id = Some(id.to_string());
    self.initial_focus_title = title.to_string();
    self.initial_focus_url = url.to_string();
    self.pending_paper = Some(PaperRef {
      id: id.to_string(),
      title: title.to_string(),
      url: url.to_string(),
    });
  }

  /// Called by the runner after `load_notes` to position the list and editor.
  pub fn apply_initial_focus(&mut self) {
    self.notes_state = NotesState::List;
    self.entries_list.multi_select_mode = false;
    self.entries_list.selected_notes.clear();

    let Some(id) = self.initial_focus_id.take() else {
      // No focus requested — select the first note if any.
      if !self.notes.is_empty() {
        self.entries_list.state.select(Some(0));
        self.sync_current_note_id();
        let note = self.get_active_notes().next().cloned();
        self.editor.load_note(note.as_ref());
      }
      return;
    };

    let pos = self
      .get_active_notes()
      .position(|n| n.linked_papers.iter().any(|p| p.id == id));
    if let Some(pos) = pos {
      self.entries_list.state.select(Some(pos));
      self.sync_current_note_id();
      let note = self.get_active_notes().nth(pos).cloned();
      self.editor.load_note(note.as_ref());
    } else {
      // Note doesn't exist yet — open create popup pre-filled with article title.
      let title = std::mem::take(&mut self.initial_focus_title);
      self.initial_focus_url = String::new();
      self.active_popup =
        ActivePopup::CreateNote(Box::new(NotePopup::with_prefill(&title)));
    }
  }

  // ── Focus & layout helpers ──────────────────────────────────────────────

  #[allow(dead_code)]
  fn change_active_control(&mut self, control: ControlType) {
    self.active_control = control;
    let list_active = control == ControlType::EntriesList;
    self.entries_list.set_active(list_active);
    self.editor.set_active(!list_active);
  }

  /// Set the current note by id, syncing the list selection and editor content.
  pub fn set_current_note(&mut self, id: Option<String>) {
    self.current_note_id = id.clone();
    if let Some(ref id) = id {
      let pos = self.get_active_notes().position(|n| n.note_id == *id);
      self.entries_list.state.select(pos);
      let note = self.notes.iter().find(|n| n.note_id == *id).cloned();
      self.editor.load_note(note.as_ref());
    } else {
      self.entries_list.state.select(None);
      self.editor.load_note(None);
    }
  }

  /// Reload the current note content (discarding edits) and return focus to the list.
  pub fn discard_current_content(&mut self) {
    let note = self.get_current_note().cloned();
    self.editor.load_note(note.as_ref());
    self.go_to_list();
  }

  fn go_to_editor(&mut self) {
    self.notes_state = NotesState::Editor;
    self.editor.set_active(true);
    self.editor.set_editor_mode(EditorMode::Insert);
    self.entries_list.set_active(false);
  }

  fn go_to_list(&mut self) {
    self.notes_state = NotesState::List;
    self.editor.set_active(false);
    self.editor.set_editor_mode(EditorMode::Normal);
    self.entries_list.set_active(true);
  }

  // ── Popup show helpers ──────────────────────────────────────────────────

  fn show_msg_box(
    &mut self,
    msg_type: MsgBoxType,
    actions: MsgBoxActions,
    pending: UICommand,
  ) {
    self.pending_command = Some(pending);
    self.active_popup = ActivePopup::MsgBox(MsgBox::new(msg_type, actions));
  }

  fn show_err_msg(&mut self, msg: String) {
    self.active_popup = ActivePopup::MsgBox(MsgBox::new(
      MsgBoxType::Error(msg),
      MsgBoxActions::Ok,
    ));
  }

  fn populate_help_popup(&self, popup: &mut HelpPopup) {
    for km in get_global_keymaps() {
      let info = km.command.get_info();
      popup.global_bindings_mut().push(CommandRow::new(
        km.key.to_string(),
        info.name,
        info.description,
      ));
    }
    for km in get_multi_select_keymaps() {
      let info = km.command.get_info();
      popup.multi_select_bindings_mut().push(CommandRow::new(
        km.key.to_string(),
        info.name,
        info.description,
      ));
    }
  }

  // ── Rendering ───────────────────────────────────────────────────────────

  /// Main render entry point — called by the runner's draw function.
  /// `area` is the portion of the frame allocated to the notes pane.
  pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
    let main_area = area;

    // Pre-collect to release the borrow on self before calling render methods.
    let active_notes: Vec<Note> = self.get_active_notes().cloned().collect();
    let has_filter = self.filter.is_some();

    match self.notes_state {
      NotesState::Editor => {
        self.editor.render_widget(frame, main_area);
      }
      NotesState::List => {
        self.entries_list.render_widget(
          frame,
          main_area,
          &active_notes,
          &self.colored_tags,
          has_filter,
        );
      }
      NotesState::Preview | NotesState::PreviewScroll => {
        // List fills main area …
        self.entries_list.render_widget(
          frame,
          main_area,
          &active_notes,
          &self.colored_tags,
          has_filter,
        );
        // … preview popup overlaid at the bottom (~40 % height).
        let note = self.get_current_note().cloned();
        if let Some(note) = note {
          let preview_h = (main_area.height * 40 / 100).max(4);
          let preview_area = Rect {
            x: main_area.x,
            y: main_area.y + main_area.height.saturating_sub(preview_h),
            width: main_area.width,
            height: preview_h,
          };
          self.draw_preview(frame, preview_area, &note);
        }
      }
    }

    // Popup overlays rendered last so they appear on top.
    self.draw_popup(frame, area);
  }

  pub fn draw_editor_surface(&mut self, frame: &mut Frame, area: Rect) {
    self.editor.render_widget(frame, area);
  }

  pub fn draw_popup_overlay(&mut self, frame: &mut Frame, area: Rect) {
    self.draw_popup(frame, area);
  }

  fn draw_preview(&self, frame: &mut Frame, area: Rect, note: &Note) {
    let para = Paragraph::new(Text::raw(note.content.clone()))
      .block(
        Block::default()
          .borders(Borders::ALL)
          .title("─── Preview ───")
          .border_style(Style::default().fg(theme::current().border)),
      )
      .wrap(Wrap { trim: false })
      .scroll((self.preview_scroll as u16, 0));
    frame.render_widget(para, area);
  }

  fn draw_popup(&mut self, frame: &mut Frame, area: Rect) {
    match &mut self.active_popup {
      ActivePopup::None => {}
      ActivePopup::Help(p) => p.render_widget(frame, area),
      ActivePopup::MsgBox(p) => p.render_widget(frame, area),
      ActivePopup::CreateNote(p) => p.render_widget(frame, area),
      ActivePopup::EditNote(p) => p.render_widget(frame, area),
      ActivePopup::Filter(p) => p.render_widget(frame, area),
      ActivePopup::Sort(p) => p.render_widget(frame, area),
      ActivePopup::FuzzyFind(p) => p.render_widget(frame, area),
      ActivePopup::Export(p) => p.render_widget(frame, area),
    }
  }

  // ── Notes data loading ──────────────────────────────────────────────────

  /// Load all notes from disk, then apply sort and filter.
  pub fn load_notes(&mut self) -> anyhow::Result<()> {
    log::trace!("Loading notes");
    self.notes = storage::load_all_notes()?;
    self.sort_notes();
    self.update_filtered_out_notes();
    self.update_colored_tags();
    Ok(())
  }

  /// Load persisted app state (sorter, full_screen) from disk.
  pub fn load_state(&mut self) {
    match AppState::load() {
      Ok(state) => {
        self.sorter = state.sorter;
        self.full_screen = state.full_screen;
      }
      Err(err) => {
        log::error!("Loading state failed, using defaults. Error: {err}");
      }
    }
  }

  /// Persist app state (sorter, full_screen) to disk.
  pub fn persist_state(&self) -> anyhow::Result<()> {
    AppState { sorter: self.sorter.clone(), full_screen: self.full_screen }
      .save()
  }

  // ── Active-notes view ───────────────────────────────────────────────────

  /// Notes that pass the current filter (or all notes when there is no filter).
  pub fn get_active_notes(&self) -> impl DoubleEndedIterator<Item = &Note> {
    self.notes.iter().filter(|n| !self.filtered_out_notes.contains(&n.note_id))
  }

  pub fn get_note(&self, id: &str) -> Option<&Note> {
    self.get_active_notes().find(|n| n.note_id == id)
  }

  pub fn get_current_note(&self) -> Option<&Note> {
    self
      .current_note_id
      .as_deref()
      .and_then(|id| self.get_active_notes().find(|n| n.note_id == id))
  }

  /// Returns every unique tag that appears in any note.
  pub fn get_all_tags(&self) -> Vec<String> {
    let mut tags = BTreeSet::new();
    for tag in self.notes.iter().flat_map(|n| &n.tags) {
      tags.insert(tag);
    }
    tags.into_iter().map(String::from).collect()
  }

  // ── Navigation ──────────────────────────────────────────────────────────

  pub fn select_next_note(&mut self) {
    let count = self.get_active_notes().count();
    if count == 0 {
      return;
    }
    let next = match self.entries_list.state.selected() {
      Some(i) => (i + 1).min(count - 1),
      None => 0,
    };
    self.entries_list.state.select(Some(next));
    self.sync_current_note_id();
  }

  pub fn select_prev_note(&mut self) {
    let count = self.get_active_notes().count();
    if count == 0 {
      return;
    }
    let prev = match self.entries_list.state.selected() {
      Some(0) | None => 0,
      Some(i) => i - 1,
    };
    self.entries_list.state.select(Some(prev));
    self.sync_current_note_id();
  }

  pub fn go_to_top(&mut self) {
    if self.get_active_notes().count() > 0 {
      self.entries_list.state.select(Some(0));
      self.sync_current_note_id();
    }
  }

  pub fn go_to_bottom(&mut self) {
    let count = self.get_active_notes().count();
    if count > 0 {
      self.entries_list.state.select(Some(count - 1));
      self.sync_current_note_id();
    }
  }

  pub fn page_up(&mut self, page_size: usize) {
    let count = self.get_active_notes().count();
    if count == 0 {
      return;
    }
    let prev = match self.entries_list.state.selected() {
      Some(i) => i.saturating_sub(page_size),
      None => 0,
    };
    self.entries_list.state.select(Some(prev));
    self.sync_current_note_id();
  }

  pub fn page_down(&mut self, page_size: usize) {
    let count = self.get_active_notes().count();
    if count == 0 {
      return;
    }
    let next = match self.entries_list.state.selected() {
      Some(i) => (i + page_size).min(count - 1),
      None => 0,
    };
    self.entries_list.state.select(Some(next));
    self.sync_current_note_id();
  }

  /// Keeps `current_note_id` in sync with the list selection.
  fn sync_current_note_id(&mut self) {
    self.current_note_id = self
      .entries_list
      .state
      .selected()
      .and_then(|i| self.get_active_notes().nth(i))
      .map(|n| n.note_id.clone());
  }

  /// After the active-notes slice changes (sort/filter), re-select the same
  /// note by id, or clamp to the last item.
  fn reselect_current_note(&mut self) {
    let count = self.get_active_notes().count();
    if count == 0 {
      self.entries_list.state.select(None);
      self.current_note_id = None;
      return;
    }

    // Try to keep the same note selected by id.
    if let Some(id) = self.current_note_id.clone() {
      let pos = self.get_active_notes().position(|n| n.note_id == id);
      if let Some(pos) = pos {
        self.entries_list.state.select(Some(pos));
        return;
      }
    }

    // Fall back: keep current index or clamp.
    let clamped =
      self.entries_list.state.selected().unwrap_or(0).min(count - 1);
    self.entries_list.state.select(Some(clamped));
    self.sync_current_note_id();
  }

  // ── CRUD ────────────────────────────────────────────────────────────────

  /// Create a new note, persist it, and register an undo entry.
  pub fn create_note(
    &mut self,
    title: String,
    tags: Vec<String>,
    paper: Option<PaperRef>,
  ) -> anyhow::Result<String> {
    let now = Utc::now();
    let note = Note {
      note_id: new_id(),
      title,
      content: String::new(),
      tags,
      linked_papers: paper.into_iter().collect(),
      created_at: now,
      updated_at: now,
    };
    let id = self.insert_note_intern(note, HistoryStack::Undo)?;
    self.last_created_note_id = Some(id.clone());
    Ok(id)
  }

  /// Insert a fully-constructed note (used by undo/redo to restore deleted notes).
  fn insert_note_intern(
    &mut self,
    note: Note,
    history_target: HistoryStack,
  ) -> anyhow::Result<String> {
    storage::save_note(&note)?;
    let id = note.note_id.clone();
    self.history.register_add(history_target, &note);
    self.notes.push(note);
    self.sort_notes();
    self.update_filtered_out_notes();
    self.update_colored_tags();
    Ok(id)
  }

  /// Update the title, linked_papers, and tags of the currently selected note.
  pub fn update_current_note_attributes(
    &mut self,
    title: String,
    linked_papers: Vec<PaperRef>,
    tags: Vec<String>,
  ) -> anyhow::Result<()> {
    let Some(id) = self.current_note_id.clone() else {
      log::warn!("update_current_note_attributes called with no selected note");
      return Ok(());
    };
    self.update_note_attributes(
      &id,
      title,
      linked_papers,
      tags,
      HistoryStack::Undo,
    )
  }

  fn update_note_attributes(
    &mut self,
    id: &str,
    title: String,
    linked_papers: Vec<PaperRef>,
    tags: Vec<String>,
    history_target: HistoryStack,
  ) -> anyhow::Result<()> {
    log::trace!("Updating note attributes for id: {id}");

    // Graceful no-op if the note has gone away between UI dispatch and now
    // (stale ID, race with delete, future refactor breaks the invariant).
    // Was an `.expect("note must exist...")` panic over a raw-mode terminal
    // — bounded by the panic hook but a real risk surface. (Audit Rel
    // MED #33.)
    let Some(note) = self.notes.iter_mut().find(|n| n.note_id == id) else {
      log::warn!(
        "notes::update_note_attributes: id {id:?} missing — ignoring update"
      );
      return Ok(());
    };

    self.history.register_change_attributes(history_target, note);

    note.title = title;
    note.linked_papers = linked_papers;
    note.tags = tags;
    note.updated_at = Utc::now();

    let clone = note.clone();
    storage::save_note(&clone)?;

    self.sort_notes();
    self.update_filter();
    self.update_filtered_out_notes();
    self.update_colored_tags();

    Ok(())
  }

  /// Save the editor's current content back to the currently selected note.
  pub fn save_current_note_content(&mut self) -> anyhow::Result<()> {
    let Some(id) = self.current_note_id.clone() else {
      log::warn!("save_current_note_content called with no selected note");
      return Ok(());
    };
    let content = self.editor.get_content();
    self.update_note_content(&id, content, HistoryStack::Undo)
  }

  fn update_note_content(
    &mut self,
    id: &str,
    content: String,
    history_target: HistoryStack,
  ) -> anyhow::Result<()> {
    log::trace!("Updating note content for id: {id}");

    // Graceful no-op on missing-id.
    let Some(note) = self.notes.iter_mut().find(|n| n.note_id == id) else {
      log::warn!(
        "notes::update_note_content: id {id:?} missing — ignoring update"
      );
      return Ok(());
    };

    self.history.register_change_content(history_target, note);

    note.content = content;
    note.updated_at = Utc::now();

    let clone = note.clone();
    storage::save_note(&clone)?;

    self.update_filtered_out_notes();

    Ok(())
  }

  pub fn delete_current_note(&mut self) -> anyhow::Result<()> {
    let Some(id) = self.current_note_id.clone() else {
      log::warn!("delete_current_note called with no selected note");
      return Ok(());
    };
    self.delete_note_intern(&id, HistoryStack::Undo)
  }

  fn delete_note_intern(
    &mut self,
    id: &str,
    history_target: HistoryStack,
  ) -> anyhow::Result<()> {
    log::trace!("Deleting note with id: {id}");

    storage::delete_note(id)?;

    // Graceful no-op when the in-memory list lacks the id even though the
    // disk delete succeeded — already-gone state with no harmful effect.
    let Some(removed) = self
      .notes
      .iter()
      .position(|n| n.note_id == id)
      .map(|pos| self.notes.remove(pos))
    else {
      log::warn!(
        "notes::delete_note_intern: id {id:?} not in list (already gone?) — \
         on-disk delete already succeeded, skipping history register"
      );
      return Ok(());
    };

    self.history.register_remove(history_target, removed);

    self.update_filter();
    self.update_filtered_out_notes();
    self.update_colored_tags();
    self.reselect_current_note();

    Ok(())
  }

  // ── Filter ──────────────────────────────────────────────────────────────

  pub fn apply_filter(&mut self, filter: Option<Filter>) {
    self.filter = filter;
    self.update_filtered_out_notes();
    self.reselect_current_note();
  }

  /// Remove tag-based filter criteria that no longer match any existing tag.
  fn update_filter(&mut self) {
    if self.filter.is_some() {
      let all_tags = self.get_all_tags();
      let filter = self.filter.as_mut().unwrap();
      filter.criteria.retain(|cr| match cr {
        FilterCriterion::Tag(TagFilterOption::Tag(tag)) => {
          all_tags.contains(tag)
        }
        FilterCriterion::Tag(TagFilterOption::NoTags) => true,
        FilterCriterion::Title(_) | FilterCriterion::Content(_) => true,
      });
      if filter.criteria.is_empty() {
        self.filter = None;
      }
    }
  }

  fn update_filtered_out_notes(&mut self) {
    if let Some(filter) = self.filter.as_ref() {
      self.filtered_out_notes = self
        .notes
        .par_iter()
        .filter(|n| !filter.check_note(n))
        .map(|n| n.note_id.clone())
        .collect();
    } else {
      self.filtered_out_notes.clear();
    }
  }

  /// Cycle through tag filters: none → tag1 → tag2 → … → NoTags → none.
  pub fn cycle_tag_filter(&mut self) {
    let all_tags = self.get_all_tags();
    if all_tags.is_empty() {
      return;
    }
    let all_options: Vec<TagFilterOption> = all_tags
      .into_iter()
      .map(TagFilterOption::Tag)
      .chain(std::iter::once(TagFilterOption::NoTags))
      .collect();

    if let Some(mut filter) = self.filter.take() {
      let tag_criteria: Vec<_> = filter
        .criteria
        .iter()
        .filter_map(|c| match c {
          FilterCriterion::Tag(t) => Some(t),
          _ => None,
        })
        .collect();

      match tag_criteria.len() {
        0 => {
          filter.criteria.push(FilterCriterion::Tag(
            all_options.into_iter().next().unwrap(),
          ));
        }
        1 => {
          let current = filter
            .criteria
            .iter_mut()
            .find_map(|c| match c {
              FilterCriterion::Tag(t) => Some(t),
              _ => None,
            })
            .unwrap();
          let pos = all_options.iter().position(|t| t == current).unwrap_or(0);
          let next = (pos + 1) % all_options.len();
          *current = all_options.into_iter().nth(next).unwrap();
        }
        _ => {
          filter.criteria.retain(|c| !matches!(c, FilterCriterion::Tag(_)));
          filter.criteria.push(FilterCriterion::Tag(
            all_options.into_iter().next().unwrap(),
          ));
        }
      }
      self.apply_filter(Some(filter));
    } else {
      let mut filter = Filter::default();
      filter
        .criteria
        .push(FilterCriterion::Tag(all_options.into_iter().next().unwrap()));
      self.apply_filter(Some(filter));
    }
  }

  // ── Sort ────────────────────────────────────────────────────────────────

  pub fn apply_sort(&mut self, criteria: Vec<SortCriteria>, order: SortOrder) {
    self.sorter.set_criteria(criteria);
    self.sorter.order = order;
    self.sort_notes();
    self.reselect_current_note();
  }

  pub fn get_sorter(&self) -> &Sorter {
    &self.sorter
  }

  fn sort_notes(&mut self) {
    self.notes.sort_by(|a, b| self.sorter.sort(a, b));
  }

  // ── Colored tags ────────────────────────────────────────────────────────

  fn update_colored_tags(&mut self) {
    let tags = self.get_all_tags();
    self.colored_tags.update_tags(tags);
  }

  pub fn get_color_for_tag(&self, tag: &str) -> Option<TagColors> {
    self.colored_tags.get_tag_color(tag)
  }

  pub fn colored_tags(&self) -> &ColoredTagsManager {
    &self.colored_tags
  }

  // ── Undo / Redo ─────────────────────────────────────────────────────────

  pub fn undo(&mut self) -> anyhow::Result<Option<String>> {
    match self.history.pop_undo() {
      Some(change) => self.apply_history_change(change, HistoryStack::Redo),
      None => Ok(None),
    }
  }

  pub fn redo(&mut self) -> anyhow::Result<Option<String>> {
    match self.history.pop_redo() {
      Some(change) => self.apply_history_change(change, HistoryStack::Undo),
      None => Ok(None),
    }
  }

  fn apply_history_change(
    &mut self,
    change: Change,
    history_target: HistoryStack,
  ) -> anyhow::Result<Option<String>> {
    match change {
      Change::AddNote { id } => {
        log::trace!("History Apply: delete note id={id}");
        self.delete_note_intern(&id, history_target)?;
        Ok(None)
      }
      Change::RemoveNote(note) => {
        log::trace!("History Apply: restore note id={}", note.note_id);
        let id = self.insert_note_intern(*note, history_target)?;
        Ok(Some(id))
      }
      Change::NoteAttribute(attr) => {
        log::trace!("History Apply: restore attributes for id={}", attr.id);
        self.update_note_attributes(
          &attr.id,
          attr.title,
          attr.linked_papers,
          attr.tags,
          history_target,
        )?;
        Ok(Some(attr.id))
      }
      Change::NoteContent { id, content } => {
        log::trace!("History Apply: restore content for id={id}");
        self.update_note_content(&id, content, history_target)?;
        Ok(Some(id))
      }
    }
  }

  // ── Popup openers ───────────────────────────────────────────────────────

  pub fn open_create_note_popup(&mut self) {
    self.active_popup =
      ActivePopup::CreateNote(Box::new(NotePopup::new_note()));
  }

  pub fn open_edit_note_popup(&mut self) {
    if let Some(note) = self.get_current_note() {
      self.active_popup =
        ActivePopup::EditNote(Box::new(NotePopup::from_note(note)));
    }
  }

  pub fn open_help_popup(&mut self, tab: KeybindingsTabs) {
    let mut popup = HelpPopup::new(tab);
    self.populate_help_popup(&mut popup);
    self.active_popup = ActivePopup::Help(popup);
  }

  pub fn open_filter_popup(&mut self) {
    let tags = self.get_all_tags();
    let existing = self.filter.clone();
    self.active_popup =
      ActivePopup::Filter(Box::new(FilterPopup::new(tags, existing)));
  }

  pub fn open_sort_popup(&mut self) {
    let popup = SortPopup::new(&self.sorter);
    self.active_popup = ActivePopup::Sort(popup);
  }

  pub fn open_fuzz_find_popup(&mut self) {
    let notes_map = self
      .get_active_notes()
      .map(|n| (n.note_id.clone(), n.title.clone()))
      .collect();
    self.active_popup =
      ActivePopup::FuzzyFind(Box::new(FuzzFindPopup::new(notes_map)));
  }

  pub fn close_popup(&mut self) {
    self.active_popup = ActivePopup::None;
  }
}

/// Generates a simple time-based unique id.
fn new_id() -> String {
  use std::time::{SystemTime, UNIX_EPOCH};
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_nanos()
    .to_string()
}

impl Default for App {
  fn default() -> Self {
    Self::new()
  }
}
