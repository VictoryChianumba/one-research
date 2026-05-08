use crate::app::App;
use crate::discovery::DiscoveryMessage;
use crate::ingestion::message::FetchMessage;
use crate::models::{FeedItem, SourcePlatform};

use super::super::{arxiv_id_from_url, save_discovery_items};

impl App {
  pub fn process_incoming(&mut self) {
    use std::sync::mpsc::TryRecvError;

    // Spinner only ticks when something is actually loading. Without this
    // gate, the wrapping increment fires every loop iteration and would
    // perpetually re-set `needs_redraw` even on idle.
    if self.is_loading {
      self.spinner_frame = self.spinner_frame.wrapping_add(1);
      self.mark_dirty();
    }
    self.poll_detect_result();
    self.process_incoming_discovery();

    // Clear "Saved." confirmation after 2 seconds.
    if let Some(t) = self.settings.save_time {
      if t.elapsed().as_secs() >= 2 {
        self.settings.save_time = None;
        self.mark_dirty();
      }
    }

    if self.fetch_rx.is_none() {
      return;
    }

    // Collect pending messages without blocking.
    let mut messages = Vec::new();
    let mut disconnected = false;

    if let Some(rx) = &self.fetch_rx {
      loop {
        match rx.try_recv() {
          Ok(msg) => messages.push(msg),
          Err(TryRecvError::Empty) => break,
          Err(TryRecvError::Disconnected) => {
            disconnected = true;
            break;
          }
        }
      }
    }

    if disconnected {
      self.is_loading = false;
      self.is_refreshing = false;
      self.fetch_rx = None;
    }

    let was_empty = self.items.is_empty();
    let mut had_structural_item_changes = false;
    let mut had_source_updates = false;
    let mut had_enriched_updates = false;

    for msg in messages {
      match msg {
        FetchMessage::Items(new_items) => {
          for mut item in new_items {
            // Apply any persisted workflow state.
            if let Some(state) = self.persisted_states.get(&item.url) {
              item.workflow_state = *state;
            }

            // URL dedup via index — O(1) replaces the prior O(N) linear
            // scan that fired ~50× per refresh × ~2,600 items.
            if let Some(&pos) = self.url_index.get(&item.url) {
              self.items[pos] = item;
              had_source_updates = true;
              continue;
            }

            // ArXiv ID dedup: collapse HF and arXiv entries for the same
            // paper. Keep the arXiv entry as primary. Same O(1) lookup.
            if let Some(aid) = arxiv_id_from_url(&item.url) {
              if let Some(&pos) = self.arxiv_id_index.get(aid) {
                if item.source_platform == SourcePlatform::ArXiv {
                  // Incoming is the canonical arXiv entry — replace HF stub.
                  // Position doesn't change, indices stay valid.
                  let ws = self.items[pos].workflow_state;
                  self.items[pos] = item;
                  self.items[pos].workflow_state = ws;
                  had_source_updates = true;
                }
                // else: existing is already arXiv, drop the HF duplicate.
                continue;
              }
            }

            // New item: push and update indices incrementally so the next
            // iteration of this same loop sees it for intra-batch dedup.
            let new_idx = self.items.len();
            self.url_index.insert(item.url.clone(), new_idx);
            if let Some(aid) = arxiv_id_from_url(&item.url) {
              self.arxiv_id_index.insert(aid.to_string(), new_idx);
            }
            self.items.push(item);
            had_structural_item_changes = true;
            had_source_updates = true;
          }
        }
        FetchMessage::EnrichedItems(new_items) => {
          for mut item in new_items {
            if let Some(state) = self.persisted_states.get(&item.url) {
              item.workflow_state = *state;
            }

            if let Some(&pos) = self.url_index.get(&item.url) {
              let workflow_state = self.items[pos].workflow_state;
              self.items[pos] = item;
              self.items[pos].workflow_state = workflow_state;
              had_enriched_updates = true;
              continue;
            }

            if let Some(aid) = arxiv_id_from_url(&item.url) {
              if let Some(&pos) = self.arxiv_id_index.get(aid) {
                let workflow_state = self.items[pos].workflow_state;
                if self.items[pos].source_platform == SourcePlatform::ArXiv
                  && item.source_platform != SourcePlatform::ArXiv
                {
                  if self.items[pos].github_repo.is_none() {
                    self.items[pos].github_repo = item.github_repo.take();
                    self.items[pos].github_owner = item.github_owner.take();
                    self.items[pos].github_repo_name =
                      item.github_repo_name.take();
                  }
                  if self.items[pos].full_content.is_none() {
                    self.items[pos].full_content = item.full_content.take();
                  }
                  had_enriched_updates = true;
                  continue;
                }
                self.items[pos] = item;
                self.items[pos].workflow_state = workflow_state;
                had_enriched_updates = true;
                continue;
              }
            }

            let new_idx = self.items.len();
            self.url_index.insert(item.url.clone(), new_idx);
            if let Some(aid) = arxiv_id_from_url(&item.url) {
              self.arxiv_id_index.insert(aid.to_string(), new_idx);
            }
            self.items.push(item);
            had_structural_item_changes = true;
          }
        }
        FetchMessage::SourceComplete(name) => {
          self.loading_sources.retain(|s| s != &name);
          self.loaded_sources.push(name);
          // Status bar shows the loading-sources list; without
          // mark_dirty, a phantom in-progress source can sit on
          // screen for ~250ms until any other event ticks the
          // redraw flag.
          self.mark_dirty();
        }
        FetchMessage::SourceError(name, err) => {
          self.status_message = Some(err);
          self.loading_sources.retain(|s| s != &name);
          self.mark_dirty();
        }
        FetchMessage::AllComplete => {
          self.is_loading = false;
          self.is_refreshing = false;
        }
      }
    }

    if had_structural_item_changes {
      self.items.sort_by(|a, b| b.published_at.cmp(&a.published_at));
      // Sort invalidated every position; indices must reflect the new order.
      self.rebuild_indices();
      self.invalidate_visible_cache();
      self.invalidate_counts_cache();
      // Hand off to the background writer — UI thread used to hitch for
      // 100-300 ms here while the 3.8 MB cache.json was serialized + fsynced.
      crate::store::cache::queue_save(self.items.clone());
      if was_empty {
        self.list_offset = 0;
      }
      self.mark_dirty();
    } else if had_source_updates || had_enriched_updates {
      self.invalidate_visible_cache();
      self.invalidate_items_derived_caches();
      crate::store::cache::queue_save(self.items.clone());
      self.mark_dirty();
    }
    if disconnected {
      // Loading-state change is visible in the status bar.
      self.mark_dirty();
    }
  }

  pub fn process_incoming_discovery(&mut self) {
    use std::sync::mpsc::TryRecvError;

    let mut messages = Vec::new();
    let mut disconnected = false;

    if let Some(rx) = &self.discovery.rx {
      loop {
        match rx.try_recv() {
          Ok(msg) => messages.push(msg),
          Err(TryRecvError::Empty) => break,
          Err(TryRecvError::Disconnected) => {
            disconnected = true;
            break;
          }
        }
      }
    }

    if disconnected {
      self.discovery.rx = None;
      self.discovery.loading = false;
    }

    let had_messages = !messages.is_empty();

    for msg in messages {
      match msg {
        DiscoveryMessage::StatusUpdate(s) => {
          self.discovery.status = s;
        }
        DiscoveryMessage::Items(items) => {
          self.merge_discovery_items(items);
          save_discovery_items(&self.discovery.items);
        }
        DiscoveryMessage::SessionSnapshot(snapshot) => {
          self.discovery.session = snapshot;
          crate::store::session::save(&self.discovery.session);
        }
        DiscoveryMessage::Complete => {
          self.discovery.rx = None;
          self.discovery.loading = false;
          let n = self.discovery.items.len();
          self.discovery.status = format!("Found {n} papers");
          self.status_message = Some("Discovery complete".to_string());

          let topic = self.discovery.session.initial_query.clone();
          if !topic.is_empty() {
            let titles: String = self
              .discovery.items
              .iter()
              .take(3)
              .map(|i| format!("• {}", i.title))
              .collect::<Vec<_>>()
              .join("\n");
            let body = if titles.is_empty() {
              String::new()
            } else {
              format!("\n\nTop results:\n{titles}")
            };
            self.push_chat_assistant_message(format!(
              "Discovery complete for \"{topic}\".\nFound {n} papers.{body}"
            ));
          }
        }
        DiscoveryMessage::Error(e) => {
          self.discovery.rx = None;
          self.discovery.loading = false;
          self.discovery.status = format!("Error: {e}");
          self.push_chat_assistant_message(format!("Discovery failed: {e}"));
          self.status_message = Some("Discovery failed".to_string());
        }
      }
    }

    // Any of the above arms mutated discovery state visible to the user.
    if had_messages || disconnected {
      self.mark_dirty();
    }
  }

  fn merge_discovery_items(&mut self, items: Vec<FeedItem>) {
    for mut item in items {
      // Belt-and-suspenders: every current discovery source already
      // sanitizes at ingestion, but the unbarriered injection point
      // is a future-contributor footgun — a new source added without
      // ingestion-time sanitize would silently ship terminal-control
      // bytes to the renderer.
      item.sanitize_in_place();
      if let Some(state) = self.persisted_states.get(&item.url) {
        item.workflow_state = *state;
      }

      // URL dedup via index — O(1).
      if let Some(&pos) = self.discovery.url_index.get(&item.url) {
        self.discovery.items[pos] = item;
        continue;
      }

      // ArXiv ID dedup — O(1).
      if let Some(aid) = arxiv_id_from_url(&item.url) {
        if let Some(&pos) = self.discovery.arxiv_id_index.get(aid) {
          if item.source_platform == SourcePlatform::ArXiv {
            let ws = self.discovery.items[pos].workflow_state;
            self.discovery.items[pos] = item;
            self.discovery.items[pos].workflow_state = ws;
          }
          continue;
        }
      }

      // New item: push and update indices incrementally.
      let new_idx = self.discovery.items.len();
      self.discovery.url_index.insert(item.url.clone(), new_idx);
      if let Some(aid) = arxiv_id_from_url(&item.url) {
        self.discovery.arxiv_id_index.insert(aid.to_string(), new_idx);
      }
      self.discovery.items.push(item);
    }
    self.discovery.items.sort_by(|a, b| b.published_at.cmp(&a.published_at));
    // Sort invalidated positions; rebuild for correctness.
    self.rebuild_discovery_indices();
    self.invalidate_visible_cache();
  }
}
