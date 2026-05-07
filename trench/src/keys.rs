use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc;

use crate::app::{
  App, AppView, CustomThemeEditorMode, CustomThemeEditorState, DiscoverResult,
  FeedTab, FocusedReader, NavDirection, NotesMode, NotesTab, PaneId,
  QuitPopupKind, RepoContext, RepoPane, SourcesDetectState,
};
use crate::config::{self, CUSTOM_THEME_ROLES, CustomThemeConfig};
use crate::models::WorkflowState;
use ui_theme::ThemeId;

use super::{
  do_refresh, force_refresh, get_pane_by_number, kbd_scroll_ok, open_url,
  spawn_ai_discovery, spawn_discovery, spawn_fulltext_fetch, spawn_repo_dir,
  spawn_repo_file, spawn_repo_open, truncate_for_notif,
};

const NOTES_MODE_ORDER: [NotesMode; 3] =
  [NotesMode::PaperNotes, NotesMode::Library, NotesMode::Capture];

/// Top-level key dispatcher — called once per key press event from the main loop.
pub fn dispatch(key: KeyEvent, app: &mut App) {
  // Tag picker popup — intercepts all keys when open.
  if app.tag_picker_active {
    handle_tag_picker(key, app);
    return;
  }

  // Quit popup — intercepts all keys until dismissed.
  if app.quit_popup_active {
    match key.code {
      KeyCode::Char('q') | KeyCode::Enter => {
        app.quit_popup_active = false;
        match app.quit_popup_kind {
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
              app.focused_pane = PaneId::Feed;
            }
          }
          _ => app.should_quit = true,
        }
      }
      KeyCode::Esc => {
        app.quit_popup_active = false;
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
    } else if app.focused_pane == PaneId::SecondaryReader {
      app.reader_secondary_editor_mut().map(|r| r.handle_event(Event::Key(key)))
    } else {
      app.reader_editor_mut().map(|r| r.handle_event(Event::Key(key)))
    };
    drop(reader_action);
    return;
  }
  if key.code == KeyCode::Char('?') && !is_text_entry_context(app) {
    app.leader_active = false;
    app.help_active = true;
    app.help_section = 0;
    app.help_scroll = 0;
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

/// True when one of the embedded reader panes is currently focused.
/// Used to decide whether `?` should show tread's reader-help overlay
/// (focused) or trench's app-level help (otherwise).  Includes the
/// floating popup reader since it's the only thing on screen when active.
fn reader_pane_focused(app: &App) -> bool {
  if app.reader_popup_active {
    return true;
  }
  matches!(app.focused_pane, PaneId::Reader | PaneId::SecondaryReader)
    && app.reader_active
}

fn is_text_entry_context(app: &App) -> bool {
  if app.search_active || app.sources_input_active || app.settings_editing {
    return true;
  }
  if app.feed_tab == FeedTab::Discoveries && app.discovery_search_focused {
    return true;
  }
  if app.chat_active && app.focused_pane == PaneId::Chat {
    return true;
  }
  if app.notes_active && app.focused_pane == PaneId::Notes {
    return true;
  }
  if app.secondary_notes_active && app.focused_pane == PaneId::SecondaryNotes {
    return true;
  }
  app.custom_theme_editor.as_ref().is_some_and(|editor| {
    matches!(
      editor.mode,
      CustomThemeEditorMode::Name | CustomThemeEditorMode::Hex
    )
  })
}

fn handle_reader_bottom_pane(key: KeyEvent, app: &mut App) {
  if app.search_active {
    match key.code {
      KeyCode::Esc => {
        app.search_active = false;
        app.search_query.clear();
        app.reader_feed_popup_selected = 0;
        app.invalidate_visible_cache();
      }
      KeyCode::Enter => {
        app.search_active = false;
      }
      KeyCode::Backspace => {
        app.pop_search_char();
        clamp_reader_feed_selection(app);
      }
      KeyCode::Char(c) => {
        app.push_search_char(c);
        clamp_reader_feed_selection(app);
      }
      _ => {}
    }
    return;
  }

  match key.code {
    KeyCode::Char('j') | KeyCode::Down => {
      if app.reader_bottom_details {
        app.reader_bottom_scroll = app.reader_bottom_scroll.saturating_add(1);
      } else {
        let count = app.visible_count();
        if count > 0 {
          app.reader_feed_popup_selected =
            (app.reader_feed_popup_selected + 1).min(count - 1);
        }
      }
    }
    KeyCode::Char('k') | KeyCode::Up => {
      if app.reader_bottom_details {
        app.reader_bottom_scroll = app.reader_bottom_scroll.saturating_sub(1);
      } else {
        app.reader_feed_popup_selected =
          app.reader_feed_popup_selected.saturating_sub(1);
      }
    }
    KeyCode::Char('d') => {
      app.reader_bottom_details = !app.reader_bottom_details;
      app.reader_bottom_scroll = 0;
    }
    KeyCode::Char('/') => {
      app.search_active = true;
      app.search_query.clear();
      app.reader_feed_popup_selected = 0;
      app.invalidate_visible_cache();
    }
    KeyCode::Tab => {
      app.feed_tab = match app.feed_tab {
        FeedTab::Inbox => FeedTab::Library,
        FeedTab::Library => FeedTab::Discoveries,
        FeedTab::Discoveries => FeedTab::History,
        FeedTab::History => FeedTab::Inbox,
      };
      app.reset_active_feed_position();
    }
    KeyCode::BackTab => {
      app.feed_tab = match app.feed_tab {
        FeedTab::Inbox => FeedTab::History,
        FeedTab::Library => FeedTab::Inbox,
        FeedTab::Discoveries => FeedTab::Library,
        FeedTab::History => FeedTab::Discoveries,
      };
      app.reset_active_feed_position();
    }
    KeyCode::Enter => {
      if !app.reader_bottom_details && !app.fulltext_loading {
        let idx = app.reader_feed_popup_selected;
        let item = {
          let visible = app.visible_items();
          visible.get(idx).map(|item| (*item).clone())
        };
        if let Some(item) = item {
          let (tx, rx) = mpsc::channel();
          app.fulltext_rx = Some(rx);
          app.fulltext_loading = true;
          app.fulltext_for_secondary =
            app.focused_reader == FocusedReader::Secondary;
          remember_fulltext_paper_context(app, &item);
          app.set_notification(format!(
            "Fetching: {}…",
            truncate_for_notif(&item.title, 40)
          ));
          spawn_fulltext_fetch(item, tx);
          app.reader_bottom_focused = false;
          app.focused_pane = PaneId::Reader;
        }
      }
    }
    KeyCode::Esc => {
      if app.reader_bottom_details {
        app.reader_bottom_details = false;
        app.reader_bottom_scroll = 0;
      } else {
        app.reader_bottom_open = false;
        app.reader_bottom_focused = false;
        app.focused_pane = PaneId::Reader;
      }
    }
    KeyCode::Char('q') => {
      app.reader_bottom_open = false;
      app.reader_bottom_focused = false;
      app.focused_pane = PaneId::Reader;
    }
    _ => {}
  }
}

fn clamp_reader_feed_selection(app: &mut App) {
  let count = app.visible_count();
  if count == 0 {
    app.reader_feed_popup_selected = 0;
  } else {
    app.reader_feed_popup_selected =
      app.reader_feed_popup_selected.min(count - 1);
  }
}

// ── Help overlay ─────────────────────────────────────────────────────────────

fn handle_help_overlay(key: KeyEvent, app: &mut App) -> bool {
  if !app.help_active {
    return false;
  }
  match key.code {
    KeyCode::Tab | KeyCode::Char('l') => {
      app.help_section = (app.help_section + 1) % crate::ui::HELP_SECTION_COUNT;
      app.help_scroll = 0;
    }
    KeyCode::BackTab | KeyCode::Char('h') => {
      app.help_section = app.help_section.saturating_sub(1);
      app.help_scroll = 0;
    }
    KeyCode::Char('j') | KeyCode::Down => {
      app.help_scroll = app.help_scroll.saturating_add(1);
    }
    KeyCode::Char('k') | KeyCode::Up => {
      app.help_scroll = app.help_scroll.saturating_sub(1);
    }
    KeyCode::Char('q') | KeyCode::Esc => {
      app.help_active = false;
    }
    _ => {}
  }
  true
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
  app.focused_pane = note_pane_for_side(app, side);
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
  match app.focused_pane {
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
  match app.focused_pane {
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
  match app.focused_pane {
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
  match app.focused_pane {
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
  if app.chat_ui.is_some() {
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
  let slash_commands = crate::commands::registry::chat_slash_specs();
  app.chat_ui =
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

fn remember_fulltext_paper_context(
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
      .discovery_url_index
      .get(url)
      .and_then(|&idx| app.discovery_items.get(idx))
  })
}

fn find_item_by_arxiv_id<'a>(
  app: &'a App,
  arxiv_id: &str,
) -> Option<&'a crate::models::FeedItem> {
  app.arxiv_id_index.get(arxiv_id).and_then(|&idx| app.items.get(idx)).or_else(
    || {
      app
        .discovery_arxiv_id_index
        .get(arxiv_id)
        .and_then(|&idx| app.discovery_items.get(idx))
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
  match app.focused_pane {
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
        app.focused_pane = focus_fallback_after_notes(app, side);
      } else {
        open_notes(app);
      }
    }
    KeyCode::Char('c') => {
      if app.chat_active {
        app.chat_active = false;
        app.chat_fullscreen = false;
        app.focused_pane =
          if app.reader_active { PaneId::Reader } else { PaneId::Feed };
      } else {
        ensure_chat(app);
        app.notes_active = false;
        app.secondary_notes_active = false;
        app.chat_active = true;
        app.focused_pane = PaneId::Chat;
      }
    }
    KeyCode::Char('s') => {
      app.settings_github_token =
        app.config.github_token.clone().unwrap_or_default();
      app.settings_s2_key =
        app.config.semantic_scholar_key.clone().unwrap_or_default();
      app.settings_claude_key =
        app.config.claude_api_key.clone().unwrap_or_default();
      app.settings_openai_key =
        app.config.openai_api_key.clone().unwrap_or_default();
      app.settings_default_chat_provider =
        app.config.default_chat_provider.clone();
      app.settings_field = 0;
      app.settings_editing = false;
      app.sources_cursor = 0;
      app.sources_input.clear();
      app.sources_input_active = false;
      app.sources_detect_state = SourcesDetectState::Idle;
      app.view = AppView::Settings;
    }
    KeyCode::Char('z') => {
      if app.chat_active {
        app.chat_at_top = !app.chat_at_top;
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
          if app.focused_pane == PaneId::Feed {
            app.focused_pane = PaneId::Reader;
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
        app.focused_pane = PaneId::Reader;
      } else if app.reader_active {
        // State 1 → State 2: show feed alongside reader
        app.reader_split_active = true;
        app.focused_pane = PaneId::Feed;
      }
    }
    KeyCode::Char('?') => {
      app.help_active = true;
      app.help_section = 0;
      app.help_scroll = 0;
    }
    KeyCode::Char('q') => {
      app.show_quit_popup();
    }
    KeyCode::Char('h') => {
      if app.reader_bottom_focused {
        return;
      }
      let t = std::time::Instant::now();
      let result = app.find_pane_in_direction(NavDirection::Left);
      log::debug!(
        "find_pane Left={:?} took {}µs",
        result,
        t.elapsed().as_micros()
      );
      if let Some(pane) = result {
        app.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Char('j') => {
      if focus_reader_bottom_from_reader(app) {
        return;
      }
      let t = std::time::Instant::now();
      let result = app.find_pane_in_direction(NavDirection::Down);
      log::debug!(
        "find_pane Down={:?} took {}µs",
        result,
        t.elapsed().as_micros()
      );
      if let Some(pane) = result {
        app.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Char('k') => {
      if app.reader_bottom_focused {
        app.reader_bottom_focused = false;
        app.focused_pane = match app.focused_reader {
          FocusedReader::Primary => PaneId::Reader,
          FocusedReader::Secondary => PaneId::SecondaryReader,
        };
        return;
      }
      let t = std::time::Instant::now();
      let result = app.find_pane_in_direction(NavDirection::Up);
      log::debug!(
        "find_pane Up={:?} took {}µs",
        result,
        t.elapsed().as_micros()
      );
      if let Some(pane) = result {
        app.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Char('l') => {
      if app.reader_bottom_focused {
        return;
      }
      let t = std::time::Instant::now();
      let result = app.find_pane_in_direction(NavDirection::Right);
      log::debug!(
        "find_pane Right={:?} took {}µs",
        result,
        t.elapsed().as_micros()
      );
      if let Some(pane) = result {
        app.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Esc => match app.focused_pane {
      PaneId::Chat => {
        app.chat_active = false;
        app.chat_fullscreen = false;
        app.focused_pane =
          if app.reader_active { PaneId::Reader } else { PaneId::Feed };
      }
      PaneId::Notes | PaneId::SecondaryNotes => {
        let side = focused_note_side(app).unwrap_or(FocusedReader::Primary);
        if let Some(na) = app.notes_app.as_mut() {
          let _ = na.persist_state();
        }
        set_notes_side_active(app, side, false);
        app.focused_pane = focus_fallback_after_notes(app, side);
      }
      PaneId::SecondaryReader | PaneId::Reader => {
        let side = if app.focused_pane == PaneId::SecondaryReader {
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
        app.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Char('1') => {
      if let Some(pane) = get_pane_by_number(1, app) {
        app.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Char('2') => {
      if let Some(pane) = get_pane_by_number(2, app) {
        app.focused_pane = pane;
        sync_focus_after_pane_change(app);
      }
    }
    KeyCode::Char('3') => {
      if let Some(pane) = get_pane_by_number(3, app) {
        app.focused_pane = pane;
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
    KeyCode::Char('w') => match app.focused_pane {
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
          app.focused_pane = PaneId::Reader;
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
          app.focused_pane = PaneId::Feed;
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
    app.focused_pane = focus_fallback_after_notes(app, side);
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
  if !(app.chat_active && app.focused_pane == PaneId::Chat) {
    return false;
  }
  log::debug!("routing to chat pane");
  if let Some(chat_ui) = app.chat_ui.as_mut() {
    let action = chat_ui.handle_key(key);
    match action {
      chat::ChatAction::Quit => {
        app.chat_active = false;
        app.chat_fullscreen = false;
        app.focused_pane =
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
      app.focused_pane = focus_fallback_after_notes(app, side);
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

fn handle_reader_pane(key: KeyEvent, app: &mut App) -> bool {
  // Secondary reader (State 3, right pane).
  if app.reader_dual_active && app.focused_pane == PaneId::SecondaryReader {
    log::debug!("routing to secondary reader pane");
    if key.code == KeyCode::Tab {
      if app.reader_bottom_open {
        app.reader_bottom_focused = true;
      }
      return true;
    }
    // Esc in Normal mode: step back one reader layer.
    if key.code == KeyCode::Esc {
      let in_normal = app
        .reader_secondary_editor_mut()
        .map(|e| e.is_normal_mode())
        .unwrap_or(true);
      if in_normal {
        reader_back(app, FocusedReader::Secondary);
        return true;
      }
    }
    if let Some(reader) = app.reader_secondary_editor_mut() {
      let action = reader.handle_event(Event::Key(key));
      // q: close the current secondary tab; collapse to primary when empty.
      if matches!(action, tread::ReaderAction::Quit) {
        let pane_empty = app.reader_secondary_close_active_tab();
        if pane_empty {
          app.reader_dual_active = false;
          app.reader_bottom_open = false;
          app.reader_bottom_focused = false;
          app.secondary_notes_active = false;
          app.focused_reader = FocusedReader::Primary;
          app.focused_pane = PaneId::Reader;
        }
      }
    }
    return true;
  }

  if !(app.reader_active && app.focused_pane == PaneId::Reader) {
    return false;
  }
  log::debug!("routing to reader pane");

  // Tab in primary reader during State 3 → focus secondary reader.
  if app.reader_dual_active && key.code == KeyCode::Tab {
    if !app.reader_secondary_tabs.is_empty() {
      app.focused_pane = PaneId::SecondaryReader;
      app.focused_reader = FocusedReader::Secondary;
    }
    return true;
  }

  // Esc in Normal mode: step back one reader layer, exiting only from a lone reader.
  if key.code == KeyCode::Esc {
    let in_normal =
      app.reader_editor_mut().map(|e| e.is_normal_mode()).unwrap_or(true);
    if in_normal {
      if !reader_back(app, FocusedReader::Primary) {
        close_all_readers(app);
      }
      return true;
    }
  }

  if let Some(reader) = app.reader_editor_mut() {
    let action = reader.handle_event(Event::Key(key));
    // q: close the current tab; apply state machine only when the pane goes empty.
    if matches!(action, tread::ReaderAction::Quit) {
      let pane_empty = app.reader_close_active_tab();
      if pane_empty {
        if app.reader_dual_active {
          // Primary ran out of tabs: promote secondary tabs to primary.
          app.reader_tabs = std::mem::take(&mut app.reader_secondary_tabs);
          app.reader_active_tab = app.reader_secondary_active_tab;
          app.reader_secondary_active_tab = 0;
          app.reader_active = !app.reader_tabs.is_empty();
          app.reader_dual_active = false;
          app.reader_bottom_open = false;
          app.reader_bottom_focused = false;
          app.notes_active = app.secondary_notes_active;
          app.notes_tabs = std::mem::take(&mut app.secondary_notes_tabs);
          app.notes_active_tab = app.secondary_notes_active_tab;
          app.secondary_notes_active = false;
          app.secondary_notes_active_tab = 0;
          app.focused_reader = FocusedReader::Primary;
          app.focused_pane =
            if app.reader_active { PaneId::Reader } else { PaneId::Feed };
        } else if app.reader_split_active {
          app.reader_split_active = false;
          app.focused_pane = PaneId::Feed;
        } else {
          app.focused_pane = PaneId::Feed;
        }
      }
    }
  }
  true
}

fn reader_back(app: &mut App, side: FocusedReader) -> bool {
  if app.reader_bottom_open {
    app.reader_bottom_open = false;
    app.reader_bottom_focused = false;
    app.reader_bottom_details = false;
    app.focused_pane = match side {
      FocusedReader::Primary => PaneId::Reader,
      FocusedReader::Secondary if app.reader_dual_active => {
        PaneId::SecondaryReader
      }
      FocusedReader::Secondary => PaneId::Reader,
    };
    app.focused_reader = side;
    return true;
  }

  if app.narrow_feed_details_open {
    app.narrow_feed_details_open = false;
    return true;
  }

  if app.reader_split_active {
    app.reader_split_active = false;
    app.focused_pane = PaneId::Reader;
    app.focused_reader = FocusedReader::Primary;
    return true;
  }

  if app.reader_dual_active {
    match side {
      FocusedReader::Secondary => {
        app.reader_dual_active = false;
        app.reader_secondary_tabs.clear();
        app.reader_secondary_active_tab = 0;
        app.secondary_notes_active = false;
        app.reader_bottom_open = false;
        app.reader_bottom_focused = false;
        app.focused_reader = FocusedReader::Primary;
        app.focused_pane = PaneId::Reader;
      }
      FocusedReader::Primary => {
        app.reader_tabs = std::mem::take(&mut app.reader_secondary_tabs);
        app.reader_active_tab = app.reader_secondary_active_tab;
        app.reader_secondary_active_tab = 0;
        app.reader_active = !app.reader_tabs.is_empty();
        app.reader_dual_active = false;
        app.reader_bottom_open = false;
        app.reader_bottom_focused = false;
        app.notes_active = app.secondary_notes_active;
        app.notes_tabs = std::mem::take(&mut app.secondary_notes_tabs);
        app.notes_active_tab = app.secondary_notes_active_tab;
        app.secondary_notes_active = false;
        app.secondary_notes_active_tab = 0;
        app.focused_reader = FocusedReader::Primary;
        app.focused_pane =
          if app.reader_active { PaneId::Reader } else { PaneId::Feed };
      }
    }
    return true;
  }

  false
}

/// Close all reader state and return focus to the feed.
fn close_all_readers(app: &mut App) {
  // Stop voice + clear pixel placements before tabs drop — voice
  // shouldn't continue once we leave the reader entirely, and
  // ghost image placements would otherwise sit over the feed.
  app.stop_all_reader_voice();
  app.clear_all_reader_image_state();
  app.reader_active = false;
  app.reader_dual_active = false;
  app.reader_split_active = false;
  app.reader_bottom_open = false;
  app.reader_bottom_focused = false;
  app.reader_tabs.clear();
  app.reader_active_tab = 0;
  app.reader_secondary_tabs.clear();
  app.reader_secondary_active_tab = 0;
  app.notes_active = false;
  app.secondary_notes_active = false;
  app.focused_reader = FocusedReader::Primary;
  app.focused_pane = PaneId::Feed;
}

// ── View handlers ─────────────────────────────────────────────────────────────

fn handle_repo_viewer(key: KeyEvent, app: &mut App) -> bool {
  if app.view != AppView::RepoViewer {
    return false;
  }
  log::debug!("routing to repo viewer");
  match key.code {
    KeyCode::Char('q') => app.close_repo_viewer(),
    KeyCode::Esc => {
      if app
        .repo_context
        .as_ref()
        .is_some_and(|ctx| ctx.pane_focus == RepoPane::File)
      {
        app.repo_switch_pane();
      } else {
        app.close_repo_viewer();
      }
    }
    KeyCode::Tab | KeyCode::BackTab => app.repo_switch_pane(),
    KeyCode::Char('j') | KeyCode::Down => app.repo_nav_down(0),
    KeyCode::Char('k') | KeyCode::Up => app.repo_nav_up(),
    KeyCode::Char('h') | KeyCode::Left => app.repo_pan_left(),
    KeyCode::Char('l') | KeyCode::Right => app.repo_pan_right(),
    KeyCode::Char('+') | KeyCode::Char('=') => app.repo_zoom_in(),
    KeyCode::Char('-') => app.repo_zoom_out(),
    KeyCode::Char('y') => app.repo_copy_path(),
    KeyCode::Char('u') => app.repo_copy_url(),
    KeyCode::Char('o') => {
      if let Some(url) = app.repo_current_url() {
        open_url(&url);
        app.set_repo_status(format!("Opened: {url}"));
      } else {
        app.set_repo_status("No repo URL available.".to_string());
      }
    }
    KeyCode::Char('d') => app.repo_download_file(),
    KeyCode::Enter => {
      log::debug!(
        "repo Enter: repo_fetch_rx active={}",
        app.repo_fetch_rx.is_some()
      );
      if app.repo_fetch_rx.is_none() {
        if let Some(target) = app.repo_enter_target() {
          let token = app.github_token.clone().unwrap_or_default();
          match target {
            crate::app::RepoEnterTarget::Dir(path) => {
              if let Some(ctx) = &app.repo_context {
                let (owner, repo, branch) = (
                  ctx.owner.clone(),
                  ctx.repo_name.clone(),
                  ctx.default_branch.clone(),
                );
                log::debug!("repo Enter: spawning dir fetch path={:?}", path);
                app.set_repo_status("Loading…");
                let (tx, rx) = mpsc::channel();
                app.repo_fetch_rx = Some(rx);
                spawn_repo_dir(owner, repo, branch, path, token, tx);
              }
            }
            crate::app::RepoEnterTarget::File(path, name) => {
              if let Some(ctx) = &app.repo_context {
                let (owner, repo) = (ctx.owner.clone(), ctx.repo_name.clone());
                log::debug!("repo Enter: spawning file fetch path={:?}", path);
                app.set_repo_status("Loading…");
                let (tx, rx) = mpsc::channel();
                app.repo_fetch_rx = Some(rx);
                spawn_repo_file(owner, repo, path, name, token, tx);
              }
            }
          }
        }
      }
    }
    KeyCode::Char('b') | KeyCode::Backspace => {
      if app.repo_back_target().is_none()
        && app.repo_context.as_ref().is_some_and(|c| !c.no_token)
      {
        app.set_repo_status("Already at root");
      } else if let Some(parent) = app.repo_back_target() {
        if app.repo_fetch_rx.is_none() {
          if let Some(ctx) = &app.repo_context {
            let (owner, repo, branch) = (
              ctx.owner.clone(),
              ctx.repo_name.clone(),
              ctx.default_branch.clone(),
            );
            let token = app.github_token.clone().unwrap_or_default();
            app.set_repo_status("Loading…");
            let (tx, rx) = mpsc::channel();
            app.repo_fetch_rx = Some(rx);
            spawn_repo_dir(owner, repo, branch, parent, token, tx);
          }
        }
      }
    }
    _ => {}
  }
  true
}

fn handle_sources_popup(key: KeyEvent, app: &mut App) -> bool {
  if app.view != AppView::Sources {
    return false;
  }
  if app.sources_input_active {
    match key.code {
      KeyCode::Esc => {
        app.sources_input_active = false;
        app.sources_detect_state = SourcesDetectState::Idle;
        app.sources_input.clear();
      }
      KeyCode::Enter => {
        match &app.sources_detect_state {
          SourcesDetectState::Idle => {
            if !app.sources_input.is_empty() {
              app.sources_detect_state = SourcesDetectState::Detecting;
              let url = app.sources_input.clone();
              let (dtx, drx) = mpsc::channel();
              app.sources_detect_rx = Some(drx);
              spawn_discovery(url, dtx);
            }
          }
          SourcesDetectState::Detecting => {
            // waiting — do nothing
          }
          SourcesDetectState::Result(result) => {
            let result = result.clone();
            match &result {
              DiscoverResult::ArxivCategory(code) => {
                if !app.config.sources.arxiv_categories.contains(code) {
                  log::debug!(
                    "sources_popup: adding arxiv category via detection: {code}"
                  );
                  app.config.sources.arxiv_categories.push(code.clone());
                  app.config.save();
                  log::debug!(
                    "sources_popup: saved — arxiv categories now: [{}]",
                    app.config.sources.arxiv_categories.join(", ")
                  );
                  force_refresh(app);
                }
              }
              DiscoverResult::RssFeed { url, name } => {
                let exists =
                  app.config.sources.custom_feeds.iter().any(|f| &f.url == url);
                if !exists {
                  app.config.sources.custom_feeds.push(config::CustomFeed {
                    url: url.clone(),
                    name: name.clone(),
                    feed_type: "rss".to_string(),
                  });
                  app.config.save();
                  force_refresh(app);
                }
              }
              DiscoverResult::HuggingFaceAlreadyEnabled
              | DiscoverResult::Failed(_) => {}
            }
            app.sources_input.clear();
            app.sources_detect_state = SourcesDetectState::Idle;
            app.sources_input_active = false;
          }
        }
      }
      KeyCode::Backspace => {
        app.sources_input.pop();
        app.sources_detect_state = SourcesDetectState::Idle;
      }
      KeyCode::Char(c) => {
        app.sources_input.push(c);
        app.sources_detect_state = SourcesDetectState::Idle;
      }
      _ => {}
    }
  } else {
    let cats = app.sources_popup_arxiv_cats();
    let cats_count = cats.len();
    let sources_count = config::PREDEFINED_SOURCES.len();
    let custom_count = app.config.sources.custom_feeds.len();
    let total = app.sources_popup_total_items();

    match key.code {
      KeyCode::Esc | KeyCode::Char('q') => {
        app.view = AppView::Settings;
        app.sources_cursor = 0;
        app.sources_input.clear();
        app.sources_detect_state = SourcesDetectState::Idle;
      }
      KeyCode::Char('j') | KeyCode::Down => {
        app.sources_cursor =
          (app.sources_cursor + 1).min(total.saturating_sub(1));
      }
      KeyCode::Char('k') | KeyCode::Up => {
        app.sources_cursor = app.sources_cursor.saturating_sub(1);
      }
      KeyCode::Enter | KeyCode::Char('/') => {
        if app.sources_cursor == 0 {
          app.sources_input_active = true;
        }
      }
      KeyCode::Char(' ') => {
        let c = app.sources_cursor;
        if c == 0 {
          app.sources_input_active = true;
        } else if c <= cats_count {
          let code = cats[c - 1].0.clone();
          if app.config.sources.arxiv_categories.contains(&code) {
            log::debug!("sources_popup: removing arxiv category: {code}");
            app.config.sources.arxiv_categories.retain(|x| x != &code);
          } else {
            log::debug!("sources_popup: adding arxiv category: {code}");
            app.config.sources.arxiv_categories.push(code);
          }
          app.config.save();
          log::debug!(
            "sources_popup: saved — arxiv categories now: [{}]",
            app.config.sources.arxiv_categories.join(", ")
          );
          force_refresh(app);
        } else if c <= cats_count + sources_count {
          let src = config::PREDEFINED_SOURCES[c - cats_count - 1];
          let cur = app
            .config
            .sources
            .enabled_sources
            .get(src)
            .copied()
            .unwrap_or(true);
          app.config.sources.enabled_sources.insert(src.to_string(), !cur);
          app.config.save();
          app.invalidate_visible_cache();
          force_refresh(app);
        }
        // custom feeds: no toggle (present = enabled, use d to delete)
      }
      KeyCode::Char('d') => {
        let c = app.sources_cursor;
        let custom_start = 1 + cats_count + sources_count;
        if c >= custom_start && c < custom_start + custom_count {
          let idx = c - custom_start;
          app.config.sources.custom_feeds.remove(idx);
          app.config.save();
          app.sources_cursor = app.sources_cursor.saturating_sub(1);
        }
      }
      _ => {}
    }
  }
  true
}

fn handle_settings_view(key: KeyEvent, app: &mut App) -> bool {
  if app.view != AppView::Settings {
    return false;
  }
  if app.theme_picker_active {
    handle_theme_picker(key, app);
    return true;
  }
  if app.settings_editing {
    match key.code {
      KeyCode::Enter => {
        match app.settings_field {
          0 => app.settings_github_token = app.settings_edit_buf.clone(),
          1 => app.settings_s2_key = app.settings_edit_buf.clone(),
          2 => app.settings_claude_key = app.settings_edit_buf.clone(),
          3 => app.settings_openai_key = app.settings_edit_buf.clone(),
          _ => {}
        }
        app.settings_editing = false;
      }
      KeyCode::Esc => {
        app.settings_editing = false;
        app.settings_edit_buf.clear();
      }
      KeyCode::Backspace => {
        app.settings_edit_buf.pop();
      }
      KeyCode::Char(c) => {
        app.settings_edit_buf.push(c);
      }
      _ => {}
    }
  } else {
    match key.code {
      KeyCode::Char('q') | KeyCode::Esc => {
        app.view = AppView::Feed;
      }
      KeyCode::Char('j') | KeyCode::Down => {
        app.settings_field = (app.settings_field + 1).min(5);
      }
      KeyCode::Char('k') | KeyCode::Up => {
        app.settings_field = app.settings_field.saturating_sub(1);
      }
      KeyCode::Enter => {
        if app.settings_field == 4 {
          app.settings_default_chat_provider =
            if app.settings_default_chat_provider == "claude" {
              "openai".to_string()
            } else {
              "claude".to_string()
            };
        } else if app.settings_field == 5 {
          open_theme_picker(app);
        } else {
          app.settings_edit_buf = match app.settings_field {
            0 => app.settings_github_token.clone(),
            1 => app.settings_s2_key.clone(),
            2 => app.settings_claude_key.clone(),
            3 => app.settings_openai_key.clone(),
            _ => String::new(),
          };
          app.settings_editing = true;
        }
      }
      KeyCode::Char('s') | KeyCode::Char('S') => {
        app.config.github_token = if app.settings_github_token.is_empty() {
          None
        } else {
          Some(app.settings_github_token.clone())
        };
        app.config.semantic_scholar_key = if app.settings_s2_key.is_empty() {
          None
        } else {
          Some(app.settings_s2_key.clone())
        };
        app.config.claude_api_key = if app.settings_claude_key.is_empty() {
          None
        } else {
          Some(app.settings_claude_key.clone())
        };
        app.config.openai_api_key = if app.settings_openai_key.is_empty() {
          None
        } else {
          Some(app.settings_openai_key.clone())
        };
        app.config.default_chat_provider =
          app.settings_default_chat_provider.clone();
        app.config.theme = app.active_theme;
        app.config.active_custom_theme_id = app.active_custom_theme_id.clone();
        // Keep github_token field in sync for repo viewer.
        app.github_token = app.config.github_token.clone();
        // Rebuild chat_ui with updated keys on next open.
        app.chat_ui = None;
        app.config.save();
        app.settings_save_time = Some(std::time::Instant::now());
      }
      KeyCode::Char('p') => {
        app.sources_cursor = 0;
        app.sources_input.clear();
        app.sources_input_active = false;
        app.sources_detect_state = SourcesDetectState::Idle;
        app.view = AppView::Sources;
        log::debug!(
          "sources_popup: opened — current arxiv categories: [{}]",
          app.config.sources.arxiv_categories.join(", ")
        );
      }
      _ => {}
    }
  }
  true
}

fn open_theme_picker(app: &mut App) {
  app.theme_picker_active = true;
  app.custom_theme_editor = None;
  app.theme_picker_original =
    Some((app.active_theme, app.active_custom_theme_id.clone()));
  app.theme_picker_cursor = theme_picker_active_row(app);
  app.theme_picker_scroll = app.theme_picker_cursor.saturating_sub(4);
}

fn handle_theme_picker(key: KeyEvent, app: &mut App) {
  if app.custom_theme_editor.is_some() {
    handle_custom_theme_editor(key, app);
    return;
  }

  let total = theme_picker_row_count(app);
  if total == 0 {
    app.theme_picker_active = false;
    return;
  }

  match key.code {
    KeyCode::Char('q') | KeyCode::Esc => {
      if let Some((theme, custom_id)) = app.theme_picker_original.take() {
        app.active_theme = theme;
        app.active_custom_theme_id = custom_id;
      }
      app.theme_picker_active = false;
    }
    KeyCode::Enter => {
      activate_theme_picker_row(app, true);
    }
    KeyCode::Char('j') | KeyCode::Down => {
      app.theme_picker_cursor = (app.theme_picker_cursor + 1).min(total - 1);
      activate_theme_picker_row(app, false);
      clamp_theme_picker_scroll(app);
    }
    KeyCode::Char('k') | KeyCode::Up => {
      app.theme_picker_cursor = app.theme_picker_cursor.saturating_sub(1);
      activate_theme_picker_row(app, false);
      clamp_theme_picker_scroll(app);
    }
    KeyCode::Char('e') => {
      if let Some(custom) = selected_custom_theme(app).cloned() {
        app.custom_theme_editor = Some(CustomThemeEditorState {
          theme: custom.clone(),
          is_new: false,
          mode: CustomThemeEditorMode::Palette,
          role_cursor: 0,
          hue_cursor: 6,
          shade_cursor: 3,
          edit_buf: String::new(),
        });
      }
    }
    KeyCode::Char('d') => {
      if let Some(custom) = selected_custom_theme(app).cloned() {
        app.custom_theme_editor = Some(CustomThemeEditorState {
          theme: custom.clone(),
          is_new: false,
          mode: CustomThemeEditorMode::DeleteConfirm,
          role_cursor: 0,
          hue_cursor: 6,
          shade_cursor: 3,
          edit_buf: String::new(),
        });
      }
    }
    _ => {}
  }
}

fn clamp_theme_picker_scroll(app: &mut App) {
  const VISIBLE_ROWS: usize = 10;
  if app.theme_picker_cursor < app.theme_picker_scroll {
    app.theme_picker_scroll = app.theme_picker_cursor;
  } else if app.theme_picker_cursor >= app.theme_picker_scroll + VISIBLE_ROWS {
    app.theme_picker_scroll =
      app.theme_picker_cursor.saturating_sub(VISIBLE_ROWS - 1);
  }
}

fn theme_picker_row_count(app: &App) -> usize {
  ThemeId::all().len() + app.config.custom_themes.len() + 1
}

fn theme_picker_active_row(app: &App) -> usize {
  let presets = ThemeId::all();
  if let Some(id) = &app.active_custom_theme_id {
    if let Some(idx) = app.config.custom_themes.iter().position(|t| &t.id == id)
    {
      return presets.len() + idx;
    }
  }
  presets.iter().position(|id| *id == app.active_theme).unwrap_or(0)
}

fn selected_custom_theme(app: &App) -> Option<&CustomThemeConfig> {
  let presets = ThemeId::all().len();
  let idx = app.theme_picker_cursor.checked_sub(presets)?;
  app.config.custom_themes.get(idx)
}

fn activate_theme_picker_row(app: &mut App, commit: bool) {
  let presets = ThemeId::all();
  let preset_count = presets.len();
  let custom_count = app.config.custom_themes.len();
  let row =
    app.theme_picker_cursor.min(theme_picker_row_count(app).saturating_sub(1));

  if row < preset_count {
    app.active_theme = presets[row];
    app.active_custom_theme_id = None;
    app.config.theme = app.active_theme;
    app.config.active_custom_theme_id = None;
    if commit {
      app.theme_picker_original = None;
      app.theme_picker_active = false;
    }
  } else if row < preset_count + custom_count {
    let custom = &app.config.custom_themes[row - preset_count];
    app.active_theme = custom.base;
    app.active_custom_theme_id = Some(custom.id.clone());
    app.config.theme = app.active_theme;
    app.config.active_custom_theme_id = app.active_custom_theme_id.clone();
    if commit {
      app.theme_picker_original = None;
      app.theme_picker_active = false;
    }
  } else if commit {
    open_new_custom_theme_editor(app);
  }
}

fn open_new_custom_theme_editor(app: &mut App) {
  let base =
    app.active_custom_theme().map(|t| t.base).unwrap_or(app.active_theme);
  let name = next_custom_theme_name(app);
  let theme = CustomThemeConfig::from_theme(
    next_custom_theme_id(app),
    name.clone(),
    base,
    app.theme(),
  );
  app.custom_theme_editor = Some(CustomThemeEditorState {
    theme,
    is_new: true,
    mode: CustomThemeEditorMode::Name,
    role_cursor: 0,
    hue_cursor: 6,
    shade_cursor: 3,
    edit_buf: name,
  });
}

fn next_custom_theme_id(app: &App) -> String {
  for n in 1.. {
    let id = format!("custom-{n}");
    if !app.config.custom_themes.iter().any(|theme| theme.id == id) {
      return id;
    }
  }
  unreachable!()
}

fn next_custom_theme_name(app: &App) -> String {
  for n in 1.. {
    let name = format!("Custom {n}");
    if !app.config.custom_themes.iter().any(|theme| theme.name == name) {
      return name;
    }
  }
  unreachable!()
}

fn handle_custom_theme_editor(key: KeyEvent, app: &mut App) {
  let Some(mode) = app.custom_theme_editor.as_ref().map(|editor| editor.mode)
  else {
    return;
  };

  match mode {
    CustomThemeEditorMode::Name => handle_custom_theme_name_editor(key, app),
    CustomThemeEditorMode::Hex => handle_custom_theme_hex_editor(key, app),
    CustomThemeEditorMode::DeleteConfirm => {
      handle_custom_theme_delete_confirm(key, app)
    }
    CustomThemeEditorMode::Palette => {
      handle_custom_theme_palette_editor(key, app)
    }
  }
}

fn handle_custom_theme_name_editor(key: KeyEvent, app: &mut App) {
  match key.code {
    KeyCode::Enter => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        let name = editor.edit_buf.trim();
        if !name.is_empty() {
          editor.theme.name = name.to_string();
        }
        editor.mode = CustomThemeEditorMode::Palette;
        editor.edit_buf.clear();
      }
    }
    KeyCode::Esc => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        if editor.is_new {
          app.custom_theme_editor = None;
        } else {
          editor.mode = CustomThemeEditorMode::Palette;
          editor.edit_buf.clear();
        }
      }
    }
    KeyCode::Backspace => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        editor.edit_buf.pop();
      }
    }
    KeyCode::Char(c) => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        editor.edit_buf.push(c);
      }
    }
    _ => {}
  }
}

fn handle_custom_theme_hex_editor(key: KeyEvent, app: &mut App) {
  match key.code {
    KeyCode::Enter => {
      let Some(editor) = app.custom_theme_editor.as_mut() else {
        return;
      };
      let value = editor.edit_buf.trim();
      if config::parse_hex_color(value).is_some() {
        let key = CUSTOM_THEME_ROLES
          [editor.role_cursor.min(CUSTOM_THEME_ROLES.len() - 1)]
        .key;
        editor.theme.colors.set_role(key, normalize_hex(value));
        editor.mode = CustomThemeEditorMode::Palette;
        editor.edit_buf.clear();
      } else {
        app.set_notification(
          "Use a 6-digit hex color, e.g. #67D7F5.".to_string(),
        );
      }
    }
    KeyCode::Esc => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        editor.mode = CustomThemeEditorMode::Palette;
        editor.edit_buf.clear();
      }
    }
    KeyCode::Backspace => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        editor.edit_buf.pop();
      }
    }
    KeyCode::Char(c) => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        if c == '#' && editor.edit_buf.is_empty() || c.is_ascii_hexdigit() {
          editor.edit_buf.push(c);
        }
      }
    }
    _ => {}
  }
}

fn handle_custom_theme_delete_confirm(key: KeyEvent, app: &mut App) {
  match key.code {
    KeyCode::Char('y') | KeyCode::Char('Y') => {
      let Some(editor) = app.custom_theme_editor.take() else {
        return;
      };
      let delete_id = editor.theme.id;
      let delete_was_active =
        app.active_custom_theme_id.as_deref() == Some(&delete_id);
      app.config.custom_themes.retain(|theme| theme.id != delete_id);
      if delete_was_active {
        app.active_custom_theme_id = None;
        app.config.active_custom_theme_id = None;
        app.active_theme = editor.theme.base;
        app.config.theme = app.active_theme;
      }
      app.config.save();
      app.settings_save_time = Some(std::time::Instant::now());
      app.theme_picker_cursor =
        app.theme_picker_cursor.min(theme_picker_row_count(app) - 1);
      clamp_theme_picker_scroll(app);
    }
    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        editor.mode = CustomThemeEditorMode::Palette;
      }
    }
    _ => {}
  }
}

fn handle_custom_theme_palette_editor(key: KeyEvent, app: &mut App) {
  match key.code {
    KeyCode::Char('q') | KeyCode::Esc => {
      app.custom_theme_editor = None;
    }
    KeyCode::Char('s') | KeyCode::Enter => {
      save_custom_theme_editor(app);
    }
    KeyCode::Char('n') => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        editor.mode = CustomThemeEditorMode::Name;
        editor.edit_buf = editor.theme.name.clone();
      }
    }
    KeyCode::Char('x') => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        let key = CUSTOM_THEME_ROLES
          [editor.role_cursor.min(CUSTOM_THEME_ROLES.len() - 1)]
        .key;
        editor.edit_buf =
          editor.theme.colors.get_role(key).unwrap_or("#000000").to_string();
        editor.mode = CustomThemeEditorMode::Hex;
      }
    }
    KeyCode::Char('d') => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        if !editor.is_new {
          editor.mode = CustomThemeEditorMode::DeleteConfirm;
        }
      }
    }
    KeyCode::Char('r') => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        editor.theme.colors =
          config::CustomThemeColors::from_theme(editor.theme.base.theme());
      }
    }
    KeyCode::Char('j') | KeyCode::Down => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        editor.role_cursor =
          (editor.role_cursor + 1).min(CUSTOM_THEME_ROLES.len() - 1);
      }
    }
    KeyCode::Char('k') | KeyCode::Up => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        editor.role_cursor = editor.role_cursor.saturating_sub(1);
      }
    }
    KeyCode::Char('h') | KeyCode::Left => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        editor.hue_cursor = editor.hue_cursor.saturating_sub(1);
      }
    }
    KeyCode::Char('l') | KeyCode::Right => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        editor.hue_cursor =
          (editor.hue_cursor + 1).min(THEME_PALETTE[0].len() - 1);
      }
    }
    KeyCode::Char('[') | KeyCode::Char('-') => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        editor.shade_cursor = editor.shade_cursor.saturating_sub(1);
      }
    }
    KeyCode::Char(']') | KeyCode::Char('+') | KeyCode::Char('=') => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        editor.shade_cursor =
          (editor.shade_cursor + 1).min(THEME_PALETTE.len() - 1);
      }
    }
    KeyCode::Char(' ') => {
      if let Some(editor) = app.custom_theme_editor.as_mut() {
        let role = CUSTOM_THEME_ROLES
          [editor.role_cursor.min(CUSTOM_THEME_ROLES.len() - 1)]
        .key;
        editor
          .theme
          .colors
          .set_role(role, selected_palette_hex(editor).to_string());
      }
    }
    _ => {}
  }
}

fn save_custom_theme_editor(app: &mut App) {
  let Some(editor) = app.custom_theme_editor.take() else {
    return;
  };
  let theme = editor.theme;
  if let Some(existing) =
    app.config.custom_themes.iter_mut().find(|t| t.id == theme.id)
  {
    *existing = theme.clone();
  } else {
    app.config.custom_themes.push(theme.clone());
  }
  app.active_theme = theme.base;
  app.active_custom_theme_id = Some(theme.id.clone());
  app.config.theme = app.active_theme;
  app.config.active_custom_theme_id = app.active_custom_theme_id.clone();
  app.theme_picker_cursor = theme_picker_active_row(app);
  app.theme_picker_original = None;
  app.theme_picker_active = false;
  app.config.save();
  app.settings_save_time = Some(std::time::Instant::now());
  clamp_theme_picker_scroll(app);
}

const THEME_PALETTE: &[&[&str]] = &[
  &[
    "#F8FAFC", "#F7FEE7", "#FEFCE8", "#FFFBEB", "#FFF7ED", "#FFF1F2",
    "#FEF2F2", "#FDF2F8", "#FDF4FF", "#FAF5FF", "#F5F3FF", "#EEF2FF",
    "#EFF6FF", "#F0F9FF", "#ECFEFF", "#F0FDFA",
  ],
  &[
    "#E2E8F0", "#ECFCCB", "#FEF9C3", "#FEF3C7", "#FFEDD5", "#FFE4E6",
    "#FEE2E2", "#FCE7F3", "#FAE8FF", "#F3E8FF", "#EDE9FE", "#E0E7FF",
    "#DBEAFE", "#E0F2FE", "#CFFAFE", "#CCFBF1",
  ],
  &[
    "#CBD5E1", "#D9F99D", "#FEF08A", "#FDE68A", "#FED7AA", "#FECDD3",
    "#FECACA", "#FBCFE8", "#F5D0FE", "#E9D5FF", "#DDD6FE", "#C7D2FE",
    "#BFDBFE", "#BAE6FD", "#A5F3FC", "#99F6E4",
  ],
  &[
    "#94A3B8", "#BEF264", "#FDE047", "#FCD34D", "#FDBA74", "#FDA4AF",
    "#FCA5A5", "#F9A8D4", "#F0ABFC", "#D8B4FE", "#C4B5FD", "#A5B4FC",
    "#93C5FD", "#7DD3FC", "#67E8F9", "#5EEAD4",
  ],
  &[
    "#64748B", "#A3E635", "#FACC15", "#F59E0B", "#FB923C", "#FB7185",
    "#F87171", "#F472B6", "#E879F9", "#C084FC", "#A78BFA", "#818CF8",
    "#60A5FA", "#38BDF8", "#22D3EE", "#2DD4BF",
  ],
  &[
    "#475569", "#84CC16", "#EAB308", "#D97706", "#F97316", "#F43F5E",
    "#EF4444", "#EC4899", "#D946EF", "#A855F7", "#8B5CF6", "#6366F1",
    "#3B82F6", "#0EA5E9", "#06B6D4", "#14B8A6",
  ],
  &[
    "#334155", "#65A30D", "#CA8A04", "#B45309", "#EA580C", "#E11D48",
    "#DC2626", "#DB2777", "#C026D3", "#9333EA", "#7C3AED", "#4F46E5",
    "#2563EB", "#0284C7", "#0891B2", "#0D9488",
  ],
  &[
    "#1E293B", "#4D7C0F", "#A16207", "#92400E", "#C2410C", "#BE123C",
    "#B91C1C", "#BE185D", "#A21CAF", "#7E22CE", "#6D28D9", "#4338CA",
    "#1D4ED8", "#0369A1", "#0E7490", "#0F766E",
  ],
  &[
    "#0F172A", "#3F6212", "#854D0E", "#78350F", "#9A3412", "#9F1239",
    "#991B1B", "#9D174D", "#86198F", "#6B21A8", "#5B21B6", "#3730A3",
    "#1E40AF", "#075985", "#155E75", "#115E59",
  ],
  &[
    "#020617", "#365314", "#713F12", "#451A03", "#7C2D12", "#4C0519",
    "#7F1D1D", "#831843", "#701A75", "#581C87", "#4C1D95", "#312E81",
    "#1E3A8A", "#0C4A6E", "#164E63", "#134E4A",
  ],
];

fn selected_palette_hex(editor: &CustomThemeEditorState) -> &'static str {
  THEME_PALETTE[editor.shade_cursor.min(THEME_PALETTE.len() - 1)]
    [editor.hue_cursor.min(THEME_PALETTE[0].len() - 1)]
}

fn normalize_hex(value: &str) -> String {
  let hex = value.strip_prefix('#').unwrap_or(value).to_ascii_uppercase();
  format!("#{hex}")
}

fn handle_feed_view(key: KeyEvent, app: &mut App) {
  // Discoveries tab — search bar input (when focused).
  if app.feed_tab == FeedTab::Discoveries && app.discovery_search_focused {
    let palette_active = app.discovery_query.starts_with('/');
    match key.code {
      KeyCode::Esc => {
        app.discovery_search_focused = false;
        app.discovery_palette_selected = 0;
        app.discovery_palette_scroll = 0;
      }
      KeyCode::Up if palette_active => {
        app.discovery_palette_selected =
          app.discovery_palette_selected.saturating_sub(1);
        clamp_discovery_palette_scroll(app);
      }
      KeyCode::Down if palette_active => {
        let count = discovery_palette_count(&app.discovery_query);
        if app.discovery_palette_selected + 1 < count {
          app.discovery_palette_selected += 1;
          clamp_discovery_palette_scroll(app);
        }
      }
      KeyCode::Tab if palette_active => {
        // Complete selected command into the input.
        if let Some(completion) = discovery_palette_completion(
          &app.discovery_query,
          app.discovery_palette_selected,
        ) {
          app.discovery_query = completion;
          app.discovery_palette_selected = 0;
          app.discovery_palette_scroll = 0;
        }
      }
      KeyCode::Enter => {
        if !app.discovery_query.is_empty() && !app.discovery_loading {
          let query = app.discovery_query.clone();
          app.discovery_palette_selected = 0;
          app.discovery_palette_scroll = 0;
          if query.starts_with('/') {
            app.discovery_search_focused = false;
            app.discovery_query.clear();
            let cmd = crate::commands::parser::parse_slash_command(&query);
            crate::commands::dispatch::dispatch_slash_command(app, cmd);
          } else {
            let config = app.config.clone();
            spawn_ai_discovery(query, config, app);
          }
        }
      }
      KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
        app.discovery_force_new = true;
        app.discovery_query.clear();
        app.discovery_palette_selected = 0;
        app.discovery_palette_scroll = 0;
      }
      KeyCode::Backspace => {
        app.discovery_query.pop();
        if !app.discovery_query.starts_with('/') {
          app.discovery_palette_selected = 0;
          app.discovery_palette_scroll = 0;
        }
      }
      KeyCode::Char(c) => {
        app.discovery_query.push(c);
        app.discovery_palette_selected = 0;
        app.discovery_palette_scroll = 0;
      }
      _ => {}
    }
    return;
  }

  // Discoveries tab — any printable char focuses the search bar.
  if app.feed_tab == FeedTab::Discoveries {
    if let KeyCode::Char(c) = key.code {
      if c != 'q' {
        app.discovery_search_focused = true;
        app.discovery_query.push(c);
        return;
      }
    }
  }

  // History tab — handle filter cycling, navigation, reopen, delete.
  if app.feed_tab == FeedTab::History {
    if handle_history_tab(key, app) {
      return;
    }
  }

  // Library tab — handle workflow-state chip cycling. Other keys (j/k, Enter,
  // i/r/w/x, etc.) fall through to the generic feed handler below.
  if app.feed_tab == FeedTab::Library {
    if handle_library_tab(key, app) {
      return;
    }
  }

  if app.search_active {
    match key.code {
      KeyCode::Esc => {
        app.search_active = false;
        app.search_query.clear();
        app.reset_active_feed_position();
      }
      KeyCode::Enter => {
        app.search_active = false;
      }
      KeyCode::Backspace => {
        app.pop_search_char();
      }
      KeyCode::Char(c) => {
        app.push_search_char(c);
      }
      _ => {}
    }
  } else if app.filter_focus {
    match key.code {
      KeyCode::Char('j') | KeyCode::Down => {
        if kbd_scroll_ok(app) {
          app.filter_cursor_down();
        }
      }
      KeyCode::Char('k') | KeyCode::Up => {
        if kbd_scroll_ok(app) {
          app.filter_cursor_up();
        }
      }
      KeyCode::Char(' ') => app.toggle_filter_at_cursor(),
      KeyCode::Char('c') => app.clear_filters(),
      KeyCode::Char('f') | KeyCode::Tab => {
        app.filter_focus = false;
      }
      KeyCode::Esc => {
        app.clear_filters();
        app.filter_focus = false;
      }
      _ => {}
    }
  } else if app.focused_pane == PaneId::Feed {
    // In State 2 the narrow feed holds focus — use a restricted key set so
    // main-feed bindings (Esc → quit, v → repo viewer) don't fire here.
    if app.reader_split_active {
      // Close description popup first if open.
      if app.narrow_feed_details_open {
        match key.code {
          KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('d') => {
            app.narrow_feed_details_open = false;
          }
          KeyCode::Char('j') | KeyCode::Down => {
            app.details_scroll = app.details_scroll.saturating_add(1);
          }
          KeyCode::Char('k') | KeyCode::Up => {
            app.details_scroll = app.details_scroll.saturating_sub(1);
          }
          _ => {}
        }
        return;
      }
      match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
          app.reader_split_active = false;
          app.narrow_feed_details_open = false;
          app.focused_pane = PaneId::Reader;
        }
        KeyCode::Char('d') => {
          app.narrow_feed_details_open = true;
          app.details_scroll = 0;
        }
        KeyCode::Char('/') => {
          app.search_active = true;
          app.search_query.clear();
          app.reset_active_feed_position();
        }
        KeyCode::Char('j') | KeyCode::Down => {
          if kbd_scroll_ok(app) {
            app.move_down();
            app.narrow_feed_details_open = false;
          }
        }
        KeyCode::Char('k') | KeyCode::Up => {
          if kbd_scroll_ok(app) {
            app.move_up();
            app.narrow_feed_details_open = false;
          }
        }
        KeyCode::Enter => {
          if !app.fulltext_loading {
            if let Some(item) = app.selected_item().cloned() {
              let (tx, rx) = mpsc::channel();
              app.fulltext_rx = Some(rx);
              app.fulltext_loading = true;
              app.fulltext_for_secondary = false;
              app.narrow_feed_details_open = false;
              remember_fulltext_paper_context(app, &item);
              app.set_notification(format!(
                "Fetching: {}…",
                truncate_for_notif(&item.title, 40)
              ));
              app.focused_pane = PaneId::Reader;
              spawn_fulltext_fetch(item, tx);
            }
          }
        }
        _ => {}
      }
      return;
    }

    match key.code {
      KeyCode::Tab => {
        app.feed_tab = match app.feed_tab {
          FeedTab::Inbox => FeedTab::Library,
          FeedTab::Library => FeedTab::Discoveries,
          FeedTab::Discoveries => FeedTab::History,
          FeedTab::History => FeedTab::Inbox,
        };
        app.reset_active_feed_position();
      }
      KeyCode::BackTab => {
        app.feed_tab = match app.feed_tab {
          FeedTab::Inbox => FeedTab::History,
          FeedTab::Library => FeedTab::Inbox,
          FeedTab::Discoveries => FeedTab::Library,
          FeedTab::History => FeedTab::Discoveries,
        };
        app.reset_active_feed_position();
      }
      KeyCode::Char('f') => {
        app.filter_focus = true;
      }
      KeyCode::Char('q') => app.show_quit_popup(),
      KeyCode::Esc => {
        app.clear_notification();
        app.status_message = None;
      }
      KeyCode::Char('l') | KeyCode::Right => {}
      KeyCode::Char('h') | KeyCode::Left => {
        // already on Feed — no-op
      }
      KeyCode::Char('j') | KeyCode::Down => {
        if kbd_scroll_ok(app) {
          app.move_down();
        }
      }
      KeyCode::Char('k') | KeyCode::Up => {
        if kbd_scroll_ok(app) {
          app.move_up();
        }
      }
      KeyCode::PageDown | KeyCode::PageUp => {}
      KeyCode::Char('g') => {
        app.go_to_top();
      }
      KeyCode::Char('G') => {
        app.go_to_bottom();
      }
      KeyCode::Char(' ') => {
        if app.selected_item().is_some() {
          app.abstract_popup_active = true;
        }
      }
      KeyCode::Enter => {
        if !app.fulltext_loading {
          if let Some(item) = app.selected_item().cloned() {
            remember_fulltext_paper_context(app, &item);
            {
              log::debug!(
                "feed Enter: spawning fulltext fetch for url={}",
                item.url
              );
              let t = std::time::Instant::now();
              app.fulltext_loading = true;
              app.set_notification(format!(
                "Fetching: {}…",
                truncate_for_notif(&item.title, 40)
              ));
              let (tx, rx) = mpsc::channel();
              app.fulltext_rx = Some(rx);
              spawn_fulltext_fetch(item, tx);
              log::debug!(
                "feed Enter: fetch setup took {}µs",
                t.elapsed().as_micros()
              );
            }
          }
        }
      }
      KeyCode::Char('/') => {
        app.search_active = true;
        app.search_query.clear();
        app.reset_active_feed_position();
      }
      KeyCode::Char('i') => app.set_workflow_state(WorkflowState::Inbox),
      KeyCode::Char('r') => app.set_workflow_state(WorkflowState::DeepRead),
      KeyCode::Char('w') => app.set_workflow_state(WorkflowState::Queued),
      KeyCode::Char('x') => app.set_workflow_state(WorkflowState::Archived),
      KeyCode::Char('o') => {
        if let Some(item) = app.selected_item() {
          let url = item.url.clone();
          let title = truncate_for_notif(&item.title, 40);
          open_url(&url);
          app.set_notification(format!("Opened in browser: {title}"));
        }
      }
      KeyCode::Char('R') => {
        if app.is_loading || app.is_refreshing {
          app.set_notification("Already refreshing...".to_string());
        } else {
          app.clear_notification();
          do_refresh(app);
        }
      }
      KeyCode::Char('v') => {
        if let Some(item) = app.selected_item() {
          if item.github_owner.is_none() || item.github_repo_name.is_none() {
            app.status_message = Some("No repo linked".to_string());
          } else if let (Some(owner), Some(repo_name)) =
            (item.github_owner.clone(), item.github_repo_name.clone())
          {
            let token = app.github_token.clone().unwrap_or_default();
            if token.is_empty() {
              app.repo_context = Some(RepoContext {
                owner,
                repo_name,
                default_branch: String::new(),
                tree_path: String::new(),
                tree_nodes: Vec::new(),
                tree_cursor: 0,
                file_path: None,
                file_name: None,
                raw_file_content: String::new(),
                file_kind: crate::app::RepoFileKind::PlainText,
                file_lines: Vec::new(),
                file_highlighted: Vec::new(),
                markdown_cache: None,
                rendered_line_count: 0,
                markdown_has_pannable_lines: false,
                file_scroll: 0,
                pane_focus: RepoPane::Tree,
                status_message: None,
                no_token: true,
                h_offset: 0,
                wrap_width: 0,
                scroll_velocity: 0.0,
              });
              app.view = AppView::RepoViewer;
            } else {
              app.repo_context = Some(RepoContext {
                owner: owner.clone(),
                repo_name: repo_name.clone(),
                default_branch: String::new(),
                tree_path: String::new(),
                tree_nodes: Vec::new(),
                tree_cursor: 0,
                file_path: None,
                file_name: None,
                raw_file_content: String::new(),
                file_kind: crate::app::RepoFileKind::PlainText,
                file_lines: Vec::new(),
                file_highlighted: Vec::new(),
                markdown_cache: None,
                rendered_line_count: 0,
                markdown_has_pannable_lines: false,
                file_scroll: 0,
                pane_focus: RepoPane::Tree,
                status_message: Some("Loading…".into()),
                no_token: false,
                h_offset: 0,
                wrap_width: 0,
                scroll_velocity: 0.0,
              });
              app.view = AppView::RepoViewer;
              let (tx, rx) = mpsc::channel();
              app.repo_fetch_rx = Some(rx);
              spawn_repo_open(owner, repo_name, token, tx);
            }
          }
        }
      }
      _ => {}
    }
  }
}

// ── Tag picker handler ────────────────────────────────────────────────────────

fn handle_tag_picker(key: KeyEvent, app: &mut App) {
  let all = crate::tags::all_tags(&app.item_tags);
  match key.code {
    KeyCode::Esc => {
      app.close_tag_picker();
    }
    KeyCode::Enter => {
      let trimmed = app.tag_picker_input.trim().to_string();
      if !trimmed.is_empty() {
        app.toggle_tag_on_targets(&trimmed);
        app.tag_picker_input.clear();
      } else if let Some(tag) = all.get(app.tag_picker_selected) {
        let tag = tag.clone();
        app.toggle_tag_on_targets(&tag);
      }
    }
    KeyCode::Char(' ') => {
      if let Some(tag) = all.get(app.tag_picker_selected) {
        let tag = tag.clone();
        app.toggle_tag_on_targets(&tag);
      }
    }
    KeyCode::Up => {
      app.tag_picker_selected = app.tag_picker_selected.saturating_sub(1);
    }
    KeyCode::Down => {
      if !all.is_empty() {
        app.tag_picker_selected =
          (app.tag_picker_selected + 1).min(all.len() - 1);
      }
    }
    KeyCode::Backspace => {
      app.tag_picker_input.pop();
    }
    KeyCode::Char(c) => {
      app.tag_picker_input.push(c);
    }
    _ => {}
  }
}

// ── Library tab handler ───────────────────────────────────────────────────────

/// Returns true if the key was consumed by the Library tab (chip cycling).
/// Anything else falls through to the generic feed handler so navigation,
/// Enter, and `i/r/w/x` state transitions work as usual.
fn handle_library_tab(key: KeyEvent, app: &mut App) -> bool {
  use crate::models::WorkflowState;

  // Visual-mode-only handlers fire before the generic chip ones so j/k extend
  // selection rather than just moving the cursor.
  if app.library_visual_mode {
    match key.code {
      KeyCode::Esc => {
        app.library_exit_visual();
        return true;
      }
      KeyCode::Char('j') | KeyCode::Down => {
        let len = app.visible_count();
        if len > 0 {
          let next = (app.library_selected_index + 1).min(len - 1);
          app.library_selected_index = next;
          app.library_recompute_selection();
        }
        return true;
      }
      KeyCode::Char('k') | KeyCode::Up => {
        app.library_selected_index =
          app.library_selected_index.saturating_sub(1);
        app.library_recompute_selection();
        return true;
      }
      KeyCode::Char('r') => {
        let n = app.apply_workflow_to_selection(WorkflowState::DeepRead);
        app.library_exit_visual();
        app.set_notification(format!("Marked {n} as read"));
        return true;
      }
      KeyCode::Char('w') => {
        let n = app.apply_workflow_to_selection(WorkflowState::Queued);
        app.library_exit_visual();
        app.set_notification(format!("Queued {n} items"));
        return true;
      }
      KeyCode::Char('x') => {
        let n = app.apply_workflow_to_selection(WorkflowState::Archived);
        app.library_exit_visual();
        app.set_notification(format!("Archived {n} items"));
        return true;
      }
      KeyCode::Char('i') => {
        let n = app.apply_workflow_to_selection(WorkflowState::Inbox);
        app.library_exit_visual();
        app.set_notification(format!("Moved {n} back to Inbox"));
        return true;
      }
      KeyCode::Char('t') => {
        let urls: Vec<String> =
          app.library_selected_urls.iter().cloned().collect();
        app.open_tag_picker(urls);
        return true;
      }
      _ => {}
    }
    // Block any other key while in visual mode so the generic feed handler
    // doesn't double-fire (e.g. don't open filter panel via `f` mid-selection).
    return true;
  }

  match key.code {
    KeyCode::Char(']') => {
      app.library_filter = app.library_filter.next();
      app.library_selected_index = 0;
      app.library_list_offset = 0;
      app.invalidate_visible_cache();
      true
    }
    KeyCode::Char('[') => {
      app.library_filter = app.library_filter.prev();
      app.library_selected_index = 0;
      app.library_list_offset = 0;
      app.invalidate_visible_cache();
      true
    }
    KeyCode::Char('}') => {
      app.library_time_filter = app.library_time_filter.next();
      app.library_selected_index = 0;
      app.library_list_offset = 0;
      app.invalidate_visible_cache();
      true
    }
    KeyCode::Char('{') => {
      app.library_time_filter = app.library_time_filter.prev();
      app.library_selected_index = 0;
      app.library_list_offset = 0;
      app.invalidate_visible_cache();
      true
    }
    KeyCode::Char('v') => {
      app.library_visual_mode = true;
      app.library_visual_anchor = app.library_selected_index;
      app.library_recompute_selection();
      true
    }
    KeyCode::Char('t') => {
      if let Some(item) = app.selected_item().cloned() {
        app.open_tag_picker(vec![item.url]);
      }
      true
    }
    _ => false,
  }
}

// ── History tab handler ───────────────────────────────────────────────────────

/// Returns true if the key was handled by the History tab and the caller should
/// stop propagation. False means fall through to the generic feed handler.
fn handle_history_tab(key: KeyEvent, app: &mut App) -> bool {
  use crate::history::{HistoryFilter, HistoryKind};
  match key.code {
    KeyCode::Char(']') => {
      app.history_filter = app.history_filter.next();
      app.history_selected_index = 0;
      app.history_list_offset = 0;
      true
    }
    KeyCode::Char('[') => {
      app.history_filter = app.history_filter.prev();
      app.history_selected_index = 0;
      app.history_list_offset = 0;
      true
    }
    KeyCode::Char('j') | KeyCode::Down => {
      let len = app.filtered_history().len();
      if len > 0 {
        let next = (app.history_selected_index + 1).min(len - 1);
        app.history_selected_index = next;
      }
      true
    }
    KeyCode::Char('k') | KeyCode::Up => {
      app.history_selected_index = app.history_selected_index.saturating_sub(1);
      true
    }
    KeyCode::Char('g') => {
      app.history_selected_index = 0;
      true
    }
    KeyCode::Char('G') => {
      let len = app.filtered_history().len();
      if len > 0 {
        app.history_selected_index = len - 1;
      }
      true
    }
    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
      let visible = app.filtered_history();
      let Some(target) = visible.get(app.history_selected_index).cloned()
      else {
        return true;
      };
      let key_to_delete = (target.kind, target.key.clone());
      app.history.retain(|e| (e.kind, e.key.clone()) != key_to_delete);
      app.rebuild_history_paper_index();
      crate::store::history::save(&app.history);
      let len = app.filtered_history().len();
      if len > 0 && app.history_selected_index >= len {
        app.history_selected_index = len - 1;
      }
      true
    }
    KeyCode::Char('o') => {
      let visible = app.filtered_history();
      let Some(entry) =
        visible.get(app.history_selected_index).map(|e| (*e).clone())
      else {
        return true;
      };
      if entry.kind == HistoryKind::Paper {
        open_url(&entry.key);
        app.notification = Some(format!(
          "Opened in browser: {}",
          truncate_for_notif(&entry.title, 40)
        ));
        app.notification_item_id = Some(entry.key.clone());
      }
      true
    }
    KeyCode::Enter => {
      let visible = app.filtered_history();
      let Some(entry) = visible.get(app.history_selected_index).cloned() else {
        return true;
      };
      match entry.kind {
        HistoryKind::Paper => {
          // Reconstruct enough of FeedItem from history meta and reopen.
          if let Some(meta) = entry.paper_meta.clone() {
            let item = reconstruct_feed_item(&entry, &meta);
            remember_fulltext_paper_context(app, &item);
            app.fulltext_loading = true;
            app.set_notification(format!(
              "Fetching: {}…",
              truncate_for_notif(&item.title, 40)
            ));
            let (tx, rx) = mpsc::channel();
            app.fulltext_rx = Some(rx);
            spawn_fulltext_fetch(item, tx);
          }
        }
        HistoryKind::Query => {
          let topic = entry.key.clone();
          let config = app.config.clone();
          // Re-running a query starts a fresh discovery session.
          app.discovery_force_new = true;
          app.feed_tab = FeedTab::Discoveries;
          app.reset_active_feed_position();
          spawn_ai_discovery(topic, config, app);
        }
      }
      // Drop the silent unused HistoryFilter import warning.
      let _ = HistoryFilter::All;
      true
    }
    _ => false,
  }
}

fn reconstruct_feed_item(
  entry: &crate::history::HistoryEntry,
  meta: &crate::history::HistoryPaperMeta,
) -> crate::models::FeedItem {
  use crate::models::{ContentType, FeedItem, SignalLevel, WorkflowState};
  FeedItem {
    id: entry.key.clone(),
    title: entry.title.clone(),
    source_platform: meta.source_platform.clone(),
    content_type: ContentType::Paper,
    domain_tags: Vec::new(),
    signal: SignalLevel::Tertiary,
    published_at: meta.published_at.clone(),
    authors: meta.authors.clone(),
    summary_short: meta.summary_short.clone(),
    workflow_state: WorkflowState::Inbox,
    url: entry.key.clone(),
    upvote_count: 0,
    github_repo: None,
    github_owner: None,
    github_repo_name: None,
    benchmark_results: Vec::new(),
    full_content: None,
    source_name: entry.source.clone(),
    title_lower: entry.title.to_lowercase(),
    authors_lower: meta.authors.iter().map(|a| a.to_lowercase()).collect(),
  }
}

// ── Discovery palette helpers ─────────────────────────────────────────────────

fn discovery_palette_filtered(query: &str) -> Vec<chat::ChatSlashCommandSpec> {
  let all = crate::commands::registry::discovery_slash_specs();
  let q = query.to_lowercase();
  all
    .into_iter()
    .filter(|s| q == "/" || s.command.starts_with(q.as_str()))
    .collect()
}

fn discovery_palette_count(query: &str) -> usize {
  discovery_palette_filtered(query).len()
}

fn discovery_palette_completion(
  query: &str,
  selected: usize,
) -> Option<String> {
  discovery_palette_filtered(query)
    .into_iter()
    .nth(selected)
    .map(|s| s.completion)
}

fn clamp_discovery_palette_scroll(app: &mut App) {
  let visible = 8usize;
  let sel = app.discovery_palette_selected;
  if sel < app.discovery_palette_scroll {
    app.discovery_palette_scroll = sel;
  } else if sel >= app.discovery_palette_scroll + visible {
    app.discovery_palette_scroll = sel + 1 - visible;
  }
}
