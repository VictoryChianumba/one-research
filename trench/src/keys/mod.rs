use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc;

use crate::app::{
  App, AppView, CustomThemeEditorMode, FeedTab, FocusedReader, NavDirection,
  NotesMode, NotesTab, PaneId, QuitPopupKind,
};

use super::{get_pane_by_number, spawn_fulltext_fetch, truncate_for_notif};

mod feed;
mod help;
mod popups;
mod reader;
mod repo;
mod settings;
mod sources;
use feed::handle_feed_view;
use help::handle_help_overlay;
use popups::handle_tag_picker;
use reader::{
  close_all_readers, handle_reader_bottom_pane, handle_reader_pane,
  reader_back, reader_pane_focused,
};
use repo::handle_repo_viewer;
use settings::handle_settings_view;
use sources::handle_sources_popup;

const NOTES_MODE_ORDER: [NotesMode; 3] =
  [NotesMode::PaperNotes, NotesMode::Library, NotesMode::Capture];

/// Top-level key dispatcher — called once per key press event from the main loop.
pub fn dispatch(key: KeyEvent, app: &mut App) {
  // Tag picker popup — intercepts all keys when open.
  if app.tag_picker.active {
    handle_tag_picker(key, app);
    return;
  }

  // Quit popup — intercepts all keys until dismissed.
  if app.quit_popup.active {
    match key.code {
      KeyCode::Char('q') | KeyCode::Enter => {
        app.quit_popup.active = false;
        match app.quit_popup.kind {
          QuitPopupKind::LeaveReader => {
            let pane_empty = app.reader_close_active_tab();
            if pane_empty {
              if app.reader_dual_active {
                app.reader_dual_active = false;
                app.reader_bottom_open = false;
                app.reader_bottom_focused = false;
                app.reader_secondary_tabs.clear();
                app.reader_secondary_active_tab = 0;
                app.secondary_notes_active = false;
              } else if app.reader_split_active {
                app.reader_split_active = false;
              }
              app.focus.focused_pane = PaneId::Feed;
            }
          }
          _ => app.should_quit = true,
        }
      }
      KeyCode::Esc => {
        app.quit_popup.active = false;
      }
      _ => {}
    }
    return;
  }

  // Abstract popup — any of Space / Enter / Esc dismisses.
  if app.abstract_popup_active {
    if matches!(key.code, KeyCode::Char(' ') | KeyCode::Esc | KeyCode::Enter) {
      app.abstract_popup_active = false;
    }
    return;
  }

  // Reader popup (A1) — fully interactive; Esc or reader Quit dismisses.
  if app.reader_popup_active {
    if key.code == KeyCode::Esc {
      if let Some(reader) = app.reader_popup_editor.as_mut() {
        reader.exit_voice_mode();
      }
      app.reader_popup_active = false;
    } else if let Some(reader) = app.reader_popup_editor.as_mut() {
      let action = reader.handle_event(Event::Key(key));
      if matches!(action, tread::ReaderAction::Quit) {
        reader.exit_voice_mode();
        app.reader_popup_active = false;
      }
    }
    return;
  }

  // Tab window prompt — intercepts [1]/[2]/Esc to choose which reader pane.
  if app.tab_window_prompt_active {
    match key.code {
      KeyCode::Char('1') => {
        app.tab_window_prompt_active = false;
        app.fulltext_for_secondary = false;
        app.fulltext_new_tab = true;
        trigger_fulltext_new_tab(app);
      }
      KeyCode::Char('2') => {
        app.tab_window_prompt_active = false;
        app.fulltext_for_secondary = true;
        app.fulltext_new_tab = true;
        trigger_fulltext_new_tab(app);
      }
      KeyCode::Esc => {
        app.tab_window_prompt_active = false;
        app.set_notification("Cancelled.".to_string());
      }
      _ => {}
    }
    return;
  }

  if handle_help_overlay(key, app) {
    return;
  }
  // `?` shows tread's reader-help overlay when a reader pane is focused
  // — vim-style key reference for the embedded reader.  Otherwise fall
  // through to trench's app-level help (Sections / Navigation / etc.).
  if key.code == KeyCode::Char('?')
    && !is_text_entry_context(app)
    && reader_pane_focused(app)
  {
    let reader_action = if app.reader_popup_active {
      app.reader_popup_editor.as_mut().map(|r| r.handle_event(Event::Key(key)))
    } else if app.focus.focused_pane == PaneId::SecondaryReader {
      app.reader_secondary_editor_mut().map(|r| r.handle_event(Event::Key(key)))
    } else {
      app.reader_editor_mut().map(|r| r.handle_event(Event::Key(key)))
    };
    drop(reader_action);
    return;
  }
  if key.code == KeyCode::Char('?') && !is_text_entry_context(app) {
    app.leader_active = false;
    app.help.active = true;
    app.help.section = 0;
    app.help.scroll.reset();
    return;
  }
  if handle_leader_or_ctrl_t(key, app) {
    return;
  }
  // State-3 bottom pane (A2) — handles its own key set when open and focused.
  if app.reader_dual_active
    && app.reader_bottom_open
    && app.reader_bottom_focused
  {
    handle_reader_bottom_pane(key, app);
    return;
  }
  if handle_chat_pane(key, app) {
    return;
  }
  if handle_notes_pane(key, app) {
    return;
  }
  if handle_reader_pane(key, app) {
    return;
  }
  if handle_repo_viewer(key, app) {
    return;
  }
  if handle_sources_popup(key, app) {
    return;
  }
  if handle_settings_view(key, app) {
    return;
  }
  handle_feed_view(key, app);
}

/// True when keyboard input should be treated as text entry rather than
/// a hotkey — search bar, sources popup input, settings field editing,
/// discovery palette, focused chat/notes panes, custom theme editor.
fn is_text_entry_context(app: &App) -> bool {
  if app.search_active || app.sources_popup.input.is_focused() || app.settings.editing {
    return true;
  }
  if app.feed_tab == FeedTab::Discoveries && app.discovery.search_focused {
    return true;
  }
  if app.chat.active && app.focus.focused_pane == PaneId::Chat {
    return true;
  }
  if app.notes_active && app.focus.focused_pane == PaneId::Notes {
    return true;
  }
  if app.secondary_notes_active && app.focus.focused_pane == PaneId::SecondaryNotes {
    return true;
  }
  app.theme_picker.custom_editor.as_ref().is_some_and(|editor| {
    matches!(
      editor.mode,
      CustomThemeEditorMode::Name | CustomThemeEditorMode::Hex
    )
  })
}

// ── Leader key (Ctrl+T) ───────────────────────────────────────────────────────

fn handle_leader_or_ctrl_t(key: KeyEvent, app: &mut App) -> bool {
  // Expire leader if timeout elapsed.
  if app.leader_active
    && app
      .leader_activated_at
      .map(|t| t.elapsed().as_millis() > app.leader_timeout_ms as u128)
      .unwrap_or(false)
  {
    app.leader_active = false;
  }

  // Ctrl+T: arm the leader.
  if key.code == KeyCode::Char('t') && key.modifiers == KeyModifiers::CONTROL {
    app.leader_active = true;
    app.leader_activated_at = Some(std::time::Instant::now());
    return true;
  }

  if !app.leader_active {
    return false;
  }
  handle_leader(key, app);
  true
}

fn open_notes(app: &mut App) {
  let side = note_side_for_focus(app);
  let context = resolve_notes_paper_context(app, side);

  if app.notes_app.is_none() {
    let mut na = notes::app::App::new();
    na.load_state();
    if let Err(e) = na.load_notes() {
      log::error!("notes: failed to load notes: {e}");
    }
    app.notes_app = Some(na);
  }

  // Drop tabs whose note no longer exists (deleted notes, stale ui.json).
  if let Some(na) = app.notes_app.as_ref() {
    app.notes_tabs.retain(|t| na.get_note_title(&t.note_id).is_some());
    app.notes_active_tab =
      app.notes_active_tab.min(app.notes_tabs.len().saturating_sub(1));
    app
      .secondary_notes_tabs
      .retain(|t| na.get_note_title(&t.note_id).is_some());
    app.secondary_notes_active_tab = app
      .secondary_notes_active_tab
      .min(app.secondary_notes_tabs.len().saturating_sub(1));
  }

  // Phase 1: find linked notes and collect titles (releases borrow before switch).
  let linked = app
    .notes_app
    .as_ref()
    .map(|na| {
      context
        .as_ref()
        .map(|ctx| na.find_notes_for_paper(&ctx.paper.id))
        .unwrap_or_default()
    })
    .unwrap_or_default();

  app.set_notes_context_for_side(side, context.clone());

  if context.is_some() {
    if linked.is_empty() {
      activate_notes_mode(app, side, NotesMode::Capture);
    } else {
      activate_notes_mode(app, side, NotesMode::PaperNotes);
    }
  } else {
    activate_notes_mode(app, side, NotesMode::Library);
  }

  set_notes_side_active(app, side, true);
  app.focus.focused_pane = note_pane_for_side(app, side);
}

fn notes_shell_shortcuts_allowed(notes_app: &notes::app::App) -> bool {
  notes_app.active_popup.is_none()
    && notes_app.notes_state != notes::app::NotesState::Editor
}

fn visible_note_ids(app: &App, side: FocusedReader) -> Vec<String> {
  let Some(notes_app) = app.notes_app.as_ref() else {
    return Vec::new();
  };
  match app.notes_mode_for_side(side) {
    NotesMode::Capture => Vec::new(),
    NotesMode::Library => {
      notes_app.get_active_notes().map(|note| note.note_id.clone()).collect()
    }
    NotesMode::PaperNotes => {
      let Some(context) = app.notes_context_for_side(side) else {
        return Vec::new();
      };
      notes_app
        .get_active_notes()
        .filter(|note| {
          note.linked_papers.iter().any(|paper| paper.id == context.paper.id)
        })
        .map(|note| note.note_id.clone())
        .collect()
    }
  }
}

fn ensure_notes_browser_selection(app: &mut App, side: FocusedReader) {
  let visible_ids = visible_note_ids(app, side);
  let Some(notes_app) = app.notes_app.as_mut() else {
    return;
  };

  notes_app.notes_state = notes::app::NotesState::List;
  if visible_ids.is_empty() {
    notes_app.set_current_note(None);
    return;
  }

  let current = notes_app.current_note_id.clone();
  if current
    .as_ref()
    .is_some_and(|id| visible_ids.iter().any(|visible| visible == id))
  {
    return;
  }

  notes_app.set_current_note(visible_ids.first().cloned());
}

fn sync_notes_tabs_for_paper_mode(
  app: &mut App,
  side: FocusedReader,
  context: &crate::app::NotesContext,
) {
  let linked = app
    .notes_app
    .as_ref()
    .map(|na| na.find_notes_for_paper(&context.paper.id))
    .unwrap_or_default();

  for note_id in &linked {
    let exists = match side {
      FocusedReader::Primary => {
        app.notes_tabs.iter().any(|tab| &tab.note_id == note_id)
      }
      FocusedReader::Secondary => {
        app.secondary_notes_tabs.iter().any(|tab| &tab.note_id == note_id)
      }
    };
    if exists {
      continue;
    }
    let title = app
      .notes_app
      .as_ref()
      .and_then(|na| na.get_note_title(note_id))
      .unwrap_or_default();
    match side {
      FocusedReader::Primary => {
        app.notes_tabs.push(NotesTab { note_id: note_id.clone(), title });
      }
      FocusedReader::Secondary => {
        app
          .secondary_notes_tabs
          .push(NotesTab { note_id: note_id.clone(), title });
      }
    }
  }
}

fn sync_notes_tab_selection_to_current_note(
  app: &mut App,
  side: FocusedReader,
) {
  let current_id = app
    .notes_app
    .as_ref()
    .and_then(|notes_app| notes_app.current_note_id.clone());
  let Some(current_id) = current_id else {
    return;
  };
  match side {
    FocusedReader::Primary => {
      if let Some(idx) =
        app.notes_tabs.iter().position(|tab| tab.note_id == current_id)
      {
        app.notes_active_tab = idx;
      }
    }
    FocusedReader::Secondary => {
      if let Some(idx) = app
        .secondary_notes_tabs
        .iter()
        .position(|tab| tab.note_id == current_id)
      {
        app.secondary_notes_active_tab = idx;
      }
    }
  }
}

fn activate_notes_mode(app: &mut App, side: FocusedReader, mode: NotesMode) {
  let context = app
    .notes_context_for_side(side)
    .cloned()
    .or_else(|| resolve_notes_paper_context(app, side));
  if context.is_some() {
    app.set_notes_context_for_side(side, context.clone());
  }
  app.set_notes_mode_for_side(side, mode);

  if let Some(notes_app) = app.notes_app.as_mut() {
    notes_app.notes_state = notes::app::NotesState::List;
    notes_app.active_popup = notes::app::ActivePopup::None;
  }

  match mode {
    NotesMode::Capture => {
      if let Some(notes_app) = app.notes_app.as_mut() {
        notes_app.set_current_note(None);
      }
    }
    NotesMode::Library => {
      ensure_notes_browser_selection(app, side);
    }
    NotesMode::PaperNotes => {
      if let Some(context) = context.as_ref() {
        sync_notes_tabs_for_paper_mode(app, side, context);
      }
      ensure_notes_browser_selection(app, side);
      sync_notes_tab_selection_to_current_note(app, side);
    }
  }
}

fn cycle_notes_mode(app: &mut App, side: FocusedReader, direction: isize) {
  let current = NOTES_MODE_ORDER
    .iter()
    .position(|mode| *mode == app.notes_mode_for_side(side))
    .unwrap_or(0) as isize;
  let next = (current + direction).rem_euclid(NOTES_MODE_ORDER.len() as isize);
  activate_notes_mode(app, side, NOTES_MODE_ORDER[next as usize]);
}

fn begin_capture_note(app: &mut App, side: FocusedReader) {
  let Some(context) = app.notes_context_for_side(side).cloned() else {
    app.set_notification("Capture needs a paper context.".to_string());
    return;
  };
  let Some(notes_app) = app.notes_app.as_mut() else {
    return;
  };
  notes_app.focus_article(
    &context.paper.id,
    &context.paper.title,
    &context.paper.url,
  );
  notes_app.apply_initial_focus();
}

fn select_notes_browser_index(
  app: &mut App,
  side: FocusedReader,
  index: usize,
) {
  let visible_ids = visible_note_ids(app, side);
  let Some(note_id) = visible_ids.get(index).cloned() else {
    return;
  };
  if let Some(notes_app) = app.notes_app.as_mut() {
    notes_app.set_current_note(Some(note_id));
    notes_app.notes_state = notes::app::NotesState::List;
  }
  sync_notes_tab_selection_to_current_note(app, side);
}

fn move_notes_browser_selection(
  app: &mut App,
  side: FocusedReader,
  delta: isize,
  page_size: usize,
  absolute: Option<usize>,
) {
  let visible_ids = visible_note_ids(app, side);
  if visible_ids.is_empty() {
    if let Some(notes_app) = app.notes_app.as_mut() {
      notes_app.set_current_note(None);
      notes_app.notes_state = notes::app::NotesState::List;
    }
    return;
  }

  let current_idx = app
    .notes_app
    .as_ref()
    .and_then(|notes_app| {
      notes_app.current_note_id.as_ref().and_then(|current_id| {
        visible_ids.iter().position(|note_id| note_id == current_id)
      })
    })
    .unwrap_or(0);

  let target = if let Some(absolute) = absolute {
    absolute.min(visible_ids.len() - 1)
  } else if delta == 0 {
    current_idx
  } else if delta > 1 || delta < -1 {
    (current_idx as isize + delta * page_size as isize)
      .clamp(0, (visible_ids.len() - 1) as isize) as usize
  } else {
    (current_idx as isize + delta).clamp(0, (visible_ids.len() - 1) as isize)
      as usize
  };

  select_notes_browser_index(app, side, target);
}

fn mutate_note_links_for_context(
  app: &mut App,
  side: FocusedReader,
  detach: bool,
) {
  let Some(context) = app.notes_context_for_side(side).cloned() else {
    app.set_notification("No paper context for this notes pane.".to_string());
    return;
  };
  let Some(notes_app) = app.notes_app.as_mut() else {
    return;
  };
  let Some(note) = notes_app.get_current_note().cloned() else {
    app.set_notification("No active note selected.".to_string());
    return;
  };

  let already_linked =
    note.linked_papers.iter().any(|paper| paper.id == context.paper.id);
  if detach && !already_linked {
    app.set_notification(
      "Current paper is not linked to this note.".to_string(),
    );
    return;
  }
  if !detach && already_linked {
    app.set_notification(
      "Current paper is already linked to this note.".to_string(),
    );
    return;
  }

  let mut linked_papers = note.linked_papers.clone();
  if detach {
    linked_papers.retain(|paper| paper.id != context.paper.id);
  } else {
    linked_papers.push(context.paper.clone());
  }

  if let Err(err) = notes_app.update_current_note_attributes(
    note.title.clone(),
    linked_papers,
    note.tags.clone(),
  ) {
    app.set_notification(format!("Failed to update note links: {err}"));
    return;
  }

  if !detach {
    sync_notes_tabs_for_paper_mode(app, side, &context);
  }
  ensure_notes_browser_selection(app, side);
  sync_notes_tab_selection_to_current_note(app, side);
  let action = if detach { "Detached" } else { "Attached" };
  app.set_notification(format!("{action} current paper in note."));
}

fn notes_side_active(app: &App, side: FocusedReader) -> bool {
  match side {
    FocusedReader::Primary => app.notes_active,
    FocusedReader::Secondary => app.secondary_notes_active,
  }
}

fn set_notes_side_active(app: &mut App, side: FocusedReader, active: bool) {
  match side {
    FocusedReader::Primary => app.notes_active = active,
    FocusedReader::Secondary => app.secondary_notes_active = active,
  }
}

fn any_notes_active(app: &App) -> bool {
  app.notes_active || app.secondary_notes_active
}

fn note_side_for_focus(app: &App) -> FocusedReader {
  match app.focus.focused_pane {
    PaneId::SecondaryReader | PaneId::SecondaryNotes => {
      FocusedReader::Secondary
    }
    _ if app.reader_dual_active => app.focused_reader,
    _ => FocusedReader::Primary,
  }
}

fn note_pane_for_side(app: &App, side: FocusedReader) -> PaneId {
  match side {
    FocusedReader::Primary => PaneId::Notes,
    FocusedReader::Secondary if app.reader_dual_active => {
      PaneId::SecondaryNotes
    }
    FocusedReader::Secondary => PaneId::Notes,
  }
}

fn focus_fallback_after_notes(app: &App, side: FocusedReader) -> PaneId {
  match side {
    FocusedReader::Secondary if app.reader_dual_active => {
      PaneId::SecondaryReader
    }
    _ if app.reader_active => PaneId::Reader,
    _ => PaneId::Feed,
  }
}

fn focused_note_side(app: &App) -> Option<FocusedReader> {
  match app.focus.focused_pane {
    PaneId::Notes => Some(FocusedReader::Primary),
    PaneId::SecondaryNotes => Some(FocusedReader::Secondary),
    _ => None,
  }
}

fn sync_notes_app_to_side(app: &mut App, side: FocusedReader) {
  app.focused_reader = side;
  let note_id = match side {
    FocusedReader::Primary => {
      app.notes_tabs.get(app.notes_active_tab).map(|tab| tab.note_id.clone())
    }
    FocusedReader::Secondary => app
      .secondary_notes_tabs
      .get(app.secondary_notes_active_tab)
      .map(|tab| tab.note_id.clone()),
  };
  if let (Some(na), Some(note_id)) = (app.notes_app.as_mut(), note_id) {
    if na.current_note_id.as_deref() != Some(note_id.as_str()) {
      na.set_current_note(Some(note_id));
    }
  }
}

fn sync_focus_after_pane_change(app: &mut App) {
  match app.focus.focused_pane {
    PaneId::Reader | PaneId::Notes => {
      app.focused_reader = FocusedReader::Primary
    }
    PaneId::SecondaryReader | PaneId::SecondaryNotes => {
      app.focused_reader = FocusedReader::Secondary;
    }
    _ => {}
  }
  if let Some(side) = focused_note_side(app) {
    sync_notes_app_to_side(app, side);
  }
}

fn focus_reader_bottom_from_reader(app: &mut App) -> bool {
  if !app.reader_dual_active || !app.reader_bottom_open {
    return false;
  }
  match app.focus.focused_pane {
    PaneId::Reader | PaneId::Notes => {
      app.focused_reader = FocusedReader::Primary;
      app.reader_bottom_focused = true;
      true
    }
    PaneId::SecondaryReader | PaneId::SecondaryNotes => {
      app.focused_reader = FocusedReader::Secondary;
      app.reader_bottom_focused = true;
      true
    }
    _ => false,
  }
}

fn ensure_chat(app: &mut App) {
  if app.chat.ui.is_some() {
    return;
  }

  let mut registry = chat::ProviderRegistry::new();
  if let Some(k) = app.config.claude_api_key.as_ref().filter(|k| !k.is_empty())
  {
    registry.register("claude", Box::new(chat::ClaudeProvider::new(k.clone())));
  }
  if let Some(k) = app.config.openai_api_key.as_ref().filter(|k| !k.is_empty())
  {
    registry.register("openai", Box::new(chat::OpenAiProvider::new(k.clone())));
  }
  let default_provider = app.config.default_chat_provider.clone();
  let slash_commands =
    crate::commands::registry::chat_slash_specs().to_vec();
  app.chat.ui =
    Some(chat::ChatUi::new(registry, default_provider, slash_commands));
}

fn paper_ref_from_item(item: &crate::models::FeedItem) -> notes::PaperRef {
  notes::PaperRef {
    id: item.id.clone(),
    title: item.title.clone(),
    url: item.url.clone(),
  }
}

fn source_label_for_item(item: &crate::models::FeedItem) -> String {
  if item.source_name.is_empty() {
    item.source_platform.short_label().to_string()
  } else {
    item.source_name.clone()
  }
}

fn notes_context_from_item(
  item: &crate::models::FeedItem,
) -> crate::app::NotesContext {
  crate::app::NotesContext {
    paper: paper_ref_from_item(item),
    source_label: source_label_for_item(item),
  }
}

pub(super) fn remember_fulltext_paper_context(
  app: &mut App,
  item: &crate::models::FeedItem,
) {
  app.last_read = Some(item.title.clone());
  app.last_read_source = Some(source_label_for_item(item));
  app.pending_fulltext_context = Some(notes_context_from_item(item));
  app.record_paper_open(item);
}

fn find_item_by_url<'a>(
  app: &'a App,
  url: &str,
) -> Option<&'a crate::models::FeedItem> {
  app.url_index.get(url).and_then(|&idx| app.items.get(idx)).or_else(|| {
    app
      .discovery.url_index
      .get(url)
      .and_then(|&idx| app.discovery.items.get(idx))
  })
}

fn find_item_by_arxiv_id<'a>(
  app: &'a App,
  arxiv_id: &str,
) -> Option<&'a crate::models::FeedItem> {
  app.arxiv_id_index.get(arxiv_id).and_then(|&idx| app.items.get(idx)).or_else(
    || {
      app
        .discovery.arxiv_id_index
        .get(arxiv_id)
        .and_then(|&idx| app.discovery.items.get(idx))
    },
  )
}

fn notes_context_from_history_entry(
  app: &App,
  entry: &crate::history::HistoryEntry,
) -> Option<crate::app::NotesContext> {
  if entry.kind != crate::history::HistoryKind::Paper {
    return None;
  }
  if let Some(item) = find_item_by_url(app, &entry.key) {
    return Some(notes_context_from_item(item));
  }
  if let Some(arxiv_id) = crate::models::arxiv_id_from_url(&entry.key) {
    if let Some(item) = find_item_by_arxiv_id(app, arxiv_id) {
      return Some(notes_context_from_item(item));
    }
    return Some(crate::app::NotesContext {
      paper: notes::PaperRef {
        id: arxiv_id.to_string(),
        title: entry.title.clone(),
        url: entry.key.clone(),
      },
      source_label: entry.source.clone(),
    });
  }
  Some(crate::app::NotesContext {
    paper: notes::PaperRef {
      id: entry.key.clone(),
      title: entry.title.clone(),
      url: entry.key.clone(),
    },
    source_label: entry.source.clone(),
  })
}

fn history_selected_paper_ref(app: &App) -> Option<crate::app::NotesContext> {
  let visible = app.filtered_history();
  let entry = visible.get(app.history_selected_index)?;
  notes_context_from_history_entry(app, entry)
}

fn resolve_notes_paper_context(
  app: &App,
  side: FocusedReader,
) -> Option<crate::app::NotesContext> {
  match app.focus.focused_pane {
    PaneId::Reader => app.reader_notes_context(FocusedReader::Primary),
    PaneId::SecondaryReader => {
      app.reader_notes_context(FocusedReader::Secondary)
    }
    PaneId::Notes | PaneId::SecondaryNotes => app
      .notes_context_for_side(side)
      .cloned()
      .or_else(|| app.reader_notes_context(side)),
    PaneId::Feed | PaneId::Details => {
      if app.feed_tab == FeedTab::History {
        history_selected_paper_ref(app)
      } else {
        app.selected_item().map(notes_context_from_item)
      }
    }
    _ => app.selected_item().map(notes_context_from_item),
  }
}

fn handle_leader(key: KeyEvent, app: &mut App) {
  log::debug!("leader dispatch: {:?}", key.code);
  let is_nav = matches!(
    key.code,
    KeyCode::Char('h')
      | KeyCode::Char('j')
      | KeyCode::Char('k')
      | KeyCode::Char('l')
  );
  if !is_nav {
    app.leader_active = false;
  }

  match key.code {
    KeyCode::Char('n') => {
      let side = note_side_for_focus(app);
      if notes_side_active(app, side) {
        set_notes_side_active(app, side, false);
        app.focus.focused_pane = focus_fallback_after_notes(app, side);
      } else {
        open_notes(app);
      }
    }
    KeyCode::Char('c') => {
      if app.chat.active {
        app.chat.active = false;
        app.chat.fullscreen = false;
        app.focus.focused_pane =
          if app.reader_active { PaneId::Reader } else { PaneId::Feed };
      } else {
        ensure_chat(app);
        app.notes_active = false;
        app.secondary_notes_active = false;
        app.chat.active = true;
        app.focus.focused_pane = PaneId::Chat;
      }
    }
    KeyCode::Char('s') => {
      app.settings.github_token =
        app.config.github_token.clone().unwrap_or_default();
      app.settings.s2_key =
        app.config.semantic_scholar_key.clone().unwrap_or_default();
      app.settings.claude_key =
        app.config.claude_api_key.clone().unwrap_or_default();
      app.settings.openai_key =
        app.config.openai_api_key.clone().unwrap_or_default();
      app.settings.default_chat_provider =
        app.config.default_chat_provider.clone();
      app.settings.field = 0;
      app.settings.editing = false;
      app.sources_popup.cursor = 0;
      app.sources_popup.input.clear();
      app.sources_popup.input.blur();
      app.sources_popup.detect.reset();
      app.modals.remove(&crate::surfaces::overlays::ActiveModal::Sources);
      app.view = AppView::Settings;
    }
    KeyCode::Char('z') => {
      if app.chat.active {
        app.chat.at_top = !app.chat.at_top;
      }
    }
    // A1 — floating reader popup
    KeyCode::Enter => {
      if !app.fulltext_loading && !app.reader_popup_active {
        if let Some(item) = app.selected_item().cloned() {
          let (tx, rx) = mpsc::channel();
          app.reader_popup_rx = Some(rx);
          app.fulltext_loading = true;
          remember_fulltext_paper_context(app, &item);
          app.set_notification(format!(
            "Fetching: {}…",
            truncate_for_notif(&item.title, 40)
          ));
          spawn_fulltext_fetch(item, tx);
        }
      }
    }
    // A2 — three-state reader/feed cycle.
    KeyCode::Char('f') => {
      if app.reader_dual_active {
        // State 3: toggle bottom feed pane.
        if app.reader_bottom_open {
          app.reader_bottom_open = false;
          app.reader_bottom_focused = false;
          app.reader_bottom_details = false;
          if app.focus.focused_pane == PaneId::Feed {
            app.focus.focused_pane = PaneId::Reader;
          }
        } else {
          app.reader_bottom_open = true;
          app.reader_bottom_focused = true;
          app.reader_bottom_details = false;
        }
      } else if app.reader_split_active {
        // State 2 → State 3: auto-fetch selected item into right pane
        app.reader_dual_active = true;
        app.reader_bottom_focused = false;
        app.reader_bottom_details = false;
        app.reader_bottom_scroll = 0;
        app.fulltext_for_secondary = true;
        if !app.fulltext_loading {
          if let Some(item) = app.selected_item().cloned() {
            let (tx, rx) = mpsc::channel();
            app.fulltext_rx = Some(rx);
            app.fulltext_loading = true;
            remember_fulltext_paper_context(app, &item);
            app.set_notification(format!(
              "Loading: {}…",
              truncate_for_notif(&item.title, 40)
            ));
            spawn_fulltext_fetch(item, tx);
          }
        }
        app.focus.focused_pane = PaneId::Reader;
      } else if app.reader_active {
        // State 1 → State 2: show feed alongside reader
        app.reader_split_active = true;
        app.focus.focused_pane = PaneId::Feed;
      }
    }
    KeyCode::Char('?') => {
      app.help.active = true;
      app.help.section = 0;
      app.help.scroll.reset();
    }
    KeyCode::Char('q') => {
      app.show_quit_popup();
    }
    KeyCode::Char('h') => {
      if app.reader_bottom_focused {
        return;
      }
      let t = std::time::Instant::now();
      let result = app.focus.find_pane_in_direction(NavDirection::Left);
      log::debug!(
        "find_pane Left={:?} took {}µs",
        result,
        t.elapsed().as_micros()
      );
      if let Some(pane) = result {
        app.focus.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Char('j') => {
      if focus_reader_bottom_from_reader(app) {
        return;
      }
      let t = std::time::Instant::now();
      let result = app.focus.find_pane_in_direction(NavDirection::Down);
      log::debug!(
        "find_pane Down={:?} took {}µs",
        result,
        t.elapsed().as_micros()
      );
      if let Some(pane) = result {
        app.focus.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Char('k') => {
      if app.reader_bottom_focused {
        app.reader_bottom_focused = false;
        app.focus.focused_pane = match app.focused_reader {
          FocusedReader::Primary => PaneId::Reader,
          FocusedReader::Secondary => PaneId::SecondaryReader,
        };
        return;
      }
      let t = std::time::Instant::now();
      let result = app.focus.find_pane_in_direction(NavDirection::Up);
      log::debug!(
        "find_pane Up={:?} took {}µs",
        result,
        t.elapsed().as_micros()
      );
      if let Some(pane) = result {
        app.focus.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Char('l') => {
      if app.reader_bottom_focused {
        return;
      }
      let t = std::time::Instant::now();
      let result = app.focus.find_pane_in_direction(NavDirection::Right);
      log::debug!(
        "find_pane Right={:?} took {}µs",
        result,
        t.elapsed().as_micros()
      );
      if let Some(pane) = result {
        app.focus.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Esc => match app.focus.focused_pane {
      PaneId::Chat => {
        app.chat.active = false;
        app.chat.fullscreen = false;
        app.focus.focused_pane =
          if app.reader_active { PaneId::Reader } else { PaneId::Feed };
      }
      PaneId::Notes | PaneId::SecondaryNotes => {
        let side = focused_note_side(app).unwrap_or(FocusedReader::Primary);
        if let Some(na) = app.notes_app.as_mut() {
          let _ = na.persist_state();
        }
        set_notes_side_active(app, side, false);
        app.focus.focused_pane = focus_fallback_after_notes(app, side);
      }
      PaneId::SecondaryReader | PaneId::Reader => {
        let side = if app.focus.focused_pane == PaneId::SecondaryReader {
          FocusedReader::Secondary
        } else {
          FocusedReader::Primary
        };
        if !reader_back(app, side) {
          close_all_readers(app);
        }
      }
      PaneId::Feed | PaneId::Details => {}
    },
    KeyCode::Char('0') => {
      if let Some(pane) = get_pane_by_number(0, app) {
        app.focus.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Char('1') => {
      if let Some(pane) = get_pane_by_number(1, app) {
        app.focus.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Char('2') => {
      if let Some(pane) = get_pane_by_number(2, app) {
        app.focus.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Char('3') => {
      if let Some(pane) = get_pane_by_number(3, app) {
        app.focus.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    // Ldr+t — open selected item as a new tab
    KeyCode::Char('t') => {
      if app.fulltext_loading {
        return;
      }
      if app.reader_dual_active {
        app.tab_window_prompt_active = true;
        app.set_notification(
          "Add to: [1] left  [2] right  Esc: cancel".to_string(),
        );
      } else {
        app.fulltext_new_tab = !app.reader_tabs.is_empty();
        app.fulltext_for_secondary = false;
        trigger_fulltext_new_tab(app);
      }
    }
    // Ldr+[ / Ldr+] — cycle tabs in focused pane
    KeyCode::Char('[') => {
      if let Some(side) =
        focused_note_side(app).filter(|side| notes_side_active(app, *side))
      {
        notes_prev_tab(app, side);
      } else if app.reader_active {
        app.reader_prev_tab();
      }
    }
    KeyCode::Char(']') => {
      if let Some(side) =
        focused_note_side(app).filter(|side| notes_side_active(app, *side))
      {
        notes_next_tab(app, side);
      } else if app.reader_active {
        app.reader_next_tab();
      }
    }
    // Ldr+w — close current tab (collapse pane when last tab)
    KeyCode::Char('w') => match app.focus.focused_pane {
      PaneId::Notes | PaneId::SecondaryNotes if any_notes_active(app) => {
        let side = focused_note_side(app).unwrap_or(FocusedReader::Primary);
        notes_close_active_tab(app, side);
      }
      PaneId::SecondaryReader => {
        let pane_empty = app.reader_secondary_close_active_tab();
        if pane_empty {
          app.reader_dual_active = false;
          app.reader_bottom_open = false;
          app.reader_bottom_focused = false;
          app.secondary_notes_active = false;
          app.focused_reader = FocusedReader::Primary;
          app.focus.focused_pane = PaneId::Reader;
        }
      }
      PaneId::Reader if app.reader_active => {
        let pane_empty = app.reader_close_active_tab();
        if pane_empty {
          if app.reader_dual_active {
            app.reader_dual_active = false;
            app.reader_bottom_open = false;
            app.reader_bottom_focused = false;
            app.reader_secondary_tabs.clear();
            app.reader_secondary_active_tab = 0;
            app.secondary_notes_active = false;
          } else if app.reader_split_active {
            app.reader_split_active = false;
          }
          app.focus.focused_pane = PaneId::Feed;
        }
      }
      _ => {}
    },
    _ => {}
  }
}

fn notes_prev_tab(app: &mut App, side: FocusedReader) {
  let active = match side {
    FocusedReader::Primary => app.notes_active_tab,
    FocusedReader::Secondary => app.secondary_notes_active_tab,
  };
  if notes_tabs_len(app, side) == 0 {
    return;
  }
  notes_switch_tab(app, side, active.saturating_sub(1));
}

fn notes_next_tab(app: &mut App, side: FocusedReader) {
  let len = notes_tabs_len(app, side);
  if len == 0 {
    return;
  }
  let active = match side {
    FocusedReader::Primary => app.notes_active_tab,
    FocusedReader::Secondary => app.secondary_notes_active_tab,
  };
  notes_switch_tab(app, side, (active + 1).min(len - 1));
}

fn notes_tabs_len(app: &App, side: FocusedReader) -> usize {
  match side {
    FocusedReader::Primary => app.notes_tabs.len(),
    FocusedReader::Secondary => app.secondary_notes_tabs.len(),
  }
}

fn notes_switch_tab(app: &mut App, side: FocusedReader, idx: usize) {
  let note_id = match side {
    FocusedReader::Primary => {
      app.notes_active_tab = idx;
      app.notes_tabs.get(idx).map(|t| t.note_id.clone())
    }
    FocusedReader::Secondary => {
      app.secondary_notes_active_tab = idx;
      app.secondary_notes_tabs.get(idx).map(|t| t.note_id.clone())
    }
  };
  if let Some(note_id) = note_id {
    if let Some(na) = app.notes_app.as_mut() {
      na.focus_note(&note_id);
      na.notes_state = notes::app::NotesState::List;
    }
  }
}

fn notes_close_active_tab(app: &mut App, side: FocusedReader) {
  let became_empty = match side {
    FocusedReader::Primary => {
      if app.notes_tabs.is_empty() {
        return;
      }
      app.notes_active_tab = app.notes_active_tab.min(app.notes_tabs.len() - 1);
      app.notes_tabs.remove(app.notes_active_tab);
      app.notes_tabs.is_empty()
    }
    FocusedReader::Secondary => {
      if app.secondary_notes_tabs.is_empty() {
        return;
      }
      app.secondary_notes_active_tab =
        app.secondary_notes_active_tab.min(app.secondary_notes_tabs.len() - 1);
      app.secondary_notes_tabs.remove(app.secondary_notes_active_tab);
      app.secondary_notes_tabs.is_empty()
    }
  };

  if became_empty {
    set_notes_side_active(app, side, false);
    app.focus.focused_pane = focus_fallback_after_notes(app, side);
    return;
  }

  let idx = match side {
    FocusedReader::Primary => {
      app.notes_active_tab = app.notes_active_tab.min(app.notes_tabs.len() - 1);
      app.notes_active_tab
    }
    FocusedReader::Secondary => {
      app.secondary_notes_active_tab =
        app.secondary_notes_active_tab.min(app.secondary_notes_tabs.len() - 1);
      app.secondary_notes_active_tab
    }
  };
  notes_switch_tab(app, side, idx);
}

/// Spawns a fulltext fetch for the selected item, using flags already set on app.
fn trigger_fulltext_new_tab(app: &mut App) {
  if app.fulltext_loading {
    app.set_notification("Already fetching…".to_string());
    return;
  }
  if let Some(item) = app.selected_item().cloned() {
    let (tx, rx) = mpsc::channel();
    app.fulltext_rx = Some(rx);
    app.fulltext_loading = true;
    remember_fulltext_paper_context(app, &item);
    app.set_notification(format!(
      "Fetching: {}…",
      truncate_for_notif(&item.title, 40)
    ));
    spawn_fulltext_fetch(item, tx);
  }
}

// ── Pane routers ─────────────────────────────────────────────────────────────

fn handle_chat_pane(key: KeyEvent, app: &mut App) -> bool {
  if !(app.chat.active && app.focus.focused_pane == PaneId::Chat) {
    return false;
  }
  log::debug!("routing to chat pane");
  if let Some(chat_ui) = app.chat.ui.as_mut() {
    let action = chat_ui.handle_key(key);
    match action {
      chat::ChatAction::Quit => {
        app.chat.active = false;
        app.chat.fullscreen = false;
        app.focus.focused_pane =
          if app.reader_active { PaneId::Reader } else { PaneId::Feed };
      }
      chat::ChatAction::SlashCommand(cmd) => {
        app.handle_slash_command(cmd);
      }
      chat::ChatAction::None | chat::ChatAction::Sending => {}
    }
  }
  true
}

fn handle_notes_pane(key: KeyEvent, app: &mut App) -> bool {
  let Some(side) =
    focused_note_side(app).filter(|side| notes_side_active(app, *side))
  else {
    return false;
  };
  sync_notes_app_to_side(app, side);
  log::debug!("routing to notes pane");
  let allows_shell_shortcuts =
    app.notes_app.as_ref().is_some_and(notes_shell_shortcuts_allowed);
  if allows_shell_shortcuts {
    if let Some(notes_app) = app.notes_app.as_mut() {
      notes_app.notes_state = notes::app::NotesState::List;
    }
    match key.code {
      KeyCode::Char('[') => {
        cycle_notes_mode(app, side, -1);
        return true;
      }
      KeyCode::Char(']') => {
        cycle_notes_mode(app, side, 1);
        return true;
      }
      KeyCode::Char('a')
        if app.notes_mode_for_side(side) != NotesMode::Capture =>
      {
        mutate_note_links_for_context(app, side, false);
        return true;
      }
      KeyCode::Char('x')
        if app.notes_mode_for_side(side) != NotesMode::Capture =>
      {
        mutate_note_links_for_context(app, side, true);
        return true;
      }
      KeyCode::Char('n')
        if app.notes_mode_for_side(side) == NotesMode::Capture =>
      {
        begin_capture_note(app, side);
        return true;
      }
      KeyCode::Enter if app.notes_mode_for_side(side) == NotesMode::Capture => {
        begin_capture_note(app, side);
        return true;
      }
      KeyCode::Char('j') | KeyCode::Down
        if app.notes_mode_for_side(side) != NotesMode::Capture =>
      {
        move_notes_browser_selection(app, side, 1, 1, None);
        return true;
      }
      KeyCode::Char('k') | KeyCode::Up
        if app.notes_mode_for_side(side) != NotesMode::Capture =>
      {
        move_notes_browser_selection(app, side, -1, 1, None);
        return true;
      }
      KeyCode::Char('g')
        if app.notes_mode_for_side(side) != NotesMode::Capture =>
      {
        move_notes_browser_selection(app, side, 0, 1, Some(0));
        return true;
      }
      KeyCode::Char('G')
        if app.notes_mode_for_side(side) != NotesMode::Capture =>
      {
        move_notes_browser_selection(app, side, 0, 1, Some(usize::MAX));
        return true;
      }
      KeyCode::PageDown
        if app.notes_mode_for_side(side) != NotesMode::Capture =>
      {
        move_notes_browser_selection(app, side, 1, 8, None);
        return true;
      }
      KeyCode::PageUp
        if app.notes_mode_for_side(side) != NotesMode::Capture =>
      {
        move_notes_browser_selection(app, side, -1, 8, None);
        return true;
      }
      _ => {}
    }
  }
  if let Some(notes_app) = app.notes_app.as_mut() {
    if notes::handle_key(key, notes_app) {
      if let Err(e) = notes_app.persist_state() {
        log::error!("notes: failed to persist state: {e}");
      }
      set_notes_side_active(app, side, false);
      app.focus.focused_pane = focus_fallback_after_notes(app, side);
    }
  }
  // Pick up a freshly created note and add its tab.
  if let Some(note_id) =
    app.notes_app.as_mut().and_then(|na| na.last_created_note_id.take())
  {
    let title = app
      .notes_app
      .as_ref()
      .and_then(|na| na.get_note_title(&note_id))
      .unwrap_or_default();
    match side {
      FocusedReader::Primary => {
        if !app.notes_tabs.iter().any(|t| t.note_id == note_id) {
          app.notes_tabs.push(NotesTab { note_id: note_id.clone(), title });
        }
        if let Some(idx) =
          app.notes_tabs.iter().position(|t| t.note_id == note_id)
        {
          app.notes_active_tab = idx;
        }
        if app.notes_mode_for_side(side) == NotesMode::Capture {
          app.set_notes_mode_for_side(side, NotesMode::PaperNotes);
        }
      }
      FocusedReader::Secondary => {
        if !app.secondary_notes_tabs.iter().any(|t| t.note_id == note_id) {
          app
            .secondary_notes_tabs
            .push(NotesTab { note_id: note_id.clone(), title });
        }
        if let Some(idx) =
          app.secondary_notes_tabs.iter().position(|t| t.note_id == note_id)
        {
          app.secondary_notes_active_tab = idx;
        }
        if app.notes_mode_for_side(side) == NotesMode::Capture {
          app.set_notes_mode_for_side(side, NotesMode::PaperNotes);
        }
      }
    }
    ensure_notes_browser_selection(app, side);
    sync_notes_tab_selection_to_current_note(app, side);
  }
  true
}


// ── View handlers ─────────────────────────────────────────────────────────────


