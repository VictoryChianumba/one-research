mod action;
mod app;
mod commands;
mod config;
mod discovery;
mod effect;
mod export;
mod github;
mod history;
// Re-export the workspace http crate so every existing `crate::http::client()`
// call site continues to compile unchanged. The real impl lives at
// `crates/http/`; chat providers + discovery + main feed-discovery now
// share the same hardened client (timeout, redirect cap, UA).
use trench_http as http;
mod ingestion;
mod keys;
mod library;
mod models;
mod primitives;
mod sanitize;
mod services;
mod store;
mod surfaces;
mod syntax;
mod tags;
pub mod theme;
mod ui;
mod workflows;
use services::{
  spawn_ai_discovery, spawn_discovery, spawn_fetch, spawn_fulltext_fetch,
  spawn_repo_dir, spawn_repo_file, spawn_repo_open,
};

use app::{App, FocusedReader, PaneId, RepoFetchResult};
use crossterm::{
  event::{
    self, DisableFocusChange, DisableMouseCapture, EnableFocusChange,
    EnableMouseCapture, Event, KeyEventKind, KeyboardEnhancementFlags,
    MouseButton, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
  },
  execute,
  terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode,
  },
};
use ingestion::message::FetchMessage;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::sync::mpsc;

/// Allowlist for URL schemes handed to the OS opener — `xdg-open` / `open`
/// will dispatch any registered scheme (including `javascript:`,
/// `vscode://`, `mailto:`). Restricting to http(s) blocks the local-handler
/// attack surface; http is kept because some legitimate paper hosts are
/// http-only. Case-insensitive per RFC 3986 §3.1 — RSS feeds in the wild
/// ship `Https://` URLs without normalization.
pub(crate) fn is_safe_url_scheme(url: &str) -> bool {
  let lower = url.to_ascii_lowercase();
  lower.starts_with("https://") || lower.starts_with("http://")
}

pub(crate) fn open_url(url: &str) {
  if !is_safe_url_scheme(url) {
    log::warn!("open_url: rejecting non-http(s) scheme: {url}");
    return;
  }
  #[cfg(target_os = "macos")]
  let _ = std::process::Command::new("open").arg(url).spawn();
  #[cfg(not(target_os = "macos"))]
  let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

/// Extract a human-readable message from a panic payload returned by
/// `std::panic::catch_unwind`. Used by every `spawn_*` helper below so that
/// thread panics surface to the UI as a routed error rather than a silent
/// thread death + forever-spinner. Idiomatic for both `panic!("...")` (which
/// boxes a `&'static str`) and `panic!("{}", x)` (which boxes a `String`).
pub(crate) fn panic_msg(payload: Box<dyn std::any::Any + Send>) -> String {
  if let Some(s) = payload.downcast_ref::<&'static str>() {
    (*s).to_string()
  } else if let Some(s) = payload.downcast_ref::<String>() {
    s.clone()
  } else {
    "thread panicked (non-string payload)".to_string()
  }
}

#[cfg(test)]
mod panic_msg_tests {
  use super::panic_msg;

  #[test]
  fn extracts_static_str_payload() {
    let result = std::panic::catch_unwind(|| panic!("static literal"));
    assert_eq!(panic_msg(result.unwrap_err()), "static literal");
  }

  #[test]
  fn extracts_owned_string_payload() {
    let result = std::panic::catch_unwind(|| {
      let n: i32 = 42;
      panic!("formatted message: {n}")
    });
    assert_eq!(panic_msg(result.unwrap_err()), "formatted message: 42");
  }

  #[test]
  fn falls_back_for_non_string_payload() {
    let result = std::panic::catch_unwind(|| {
      // Payload is a non-string type (a struct).
      std::panic::panic_any(42i32)
    });
    assert_eq!(
      panic_msg(result.unwrap_err()),
      "thread panicked (non-string payload)"
    );
  }

  #[test]
  fn channel_routing_via_catch_unwind_smoke() {
    // Smoke test for the spawn_*-pattern: a thread that panics must surface
    // via the cloned sender rather than dying silently.
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel::<Result<i32, String>>();

    std::thread::spawn(move || {
      let tx_panic = tx.clone();
      let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
          panic!("simulated worker failure");
        }));
      // The closure body is unconditional `panic!`, so the closure's return
      // type is `!` and `outcome` is provably `Err(_)` — let-else makes that
      // explicit and avoids an irrefutable-pattern lint.
      let Err(payload) = outcome else { unreachable!() };
      let msg = panic_msg(payload);
      let _ = tx_panic.send(Err(format!("panicked: {msg}")));
    })
    .join()
    .ok();

    let received = rx
      .recv_timeout(std::time::Duration::from_secs(1))
      .expect("receiver should get the panic-routed Err");
    match received {
      Err(s) => assert!(s.contains("simulated worker failure"), "got: {s}"),
      Ok(n) => panic!("expected Err, got Ok({n})"),
    }
  }
}

#[cfg(test)]
mod url_scheme_tests {
  use super::is_safe_url_scheme;

  #[test]
  fn accepts_https_and_http() {
    assert!(is_safe_url_scheme("https://arxiv.org/abs/2603.00001"));
    assert!(is_safe_url_scheme("https://github.com/owner/repo"));
    assert!(is_safe_url_scheme("http://legacy.example.edu/proceedings"));
  }

  #[test]
  fn rejects_local_handler_schemes() {
    // These would dispatch to whatever handler the user has registered.
    assert!(!is_safe_url_scheme("javascript:alert(1)"));
    assert!(!is_safe_url_scheme("file:///etc/passwd"));
    assert!(!is_safe_url_scheme("vscode://settings"));
    assert!(!is_safe_url_scheme("slack://channel?id=abc"));
    assert!(!is_safe_url_scheme("mailto:user@example.com"));
  }

  #[test]
  fn rejects_empty_and_scheme_only() {
    assert!(!is_safe_url_scheme(""));
    assert!(!is_safe_url_scheme("https"));
    assert!(!is_safe_url_scheme("http"));
  }

  #[test]
  fn accepts_mixed_case_schemes() {
    // Per RFC 3986 §3.1 schemes are case-insensitive; RSS feeds in the
    // wild do ship `Https://` URLs that we shouldn't silently refuse.
    assert!(is_safe_url_scheme("HTTPS://arxiv.org/abs/2603.00001"));
    assert!(is_safe_url_scheme("Http://example.com"));
    assert!(is_safe_url_scheme("hTTpS://github.com/owner/repo"));
  }
}

pub(crate) fn truncate_for_notif(s: &str, max: usize) -> String {
  let mut chars = s.chars();
  let mut out = String::new();
  let mut n = 0;
  for c in &mut chars {
    if n >= max {
      if chars.next().is_some() {
        out.push('…');
      }
      break;
    }
    out.push(c);
    n += 1;
  }
  out
}


// ── Refresh helper ────────────────────────────────────────────────────────

/// Built-in source names that produce loading_sources entries on every
/// fetch cycle. Covers what the user sees in the loading spinner. Kept in
/// lockstep with the dispatch logic in `spawn_fetch` (arxiv/hf/openreview/core
/// have specialized fetch paths; the rest go through spawn_fetch's rss_feeds
/// loop). Adding a new built-in source here without wiring spawn_fetch (or
/// vice versa) leaves the spinner showing phantom or missing sources.
const BUILTIN_LOADING_SOURCES: &[&str] = &[
  "arxiv",
  "huggingface",
  "openreview",
  "core",
  "openai",
  "deepmind",
  "import_ai",
  "bair",
  "mit_news_ai",
  "enriching",
];

/// Build the loading_sources list shown in the spinner. Single source of
/// truth shared by `do_refresh` and the startup fetch in `main`.
fn build_loading_sources(custom_feeds: &[config::CustomFeed]) -> Vec<String> {
  let mut out: Vec<String> =
    BUILTIN_LOADING_SOURCES.iter().map(|s| s.to_string()).collect();
  for feed in custom_feeds {
    out.push(feed.name.clone());
  }
  out
}

/// Spawn a fresh fetch cycle and attach the receiver to `app`.
pub(crate) fn do_refresh(app: &mut App) {
  if app.is_loading || app.is_refreshing {
    return;
  }
  let (tx, rx) = mpsc::channel::<FetchMessage>();
  app.fetch_rx = Some(rx);
  app.loading_sources = build_loading_sources(&app.config.sources.custom_feeds);
  app.is_loading = true;
  app.is_refreshing = true;
  spawn_fetch(tx, app.config.clone());
}


/// Like do_refresh, but always runs — reloads config from disk, abandons any
/// in-flight fetch, clears the item cache, then starts a fresh fetch.
pub(crate) fn force_refresh(app: &mut App) {
  app.config = config::Config::load();
  app.is_loading = false;
  app.is_refreshing = false;
  app.fetch_rx = None;
  app.reset_items();
  do_refresh(app);
}

/// Returns true if enough time has elapsed since the last keyboard scroll.
/// Updates `last_scroll_time` on success.
pub(crate) fn kbd_scroll_ok(app: &mut app::App) -> bool {
  let now = std::time::Instant::now();
  if let Some(last) = app.last_scroll_time {
    if last.elapsed().as_millis() < app.scroll_debounce_ms as u128 {
      return false;
    }
  }
  app.last_scroll_time = Some(now);
  true
}

/// Returns true if enough time has elapsed since the last mouse scroll.
/// Uses a higher debounce threshold to tame trackpad inertia.
fn mouse_scroll_ok(app: &mut app::App) -> bool {
  let now = std::time::Instant::now();
  if let Some(last) = app.last_mouse_scroll_time {
    if last.elapsed().as_millis() < app.mouse_scroll_debounce_ms as u128 {
      return false;
    }
  }
  app.last_mouse_scroll_time = Some(now);
  true
}

fn handle_mouse(
  mouse: crossterm::event::MouseEvent,
  app: &mut app::App,
  terminal: &ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) {
  if app.view != app::AppView::Feed {
    return;
  }
  let Ok(size) = terminal.size() else { return };

  // Geometry for scrollbar hit-test.
  let scrollbar_col = size.width.saturating_sub(ui::RIGHT_COL_WIDTH + 1);
  let track_top = 4u16;
  let track_bottom = size.height.saturating_sub(6);

  // Which pane is the cursor in right now?
  let hovered = app.focus.pane_at(mouse.column, mouse.row);

  match mouse.kind {
    // ── Scroll wheel / trackpad ────────────────────────────────────────────
    MouseEventKind::ScrollDown => {
      if mouse_scroll_ok(app) {
        match hovered {
          Some(PaneId::Details) => {}
          Some(PaneId::Notes) => {
            if let Some(note_id) = app
              .notes_tabs
              .get(app.notes_active_tab)
              .map(|t| t.note_id.clone())
            {
              if let Some(notes_app) = app.notes_app.as_mut() {
                notes_app.focus_note(&note_id);
              }
            }
            if let Some(notes_app) = app.notes_app.as_mut() {
              notes_app.select_next_note();
            }
          }
          Some(PaneId::SecondaryNotes) => {
            if let Some(note_id) = app
              .secondary_notes_tabs
              .get(app.secondary_notes_active_tab)
              .map(|t| t.note_id.clone())
            {
              if let Some(notes_app) = app.notes_app.as_mut() {
                notes_app.focus_note(&note_id);
              }
            }
            if let Some(notes_app) = app.notes_app.as_mut() {
              notes_app.select_next_note();
            }
          }
          Some(PaneId::Chat) => {
            if let Some(chat_ui) = app.chat.ui.as_mut() {
              chat_ui.scroll_offset = chat_ui.scroll_offset.saturating_add(3);
            }
          }
          _ => {
            if app.filter_focus {
              app.filter_cursor_down();
            } else {
              app.move_down();
            }
          }
        }
      }
    }
    MouseEventKind::ScrollUp => {
      if mouse_scroll_ok(app) {
        match hovered {
          Some(PaneId::Details) => {}
          Some(PaneId::Notes) => {
            if let Some(note_id) = app
              .notes_tabs
              .get(app.notes_active_tab)
              .map(|t| t.note_id.clone())
            {
              if let Some(notes_app) = app.notes_app.as_mut() {
                notes_app.focus_note(&note_id);
              }
            }
            if let Some(notes_app) = app.notes_app.as_mut() {
              notes_app.select_prev_note();
            }
          }
          Some(PaneId::SecondaryNotes) => {
            if let Some(note_id) = app
              .secondary_notes_tabs
              .get(app.secondary_notes_active_tab)
              .map(|t| t.note_id.clone())
            {
              if let Some(notes_app) = app.notes_app.as_mut() {
                notes_app.focus_note(&note_id);
              }
            }
            if let Some(notes_app) = app.notes_app.as_mut() {
              notes_app.select_prev_note();
            }
          }
          Some(PaneId::Chat) => {
            if let Some(chat_ui) = app.chat.ui.as_mut() {
              chat_ui.scroll_offset = chat_ui.scroll_offset.saturating_sub(3);
            }
          }
          _ => {
            if app.filter_focus {
              app.filter_cursor_up();
            } else {
              app.move_up();
            }
          }
        }
      }
    }
    // ── Left click ─────────────────────────────────────────────────────────
    MouseEventKind::Down(MouseButton::Left) => {
      // Scrollbar track click (feed list jump) — handled before pane focus.
      if mouse.column == scrollbar_col
        && mouse.row >= track_top
        && mouse.row < track_bottom
        && hovered == Some(PaneId::Feed)
      {
        let track_height = (track_bottom - track_top) as usize;
        let click_offset = (mouse.row - track_top) as usize;
        let total = app.visible_count();
        if total > 0 && track_height > 0 {
          let new_index =
            ((click_offset * total) / track_height).min(total - 1);
          app.set_active_selected_index(new_index);
        }
        return;
      }

      // Click any focusable open pane → focus it.
      if let Some(pane) = app.focus.focusable_pane_at(mouse.column, mouse.row) {
        app.focus.focused_pane = pane;
        match pane {
          PaneId::Reader | PaneId::Notes => {
            app.focused_reader = FocusedReader::Primary;
            if pane == PaneId::Notes {
              if let Some(note_id) = app
                .notes_tabs
                .get(app.notes_active_tab)
                .map(|t| t.note_id.clone())
              {
                if let Some(notes_app) = app.notes_app.as_mut() {
                  notes_app.focus_note(&note_id);
                }
              }
            }
          }
          PaneId::SecondaryReader | PaneId::SecondaryNotes => {
            app.focused_reader = FocusedReader::Secondary;
            if pane == PaneId::SecondaryNotes {
              if let Some(note_id) = app
                .secondary_notes_tabs
                .get(app.secondary_notes_active_tab)
                .map(|t| t.note_id.clone())
              {
                if let Some(notes_app) = app.notes_app.as_mut() {
                  notes_app.focus_note(&note_id);
                }
              }
            }
          }
          _ => {}
        }
        if matches!(pane, PaneId::Feed) {
          app.filter_focus = false;
        }
      }
    }
    _ => {}
  }
}


// ─────────────────────────────────────────────────────────────────────────────

/// `0` = primary pane (Reader if active, else Feed).
/// `1`/`2`/`3` = secondary open panes sorted top-to-bottom, left-to-right.
pub(crate) fn get_pane_by_number(n: u8, app: &App) -> Option<PaneId> {
  match n {
    0 => Some(if app.reader_active { PaneId::Reader } else { PaneId::Feed }),
    1..=3 => app
      .focus
      .secondary_panes_sorted(app.reader_active)
      .into_iter()
      .nth((n - 1) as usize),
    _ => None,
  }
}

fn migrate_legacy_config_dir() {
  let Some(home) = dirs::home_dir() else {
    return;
  };

  let old_root = home.join(".config/tentative");
  if !old_root.exists() {
    return;
  }

  let new_root = home.join(".config/trench");
  if let Err(e) = std::fs::create_dir_all(&new_root) {
    eprintln!("trench: could not prepare config dir ({e}); continuing");
    return;
  }

  for name in [
    "config.json",
    "state.json",
    "cache.json",
    "enrichment_cache.json",
    "discovery_cache.json",
    "trench.log",
    "hf_repo_cache.json",
    "chats",
    "notes",
  ] {
    let old_path = old_root.join(name);
    let new_path = new_root.join(name);
    if !old_path.exists() || new_path.exists() {
      continue;
    }
    // Reject pre-planted symlinks. If a hostile process briefly had write
    // access to ~/.config/tentative/, planting `state.json` as a symlink
    // pointing to a victim file would let our rename move that file
    // unexpectedly. Skip symlinks; the legacy dir is ephemeral and a
    // subsequent launch can retry.
    match std::fs::symlink_metadata(&old_path) {
      Ok(m) if m.file_type().is_symlink() => {
        eprintln!(
          "trench: refusing to migrate symlink at {}; skipping",
          old_path.display()
        );
        continue;
      }
      _ => {}
    }
    if let Err(e) = std::fs::rename(&old_path, &new_path) {
      eprintln!(
        "trench: could not migrate {} to new config dir ({e}); continuing",
        old_path.display()
      );
    }
  }

  let old_root_empty = std::fs::read_dir(&old_root)
    .map(|mut entries| entries.next().is_none())
    .unwrap_or(false);
  if old_root_empty {
    let _ = std::fs::remove_dir(&old_root);
  }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let startup_t0 = std::time::Instant::now();
  migrate_legacy_config_dir();

  let log_level = if std::env::var_os("TRENCH_DEBUG_LOG").is_some() {
    log::LevelFilter::Debug
  } else {
    log::LevelFilter::Info
  };
  let log_file = dirs::home_dir().and_then(|home| {
    let path = home.join(".config/trench/trench.log");
    std::fs::create_dir_all(path.parent()?).ok()?;
    // Rotate on startup: move existing trench.log → trench.log.1 so the
    // prior session's diagnostics survive a crash investigation. Was
    // `truncate(true)` which wiped the log before the user could read it
    // (audit Rel MED #15). Bounded growth: at most 2 files at a time.
    let rotated = path.with_extension("log.1");
    let _ = std::fs::rename(&path, &rotated);
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).write(true).truncate(true);
    // Owner-read/write only on Unix. Default umask leaves trench.log
    // world-readable, which leaks API error messages and partial system
    // prompt fragments to other local users (audit Sec MED #14).
    #[cfg(unix)]
    {
      use std::os::unix::fs::OpenOptionsExt;
      opts.mode(0o600);
    }
    opts.open(&path).ok()
  });

  // Surface a startup warning on Windows so users on multi-user systems
  // know that cache.json, history, chat sessions, notes, and trench.log
  // are written with default ACLs (typically world-readable). The
  // Unix-only set_private 0o600 step is a no-op on Windows; a real DACL
  // fix via windows-sys is tracked separately (audit Sec MED #8).
  #[cfg(windows)]
  log::warn!(
    "trench on Windows: data files use default ACLs and may be \
     readable by other local users. set_private is a no-op on Windows."
  );
  match log_file {
    Some(f) => {
      env_logger::Builder::new()
        .target(env_logger::Target::Pipe(Box::new(f)))
        .filter_level(log_level)
        .init();
    }
    None => {
      env_logger::Builder::new().filter_level(log::LevelFilter::Off).init();
    }
  }

  // Install a panic hook that restores the terminal before printing the
  // backtrace.  Without this, a panic mid-run leaves the user stuck in
  // alt-screen / raw mode with no visible cursor.  Best-effort — every
  // step ignores its own errors so a partial failure (e.g. stderr
  // already closed) doesn't prevent the rest from running and doesn't
  // double-panic.
  {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
      // Pop the kitty-keyboard flags + focus-change BEFORE leaving the alt
      // screen, otherwise the responses to those sequences leak into the
      // user's shell after the panic and corrupt input until manual `reset`
      // (audit Rel CRIT C3).
      let _ = crossterm::execute!(
        std::io::stderr(),
        crossterm::event::PopKeyboardEnhancementFlags,
        crossterm::event::DisableFocusChange,
      );
      let _ = crossterm::terminal::disable_raw_mode();
      let _ = crossterm::execute!(
        std::io::stderr(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show,
      );
      // Blank line before the panic message so it doesn't collide
      // with any partial frame the alt-screen leave just flushed.
      eprintln!();
      default_hook(info);
    }));
  }

  enable_raw_mode()?;
  let mut stdout = io::stdout();
  // EnableFocusChange so the (eventual) embedded reader can detect tmux
  // pane switches and clear pixel-image placements before they bleed
  // across panes.  No effect on the feed UI — it ignores focus events.
  // DISAMBIGUATE_ESCAPE_CODES so Shift+Enter and other modified specials
  // are distinguishable from plain Enter — needed by tread for the
  // citation-popup binding (`Shift+Enter` vs `Enter` for jump-to-link).
  // Trench's existing keys.rs already uses `KeyCode::Enter` (not
  // `Char('\n')`) at every Enter site, so this flag is a behaviour-
  // preserving addition for the feed UI.  Terminals that don't speak
  // the kitty keyboard protocol silently ignore the push.
  execute!(
    stdout,
    EnterAlternateScreen,
    EnableMouseCapture,
    EnableFocusChange,
  )?;
  let _ = execute!(
    stdout,
    PushKeyboardEnhancementFlags(
      KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
    ),
  );
  let backend = CrosstermBackend::new(stdout);
  let mut terminal = Terminal::new(backend)?;

  let mut app = App::new();
  log::debug!("startup: App::new {}ms", startup_t0.elapsed().as_millis());

  // Load config.
  let t = std::time::Instant::now();
  let cfg = config::Config::load();
  app.github_token = cfg.github_token.clone();
  app.active_theme = cfg.theme;
  app.active_custom_theme_id = cfg.active_custom_theme_id.clone();
  app.config = cfg;
  app.reconcile_custom_theme_selection();
  log::debug!("startup: config load {}ms", t.elapsed().as_millis());

  // Load persisted workflow states and UI state.
  let t = std::time::Instant::now();
  app.persisted_states = store::load();
  let ui = store::load_ui();
  app.last_read = ui.last_read;
  app.last_read_source = ui.last_read_source;
  app.notes_tabs = ui.notes_tabs;
  // Clamp in case ui.json was written with a tab count that has since shrunk.
  app.notes_active_tab =
    ui.notes_active_tab.min(app.notes_tabs.len().saturating_sub(1));
  app.secondary_notes_tabs = ui.secondary_notes_tabs;
  app.secondary_notes_active_tab = ui
    .secondary_notes_active_tab
    .min(app.secondary_notes_tabs.len().saturating_sub(1));
  log::debug!("startup: state/ui load {}ms", t.elapsed().as_millis());

  // 1. Load cache immediately → populate app.items.
  let t = std::time::Instant::now();
  let cached = store::cache::load();
  if !cached.is_empty() {
    app.items = cached;
  }
  // Build url_index + arxiv_id_index over the loaded items so the dedup
  // hot path in process_incoming gets O(1) lookups from the very first
  // batch. Same for discovery_items, which were loaded in App::new.
  app.rebuild_indices();
  app.rebuild_discovery_indices();
  log::debug!(
    "startup: cache load + index rebuild {}ms ({} cached items)",
    t.elapsed().as_millis(),
    app.items.len()
  );

  // 2. Apply persisted states to cached items.
  let t = std::time::Instant::now();
  for item in &mut app.items {
    if let Some(state) = app.persisted_states.get(&item.url) {
      item.workflow_state = *state;
    }
  }
  for item in &mut app.discovery.items {
    if let Some(state) = app.persisted_states.get(&item.url) {
      item.workflow_state = *state;
    }
  }
  log::debug!("startup: persisted state apply {}ms", t.elapsed().as_millis());

  app.list_offset = 0;

  // 4. Spawn background thread to fetch all sources then enrich.
  {
    let (tx, rx) = mpsc::channel::<FetchMessage>();
    app.fetch_rx = Some(rx);
    app.loading_sources =
      build_loading_sources(&app.config.sources.custom_feeds);
    app.is_loading = true;
    spawn_fetch(tx, app.config.clone());
  }

  // 3. Start the TUI loop. Wrap the loop body in an inner closure so
  // cleanup at the end of `main` runs unconditionally — even if
  // `terminal.draw(...)?` returns Err mid-frame (TTY dropped, SSH
  // session died, etc.) we still execute disable_raw_mode +
  // LeaveAlternateScreen + flag-pop (audit Rel HIGH H7). The panic
  // hook covers panics; this closure covers Err returns.
  let mut first_draw_logged = false;
  let run_result: std::io::Result<()> = (|| -> std::io::Result<()> {
    loop {
    // Drain any pending fetch results before drawing. process_incoming +
    // process_incoming_discovery internally call mark_dirty when state
    // changes; the spinner increment is now gated on is_loading.
    app.process_incoming();

    // Tick the embedded reader(s) each frame so voice playback state
    // (active-word highlight, paragraph advance during continuous
    // reading) animates without waiting for a key event.  tread::tick
    // returns true when user-visible state changed; we OR that into
    // trench's dirty flag so the next frame redraws.
    if let Some(editor) = app.reader_editor_mut() {
      if editor.tick() {
        app.mark_dirty();
      }
    }
    if let Some(editor) = app.reader_secondary_editor_mut() {
      if editor.tick() {
        app.mark_dirty();
      }
    }
    if let Some(editor) = app.reader_popup_editor.as_mut() {
      if editor.tick() {
        app.mark_dirty();
      }
    }

    // Tick chat UI each frame (spinner + pending response channel + word-by-
    // word streaming reveal). When chat is streaming we want the next frame
    // to render — capture is_streaming BEFORE tick so the FINAL word still
    // triggers a redraw even though tick clears the flag on completion.
    if let Some(chat_ui) = app.chat.ui.as_mut() {
      let was_streaming = chat_ui.is_streaming;
      chat_ui.tick();
      if was_streaming || chat_ui.is_streaming {
        app.mark_dirty();
      }
    }

    // Tick repo viewer momentum scroll. If any repo context is decaying its
    // velocity, mark dirty so the next frame renders the new scroll offset.
    let was_repo_animating = app
      .repo_context
      .as_ref()
      .map(|c| c.scroll_velocity.abs() >= 0.5)
      .unwrap_or(false);
    app.repo_tick();
    if was_repo_animating {
      app.mark_dirty();
    }

    // ── Drain background fetch results ────────────────────────────────
    if let Some(rx) = app.fulltext_rx.as_ref() {
      let t = std::time::Instant::now();
      match rx.try_recv() {
        Ok(result) => {
          log::debug!(
            "fulltext drain: received result, took {}µs to recv",
            t.elapsed().as_micros()
          );
          app.fulltext_rx = None;
          app.fulltext_loading = false;
          match result {
            Ok(fetched_paper) => {
              log::debug!(
                "reader_open: {} blocks from fetcher",
                fetched_paper.blocks.len()
              );
              // arxiv URLs get the rich LaTeX path via fetch_paper —
              // structured math, tables, figures.  ~2s blocking; v2
              // can background on a worker.  Non-arxiv keeps the
              // PaperData the fetcher already produced (HTML walked
              // by from_html, or summary plain-text).  Image
              // rendering wiring is task #39, hardcode false here.
              let notes_context = app.pending_fulltext_context.take();
              let title = app.last_read.clone().unwrap_or_default();
              let detected_arxiv_id = notes_context
                .as_ref()
                .and_then(|ctx| tread::extract_arxiv_id(&ctx.paper.url));
              let kitty_supported = false;
              let (arxiv_id, paper) = if let Some(id) = detected_arxiv_id {
                match tread::fetch_paper(&id, kitty_supported) {
                  Ok(p) => (Some(id), p),
                  Err(e) => {
                    log::warn!(
                      "tread::fetch_paper failed for {id}, using fetcher result: {e}"
                    );
                    (
                      notes_context.as_ref().map(|ctx| ctx.paper.id.clone()),
                      fetched_paper,
                    )
                  }
                }
              } else {
                (
                  notes_context.as_ref().map(|ctx| ctx.paper.id.clone()),
                  fetched_paper,
                )
              };
              let reader = tread::Reader::init(
                paper,
                None,
                arxiv_id.clone(),
                80,
                24,
                kitty_supported,
                Some(app.voice_controller.clone()),
              );
              if app.fulltext_for_secondary {
                if app.fulltext_new_tab {
                  app.reader_secondary_push_tab(
                    title,
                    arxiv_id,
                    notes_context,
                    reader,
                  );
                } else {
                  app.reader_secondary_replace_active_tab(
                    title,
                    arxiv_id,
                    notes_context,
                    reader,
                  );
                }
                app.focused_reader = FocusedReader::Secondary;
                app.focus.focused_pane = PaneId::SecondaryReader;
                app.fulltext_for_secondary = false;
              } else {
                if app.fulltext_new_tab {
                  app.reader_push_tab(title, arxiv_id, notes_context, reader);
                } else {
                  app.reader_replace_active_tab(
                    title,
                    arxiv_id,
                    notes_context,
                    reader,
                  );
                }
                app.focus.focused_pane = PaneId::Reader;
              }
              app.fulltext_new_tab = false;
              app.clear_notification();
            }
            Err(e) => {
              app.pending_fulltext_context = None;
              app.set_notification(format!("Failed to fetch content: {e}"));
            }
          }
          app.mark_dirty();
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
          log::debug!("fulltext drain: channel disconnected");
          app.fulltext_rx = None;
          app.fulltext_loading = false;
          app.fulltext_for_secondary = false;
          app.fulltext_new_tab = false;
          app.pending_fulltext_context = None;
          app.set_notification("Fetch error: thread disconnected".to_string());
          app.mark_dirty();
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
      }
    }

    // ── Drain reader popup fetch ──────────────────────────────────────
    if let Some(rx) = app.reader_popup_rx.as_ref() {
      match rx.try_recv() {
        Ok(result) => {
          app.reader_popup_rx = None;
          app.fulltext_loading = false;
          app.pending_fulltext_context = None;
          match result {
            Ok(paper) => {
              let reader = tread::Reader::init(
                paper,
                None,
                None,
                80,
                24,
                false,
                Some(app.voice_controller.clone()),
              );
              app.reader_popup_editor = Some(reader);
              // Reset the popup's image cache to a fresh state — the
              // previous occupant (if any) had different kitty_ids.
              app.reader_popup_image_state = tread::ImageState::default();
              app.reader_popup_active = true;
              app.clear_notification();
            }
            Err(e) => {
              app.set_notification(format!("Failed to fetch content: {e}"));
            }
          }
          app.mark_dirty();
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
          app.reader_popup_rx = None;
          app.fulltext_loading = false;
          app.pending_fulltext_context = None;
          app.set_notification("Fetch error: thread disconnected".to_string());
          app.mark_dirty();
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
      }
    }

    if let Some(rx) = app.repo_fetch_rx.as_ref() {
      let t = std::time::Instant::now();
      match rx.try_recv() {
        Ok(result) => {
          log::debug!(
            "repo_fetch drain: received result, took {}µs to recv",
            t.elapsed().as_micros()
          );
          app.repo_fetch_rx = None;
          match result {
            RepoFetchResult::RepoOpened { branch, tree } => match tree {
              Ok(nodes) => {
                if let Some(ctx) = app.repo_context.as_mut() {
                  ctx.default_branch = branch;
                  ctx.tree_nodes = nodes;
                  ctx.status_message = None;
                }
              }
              Err(e) => app.set_repo_status(format!("Error: {e}")),
            },
            RepoFetchResult::DirLoaded { path, result } => {
              app.repo_apply_dir(path, result);
            }
            RepoFetchResult::FileLoaded { path, name, result } => {
              app.repo_apply_file(path, name, result);
            }
          }
          app.mark_dirty();
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
          log::debug!("repo_fetch drain: channel disconnected");
          app.repo_fetch_rx = None;
          app.set_repo_status("Fetch error: thread disconnected");
          app.mark_dirty();
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
      }
    }

    // Gate the draw on the dirty flag. `check_needs_redraw` reads-and-clears
    // in one call (cli-text-reader pattern). Idle frames cost ~0 work since
    // every per-frame allocation lives inside `ui::draw`.
    if app.check_needs_redraw() {
      let t_draw = std::time::Instant::now();
      terminal.draw(|frame| ui::draw(frame, &mut app))?;
      let draw_ms = t_draw.elapsed().as_millis();
      if !first_draw_logged {
        log::debug!(
          "startup: first frame ready in {}ms",
          startup_t0.elapsed().as_millis()
        );
        first_draw_logged = true;
      }
      if draw_ms > 16 {
        log::debug!("terminal.draw took {}ms (slow frame)", draw_ms);
      }
    }

    // Cadence: 16ms when something is animating or already dirty (so we
    // process events at 60Hz during interaction), 250ms when truly idle (so
    // CPU drops to near-zero and battery is preserved). Mirrors
    // cli-text-reader/src/editor/display_loop.rs:233.
    let timeout = if app.needs_redraw || app.has_active_animation() {
      std::time::Duration::from_millis(16)
    } else {
      std::time::Duration::from_millis(250)
    };

    if event::poll(timeout)? {
      match event::read()? {
        Event::Key(key) => {
          if key.kind != KeyEventKind::Press {
            continue;
          }

          log::debug!(
            "key event: {:?} leader_active={} focused_pane={:?}",
            key.code,
            app.leader_active,
            app.focus.focused_pane
          );
          keys::dispatch(key, &mut app);
          app.mark_dirty();
        }
        Event::Mouse(mouse) => {
          handle_mouse(mouse, &mut app, &terminal);
          app.mark_dirty();
        }
        Event::Resize(_, _) => {
          // Pane reflow moves every image's placement coords;
          // clear the cached placements so the next draw re-emits
          // at the new positions instead of stacking ghosts.
          app.clear_all_reader_image_state();
          app.mark_dirty();
        }
        Event::FocusLost => {
          // tmux pane switch: kitty placements painted at absolute
          // screen coords would otherwise bleed into whatever pane
          // is on top.  Delete them; FocusGained re-emits via the
          // next draw cycle.
          app.clear_all_reader_image_state();
        }
        _ => {}
      }
    }

    // Dispatch any stale events that arrived during the draw call. Previous
    // behaviour silently discarded these via `let _ = event::read()`, which
    // dropped user input on slow frames; now they go through the same path
    // as the primary dispatch above.
    while event::poll(std::time::Duration::from_millis(0))? {
      match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
          keys::dispatch(key, &mut app);
          app.mark_dirty();
        }
        Event::Mouse(mouse) => {
          handle_mouse(mouse, &mut app, &terminal);
          app.mark_dirty();
        }
        Event::Resize(_, _) => {
          app.clear_all_reader_image_state();
          app.mark_dirty();
        }
        Event::FocusLost => {
          app.clear_all_reader_image_state();
        }
        _ => {}
      }
    }

    if app.should_quit {
      break;
    }
    }
    Ok(())
  })();

  // Drain any pending cache write the background writer hasn't flushed yet,
  // so the on-disk cache.json reflects the final in-memory state.
  store::cache::flush_blocking();

  store::save_ui(&store::UiState {
    last_read: app.last_read.clone(),
    last_read_source: app.last_read_source.clone(),
    notes_tabs: app.notes_tabs.clone(),
    notes_active_tab: app.notes_active_tab,
    secondary_notes_tabs: app.secondary_notes_tabs.clone(),
    secondary_notes_active_tab: app.secondary_notes_active_tab,
  });

  // Balance the kitty-keyboard push from setup. Best-effort — if any of
  // these cleanup steps fails (already-closed TTY, etc.), keep going so
  // the rest still run. Propagate the loop's error AFTER cleanup so the
  // user sees both the cleanup attempt and the original failure.
  let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
  let _ = disable_raw_mode();
  let _ = execute!(
    terminal.backend_mut(),
    LeaveAlternateScreen,
    DisableMouseCapture,
    DisableFocusChange,
  );
  run_result.map_err(Into::into)
}
