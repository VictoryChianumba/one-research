use crossterm::event::{KeyCode, KeyEvent};
use std::sync::mpsc;

use crate::app::{App, AppView, DiscoverResult, SourcesDetectState};
use crate::config;
use super::super::{force_refresh, spawn_discovery};

pub(super) fn handle_sources_popup(key: KeyEvent, app: &mut App) -> bool {
  if app.view != AppView::Sources {
    return false;
  }
  if app.sources_popup.input_active {
    match key.code {
      KeyCode::Esc => {
        app.sources_popup.input_active = false;
        app.sources_popup.detect_state = SourcesDetectState::Idle;
        app.sources_popup.input.clear();
      }
      KeyCode::Enter => {
        match &app.sources_popup.detect_state {
          SourcesDetectState::Idle => {
            if !app.sources_popup.input.is_empty() {
              app.sources_popup.detect_state = SourcesDetectState::Detecting;
              let url = app.sources_popup.input.clone();
              let (dtx, drx) = mpsc::channel();
              app.sources_popup.detect_rx = Some(drx);
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
            app.sources_popup.input.clear();
            app.sources_popup.detect_state = SourcesDetectState::Idle;
            app.sources_popup.input_active = false;
          }
        }
      }
      KeyCode::Backspace => {
        app.sources_popup.input.pop();
        app.sources_popup.detect_state = SourcesDetectState::Idle;
      }
      KeyCode::Char(c) => {
        app.sources_popup.input.push(c);
        app.sources_popup.detect_state = SourcesDetectState::Idle;
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
        app.sources_popup.cursor = 0;
        app.sources_popup.input.clear();
        app.sources_popup.detect_state = SourcesDetectState::Idle;
      }
      KeyCode::Char('j') | KeyCode::Down => {
        app.sources_popup.cursor =
          (app.sources_popup.cursor + 1).min(total.saturating_sub(1));
      }
      KeyCode::Char('k') | KeyCode::Up => {
        app.sources_popup.cursor = app.sources_popup.cursor.saturating_sub(1);
      }
      KeyCode::Enter | KeyCode::Char('/') => {
        if app.sources_popup.cursor == 0 {
          app.sources_popup.input_active = true;
        }
      }
      KeyCode::Char(' ') => {
        let c = app.sources_popup.cursor;
        if c == 0 {
          app.sources_popup.input_active = true;
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
        let c = app.sources_popup.cursor;
        let custom_start = 1 + cats_count + sources_count;
        if c >= custom_start && c < custom_start + custom_count {
          let idx = c - custom_start;
          app.config.sources.custom_feeds.remove(idx);
          app.config.save();
          app.sources_popup.cursor = app.sources_popup.cursor.saturating_sub(1);
        }
      }
      _ => {}
    }
  }
  true
}
