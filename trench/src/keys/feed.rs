use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc;

use super::super::{
  do_refresh, kbd_scroll_ok, open_url, spawn_ai_discovery,
  spawn_fulltext_fetch, spawn_repo_open, spawn_tread_fetch, truncate_for_notif,
};
use super::remember_fulltext_paper_context;
use crate::app::{App, AppView, FeedTab, PaneId, RepoContext, RepoPane};
use crate::models::WorkflowState;

pub(super) fn handle_feed_view(key: KeyEvent, app: &mut App) {
  // Discoveries tab — search bar input (when focused).
  if app.feed.feed_tab == FeedTab::Discoveries
    && app.feed.discovery.search_focused
  {
    let palette_active = app.feed.discovery.query.starts_with('/');
    match key.code {
      KeyCode::Esc => {
        app.feed.discovery.search_focused = false;
        app.feed.discovery.palette.reset();
      }
      KeyCode::Up if palette_active => {
        // Mirror the prior hardcoded `visible=8` until layout starts
        // pushing viewport size into the palette state (Phase 4).
        app.feed.discovery.palette.set_viewport(8);
        app.feed.discovery.palette.move_up();
      }
      KeyCode::Down if palette_active => {
        let count = discovery_palette_count(&app.feed.discovery.query);
        app.feed.discovery.palette.set_viewport(8);
        app.feed.discovery.palette.set_count(count);
        app.feed.discovery.palette.move_down();
      }
      KeyCode::Tab if palette_active => {
        // Complete selected command into the input.
        if let Some(completion) = discovery_palette_completion(
          &app.feed.discovery.query,
          app.feed.discovery.palette.selected(),
        ) {
          app.set_discovery_query(completion);
          app.feed.discovery.palette.reset();
        }
      }
      KeyCode::Enter => {
        if !app.feed.discovery.query.is_empty() && !app.feed.discovery.loading {
          let query = app.feed.discovery.query.clone();
          app.feed.discovery.palette.reset();
          if query.starts_with('/') {
            app.feed.discovery.search_focused = false;
            app.clear_discovery_query();
            let cmd = crate::commands::parser::parse_slash_command(&query);
            crate::commands::dispatch::dispatch_slash_command(app, cmd);
          } else {
            let config = app.config.clone();
            spawn_ai_discovery(query, config, app);
          }
        }
      }
      KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
        app.feed.discovery.force_new = true;
        app.clear_discovery_query();
        app.feed.discovery.palette.reset();
      }
      KeyCode::Backspace => {
        app.pop_discovery_char();
        if !app.feed.discovery.query.starts_with('/') {
          app.feed.discovery.palette.reset();
        }
      }
      KeyCode::Char(c) => {
        app.push_discovery_char(c);
        app.feed.discovery.palette.reset();
      }
      _ => {}
    }
    return;
  }

  // Discoveries tab — any printable char focuses the search bar.
  if app.feed.feed_tab == FeedTab::Discoveries {
    if let KeyCode::Char(c) = key.code {
      if c != 'q' {
        app.feed.discovery.search_focused = true;
        app.push_discovery_char(c);
        return;
      }
    }
  }

  // History tab — handle filter cycling, navigation, reopen, delete.
  if app.feed.feed_tab == FeedTab::History {
    if handle_history_tab(key, app) {
      return;
    }
  }

  // Library tab — handle workflow-state chip cycling. Other keys (j/k, Enter,
  // i/r/w/x, etc.) fall through to the generic feed handler below.
  if app.feed.feed_tab == FeedTab::Library {
    if handle_library_tab(key, app) {
      return;
    }
  }

  if app.feed.search_active {
    match key.code {
      KeyCode::Esc => {
        app.feed.exit_search();
        app.clear_search_query();
        app.reset_active_feed_position();
      }
      KeyCode::Enter => {
        app.feed.exit_search();
      }
      KeyCode::Backspace => {
        app.pop_search_char();
      }
      KeyCode::Char(c) => {
        app.push_search_char(c);
      }
      _ => {}
    }
  } else if app.feed.filter_focus {
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
        app.feed.exit_filter_focus();
      }
      KeyCode::Esc => {
        app.clear_filters();
        app.feed.exit_filter_focus();
      }
      _ => {}
    }
  } else if app.focus.focused_pane == PaneId::Feed {
    // In State 2 the narrow feed holds focus — use a restricted key set so
    // main-feed bindings (Esc → quit, v → repo viewer) don't fire here.
    if app.reader.split_active {
      // Close description popup first if open.
      if app.narrow_feed_details_open {
        match key.code {
          KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('d') => {
            app.narrow_feed_details_open = false;
          }
          KeyCode::Char('j') | KeyCode::Down => {
            app.details_scroll.scroll_down(1);
          }
          KeyCode::Char('k') | KeyCode::Up => {
            app.details_scroll.scroll_up(1);
          }
          _ => {}
        }
        return;
      }
      match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
          app.reader.split_active = false;
          app.narrow_feed_details_open = false;
          app.focus.focused_pane = PaneId::Reader;
        }
        KeyCode::Char('d') => {
          app.narrow_feed_details_open = true;
          app.details_scroll.reset();
        }
        KeyCode::Char('/') => {
          app.feed.enter_search();
          app.clear_search_query();
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
              app.narrow_feed_details_open = false;
              remember_fulltext_paper_context(app, &item);
              app.set_notification(format!(
                "Fetching: {}…",
                truncate_for_notif(&item.title, 40)
              ));
              app.focus.focused_pane = PaneId::Reader;
              super::spawn_paper_open(
                app,
                item,
                crate::action::ReaderTarget::Primary,
                crate::action::OpenMode::ReplaceActive,
              );
            }
          }
        }
        _ => {}
      }
      return;
    }

    match key.code {
      KeyCode::Tab => {
        app.feed.cycle_tab();
        app.reset_active_feed_position();
      }
      KeyCode::BackTab => {
        app.feed.cycle_tab_back();
        app.reset_active_feed_position();
      }
      KeyCode::Char('f') => {
        app.feed.enter_filter_focus();
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
            log::debug!("feed Enter: spawning fetch for url={}", item.url);
            let t = std::time::Instant::now();
            app.set_notification(format!(
              "Fetching: {}…",
              truncate_for_notif(&item.title, 40)
            ));
            super::spawn_paper_open(
              app,
              item,
              crate::action::ReaderTarget::Primary,
              crate::action::OpenMode::ReplaceActive,
            );
            log::debug!("feed Enter: fetch setup took {}µs", t.elapsed().as_micros());
          }
        }
      }
      KeyCode::Char('/') => {
        app.feed.enter_search();
        app.clear_search_query();
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

/// Returns true if the key was consumed by the Library tab (chip cycling).
/// Anything else falls through to the generic feed handler so navigation,
/// Enter, and `i/r/w/x` state transitions work as usual.
fn handle_library_tab(key: KeyEvent, app: &mut App) -> bool {
  // Visual-mode-only handlers fire before the generic chip ones so j/k extend
  // selection rather than just moving the cursor.
  if app.feed.library_visual_mode {
    match key.code {
      KeyCode::Esc => {
        app.library_exit_visual();
        return true;
      }
      KeyCode::Char('j') | KeyCode::Down => {
        let len = app.visible_count();
        if len > 0 {
          let next = (app.feed.library_list.selected() + 1).min(len - 1);
          app.library_extend_selection(next);
        }
        return true;
      }
      KeyCode::Char('k') | KeyCode::Up => {
        let next = app.feed.library_list.selected().saturating_sub(1);
        app.library_extend_selection(next);
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
          app.feed.library_selected_urls.iter().cloned().collect();
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
      app.mutate_library_filter(|f| *f = f.next());
      app.feed.library_list.reset();
      true
    }
    KeyCode::Char('[') => {
      app.mutate_library_filter(|f| *f = f.prev());
      app.feed.library_list.reset();
      true
    }
    KeyCode::Char('V') => {
      // Capital V = visual-line mode (Vim convention). Lowercase v
      // remains globally bound to "open repo viewer for selected item"
      // in the generic feed handler at handle_feed_view.
      app.feed.enter_library_visual_mode();
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

/// Returns true if the key was handled by the History tab and the caller should
/// stop propagation. False means fall through to the generic feed handler.
fn handle_history_tab(key: KeyEvent, app: &mut App) -> bool {
  use crate::history::{HistoryFilter, HistoryKind};
  match key.code {
    KeyCode::Char(']') => {
      app.mutate_history_filter(|f| *f = f.next());
      app.feed.history_list.reset();
      true
    }
    KeyCode::Char('[') => {
      app.mutate_history_filter(|f| *f = f.prev());
      app.feed.history_list.reset();
      true
    }
    KeyCode::Char('j') | KeyCode::Down => {
      let len = app.filtered_history().len();
      if len > 0 {
        let next = (app.feed.history_list.selected() + 1).min(len - 1);
        app.set_active_selected_index(next);
      }
      true
    }
    KeyCode::Char('k') | KeyCode::Up => {
      let next = app.feed.history_list.selected().saturating_sub(1);
      app.set_active_selected_index(next);
      true
    }
    KeyCode::Char('g') => {
      app.set_active_selected_index(0);
      true
    }
    KeyCode::Char('G') => {
      let len = app.filtered_history().len();
      if len > 0 {
        app.set_active_selected_index(len - 1);
      }
      true
    }
    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
      let visible = app.filtered_history();
      let Some(target) = visible.get(app.feed.history_list.selected()).cloned()
      else {
        return true;
      };
      let key_to_delete = (target.kind, target.key.clone());
      app.mutate_history(|h| {
        h.retain(|e| (e.kind, e.key.clone()) != key_to_delete)
      });
      crate::store::history::save(&app.workspace.history);
      let len = app.filtered_history().len();
      if len > 0 && app.feed.history_list.selected() >= len {
        app.set_active_selected_index(len - 1);
      }
      true
    }
    KeyCode::Char('o') => {
      let visible = app.filtered_history();
      let Some(entry) =
        visible.get(app.feed.history_list.selected()).map(|e| (*e).clone())
      else {
        return true;
      };
      if entry.kind == HistoryKind::Paper {
        open_url(&entry.key);
        app.notification.message = Some(format!(
          "Opened in browser: {}",
          truncate_for_notif(&entry.title, 40)
        ));
        app.notification.item_id = Some(entry.key.clone());
      }
      true
    }
    KeyCode::Enter => {
      let Some(entry) =
        app.history_get(app.feed.history_list.selected()).cloned()
      else {
        return true;
      };
      match entry.kind {
        HistoryKind::Paper => {
          if let Some(item) = app.history_item(&entry) {
            let _ = app.activate_history_item_target(&entry);
            remember_fulltext_paper_context(app, &item);
            app.set_notification(format!(
              "Fetching: {}…",
              truncate_for_notif(&item.title, 40)
            ));
            super::spawn_paper_open(
              app,
              item,
              crate::action::ReaderTarget::Primary,
              crate::action::OpenMode::ReplaceActive,
            );
          }
        }
        HistoryKind::Query => {
          let topic = entry.key.clone();
          let config = app.config.clone();
          // Re-running a query starts a fresh discovery session.
          app.feed.discovery.force_new = true;
          app.feed.set_tab(FeedTab::Discoveries);
          app.reset_active_feed_position();
          spawn_ai_discovery(topic, config, app);
        }
      }
      let _ = HistoryFilter::All;
      true
    }
    _ => false,
  }
}

fn discovery_palette_filtered(
  query: &str,
) -> Vec<&'static chat::ChatSlashCommandSpec> {
  let all = crate::commands::registry::discovery_slash_specs();
  let q = query.to_lowercase();
  all.iter().filter(|s| q == "/" || s.command.starts_with(q.as_str())).collect()
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
    .map(|s| s.completion.clone())
}
