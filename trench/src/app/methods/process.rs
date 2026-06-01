use crate::app::App;
use crate::browse::BrowseMessage;
use crate::discovery::DiscoveryMessage;
use crate::ingestion::message::FetchMessage;
use crate::models::{FeedItem, SourcePlatform};

use super::super::{arxiv_id_from_url, save_discovery_items};

/// Outcome of merging a single incoming item into `workspace.items_store`.
/// Used by `App::merge_fetched_item` so both `FetchMessage::Items` and
/// `BrowseMessage::Items` arms can share the same dedup + workflow-state
/// preservation logic (ADR-010 §D4).
pub(crate) enum ItemMergeOutcome {
  /// Existing item replaced (URL hit, or arXiv-ID hit where incoming is
  /// the canonical arXiv entry replacing an HF stub).
  Updated,
  /// New item pushed to `items_store`.
  NewPushed,
  /// Incoming dropped because an existing arXiv entry already covers
  /// the same arXiv ID (HF duplicate of a canonical arXiv paper).
  DroppedDuplicate,
}

impl App {
  /// Merge one fetched item into `workspace.items_store`, applying the
  /// canonical dedup rules: URL hit replaces in place; arXiv-ID hit
  /// replaces only if incoming is the canonical arXiv entry (otherwise
  /// drop); otherwise push. Persisted workflow state from
  /// `persisted_states` is restored on every path. Used by both bulk
  /// ingestion (`FetchMessage::Items`) and on-demand browse fetches
  /// (`BrowseMessage::Items`) — ADR-010 §D4.
  pub(crate) fn merge_fetched_item(
    &mut self,
    mut item: FeedItem,
  ) -> ItemMergeOutcome {
    if let Some(state) = self.workspace.persisted_states.get(&item.url) {
      item.workflow_state = *state;
    }
    if let Some(pos) = self.workspace.items_store.find_index_by_url(&item.url) {
      self.workspace.items_store.replace_at(pos, item);
      return ItemMergeOutcome::Updated;
    }
    if let Some(aid) = arxiv_id_from_url(&item.url) {
      if let Some(pos) = self.workspace.items_store.find_index_by_arxiv_id(aid)
      {
        if item.source_platform == SourcePlatform::ArXiv {
          let ws = self
            .workspace
            .items_store
            .get(pos)
            .expect("index points into items_store")
            .workflow_state;
          item.workflow_state = ws;
          self.workspace.items_store.replace_at(pos, item);
          return ItemMergeOutcome::Updated;
        }
        return ItemMergeOutcome::DroppedDuplicate;
      }
    }
    self.workspace.items_store.push(item);
    ItemMergeOutcome::NewPushed
  }

  pub fn process_incoming(&mut self) {
    use std::sync::mpsc::TryRecvError;

    // Spinner only ticks when something is actually loading. Without this
    // gate, the wrapping increment fires every loop iteration and would
    // perpetually re-set `needs_redraw` even on idle.
    if self.async_jobs.is_loading {
      self.async_jobs.spinner_frame =
        self.async_jobs.spinner_frame.wrapping_add(1);
      self.mark_dirty();
    }
    self.poll_detect_result();
    self.process_incoming_discovery();
    self.process_incoming_browse();

    // Clear "Saved." confirmation after 2 seconds.
    if let Some(t) = self.settings.save_time {
      if t.elapsed().as_secs() >= 2 {
        self.settings.save_time = None;
        self.mark_dirty();
      }
    }

    if self.async_jobs.fetch_rx.is_none() {
      return;
    }

    // Collect pending messages without blocking.
    let mut messages = Vec::new();
    let mut disconnected = false;

    if let Some(rx) = &self.async_jobs.fetch_rx {
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
      self.async_jobs.is_loading = false;
      self.is_refreshing = false;
      self.async_jobs.fetch_rx = None;
    }

    let was_empty = self.workspace.items_store.is_empty();
    let mut had_structural_item_changes = false;
    let mut had_source_updates = false;
    let mut had_enriched_updates = false;

    for msg in messages {
      match msg {
        FetchMessage::Items(new_items) => {
          for item in new_items {
            match self.merge_fetched_item(item) {
              ItemMergeOutcome::NewPushed => {
                had_structural_item_changes = true;
                had_source_updates = true;
              }
              ItemMergeOutcome::Updated => {
                had_source_updates = true;
              }
              ItemMergeOutcome::DroppedDuplicate => {}
            }
          }
        }
        FetchMessage::EnrichedItems(new_items) => {
          for mut item in new_items {
            if let Some(state) = self.workspace.persisted_states.get(&item.url)
            {
              item.workflow_state = *state;
            }

            if let Some(pos) =
              self.workspace.items_store.find_index_by_url(&item.url)
            {
              let workflow_state = self
                .workspace
                .items_store
                .get(pos)
                .expect("index points into items_store")
                .workflow_state;
              item.workflow_state = workflow_state;
              self.workspace.items_store.replace_at(pos, item);
              had_enriched_updates = true;
              continue;
            }

            if let Some(aid) = arxiv_id_from_url(&item.url) {
              if let Some(pos) =
                self.workspace.items_store.find_index_by_arxiv_id(aid)
              {
                let existing = self
                  .workspace
                  .items_store
                  .get(pos)
                  .expect("index points into items_store");
                let workflow_state = existing.workflow_state;
                let existing_is_canonical_arxiv =
                  existing.source_platform == SourcePlatform::ArXiv;
                if existing_is_canonical_arxiv
                  && item.source_platform != SourcePlatform::ArXiv
                {
                  // Existing arxiv entry: merge in the non-arxiv item's
                  // optional fields without overwriting the canonical
                  // record.  `get_mut` gives in-place access so we don't
                  // round-trip through replace_at.
                  let existing_mut = self
                    .workspace
                    .items_store
                    .get_mut(pos)
                    .expect("index points into items_store");
                  if existing_mut.github_repo.is_none() {
                    existing_mut.github_repo = item.github_repo.take();
                    existing_mut.github_owner = item.github_owner.take();
                    existing_mut.github_repo_name =
                      item.github_repo_name.take();
                  }
                  if existing_mut.full_content.is_none() {
                    existing_mut.full_content = item.full_content.take();
                  }
                  had_enriched_updates = true;
                  continue;
                }
                item.workflow_state = workflow_state;
                self.workspace.items_store.replace_at(pos, item);
                had_enriched_updates = true;
                continue;
              }
            }

            self.workspace.items_store.push(item);
            had_structural_item_changes = true;
          }
        }
        FetchMessage::SourceComplete(name) => {
          self.async_jobs.loading_sources.retain(|s| s != &name);
          self.async_jobs.loaded_sources.push(name);
          // Status bar shows the loading-sources list; without
          // mark_dirty, a phantom in-progress source can sit on
          // screen for ~250ms until any other event ticks the
          // redraw flag.
          self.mark_dirty();
        }
        FetchMessage::SourceError(name, err) => {
          self.status_message = Some(err);
          self.async_jobs.loading_sources.retain(|s| s != &name);
          self.mark_dirty();
        }
        FetchMessage::AllComplete => {
          self.async_jobs.is_loading = false;
          self.is_refreshing = false;
        }
      }
    }

    if had_structural_item_changes {
      // sort_by on ItemStore sorts items *and* rebuilds both indices —
      // the bug class C9 closes (forget-to-rebuild after sort) is gone
      // by construction.
      self
        .workspace
        .items_store
        .sort_by(|a, b| b.published_at.cmp(&a.published_at));
      self.route_effects(&[crate::effect::Effect::ItemsChanged]);
      // Hand off to the background writer — UI thread used to hitch for
      // 100-300 ms here while the 3.8 MB cache.json was serialized + fsynced.
      crate::store::cache::queue_save(
        self.workspace.items_store.items().to_vec(),
      );
      if was_empty {
        self.feed.inbox_list.set_offset(0);
      }
      self.mark_dirty();
    } else if had_source_updates || had_enriched_updates {
      self.route_effects(&[crate::effect::Effect::ItemsChanged]);
      crate::store::cache::queue_save(
        self.workspace.items_store.items().to_vec(),
      );
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
              .discovery
              .items
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

  /// Drain the browse-fetch channel (ADR-010 §D3) and merge each batch
  /// into `workspace.items_store` via the shared dedup helper, then
  /// record the URL list under its category in `browse.loaded_categories`
  /// so the details pane resolves them to live `FeedItem`s. Sorts and
  /// persists the cache once at the end if any new items landed —
  /// matches the bulk-refresh path's invariants in `process_incoming`.
  fn process_incoming_browse(&mut self) {
    use std::sync::mpsc::TryRecvError;

    let mut messages = Vec::new();
    if let Some(rx) = &self.browse.rx {
      loop {
        match rx.try_recv() {
          Ok(msg) => messages.push(msg),
          Err(TryRecvError::Empty) => break,
          // Should not disconnect — `tx` lives on the model for the App
          // lifetime. If it does (e.g. tests `.take()` the rx), drop it.
          Err(TryRecvError::Disconnected) => {
            self.browse.rx = None;
            break;
          }
        }
      }
    }

    if messages.is_empty() {
      return;
    }

    let mut had_structural_changes = false;
    let mut had_updates = false;
    for msg in messages {
      match msg {
        BrowseMessage::Items { category, start, items } => {
          let mut fetched = Vec::with_capacity(items.len());
          for item in items {
            let url = item.url.clone();
            match self.merge_fetched_item(item) {
              ItemMergeOutcome::NewPushed => {
                had_structural_changes = true;
                had_updates = true;
              }
              ItemMergeOutcome::Updated => {
                had_updates = true;
              }
              ItemMergeOutcome::DroppedDuplicate => {}
            }
            fetched.push(url);
          }
          // ADR-015 §F4: `start == 0` is the first page — replace any
          // stale buffer. A later page appends, dedupes against what's
          // already buffered, and advances the pager. A short page
          // (fewer than a full `BROWSE_PAGE_SIZE`) exhausts the archive.
          let page_len = fetched.len();
          if start == 0 {
            self.browse.loaded_categories.insert(
              category.clone(),
              crate::app::CategoryBuffer::first_page(fetched),
            );
          } else {
            let buf = self
              .browse
              .loaded_categories
              .entry(category.clone())
              .or_default();
            for url in fetched {
              if !buf.urls.contains(&url) {
                buf.urls.push(url);
              }
            }
            buf.next_offset = start + page_len;
            buf.exhausted = page_len < crate::app::BROWSE_PAGE_SIZE;
          }
          self.browse.inflight.remove(&category);
          let total = self.browse.loaded_count(&category).unwrap_or(0);
          self.status_message =
            Some(format!("{category}: {total} papers loaded"));
        }
        BrowseMessage::Error { category, error } => {
          self.browse.inflight.remove(&category);
          log::error!("browse[{category}]: {error}");
          self.status_message = Some(format!("{category}: fetch failed"));
        }
        BrowseMessage::SearchResults { query, items } => {
          let n = items.len();
          for item in items {
            match self.merge_fetched_item(item) {
              ItemMergeOutcome::NewPushed => {
                had_structural_changes = true;
                had_updates = true;
              }
              ItemMergeOutcome::Updated => had_updates = true,
              ItemMergeOutcome::DroppedDuplicate => {}
            }
          }
          self.feed.search_loading = false;
          self.status_message = Some(if n == 0 {
            format!("No arXiv results for “{query}”")
          } else {
            format!("Found {n} arXiv result(s) for “{query}”")
          });
        }
      }
    }

    if had_structural_changes {
      self
        .workspace
        .items_store
        .sort_by(|a, b| b.published_at.cmp(&a.published_at));
      self.route_effects(&[crate::effect::Effect::ItemsChanged]);
      crate::store::cache::queue_save(
        self.workspace.items_store.items().to_vec(),
      );
    } else if had_updates {
      self.route_effects(&[crate::effect::Effect::ItemsChanged]);
      crate::store::cache::queue_save(
        self.workspace.items_store.items().to_vec(),
      );
    }
    self.mark_dirty();
  }

  fn merge_discovery_items(&mut self, items: Vec<FeedItem>) {
    for mut item in items {
      // Belt-and-suspenders: every current discovery source already
      // sanitizes at ingestion, but the unbarriered injection point
      // is a future-contributor footgun — a new source added without
      // ingestion-time sanitize would silently ship terminal-control
      // bytes to the renderer.
      item.sanitize_in_place();
      if let Some(state) = self.workspace.persisted_states.get(&item.url) {
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
    self.route_effects(&[crate::effect::Effect::ItemsChanged]);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::App;
  use crate::models::{
    ContentType, FeedItem, SignalLevel, SourcePlatform, WorkflowState,
  };

  /// Minimal FeedItem for merge tests. Caller picks the differentiating
  /// fields (url, platform) and we fill in defaults for the rest. URLs
  /// use a `test://merge/...` scheme so they never collide with real
  /// cache contents.
  fn make_item(url: &str, platform: SourcePlatform) -> FeedItem {
    FeedItem {
      id: url.to_string(),
      title: format!("Test paper for {url}"),
      source_platform: platform,
      content_type: ContentType::Paper,
      domain_tags: vec![],
      signal: SignalLevel::Primary,
      published_at: "2026-05-19".to_string(),
      authors: vec!["Test Author".to_string()],
      summary_short: String::new(),
      workflow_state: WorkflowState::Inbox,
      url: url.to_string(),
      upvote_count: 0,
      github_repo: None,
      github_owner: None,
      github_repo_name: None,
      benchmark_results: vec![],
      full_content: None,
      source_name: "test".to_string(),
      title_lower: String::new(),
      authors_lower: vec![],
    }
  }

  // ── merge_fetched_item: the load-bearing helper ────────────────────
  //
  // Both `FetchMessage::Items` (bulk refresh) and `BrowseMessage::Items`
  // (on-demand) call this. The four outcomes below encode the dedup
  // contract — every PR that touches the merge path must keep these
  // tests passing or the bulk-refresh path silently changes behaviour.

  #[test]
  fn merge_new_url_pushes_and_grows_store() {
    let mut app = App::new();
    let before = app.workspace.items_store.items().len();
    let item = make_item(
      "test://merge/new-url-pushes-and-grows-store-001",
      SourcePlatform::Blog,
    );

    let outcome = app.merge_fetched_item(item);

    assert!(matches!(outcome, ItemMergeOutcome::NewPushed));
    assert_eq!(app.workspace.items_store.items().len(), before + 1);
  }

  #[test]
  fn merge_same_url_replaces_in_place_without_growing() {
    let mut app = App::new();
    let url = "test://merge/same-url-replaces-002";
    app.merge_fetched_item(make_item(url, SourcePlatform::Blog));
    let after_first = app.workspace.items_store.items().len();

    let mut updated = make_item(url, SourcePlatform::Blog);
    updated.title = "Updated title".to_string();
    let outcome = app.merge_fetched_item(updated);

    assert!(matches!(outcome, ItemMergeOutcome::Updated));
    assert_eq!(app.workspace.items_store.items().len(), after_first);
    let found = app.workspace.items_store.find_by_url(url).expect("present");
    assert_eq!(found.title, "Updated title");
  }

  #[test]
  fn merge_arxiv_canonical_replaces_hf_stub_for_same_arxiv_id() {
    let mut app = App::new();
    // First insert: HuggingFace stub pointing at an arXiv-shaped URL
    // via the huggingface.co/papers/ path, which arxiv_id_from_url
    // recognises and routes through the arxiv-ID index.
    let hf_url = "https://huggingface.co/papers/9999.99003";
    app.merge_fetched_item(make_item(hf_url, SourcePlatform::HuggingFace));
    let before = app.workspace.items_store.items().len();

    // Second insert: the canonical arXiv entry for the same paper.
    // Different URL, same arXiv ID. Should replace the HF stub.
    let arxiv_url = "https://arxiv.org/abs/9999.99003";
    let outcome =
      app.merge_fetched_item(make_item(arxiv_url, SourcePlatform::ArXiv));

    assert!(matches!(outcome, ItemMergeOutcome::Updated));
    assert_eq!(
      app.workspace.items_store.items().len(),
      before,
      "arxiv-ID hit replaces in place; no growth"
    );
  }

  #[test]
  fn merge_hf_dup_against_existing_arxiv_drops_silently() {
    let mut app = App::new();
    // First insert: canonical arXiv entry.
    let arxiv_url = "https://arxiv.org/abs/9999.99004";
    app.merge_fetched_item(make_item(arxiv_url, SourcePlatform::ArXiv));
    let before = app.workspace.items_store.items().len();

    // Second insert: HF stub for the same paper. Should drop, not
    // overwrite the canonical arXiv entry.
    let hf_url = "https://huggingface.co/papers/9999.99004";
    let outcome =
      app.merge_fetched_item(make_item(hf_url, SourcePlatform::HuggingFace));

    assert!(matches!(outcome, ItemMergeOutcome::DroppedDuplicate));
    assert_eq!(app.workspace.items_store.items().len(), before);
    // The canonical arXiv URL is still findable; the HF URL is not.
    assert!(app.workspace.items_store.find_by_url(arxiv_url).is_some());
    assert!(app.workspace.items_store.find_by_url(hf_url).is_none());
  }

  #[test]
  fn merge_restores_persisted_workflow_state() {
    let mut app = App::new();
    let url = "test://merge/persisted-state-005";
    // Pre-seed a persisted workflow state for this URL.
    app
      .workspace
      .persisted_states
      .insert(url.to_string(), WorkflowState::Queued);

    // Incoming item has WorkflowState::Inbox by default — should be
    // overridden by the persisted state at merge time.
    let item = make_item(url, SourcePlatform::Blog);
    assert_eq!(item.workflow_state, WorkflowState::Inbox);
    app.merge_fetched_item(item);

    let stored =
      app.workspace.items_store.find_by_url(url).expect("present after merge");
    assert_eq!(
      stored.workflow_state,
      WorkflowState::Queued,
      "persisted state must override incoming workflow_state"
    );
  }

  // ── process_incoming_browse: the BrowseMessage::Items end-to-end ───

  #[test]
  fn process_incoming_browse_populates_loaded_categories() {
    let mut app = App::new();
    let category = "test-merge-cat".to_string();
    let item_a =
      make_item("test://merge/browse-incoming-a-006", SourcePlatform::ArXiv);
    let item_b =
      make_item("test://merge/browse-incoming-b-006", SourcePlatform::ArXiv);
    let url_a = item_a.url.clone();
    let url_b = item_b.url.clone();

    // Mark the category as inflight so the post-receive cleanup is testable.
    app.browse.inflight.insert(category.clone());
    let _ = app.browse.tx.send(BrowseMessage::Items {
      category: category.clone(),
      start: 0,
      items: vec![item_a, item_b],
    });

    // Drain the channel — should record both URLs against the category
    // and clear the inflight flag.
    app.process_incoming_browse();

    let buf = app
      .browse
      .loaded_categories
      .get(&category)
      .expect("category entry recorded");
    assert_eq!(buf.urls.len(), 2);
    assert!(buf.urls.iter().any(|u| u == &url_a));
    assert!(buf.urls.iter().any(|u| u == &url_b));
    // ADR-015 §F1: the buffer derives where the next page starts and
    // whether the archive is exhausted. Two items (< BROWSE_PAGE_SIZE)
    // is a short page, so the next fetch starts at 2 and there is
    // nothing more to pull. PR 2 reads exactly these two fields to
    // decide whether a scroll-to-tail should page deeper.
    assert_eq!(buf.next_offset, 2, "next page starts past what we fetched");
    assert!(buf.exhausted, "a short first page exhausts the archive");
    assert!(
      !app.browse.inflight.contains(&category),
      "inflight cleared after items message"
    );
  }

  #[test]
  fn browse_pagination_appends_advances_and_exhausts() {
    // ADR-015 §F4: the scroll-tail pager extends an existing buffer.
    // This locks the handler's append arithmetic — the part PR 2's
    // pager depends on to know where to resume and when to stop.
    let mut app = App::new();
    let category = "test-page-cat".to_string();

    // First page is *full* (BROWSE_PAGE_SIZE items): the archive has
    // more, so the buffer parks `next_offset` at the page boundary and
    // stays un-exhausted. WHY it matters: a full page is the signal the
    // pager uses to keep going.
    let page1: Vec<_> = (0..crate::app::BROWSE_PAGE_SIZE)
      .map(|i| {
        make_item(&format!("test://page/p1-{i:03}"), SourcePlatform::ArXiv)
      })
      .collect();
    let _ = app.browse.tx.send(BrowseMessage::Items {
      category: category.clone(),
      start: 0,
      items: page1,
    });
    app.process_incoming_browse();
    {
      let buf = app.browse.loaded_categories.get(&category).unwrap();
      assert_eq!(buf.urls.len(), crate::app::BROWSE_PAGE_SIZE);
      assert_eq!(buf.next_offset, crate::app::BROWSE_PAGE_SIZE);
      assert!(!buf.exhausted, "a full first page leaves room for more");
    }

    // Second page from `next_offset` is short: it appends (not replaces),
    // advances the offset past it, and exhausts the archive so the pager
    // stops. WHY it matters: a short page is the terminating signal.
    let page2 = vec![
      make_item("test://page/p2-a", SourcePlatform::ArXiv),
      make_item("test://page/p2-b", SourcePlatform::ArXiv),
    ];
    let _ = app.browse.tx.send(BrowseMessage::Items {
      category: category.clone(),
      start: crate::app::BROWSE_PAGE_SIZE,
      items: page2,
    });
    app.process_incoming_browse();
    let buf = app.browse.loaded_categories.get(&category).unwrap();
    assert_eq!(
      buf.urls.len(),
      crate::app::BROWSE_PAGE_SIZE + 2,
      "second page appends onto the first, not replaces it"
    );
    assert_eq!(buf.next_offset, crate::app::BROWSE_PAGE_SIZE + 2);
    assert!(buf.exhausted, "a short second page exhausts the archive");
  }

  #[test]
  fn browse_autofill_fires_after_settle_and_guards_against_restorm() {
    use std::time::{Duration, Instant};
    // ADR-015 §F5: resting on a Category past the settle window auto-
    // loads its first page; a failed fetch must not re-fire forever.
    let mut app = App::new();
    app.feed.feed_tab = crate::app::FeedTab::Browse;
    app.feed.subject_follow = true;
    app.browse.drill_into(crate::app::RailNode::Group("cs"));
    app.browse.drill_into(crate::app::RailNode::Archive("cs", "cs"));
    let code = app.browse.rail_selected_category().unwrap().code.to_string();

    // Cursor has rested past the settle window → fires page 1.
    app.browse.autofill_anchor =
      Some((code.clone(), Instant::now() - Duration::from_millis(500)));
    app.poll_browse_autofill();
    assert!(
      app.browse.inflight.contains(&code),
      "a settled rest on an unfetched category auto-fires page 1"
    );
    assert!(app.browse.autofill_attempted.contains(&code));

    // A failed fetch clears inflight but leaves no buffer. WHY it
    // matters: without the attempted guard the next settle window would
    // re-fire, hammering arXiv on a persistently-failing category.
    app.browse.inflight.remove(&code);
    app.browse.autofill_anchor =
      Some((code.clone(), Instant::now() - Duration::from_millis(500)));
    app.poll_browse_autofill();
    assert!(
      !app.browse.inflight.contains(&code),
      "attempted guard prevents an auto-fill retry storm"
    );
  }

  #[test]
  fn process_incoming_browse_error_clears_inflight() {
    let mut app = App::new();
    let category = "test-merge-cat-error".to_string();
    app.browse.inflight.insert(category.clone());
    let _ = app.browse.tx.send(BrowseMessage::Error {
      category: category.clone(),
      error: "synthetic test failure".to_string(),
    });

    app.process_incoming_browse();

    assert!(
      !app.browse.inflight.contains(&category),
      "inflight cleared even on error so the user can retry"
    );
    assert!(
      !app.browse.loaded_categories.contains_key(&category),
      "errors do not populate loaded_categories"
    );
  }

  /// Regression: the visible cache is keyed only on `FeedTab`, but the
  /// Browse list also depends on `subject_follow` + the rail drill point.
  /// Toggling follow used to leave the stale (unscoped) index set in
  /// place, so `selected_item()` resolved against a different list than
  /// the feed pane rendered — pressing Enter opened whatever sat at the
  /// old index instead of the highlighted paper. The toggle must realign
  /// selection with the now-scoped feed.
  #[test]
  fn browse_subject_follow_toggle_realigns_selection_with_scoped_feed() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = App::new();
    // One item outside the scope, one inside. With the rail drilled to
    // the physics group's first archive (astro-ph), follow-on must hide
    // the cs.LG paper and surface only the astro-ph one.
    let mut cs = make_item("test://browse/cs", SourcePlatform::ArXiv);
    cs.domain_tags = vec!["cs.LG".to_string()];
    let mut astro = make_item("test://browse/astro", SourcePlatform::ArXiv);
    astro.domain_tags = vec!["astro-ph.CO".to_string()];
    let astro_url = astro.url.clone();
    app.merge_fetched_item(cs);
    app.merge_fetched_item(astro);

    app.feed.feed_tab = crate::app::FeedTab::Browse;
    // Drill to physics → archives level; cursor at row 0 = astro-ph.
    app.browse.rail_path = vec![crate::app::RailNode::Group("physics")];
    assert_eq!(app.browse.focus, crate::app::BrowseFocus::Rail);

    // Follow off: both papers visible. Warm the cache at index 0.
    assert_eq!(app.visible_count(), 2, "unscoped feed shows both papers");

    // Toggle subject-follow through the real key path ('x' on the rail).
    crate::keys::dispatch(
      KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
      &mut app,
    );
    assert!(app.feed.subject_follow, "'x' turns subject-follow on");

    // Scoped feed now holds only the astro-ph paper, and the selection
    // (read by Enter via `selected_item`) must point at it — not the
    // stale index-0 item from the unscoped list.
    assert_eq!(app.visible_count(), 1, "feed narrows to the scoped archive");
    assert_eq!(
      app.selected_item().map(|i| i.url.as_str()),
      Some(astro_url.as_str()),
      "selection must resolve against the scoped feed, so Enter opens \
       the paper that is actually displayed",
    );
  }
}
