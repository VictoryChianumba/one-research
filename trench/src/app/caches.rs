use crate::app::{App, FilterState};
use crate::effect::Effect;
use crate::models::WorkflowState;

/// Effect routing + mutator chokepoints.
///
/// Pre-ADR-009 this file owned both the cache fields' invalidator methods
/// (`invalidate_visible_cache` and four siblings) and the
/// `observe_effect` translator that mapped each [`Effect`] to the right
/// invalidations.  ADR-009 cluster #5 moved both onto `RenderCaches`
/// (see `app/state/render_caches.rs`); this file keeps the App-level
/// orchestration:
///
///   - [`App::route_effects`] — drains a `Vec<Effect>` and forwards each
///     to the observer.  Future non-cache observers (focus, notifications,
///     audit logs) plug in here alongside the cache observer.
///   - The `mutate_*` chokepoints below — wrap state mutations with
///     effect emission so individual call sites don't need to remember
///     which effect to emit.
impl App {
  /// Drain a vec of effects, applying each to every registered observer.
  /// Today the only observer is [`RenderCaches::observe`]; future
  /// observers register the same way.
  pub(crate) fn route_effects(&self, effects: &[Effect]) {
    for effect in effects {
      self.render_caches.observe(effect);
    }
  }

  // ── Mutator chokepoints ─────────────────────────────────────────────
  //
  // These wrap state mutations with effect emission + routing. The
  // public-facing helpers (`push_search_char`, `pop_search_char`,
  // `clear_search_query`, etc.) call into these so individual call
  // sites don't need to remember which effect to emit.

  pub(crate) fn mutate_search_query(&mut self, f: impl FnOnce(&mut String)) {
    f(&mut self.feed.search_query);
    self.feed.search_query_lower = self.feed.search_query.to_lowercase();
    self.route_effects(&[Effect::SearchQueryChanged]);
  }

  pub fn push_search_char(&mut self, c: char) {
    self.mutate_search_query(|q| q.push(c));
    self.reset_active_feed_position();
    // A longer query can only shrink the match set, so `append = true`
    // lets nucleo re-match prior survivors instead of the whole corpus.
    if self.feed_search.is_none() {
      self.activate_feed_search();
    } else {
      self.refresh_feed_search_query(true);
    }
  }

  pub fn pop_search_char(&mut self) {
    self.mutate_search_query(|q| {
      q.pop();
    });
    self.reset_active_feed_position();
    if self.feed.search_query.is_empty() {
      self.deactivate_feed_search();
    } else {
      // Query shrank — nucleo must re-evaluate from scratch (`append = false`).
      self.refresh_feed_search_query(false);
    }
  }

  pub fn clear_search_query(&mut self) {
    self.mutate_search_query(|q| q.clear());
    self.deactivate_feed_search();
  }

  /// Create the nucleo search worker (if absent) and inject the current
  /// items_store corpus, then push the active query. Called on the first
  /// typed search char (ADR-013 §D1).
  pub fn activate_feed_search(&mut self) {
    if self.feed_search.is_none() {
      let mut engine = crate::search::engine::FeedSearch::new();
      engine.sync(self.workspace.items_store.items());
      self.feed_search = Some(engine);
    }
    self.refresh_feed_search_query(false);
  }

  /// Drop the worker, freeing its thread pool. Called when search clears.
  pub fn deactivate_feed_search(&mut self) {
    self.feed_search = None;
  }

  /// Push the current parsed query into the worker's pattern.
  pub(crate) fn refresh_feed_search_query(&mut self, append: bool) {
    let query = crate::search::Query::parse(&self.feed.search_query);
    if let Some(engine) = self.feed_search.as_mut() {
      engine.set_query(&query, append);
    }
  }

  /// Inject newly-arrived items into the worker (incremental, by URL).
  /// Cheap after any merge: re-sorts are a no-op and already-seen URLs
  /// are skipped (ADR-013 §D5).
  pub fn sync_feed_search_corpus(&mut self) {
    if self.feed_search.is_none() {
      return;
    }
    let items = self.workspace.items_store.items();
    if let Some(engine) = self.feed_search.as_mut() {
      engine.sync(items);
    }
  }

  /// Rebuild the worker corpus from scratch — used when items_store was
  /// replaced wholesale (a refresh `clear` shrinks the corpus).
  pub fn rebuild_feed_search_corpus(&mut self) {
    if self.feed_search.is_none() {
      return;
    }
    let query = crate::search::Query::parse(&self.feed.search_query);
    let items = self.workspace.items_store.items();
    if let Some(engine) = self.feed_search.as_mut() {
      engine.reset();
      engine.sync(items);
      engine.set_query(&query, false);
    }
  }

  // Discovery search-bar gestures live on `DiscoveryModel` directly
  // after C7 PR 3 (ADR-005 §S5). Call sites use `app.discovery.push_char(c)`
  // / `pop_char()` / `clear_query()` / `set_query(s)`.

  pub(crate) fn set_workflow_state_for_url(
    &mut self,
    url: &str,
    state: WorkflowState,
  ) -> bool {
    // Delegate to the W3-hybrid model method (ADR-001 D5). Split borrow
    // on disjoint fields: `feed`, `workspace` (items + persisted_states),
    // and `discovery` (the discovery.items fallback search path).
    let effects = self.feed.set_workflow_state_for_url(
      &mut self.workspace,
      &mut self.discovery,
      url,
      state,
    );
    let found = !effects.is_empty();
    if found {
      self.route_effects(&effects);
    }
    found
  }

  pub fn mutate_history<R>(
    &mut self,
    f: impl FnOnce(&mut Vec<crate::history::HistoryEntry>) -> R,
  ) -> R {
    let r = f(&mut self.workspace.history);
    self.route_effects(&[Effect::HistoryMutated]);
    r
  }

  /// Mutator chokepoint for `history_filter`. The filtered_history_cache
  /// depends on the time-window filter just as much as on history itself.
  pub fn mutate_history_filter(
    &mut self,
    f: impl FnOnce(&mut crate::history::HistoryFilter),
  ) {
    f(&mut self.feed.history_filter);
    self.route_effects(&[Effect::HistoryFilterChanged]);
  }

  /// Mutator chokepoint for `library_filter`. The visible_cache for the
  /// Library tab depends on this filter; changing the scoped view must
  /// invalidate.
  pub fn mutate_library_filter(
    &mut self,
    f: impl FnOnce(&mut crate::library::LibraryFilter),
  ) {
    f(&mut self.feed.library_filter);
    self.route_effects(&[Effect::LibraryFilterChanged]);
  }

  pub(crate) fn mutate_filters(&mut self, f: impl FnOnce(&mut FilterState)) {
    f(&mut self.feed.active_filters);
    self.route_effects(&[Effect::FiltersChanged]);
  }
}
