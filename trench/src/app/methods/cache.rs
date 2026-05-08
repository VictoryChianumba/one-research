use crate::app::{App, FilterState};
use crate::models::WorkflowState;

/// Cache invalidators + mutator chokepoints. Five caches with five mutator
/// helpers — every state-mutation that affects a cache must go through a
/// mutator. The bare `invalidate_*` methods are private (except
/// `invalidate_visible_cache` which has one external caller).
impl App {
  pub(crate) fn invalidate_visible_cache(&self) {
    *self.visible_cache.borrow_mut() = None;
  }

  pub(crate) fn invalidate_counts_cache(&self) {
    *self.counts_cache.borrow_mut() = None;
  }

  pub(crate) fn invalidate_filter_source_names_cache(&self) {
    *self.filter_source_names_cache.borrow_mut() = None;
  }

  pub(crate) fn invalidate_filter_summary_cache(&self) {
    *self.filter_summary_cache.borrow_mut() = None;
  }

  pub(crate) fn invalidate_filtered_history_cache(&self) {
    *self.filtered_history_cache.borrow_mut() = None;
  }

  /// Aggregate invalidator for every cache that derives from `app.items`.
  /// Internal use only — callers should go through the mutate_* helpers.
  pub(crate) fn invalidate_items_derived_caches(&self) {
    self.invalidate_counts_cache();
    self.invalidate_filter_source_names_cache();
  }


  pub(crate) fn mutate_search_query(&mut self, f: impl FnOnce(&mut String)) {
    f(&mut self.search_query);
    self.search_query_lower = self.search_query.to_lowercase();
    self.invalidate_visible_cache();
    self.invalidate_filtered_history_cache();
  }

  pub fn push_search_char(&mut self, c: char) {
    self.mutate_search_query(|q| q.push(c));
    self.reset_active_feed_position();
  }

  pub fn pop_search_char(&mut self) {
    self.mutate_search_query(|q| {
      q.pop();
    });
    self.reset_active_feed_position();
  }

  pub fn clear_search_query(&mut self) {
    self.mutate_search_query(|q| q.clear());
  }

  /// Mirror of `push_search_char` for the discovery palette.
  pub fn push_discovery_char(&mut self, c: char) {
    self.discovery.query.push(c);
    self.discovery.query_lower = self.discovery.query.to_lowercase();
  }

  pub fn pop_discovery_char(&mut self) {
    self.discovery.query.pop();
    self.discovery.query_lower = self.discovery.query.to_lowercase();
  }

  pub fn clear_discovery_query(&mut self) {
    self.discovery.query.clear();
    self.discovery.query_lower.clear();
  }

  /// Set the discovery query to an arbitrary string (used by slash-palette
  /// completion). Refreshes the lowercased mirror.
  pub fn set_discovery_query(&mut self, s: String) {
    self.discovery.query = s;
    self.discovery.query_lower = self.discovery.query.to_lowercase();
  }

  pub(crate) fn set_workflow_state_for_url(
    &mut self,
    url: &str,
    state: WorkflowState,
  ) -> bool {
    let mut found = false;
    for item in self.items.iter_mut() {
      if item.url == url {
        item.workflow_state = state;
        found = true;
        break;
      }
    }
    if !found {
      for item in self.discovery.items.iter_mut() {
        if item.url == url {
          item.workflow_state = state;
          found = true;
          break;
        }
      }
    }
    if found {
      self.persisted_states.insert(url.to_string(), state);
      self.invalidate_visible_cache();
      self.invalidate_counts_cache();
    }
    found
  }

  pub fn mutate_history<R>(
    &mut self,
    f: impl FnOnce(&mut Vec<crate::history::HistoryEntry>) -> R,
  ) -> R {
    let r = f(&mut self.history);
    self.invalidate_filtered_history_cache();
    r
  }

  /// Mutator chokepoint for `history_filter`. The filtered_history_cache
  /// depends on the time-window filter just as much as on history itself.
  pub fn mutate_history_filter(
    &mut self,
    f: impl FnOnce(&mut crate::history::HistoryFilter),
  ) {
    f(&mut self.history_filter);
    self.invalidate_filtered_history_cache();
  }

  /// Mutator chokepoint for `library_filter`. The visible_cache for the
  /// Library tab depends on this filter; cycling the chip selection must
  /// invalidate.
  pub fn mutate_library_filter(
    &mut self,
    f: impl FnOnce(&mut crate::library::LibraryFilter),
  ) {
    f(&mut self.library_filter);
    self.invalidate_visible_cache();
  }

  pub(crate) fn mutate_filters(&mut self, f: impl FnOnce(&mut FilterState)) {
    f(&mut self.active_filters);
    self.invalidate_visible_cache();
    self.invalidate_filter_summary_cache();
    self.invalidate_filtered_history_cache();
  }
}
