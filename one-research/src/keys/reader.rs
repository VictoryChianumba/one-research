use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use super::super::{spawn_ai_discovery, truncate_for_notif};
use super::remember_fulltext_paper_context;
use crate::app::{App, FeedTab, FocusedReader, PaneId};

pub(super) fn reader_pane_focused(app: &App) -> bool {
  if app.reader_popup.active {
    return true;
  }
  matches!(app.focus.focused_pane, PaneId::Reader | PaneId::SecondaryReader)
    && app.reader.active
}

/// Keys that typically arrive in rapid succession via OS key-repeat
/// (j/k/h/l, arrows, page-up/down, half-page Ctrl+d/u, word motions).
/// These are the ones that flood `a=T` re-transmits during scroll on
/// non-cache hosts — hence the burst-skip gate.  Jumps (`gg`, `G`, `H`/
/// `M`/`L`, `{`/`}`) fire once per keystroke and don't need marking.
pub(super) fn is_scroll_key(key: KeyEvent) -> bool {
  match key.code {
    KeyCode::Char('j')
    | KeyCode::Char('k')
    | KeyCode::Char('h')
    | KeyCode::Char('l')
    | KeyCode::Char('w')
    | KeyCode::Char('b')
    | KeyCode::Char('e')
    | KeyCode::Down
    | KeyCode::Up
    | KeyCode::Left
    | KeyCode::Right
    | KeyCode::PageDown
    | KeyCode::PageUp => true,
    KeyCode::Char('d') | KeyCode::Char('u') => {
      key.modifiers.contains(KeyModifiers::CONTROL)
    }
    _ => false,
  }
}

pub(super) fn handle_reader_bottom_pane(key: KeyEvent, app: &mut App) {
  if app.feed.search_active {
    match key.code {
      KeyCode::Esc => {
        app.feed.search_active = false;
        app.clear_search_query();
        app.reader_bottom.feed_popup_selected = 0;
      }
      KeyCode::Enter => {
        app.feed.search_active = false;
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
      let count = reader_feed_count(app);
      if count > 0 {
        app.reader_bottom.feed_popup_selected =
          (app.reader_bottom.feed_popup_selected + 1).min(count - 1);
        // In details mode the structured detail tracks the selection, which
        // `draw_item_detail` reads from the main feed — keep it in sync.
        if app.reader_bottom.details {
          app.set_active_selected_index(app.reader_bottom.feed_popup_selected);
        }
      }
    }
    KeyCode::Char('k') | KeyCode::Up => {
      app.reader_bottom.feed_popup_selected =
        app.reader_bottom.feed_popup_selected.saturating_sub(1);
      if app.reader_bottom.details {
        app.set_active_selected_index(app.reader_bottom.feed_popup_selected);
      }
    }
    KeyCode::Char('d') => {
      app.reader_bottom.details = !app.reader_bottom.details;
      app.reader_bottom.scroll.reset();
      // Sync so `draw_item_detail` shows the drawer's selected paper, not the
      // stale main-feed selection.
      if app.reader_bottom.details {
        app.set_active_selected_index(app.reader_bottom.feed_popup_selected);
      }
    }
    KeyCode::Char(' ') => {
      // Quick-view the abstract — uniform with the main / reader feed. Sync the
      // main feed selection to the drawer's first so the abstract (and a later
      // feed return) lands on the paper you're looking at.
      if !app.reader_bottom.details {
        app.set_active_selected_index(app.reader_bottom.feed_popup_selected);
        if app.selected_item().is_some() {
          app.view_flags.abstract_popup_active = true;
        }
      }
    }
    KeyCode::Char('/') => {
      app.feed.search_active = true;
      app.clear_search_query();
      app.reader_bottom.feed_popup_selected = 0;
    }
    KeyCode::Tab => {
      app.feed.feed_tab = match app.feed.feed_tab {
        FeedTab::Inbox => FeedTab::Browse,
        FeedTab::Browse => FeedTab::Library,
        FeedTab::Library => FeedTab::Discoveries,
        FeedTab::Discoveries => FeedTab::History,
        FeedTab::History => FeedTab::Inbox,
      };
      app.reset_active_feed_position();
    }
    KeyCode::BackTab => {
      app.feed.feed_tab = match app.feed.feed_tab {
        FeedTab::Inbox => FeedTab::History,
        FeedTab::Browse => FeedTab::Inbox,
        FeedTab::Library => FeedTab::Browse,
        FeedTab::Discoveries => FeedTab::Library,
        FeedTab::History => FeedTab::Discoveries,
      };
      app.reset_active_feed_position();
    }
    KeyCode::Enter => {
      if !app.reader_bottom.details && !app.async_jobs.fulltext_loading {
        let idx = app.reader_bottom.feed_popup_selected;
        if app.feed.feed_tab == FeedTab::History {
          let entry = app.history_get(idx).cloned();
          if let Some(entry) = entry {
            match entry.kind {
              crate::history::HistoryKind::Paper => {
                if let Some(item) = app.history_item(&entry) {
                  let _ = app.activate_history_item_target(&entry);
                  let target = if app.reader.focused == FocusedReader::Secondary
                  {
                    crate::action::ReaderTarget::Secondary
                  } else {
                    crate::action::ReaderTarget::Primary
                  };
                  remember_fulltext_paper_context(app, &item);
                  app.set_notification(format!(
                    "Fetching: {}…",
                    truncate_for_notif(&item.title, 40)
                  ));
                  super::spawn_paper_open(
                    app,
                    item,
                    target,
                    crate::action::OpenMode::ReplaceActive,
                  );
                  app.reader_bottom.focused = false;
                  app.focus.focused_pane = PaneId::Reader;
                }
              }
              crate::history::HistoryKind::Query => {
                let topic = entry.key.clone();
                let config = app.config.clone();
                app.discovery.force_new = true;
                app.feed.feed_tab = FeedTab::Discoveries;
                app.reset_active_feed_position();
                spawn_ai_discovery(topic, config, app);
              }
            }
          }
        } else if let Some(item) = app.visible_get(idx).cloned() {
          let target = if app.reader.focused == FocusedReader::Secondary {
            crate::action::ReaderTarget::Secondary
          } else {
            crate::action::ReaderTarget::Primary
          };
          remember_fulltext_paper_context(app, &item);
          app.set_notification(format!(
            "Fetching: {}…",
            truncate_for_notif(&item.title, 40)
          ));
          super::spawn_paper_open(
            app,
            item,
            target,
            crate::action::OpenMode::ReplaceActive,
          );
          app.reader_bottom.focused = false;
          app.focus.focused_pane = PaneId::Reader;
        }
      }
    }
    KeyCode::Esc => {
      if app.reader_bottom.details {
        app.reader_bottom.details = false;
        app.reader_bottom.scroll.reset();
      } else {
        app.reader_bottom.open = false;
        app.reader_bottom.focused = false;
        app.focus.focused_pane = PaneId::Reader;
      }
    }
    KeyCode::Char('q') => {
      app.reader_bottom.open = false;
      app.reader_bottom.focused = false;
      app.focus.focused_pane = PaneId::Reader;
    }
    _ => {}
  }
}

fn clamp_reader_feed_selection(app: &mut App) {
  let count = reader_feed_count(app);
  if count == 0 {
    app.reader_bottom.feed_popup_selected = 0;
  } else {
    app.reader_bottom.feed_popup_selected =
      app.reader_bottom.feed_popup_selected.min(count - 1);
  }
}

// TODO(reader-drawer-tabs): the reader-feed drawer should let users
// browse across feed tabs (Inbox, Library, Discoveries, History)
// with the same UX as the main feed view. Tab/BackTab here already
// cycles app.feed.feed_tab globally, but full parity is still missing:
// per-tab filters, Library workflow views, etc. Revisit
// when refactor B settles and we can hoist the tab-aware count/
// selection into the dispatcher cleanly.
fn reader_feed_count(app: &App) -> usize {
  if app.feed.feed_tab == FeedTab::History {
    app.history_count()
  } else {
    app.visible_count()
  }
}

pub(super) fn handle_reader_pane(key: KeyEvent, app: &mut App) -> bool {
  // Secondary reader (State 3, right pane).
  if app.reader.dual_active && app.focus.focused_pane == PaneId::SecondaryReader
  {
    log::debug!("routing to secondary reader pane");
    if key.code == KeyCode::Tab {
      if app.reader_bottom.open {
        app.reader_bottom.focused = true;
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
    if is_scroll_key(key)
      && let Some(tab) = app.reader_secondary_active_tab_mut()
    {
      tab.burst.note_event();
    }
    if let Some(reader) = app.reader_secondary_editor_mut() {
      let action = reader.handle_event(Event::Key(key));
      // q: close the current secondary tab; collapse to primary when empty.
      if matches!(action, tread::ReaderAction::Quit) {
        let pane_empty = app.reader_secondary_close_active_tab();
        if pane_empty {
          app.reader.dual_active = false;
          app.reader_bottom.open = false;
          app.reader_bottom.focused = false;
          app.notes.set_visible(FocusedReader::Secondary, false);
          app.reader.focused = FocusedReader::Primary;
          app.focus.focused_pane = PaneId::Reader;
        }
      }
    }
    return true;
  }

  if !(app.reader.active && app.focus.focused_pane == PaneId::Reader) {
    return false;
  }
  log::debug!("routing to reader pane");

  // Tab in primary reader during State 3 → focus secondary reader.
  if app.reader.dual_active && key.code == KeyCode::Tab {
    if !app.reader.secondary.tabs.is_empty() {
      app.focus.focused_pane = PaneId::SecondaryReader;
      app.reader.focused = FocusedReader::Secondary;
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

  if is_scroll_key(key)
    && let Some(tab) = app.reader_active_tab_mut()
  {
    tab.burst.note_event();
  }
  if let Some(reader) = app.reader_editor_mut() {
    let action = reader.handle_event(Event::Key(key));
    // q: close the current tab; apply state machine only when the pane goes empty.
    if matches!(action, tread::ReaderAction::Quit) {
      let pane_empty = app.reader_close_active_tab();
      if pane_empty {
        if app.reader.dual_active {
          // Primary ran out of tabs: promote secondary tabs to primary.
          app.reader.primary.tabs =
            std::mem::take(&mut app.reader.secondary.tabs);
          app.reader.primary.active_tab = app.reader.secondary.active_tab;
          app.reader.secondary.active_tab = 0;
          app.reader.active = !app.reader.primary.tabs.is_empty();
          app.reader.dual_active = false;
          app.reader_bottom.open = false;
          app.reader_bottom.focused = false;
          app.notes.collapse_secondary_into_primary();
          app.reader.focused = FocusedReader::Primary;
          app.focus.focused_pane =
            if app.reader.active { PaneId::Reader } else { PaneId::Feed };
        } else if app.reader.split_active {
          app.reader.split_active = false;
          app.focus.focused_pane = PaneId::Feed;
        } else {
          app.focus.focused_pane = PaneId::Feed;
        }
      }
    }
  }
  true
}

pub(super) fn reader_back(app: &mut App, side: FocusedReader) -> bool {
  if app.reader_bottom.open {
    app.reader_bottom.open = false;
    app.reader_bottom.focused = false;
    app.reader_bottom.details = false;
    app.focus.focused_pane = match side {
      FocusedReader::Primary => PaneId::Reader,
      FocusedReader::Secondary if app.reader.dual_active => {
        PaneId::SecondaryReader
      }
      FocusedReader::Secondary => PaneId::Reader,
    };
    app.reader.focused = side;
    return true;
  }

  if app.view_flags.narrow_feed_details_open {
    app.view_flags.narrow_feed_details_open = false;
    return true;
  }

  if app.reader.split_active {
    app.reader.split_active = false;
    app.focus.focused_pane = PaneId::Reader;
    app.reader.focused = FocusedReader::Primary;
    return true;
  }

  if app.reader.dual_active {
    match side {
      FocusedReader::Secondary => {
        app.reader.dual_active = false;
        app.reader.secondary.tabs.clear();
        app.reader.secondary.active_tab = 0;
        app.notes.set_visible(FocusedReader::Secondary, false);
        app.reader_bottom.open = false;
        app.reader_bottom.focused = false;
        app.reader.focused = FocusedReader::Primary;
        app.focus.focused_pane = PaneId::Reader;
      }
      FocusedReader::Primary => {
        app.reader.primary.tabs =
          std::mem::take(&mut app.reader.secondary.tabs);
        app.reader.primary.active_tab = app.reader.secondary.active_tab;
        app.reader.secondary.active_tab = 0;
        app.reader.active = !app.reader.primary.tabs.is_empty();
        app.reader.dual_active = false;
        app.reader_bottom.open = false;
        app.reader_bottom.focused = false;
        app.notes.collapse_secondary_into_primary();
        app.reader.focused = FocusedReader::Primary;
        app.focus.focused_pane =
          if app.reader.active { PaneId::Reader } else { PaneId::Feed };
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
  app.reader.active = false;
  app.reader.dual_active = false;
  app.reader.split_active = false;
  app.reader_bottom.open = false;
  app.reader_bottom.focused = false;
  app.reader.primary.tabs.clear();
  app.reader.primary.active_tab = 0;
  app.reader.secondary.tabs.clear();
  app.reader.secondary.active_tab = 0;
  app.notes.hide_all();
  app.reader.focused = FocusedReader::Primary;
  app.focus.focused_pane = PaneId::Feed;
}
