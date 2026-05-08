use crossterm::event::{KeyCode, KeyEvent};
use std::sync::mpsc;

use crate::app::{App, AppView, RepoPane};
use super::super::{open_url, spawn_repo_dir, spawn_repo_file};

pub(super) fn handle_repo_viewer(key: KeyEvent, app: &mut App) -> bool {
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
