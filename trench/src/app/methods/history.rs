use crate::app::{App, FeedTab};
use crate::library::LibraryFilter;
use crate::models::{FeedItem, WorkflowState};

impl App {
  pub fn record_paper_open(&mut self, item: &FeedItem) {
    let meta = crate::history::HistoryPaperMeta {
      authors: item.authors.clone(),
      source_platform: item.source_platform.clone(),
      published_at: item.published_at.clone(),
      summary_short: item.summary_short.clone(),
    };
    let source = if item.source_name.is_empty() {
      item.source_platform.short_label().to_string()
    } else {
      item.source_name.clone()
    };
    self.mutate_history(|h| {
      crate::history::record_paper(
        h,
        item.url.clone(),
        item.title.clone(),
        source,
        meta,
      );
    });
    crate::store::history::save(&self.workspace.history);
  }

  pub fn record_discovery_query(
    &mut self,
    topic: &str,
    intent: crate::discovery::intent::QueryIntent,
  ) {
    self.mutate_history(|h| {
      crate::history::record_query(h, topic.to_string(), intent.label());
    });
    crate::store::history::save(&self.workspace.history);
  }

  /// Mutator chokepoint for `history`. Invokes `f`, then invalidates the
  /// render_caches.filtered_history so subsequent reads see the updated data.

  /// Pre-draw update: hoists state mutations that were previously
  /// performed inline during render (refactor B — render purification).
  /// Runs once per frame before draw, so render functions only need
  /// `&App` for the migrated state.
  pub fn pre_draw_update(&mut self) {
    // Reset details_scroll when selection changes.
    let current_key = self.details_subject_key();
    if current_key != self.details_last_item_url {
      self.details_scroll.reset();
      self.details_last_item_url = current_key;
    }

    // details_scroll.max: the narrow feed details popup uses unbounded
    // scroll (its paragraph clips into empty rows); the main details
    // panel always truncates to viewport (no scroll). Narrow popup
    // wins when open. Encodes the "popup-overwrites-panel" semantic
    // from the prior render-order interaction.
    //
    // TODO(scroll-bound): the popup should ideally stop scrolling at the
    // end of its content rather than running into empty rows. A first
    // attempt (commit 9543768) computed a textwrap-based bound in the
    // popup's render path, but caused a visible regression and was
    // reverted in d3a700e. Revisit when B2 introduces FeedLayout — the
    // popup's wrap width becomes available in pre_draw_update there,
    // and a different bound formula (one that doesn't trigger whatever
    // regression we saw) can be tried.
    if self.view_flags.narrow_feed_details_open {
      self.details_scroll.set_max(usize::MAX);
    } else {
      self.details_scroll.set_max(0);
    }

    // reader_bottom.scroll.max: details mode allows unbounded scroll
    // (same paragraph-clipping pattern). The feed-mode counterpart
    // requires viewport_rows from the bottom-drawer list area, which
    // isn't known until layout — see `apply_frame_layout` (C6 / ADR-008)
    // for that branch.
    if self.reader_bottom.open && self.reader_bottom.details {
      self.reader_bottom.scroll.set_max(usize::MAX);
    }
  }

  /// Per-frame post-layout hook (ADR-008). Runs between the layout pass
  /// and the render pass, with `FrameLayout` carrying the post-layout
  /// `Rect`s that layout-derived mutations need.
  ///
  /// C6 PR 2 routes the reader-bottom feed-mode auto-scroll through
  /// this hook — the last `// Intentional render-time mutation` site
  /// in `ui/layout/reader.rs` is now gone.
  pub fn apply_frame_layout(&mut self, layout: &crate::ui::FrameLayout) {
    if let Some(list_area) = layout.reader_bottom_feed_list {
      let viewport_rows = list_area.height as usize;
      if viewport_rows == 0 {
        return;
      }
      let total = if self.feed.feed_tab == crate::app::FeedTab::History {
        self.history_count()
      } else {
        self.visible_count()
      };
      // Mirrors the pre-C6 inline math at reader.rs:434-441 exactly:
      // cap max at `total - 1`, then clamp offset so the selection is
      // within [offset, offset + viewport_rows).
      self.reader_bottom.scroll.set_max(total.saturating_sub(1));
      let sel = self.reader_bottom.feed_popup_selected;
      let mut offset = self.reader_bottom.scroll.offset();
      if sel < offset {
        offset = sel;
      } else if sel >= offset.saturating_add(viewport_rows) {
        offset = sel + 1 - viewport_rows;
      }
      self.reader_bottom.scroll.set_offset(offset);
    }
  }

  /// URL-shaped key identifying the currently-shown details subject.
  /// `Some(url)` for feed items, `Some("query:{q}")` for history query
  /// entries, `None` when nothing is selected.
  fn details_subject_key(&self) -> Option<String> {
    use crate::app::FeedTab;
    use crate::history::HistoryKind;
    match self.feed.feed_tab {
      FeedTab::History => {
        let history = self.filtered_history();
        let entry = history.get(self.feed.history_list.selected())?;
        Some(match entry.kind {
          HistoryKind::Paper => entry.key.clone(),
          HistoryKind::Query => format!("query:{}", entry.key),
        })
      }
      _ => self.selected_item().map(|i| i.url.clone()),
    }
  }

  pub fn filtered_history(&self) -> Vec<&crate::history::HistoryEntry> {
    if self.render_caches.filtered_history.borrow().is_none() {
      let indices = self.compute_filtered_history_indices();
      *self.render_caches.filtered_history.borrow_mut() = Some(indices);
    }
    let cache = self.render_caches.filtered_history.borrow();
    let indices = cache.as_ref().expect("populated above");
    indices.iter().map(|&i| &self.workspace.history[i]).collect()
  }

  pub fn history_count(&self) -> usize {
    self.filtered_history().len()
  }

  pub fn history_get(
    &self,
    idx: usize,
  ) -> Option<&crate::history::HistoryEntry> {
    let history = self.filtered_history();
    history.get(idx).copied()
  }

  pub fn history_window(
    &self,
    start: usize,
    end: usize,
  ) -> Vec<&crate::history::HistoryEntry> {
    let history = self.filtered_history();
    let start = start.min(history.len());
    let end = end.min(history.len());
    history[start..end].to_vec()
  }

  pub fn history_item(
    &self,
    entry: &crate::history::HistoryEntry,
  ) -> Option<FeedItem> {
    if entry.kind != crate::history::HistoryKind::Paper {
      return None;
    }
    if let Some(idx) = self.workspace.items_store.find_index_by_url(&entry.key)
    {
      return self.workspace.items_store.get(idx).cloned();
    }
    if let Some(arxiv_id) = crate::models::arxiv_id_from_url(&entry.key) {
      if let Some(idx) =
        self.workspace.items_store.find_index_by_arxiv_id(arxiv_id)
      {
        return self.workspace.items_store.get(idx).cloned();
      }
      if let Some(&idx) = self.discovery.arxiv_id_index.get(arxiv_id) {
        return self.discovery.items.get(idx).cloned();
      }
    }
    self
      .discovery
      .url_index
      .get(&entry.key)
      .and_then(|&idx| self.discovery.items.get(idx))
      .cloned()
      .or_else(|| {
        entry
          .paper_meta
          .as_ref()
          .map(|m| reconstruct_history_feed_item(entry, m))
      })
  }

  pub fn activate_history_item_target(
    &mut self,
    entry: &crate::history::HistoryEntry,
  ) -> bool {
    let Some((tab, workflow_state, url)) = self.history_item_target(entry)
    else {
      return false;
    };

    self.feed.feed_tab = tab;
    if tab == FeedTab::Library {
      self.feed.library_filter = match workflow_state {
        WorkflowState::Queued => LibraryFilter::Queue,
        WorkflowState::DeepRead => LibraryFilter::Read,
        WorkflowState::Archived => LibraryFilter::Archived,
        WorkflowState::Inbox => LibraryFilter::All,
      };
    }
    self.render_caches.invalidate_visible();

    if let Some(pos) =
      self.visible_items().iter().position(|item| item.url == url)
    {
      self.set_active_selected_index(pos);
    } else {
      self.set_active_selected_index(0);
    }
    true
  }

  fn compute_filtered_history_indices(&self) -> Vec<usize> {
    let now = chrono::Utc::now();
    let q = self.feed.search_query_lower.as_str();
    let src_filter = &self.feed.active_filters.sources;
    self
      .workspace
      .history
      .iter()
      .enumerate()
      .filter(|(_, e)| self.feed.history_filter.matches(e, now))
      .filter(|(_, e)| q.is_empty() || e.title_lower.contains(q))
      .filter(|(_, e)| src_filter.is_empty() || src_filter.contains(&e.source))
      .map(|(i, _)| i)
      .collect()
  }
}

impl App {
  fn history_item_target(
    &self,
    entry: &crate::history::HistoryEntry,
  ) -> Option<(FeedTab, WorkflowState, String)> {
    if entry.kind != crate::history::HistoryKind::Paper {
      return None;
    }
    if let Some(idx) = self.workspace.items_store.find_index_by_url(&entry.key)
    {
      let item = self.workspace.items_store.get(idx)?;
      return Some((
        workspace_feed_tab(item.workflow_state),
        item.workflow_state,
        item.url.clone(),
      ));
    }
    if let Some(arxiv_id) = crate::models::arxiv_id_from_url(&entry.key) {
      if let Some(idx) =
        self.workspace.items_store.find_index_by_arxiv_id(arxiv_id)
      {
        let item = self.workspace.items_store.get(idx)?;
        return Some((
          workspace_feed_tab(item.workflow_state),
          item.workflow_state,
          item.url.clone(),
        ));
      }
      if let Some(&idx) = self.discovery.arxiv_id_index.get(arxiv_id) {
        let item = self.discovery.items.get(idx)?;
        return Some((
          FeedTab::Discoveries,
          item.workflow_state,
          item.url.clone(),
        ));
      }
    }
    self
      .discovery
      .url_index
      .get(&entry.key)
      .and_then(|&idx| self.discovery.items.get(idx))
      .map(|item| (FeedTab::Discoveries, item.workflow_state, item.url.clone()))
  }
}

fn workspace_feed_tab(state: WorkflowState) -> FeedTab {
  match state {
    WorkflowState::Inbox => FeedTab::Inbox,
    WorkflowState::Queued
    | WorkflowState::DeepRead
    | WorkflowState::Archived => FeedTab::Library,
  }
}

fn reconstruct_history_feed_item(
  entry: &crate::history::HistoryEntry,
  meta: &crate::history::HistoryPaperMeta,
) -> FeedItem {
  use crate::models::{ContentType, SignalLevel, WorkflowState};
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
