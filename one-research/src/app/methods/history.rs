use std::time::Instant;

use crate::app::{App, FeedTab};
use crate::library::LibraryFilter;
use crate::models::{FeedItem, WorkflowState};

/// How many rows from the tail of the visible feed the Browse pager
/// fires (ADR-015 §F4). Paging a little early keeps the next page in
/// flight before the user actually reaches the bottom, so the buffer
/// stays ahead of the scroll rather than stalling at the edge.
const BROWSE_PAGE_AHEAD_ROWS: usize = 5;

/// How long the rail cursor must rest on a Category before arrival
/// auto-fill fires (ADR-015 §F5). The settle window is the rate-limit
/// discipline: arrowing through ten categories fires one fetch — the
/// one you stop on — not ten. The idle event loop wakes every ≤250ms,
/// so the effective delay is this value rounded up to the next tick.
const BROWSE_AUTOFILL_SETTLE_MS: u128 = 400;

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

    self.maybe_page_browse_subject();
  }

  /// ADR-015 §F4 — scroll-driven Browse pagination. When the feed is
  /// scoped to a single Category (subject-follow on, rail drilled to
  /// depth 2) and the cursor nears the tail of the buffered papers,
  /// fetch the next arXiv page from the buffer's `next_offset`.
  ///
  /// Deliberately only *extends* a category that already has a buffer:
  /// initiating a never-fetched category from a scroll event is PR 3's
  /// auto-fill, out of scope here. The `inflight` guard blocks duplicate
  /// page fetches (R4) and `exhausted` stops paging at the archive end;
  /// because an appended page grows `visible_count` — pushing the cursor
  /// back from the tail — the per-frame call re-arms only on real scroll.
  fn maybe_page_browse_subject(&mut self) {
    if self.feed.feed_tab != FeedTab::Browse || !self.feed.subject_follow {
      return;
    }
    // Only Category scope (depth 2) pages — at Group/Archive scope
    // "load more" has no single category to extend.
    if self.browse.rail_path.len() != 2 {
      return;
    }
    let Some(cat) = self.browse.rail_selected_category() else {
      return;
    };
    let code = cat.code.to_string();

    // Extend only an existing, non-exhausted buffer (PR 2, not PR 3).
    let Some(next_offset) = self
      .browse
      .loaded_categories
      .get(&code)
      .and_then(|b| if b.exhausted { None } else { Some(b.next_offset) })
    else {
      return;
    };
    if self.browse.inflight.contains(&code) {
      return;
    }

    // Near the tail of what the user can see?
    let selected = self.active_selected_index();
    let visible = self.visible_count();
    if selected + BROWSE_PAGE_AHEAD_ROWS < visible {
      return;
    }

    self.browse.inflight.insert(code.clone());
    crate::browse::pipeline::spawn_browse_fetch(
      code,
      next_offset,
      crate::app::BROWSE_PAGE_SIZE,
      self.browse.tx.clone(),
    );
  }

  /// ADR-015 §F5 — arrival auto-fill. Called every event-loop iteration
  /// (not just on redraw — the settle timer must advance while idle).
  /// When the rail cursor rests on a Category past the settle window,
  /// fetch its first page so a drilled-to category fills itself instead
  /// of waiting for `Enter`.
  ///
  /// Gated on subject-follow ON: that's the deliberate "browse
  /// categories" mode where the feed is scoped to the landed category,
  /// so landing→fill has a visible payoff. With follow off the feed is
  /// the firehose and the fetched items wouldn't be shown — auto-fetching
  /// everything cursored past would spend arXiv's budget for nothing.
  pub fn poll_browse_autofill(&mut self) {
    let resting_on = if self.feed.feed_tab == FeedTab::Browse
      && self.feed.subject_follow
      && self.browse.rail_path.len() == 2
    {
      self.browse.rail_selected_category().map(|c| c.code.to_string())
    } else {
      None
    };

    let Some(code) = resting_on else {
      self.browse.autofill_anchor = None;
      return;
    };

    // New arrival on a different category — (re)start the settle clock
    // and wait for the next poll.
    let still_resting = matches!(
      &self.browse.autofill_anchor,
      Some((anchor, _)) if *anchor == code,
    );
    if !still_resting {
      self.browse.autofill_anchor = Some((code, Instant::now()));
      return;
    }

    let settled =
      self.browse.autofill_anchor.as_ref().is_some_and(|(_, since)| {
        since.elapsed().as_millis() >= BROWSE_AUTOFILL_SETTLE_MS
      });
    if !settled {
      return;
    }

    // Fire page 1 once. Buffered / inflight / already-attempted all skip
    // — the attempted guard is what stops a failing fetch from re-firing
    // every settle window.
    if self.browse.loaded_categories.contains_key(&code)
      || self.browse.inflight.contains(&code)
      || self.browse.autofill_attempted.contains(&code)
    {
      return;
    }
    self.browse.autofill_attempted.insert(code.clone());
    self.browse.inflight.insert(code.clone());
    self.status_message = Some(format!("{code}: auto-loading…"));
    crate::browse::pipeline::spawn_browse_fetch(
      code,
      0,
      crate::app::BROWSE_PAGE_SIZE,
      self.browse.tx.clone(),
    );
    self.mark_dirty();
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
      // Each drawer item renders as 2 rows (content + blank separator), so
      // the number of items visible is half the available rows. Cap max at
      // `total - 1`, then clamp offset so the selection stays within
      // [offset, offset + items_per_viewport).
      let items_per_viewport = (viewport_rows / 2).max(1);
      self.reader_bottom.scroll.set_max(total.saturating_sub(1));
      let sel = self.reader_bottom.feed_popup_selected;
      let mut offset = self.reader_bottom.scroll.offset();
      if sel < offset {
        offset = sel;
      } else if sel >= offset.saturating_add(items_per_viewport) {
        offset = sel + 1 - items_per_viewport;
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
