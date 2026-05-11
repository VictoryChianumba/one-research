use crate::app::App;
use crate::models::FeedItem;

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
  /// filtered_history_cache so subsequent reads see the updated data.

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
    // reverted in d3a700e. Revisit later with a different bound formula.
    if self.narrow_feed_details_open {
      self.details_scroll.set_max(usize::MAX);
    } else {
      self.details_scroll.set_max(0);
    }

    // reader_bottom_scroll.max: details mode allows unbounded scroll
    // (same paragraph-clipping pattern); feed mode's max is set by
    // the feed-pane render path because it needs viewport_rows
    // (handled in B2b).
    if self.reader_bottom_open && self.reader_bottom_details {
      self.reader_bottom_scroll.set_max(usize::MAX);
    }

    // B2a: hoist the auto-scroll math for the active feed list out of
    // draw_item_table / draw_history_tab. Uses the focus pane-rect
    // cache (populated by last frame's draw_main_row), which is one
    // frame behind on resize but corrects on the next frame. First-
    // frame default is Rect::default (zero-sized), which becomes a
    // no-op and lets the offset stay at the default 0.
    let feed_pane_info = self.focus.pane(crate::app::PaneId::Feed);
    if feed_pane_info.is_open && feed_pane_info.rect.height > 1 {
      let rect = feed_pane_info.rect;
      self.update_active_list_offset_for_viewport(rect);
    }
  }

  /// Hoisted from draw_item_table (Inbox/Library/Discoveries) and
  /// draw_history_tab (History). Scroll list_offset so selected stays
  /// visible. Two formulas because the tabs use different data paths
  /// (visible_count vs filtered_history) and slightly different
  /// scroll-trigger conditions (item-height-aware for the table tabs,
  /// uniform row-height for history).
  fn update_active_list_offset_for_viewport(&mut self, feed_pane_rect: ratatui::layout::Rect) {
    use crate::app::FeedTab;
    // inner: draw_item_table and draw_history_tab both shift y+1 / height-1
    // for the pane title row. viewport_rows then subtracts 2 (header rows).
    let inner_height = feed_pane_rect.height.saturating_sub(1);
    let inner_width = feed_pane_rect.width;
    let viewport_rows = (inner_height as usize).saturating_sub(2);
    if viewport_rows == 0 {
      return;
    }

    match self.feed_tab {
      FeedTab::Inbox | FeedTab::Library | FeedTab::Discoveries => {
        // Width budget for the title column (used by the item-height heuristic).
        // Mirrors draw_item_table's formula: 1+7+5+14+10+8+6 fixed cols + spacing.
        let title_wrap_w = ((inner_width as usize)
          .saturating_sub(1 + 7 + 5 + 14 + 10 + 8 + 6))
          .max(10);
        let total_items_pre = self.visible_count();
        if total_items_pre == 0 {
          return;
        }
        let cur_offset = self.active_list_offset();
        let visible_count =
          self.count_visible_items_at(cur_offset, viewport_rows, title_wrap_w);
        let selected_index = self.active_selected_index();
        let mut list_offset = cur_offset;
        if selected_index < list_offset {
          list_offset = selected_index;
        } else if visible_count >= 2
          && selected_index >= list_offset + visible_count.saturating_sub(2)
          && list_offset + visible_count < total_items_pre
        {
          list_offset = (selected_index + 2).saturating_sub(visible_count);
        }
        list_offset = list_offset.min(total_items_pre.saturating_sub(1));
        self.set_active_list_offset(list_offset);
      }
      FeedTab::History => {
        let total = self.filtered_history().len();
        if total == 0 {
          return;
        }
        let selected =
          self.history_list.selected().min(total.saturating_sub(1));
        let max_offset = total.saturating_sub(viewport_rows.min(total));
        let mut offset = self.history_list.offset().min(max_offset);
        if selected < offset {
          offset = selected;
        } else if selected >= offset + viewport_rows {
          offset = selected + 1 - viewport_rows;
        }
        self.history_list.set_offset(offset);
      }
    }
  }

  /// Item-height-aware visible-item count for the active feed list.
  /// Mirrors the heuristic at draw_item_table: each item takes 3 rows
  /// when its title exceeds the title-column width (wraps to 2 lines),
  /// 2 rows otherwise. Capped at viewport_rows.
  fn count_visible_items_at(
    &self,
    list_offset: usize,
    viewport_rows: usize,
    title_wrap_w: usize,
  ) -> usize {
    let mut rows_used = 0usize;
    let mut count = 0usize;
    for idx in list_offset..self.visible_count() {
      let Some(item) = self.visible_get(idx) else { break };
      let item_height = if item.title.len() > title_wrap_w { 3 } else { 2 };
      if rows_used + item_height > viewport_rows {
        break;
      }
      rows_used += item_height;
      count += 1;
    }
    count.max(1)
  }

  /// URL-shaped key identifying the currently-shown details subject.
  /// `Some(url)` for feed items, `Some("query:{q}")` for history query
  /// entries, `None` when nothing is selected.
  fn details_subject_key(&self) -> Option<String> {
    use crate::app::FeedTab;
    use crate::history::HistoryKind;
    match self.feed_tab {
      FeedTab::History => {
        let history = self.filtered_history();
        let entry = history.get(self.history_list.selected())?;
        Some(match entry.kind {
          HistoryKind::Paper => entry.key.clone(),
          HistoryKind::Query => format!("query:{}", entry.key),
        })
      }
      _ => self.selected_item().map(|i| i.url.clone()),
    }
  }

  pub fn filtered_history(&self) -> Vec<&crate::history::HistoryEntry> {
    if self.filtered_history_cache.borrow().is_none() {
      let indices = self.compute_filtered_history_indices();
      *self.filtered_history_cache.borrow_mut() = Some(indices);
    }
    let cache = self.filtered_history_cache.borrow();
    let indices = cache.as_ref().expect("populated above");
    indices.iter().map(|&i| &self.workspace.history[i]).collect()
  }

  fn compute_filtered_history_indices(&self) -> Vec<usize> {
    let now = chrono::Utc::now();
    let q = self.search_query_lower.as_str();
    let src_filter = &self.active_filters.sources;
    self
      .workspace
      .history
      .iter()
      .enumerate()
      .filter(|(_, e)| self.history_filter.matches(e, now))
      .filter(|(_, e)| q.is_empty() || e.title_lower.contains(q))
      .filter(|(_, e)| src_filter.is_empty() || src_filter.contains(&e.source))
      .map(|(i, _)| i)
      .collect()
  }
}
