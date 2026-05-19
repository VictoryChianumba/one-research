use crate::app::{App, FilterState};
use crate::models::{ContentType, SignalLevel, WorkflowState};

use super::super::toggle_set;

impl App {
  pub fn library_recompute_selection(&mut self) {
    if !self.feed.library_visual_mode {
      self.feed.library_selected_urls.clear();
      return;
    }
    let cursor = self.feed.library_list.selected();
    let anchor = self.feed.library_visual_anchor;
    let (lo, hi) =
      if cursor <= anchor { (cursor, anchor) } else { (anchor, cursor) };
    let window = self.visible_window(lo, hi.saturating_add(1));
    self.feed.library_selected_urls =
      window.iter().map(|it| it.url.clone()).collect();
  }

  /// Move the visual-mode cursor to `new_cursor` and update
  /// `library_selected_urls` incrementally — adds/removes only the items at
  /// the boundary between the old and new selection ranges. Single-step j/k
  /// is one HashSet insert or remove per keystroke, instead of cloning every
  /// URL in [lo..=hi]. Falls back to a no-op when not in visual mode.
  pub fn library_extend_selection(&mut self, new_cursor: usize) {
    // Sync count for the same reason set_active_selected_index does:
    // ListState::set_selected clamps to count-1, and count starts at
    // 0 so without this the new_cursor gets forced to 0.
    let count = self.visible_count();
    self.feed.library_list.set_count(count);
    if !self.feed.library_visual_mode {
      self.feed.library_list.set_selected(new_cursor);
      return;
    }
    let anchor = self.feed.library_visual_anchor;
    let old_cursor = self.feed.library_list.selected();
    let old_lo = old_cursor.min(anchor);
    let old_hi = old_cursor.max(anchor);
    let new_lo = new_cursor.min(anchor);
    let new_hi = new_cursor.max(anchor);

    // Materialize URL changes into local Vec<String>s so the
    // visible_window borrow (which holds &self) drops before we touch
    // the `library_selected_urls: HashSet<String>` field. For typical
    // single-step j/k moves each Vec has 0 or 1 entries.
    let removed_urls: Vec<String> = {
      let mut out = Vec::new();
      if new_lo > old_lo {
        out.extend(
          self.visible_window(old_lo, new_lo).iter().map(|it| it.url.clone()),
        );
      }
      if new_hi < old_hi {
        out.extend(
          self
            .visible_window(new_hi.saturating_add(1), old_hi.saturating_add(1))
            .iter()
            .map(|it| it.url.clone()),
        );
      }
      out
    };
    let added_urls: Vec<String> = {
      let mut out = Vec::new();
      if new_lo < old_lo {
        out.extend(
          self.visible_window(new_lo, old_lo).iter().map(|it| it.url.clone()),
        );
      }
      if new_hi > old_hi {
        out.extend(
          self
            .visible_window(old_hi.saturating_add(1), new_hi.saturating_add(1))
            .iter()
            .map(|it| it.url.clone()),
        );
      }
      out
    };

    for url in &removed_urls {
      self.feed.library_selected_urls.remove(url);
    }
    for url in added_urls {
      self.feed.library_selected_urls.insert(url);
    }

    self.feed.library_list.set_selected(new_cursor);
  }

  pub fn library_exit_visual(&mut self) {
    self.feed.exit_library_visual_mode();
  }

  /// Apply a workflow-state transition to every selected item. Returns the
  /// number of items affected.
  pub fn apply_workflow_to_selection(
    &mut self,
    state: crate::models::WorkflowState,
  ) -> usize {
    if self.feed.library_selected_urls.is_empty() {
      return 0;
    }
    let urls: Vec<String> =
      self.feed.library_selected_urls.iter().cloned().collect();
    let mut count = 0;
    for url in urls {
      if self.set_workflow_state_for_url(&url, state) {
        count += 1;
      }
    }
    crate::store::save(&self.workspace.persisted_states);
    count
  }

  /// Set a single item's workflow_state (in items or discovery_items by URL),
  /// update the persisted-state override, and invalidate the caches that
  /// depend on workflow_state. Does NOT save persisted_states to disk —
  /// caller batches the save.

  /// Open the tag picker for a list of target URLs (single item or multi-select).
  pub fn open_tag_picker(&mut self, target_urls: Vec<String>) {
    if target_urls.is_empty() {
      return;
    }
    self.tag_picker.target_urls = target_urls;
    self.tag_picker.input.clear();
    self.tag_picker.selected = 0;
    self.tag_picker.active = true;
  }

  pub fn close_tag_picker(&mut self) {
    self.tag_picker.active = false;
    self.tag_picker.input.clear();
    self.tag_picker.selected = 0;
    self.tag_picker.target_urls.clear();
  }

  /// Toggle a tag on every target URL. If any target lacks the tag, add it to all;
  /// otherwise remove from all (idempotent toggle).
  pub fn toggle_tag_on_targets(&mut self, tag: &str) {
    let tag = crate::tags::normalize(tag);
    if tag.is_empty() {
      return;
    }
    let urls = self.tag_picker.target_urls.clone();
    let any_missing = urls.iter().any(|url| {
      !crate::tags::for_url(&self.workspace.item_tags, url)
        .iter()
        .any(|t| t == &tag)
    });
    for url in &urls {
      if any_missing {
        crate::tags::add(&mut self.workspace.item_tags, url, tag.clone());
      } else {
        crate::tags::remove(&mut self.workspace.item_tags, url, &tag);
      }
    }
    crate::store::tags::save(&self.workspace.item_tags);
    self.route_effects(&[crate::effect::Effect::TagsChanged]);
  }

  pub fn filter_cursor_down(&mut self) {
    let max = self.filter_total_items().saturating_sub(1);
    self.feed.filter_cursor = (self.feed.filter_cursor + 1).min(max);
  }

  pub fn filter_cursor_up(&mut self) {
    self.feed.filter_cursor = self.feed.filter_cursor.saturating_sub(1);
  }

  /// Total number of selectable rows in the filter panel (dynamic source +
  /// tag counts + fixed sections). Workflow-state filtering moved to the
  /// Library tab chips, so the panel covers sources, signals, content types,
  /// tags, sort modes, the subject-follow toggle, and clear-all.
  pub fn filter_total_items(&self) -> usize {
    self.filter_source_names().len()
      + 3
      + 3
      + crate::tags::all_tags(&self.workspace.item_tags).len()
      + 4 // sort modes (Dated / Random / Popular / Trending)
      + 1 // follow toggle
      + 1 // clear-all
  }

  /// Sorted unique source label strings derived from loaded items.
  /// Lazy memoize: the BTreeSet build + per-item String clone happens on the
  /// first read after invalidation; every other call (per draw frame, per
  /// j/k keystroke in the filter panel) is an O(1) clone of the cached vec.
  pub fn filter_source_names(&self) -> Vec<String> {
    if let Some(c) = self.render_caches.filter_source_names.borrow().as_ref() {
      return c.clone();
    }
    let mut names: std::collections::BTreeSet<String> = self
      .workspace
      .items_store
      .iter()
      .map(|item| {
        if item.source_name.is_empty() {
          item.source_platform.short_label().to_string()
        } else {
          item.source_name.clone()
        }
      })
      .collect();
    // Always include at least "arxiv" and "hf" so the filter is useful
    // even before a fetch completes.
    for seed in &["arxiv", "hf"] {
      names.insert(seed.to_string());
    }
    let computed: Vec<String> = names.into_iter().collect();
    *self.render_caches.filter_source_names.borrow_mut() =
      Some(computed.clone());
    computed
  }

  /// Mutator chokepoint for `active_filters`. Invalidates every cache that
  /// depends on the filter set: render_caches.visible (filtered items), filter_summary
  /// (the rendered summary line), render_caches.filtered_history (filter affects
  /// history view too).

  pub fn toggle_filter_at_cursor(&mut self) {
    let source_names = self.filter_source_names();
    let src_count = source_names.len();
    let tag_names = crate::tags::all_tags(&self.workspace.item_tags);
    let tag_count = tag_names.len();
    let c = self.feed.filter_cursor;

    // Layout (ADR-011 §E3/§E4 added Sort + Follow before clear-all):
    //   [sources] [3 signals] [3 content types] [tags]
    //   [4 sort modes] [1 follow toggle] [clear-all]
    let signals_start = src_count;
    let content_start = signals_start + 3;
    let tags_start = content_start + 3;
    let sort_start = tags_start + tag_count;
    let follow_start = sort_start + 4;
    let clear_all = follow_start + 1;

    // Sort + Follow live on FeedModel directly, not FilterState. The
    // visible-items cache must still be invalidated, so we mutate then
    // route Effect::FiltersChanged explicitly (mirrors mutate_filters).
    if c >= sort_start && c < follow_start {
      let mode = match c - sort_start {
        0 => crate::feed::FeedSortMode::Dated,
        1 => crate::feed::FeedSortMode::Random,
        2 => crate::feed::FeedSortMode::Popular,
        _ => crate::feed::FeedSortMode::Trending,
      };
      // Re-shuffle on every Random selection so the user can refresh
      // the order by Space-selecting Random again.
      if mode == crate::feed::FeedSortMode::Random {
        self.feed.random_seed = std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)
          .map(|d| d.as_nanos() as u64)
          .unwrap_or(0);
      }
      self.feed.sort_mode = mode;
      self.route_effects(&[crate::effect::Effect::FiltersChanged]);
      self.reset_active_feed_position();
      return;
    }
    if c == follow_start {
      self.feed.subject_follow = !self.feed.subject_follow;
      self.route_effects(&[crate::effect::Effect::FiltersChanged]);
      self.reset_active_feed_position();
      return;
    }

    self.mutate_filters(|filters| {
      if c < src_count {
        let name = source_names[c].clone();
        if !filters.sources.remove(&name) {
          filters.sources.insert(name);
        }
      } else if c < content_start {
        match c - signals_start {
          0 => toggle_set(&mut filters.signals, SignalLevel::Primary),
          1 => toggle_set(&mut filters.signals, SignalLevel::Secondary),
          _ => toggle_set(&mut filters.signals, SignalLevel::Tertiary),
        }
      } else if c < tags_start {
        match c - content_start {
          0 => toggle_set(&mut filters.content_types, ContentType::Paper),
          1 => toggle_set(&mut filters.content_types, ContentType::Article),
          _ => toggle_set(&mut filters.content_types, ContentType::Digest),
        }
      } else if c < sort_start {
        let tag = tag_names[c - tags_start].clone();
        if !filters.tags.remove(&tag) {
          filters.tags.insert(tag);
        }
      } else if c == clear_all {
        *filters = FilterState::new();
      }
    });
    self.reset_active_feed_position();
  }

  pub fn clear_filters(&mut self) {
    self.mutate_filters(|f| *f = FilterState::new());
    self.reset_active_feed_position();
  }

  pub fn set_workflow_state(&mut self, state: WorkflowState) {
    // W3 hybrid (ADR-001 D5): delegate to the FeedModel gesture, which
    // owns the lookup + mutation and emits the cache-invalidation event.
    // Split-borrow: feed + workspace + discovery + config are disjoint
    // fields of App, so the compiler allows three `&mut` borrows + one
    // `&` borrow held simultaneously across the call.
    let effects = self.feed.set_workflow_state_at_cursor(
      &mut self.workspace,
      &mut self.discovery,
      &self.browse,
      &self.config,
      state,
    );
    if !effects.is_empty() {
      self.route_effects(&effects);
      crate::store::save(&self.workspace.persisted_states);
    }
  }
}
