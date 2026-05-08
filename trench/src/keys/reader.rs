use crossterm::event::{Event, KeyCode, KeyEvent};
use std::sync::mpsc;

use crate::app::{App, FeedTab, FocusedReader, PaneId};
use super::remember_fulltext_paper_context;
use super::super::{spawn_fulltext_fetch, truncate_for_notif};

pub(super) fn reader_pane_focused(app: &App) -> bool {
  if app.reader_popup_active {
    return true;
  }
  matches!(app.focused_pane, PaneId::Reader | PaneId::SecondaryReader)
    && app.reader_active
}

pub(super) fn handle_reader_bottom_pane(key: KeyEvent, app: &mut App) {
  if app.search_active {
    match key.code {
      KeyCode::Esc => {
        app.search_active = false;
        app.clear_search_query();
        app.reader_feed_popup_selected = 0;
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
      app.clear_search_query();
      app.reader_feed_popup_selected = 0;
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
        let item = app.visible_get(idx).cloned();
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

pub(super) fn handle_reader_pane(key: KeyEvent, app: &mut App) -> bool {
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


pub(super) fn reader_back(app: &mut App, side: FocusedReader) -> bool {
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

pub(super) fn close_all_readers(app: &mut App) {
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
