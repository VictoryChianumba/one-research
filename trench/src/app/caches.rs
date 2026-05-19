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
  /// Library tab depends on this filter; cycling the chip selection must
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
