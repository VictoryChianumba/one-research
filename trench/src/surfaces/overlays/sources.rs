//! Sources popup — first overlay surface conversion. Owns its cursor,
//! URL detection input, and async detect state, plus the input handler.
//!
//! Phase 2 milestone: this struct + its `handle_key` method replace
//! the `app.sources_popup: SourcesPopupState` field and the
//! `keys/sources.rs::handle_sources_popup` free function. Behavior is
//! preserved exactly — the diff is one of *ownership*, not semantics.

use crossterm::event::{KeyCode, KeyEvent};
use std::sync::mpsc;

use crate::action::Action;
use crate::app::{App, AppView, DiscoverResult};
use crate::config;
use crate::effect::Effect;
use crate::primitives::{AsyncLoadState, TextInputState};

/// Sources popup surface state. Was `SourcesPopupState` in
/// app/state/popups.rs.
#[derive(Default)]
pub struct SourcesSurface {
  /// Selected row in the sources list (input field is row 0).
  pub cursor: usize,
  /// URL detection input. `input.is_focused()` mirrors the prior
  /// `input_active: bool` companion field.
  pub input: TextInputState,
  /// URL detection async state machine.
  pub detect: AsyncLoadState<DiscoverResult>,
}

impl SourcesSurface {
  pub fn new() -> Self {
    Self::default()
  }

  /// Apply a typed action. Phase 2 only emits two variants
  /// (DismissTopModal, OpenSettings); rest of input is still
  /// translated inline by `handle_key`.
  pub fn apply_action(&mut self, action: Action, app: &mut App) -> Vec<Effect> {
    match action {
      Action::DismissTopModal | Action::OpenSettings => {
        // Both treat the modal as dismissed; OpenSettings additionally
        // routes the active view back to Settings.
        app.view = AppView::Settings;
        self.cursor = 0;
        self.input.clear();
        self.input.blur();
        self.detect.reset();
      }
    }
    Vec::new()
  }

  /// Translate a key event into either a typed Action (for cross-cutting
  /// concerns like dismiss) or a direct state mutation. Returns the
  /// effects produced by the action — empty until Phase 3 lands the
  /// effect vocabulary.
  ///
  /// Caller invariant: only call when this surface is the active modal.
  /// `app` is borrowed for non-self side effects (config save, refresh,
  /// async spawn). When effect routing lands in Phase 3, the &mut App
  /// parameter narrows to specific services.
  pub fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> Vec<Effect> {
    if self.input.is_focused() {
      self.handle_input_focused(key, app)
    } else {
      self.handle_list_focused(key, app)
    }
  }

  fn handle_input_focused(
    &mut self,
    key: KeyEvent,
    app: &mut App,
  ) -> Vec<Effect> {
    match key.code {
      KeyCode::Esc => {
        self.input.blur();
        self.detect.reset();
        self.input.clear();
      }
      KeyCode::Enter => {
        let ready = matches!(self.detect, AsyncLoadState::Ready(_));
        let loading = self.detect.is_loading();

        if ready {
          let Some(result) = self.detect.take() else {
            return Vec::new();
          };
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
                crate::force_refresh(app);
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
                crate::force_refresh(app);
              }
            }
            DiscoverResult::HuggingFaceAlreadyEnabled
            | DiscoverResult::Failed(_) => {}
          }
          self.input.clear();
          self.input.blur();
        } else if loading {
          // waiting — do nothing
        } else if !self.input.is_empty() {
          // Idle (or Disconnected, treated as a fresh start) — kick off
          // a new detection.
          let url = self.input.buffer().to_string();
          let (dtx, drx) = mpsc::channel();
          self.detect.start(drx);
          crate::spawn_discovery(url, dtx);
        }
      }
      KeyCode::Backspace => {
        self.input.pop_char();
        self.detect.reset();
      }
      KeyCode::Char(c) => {
        self.input.push_char(c);
        self.detect.reset();
      }
      _ => {}
    }
    Vec::new()
  }

  fn handle_list_focused(
    &mut self,
    key: KeyEvent,
    app: &mut App,
  ) -> Vec<Effect> {
    let cats = app.sources_popup_arxiv_cats();
    let cats_count = cats.len();
    let sources_count = config::PREDEFINED_SOURCES.len();
    let custom_count = app.config.sources.custom_feeds.len();
    let total = app.sources_popup_total_items();

    match key.code {
      KeyCode::Esc | KeyCode::Char('q') => {
        return self.apply_action(Action::OpenSettings, app);
      }
      KeyCode::Char('j') | KeyCode::Down => {
        self.cursor = (self.cursor + 1).min(total.saturating_sub(1));
      }
      KeyCode::Char('k') | KeyCode::Up => {
        self.cursor = self.cursor.saturating_sub(1);
      }
      KeyCode::Enter | KeyCode::Char('/') => {
        if self.cursor == 0 {
          self.input.focus();
        }
      }
      KeyCode::Char(' ') => {
        let c = self.cursor;
        if c == 0 {
          self.input.focus();
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
          crate::force_refresh(app);
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
          crate::force_refresh(app);
        }
        // custom feeds: no toggle (present = enabled, use d to delete)
      }
      KeyCode::Char('d') => {
        let c = self.cursor;
        let custom_start = 1 + cats_count + sources_count;
        if c >= custom_start && c < custom_start + custom_count {
          let idx = c - custom_start;
          app.config.sources.custom_feeds.remove(idx);
          app.config.save();
          self.cursor = self.cursor.saturating_sub(1);
        }
      }
      _ => {}
    }
    Vec::new()
  }
}
