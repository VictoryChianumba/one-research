use crate::app::{App, FilterState};
use crate::effect::Effect;
use crate::models::WorkflowState;

/// Cache invalidators + the effect-routing observer.
///
/// Five independent caches:
/// - `visible_cache`: filtered + searched item indices (per FeedTab)
/// - `counts_cache`: workflow-state breakdown + recent-48h + queue preview
/// - `filter_source_names_cache`: sorted unique source-label set
/// - `filter_summary_cache`: human-readable summary of `active_filters`
/// - `filtered_history_cache`: indices of history entries visible under
///   the current time window + search + active filters
///
/// Phase 3 reframes invalidation: instead of every mutator calling
/// `invalidate_*` directly, mutators emit [`Effect`] variants and the
/// observer below translates each effect into the correct invalidations.
/// This decouples surfaces from cache-implementation details — surfaces
/// know what semantically happened, not which caches it touched.
///
/// The chokepoint mutators (`mutate_search_query`, `mutate_history`,
/// etc.) remain as the API surface; their bodies progressively migrate
/// from direct invalidation to effect emission.
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

  /// Aggregate invalidator for every cache that derives from `app.workspace.items`.
  pub(crate) fn invalidate_items_derived_caches(&self) {
    self.invalidate_counts_cache();
    self.invalidate_filter_source_names_cache();
  }

  // ── Effect routing ──────────────────────────────────────────────────

  /// Drain a vec of effects, applying each to the cache layer.
  /// Phase 3 keystone: surfaces emit semantic events, the observer
  /// translates to invalidations. Future non-cache observers (focus,
  /// notifications, audit logs) plug in alongside without changing
  /// the surface emit sites.
  pub(crate) fn route_effects(&self, effects: &[Effect]) {
    for effect in effects {
      self.observe_effect(effect);
    }
  }

  /// Translate one [`Effect`] into the correct set of cache
  /// invalidations. The match is the *contract* between effects and
  /// caches; adding a new cache means updating one match arm, not
  /// auditing every emit site.
  fn observe_effect(&self, effect: &Effect) {
    match effect {
      Effect::SearchQueryChanged => {
        self.invalidate_visible_cache();
        self.invalidate_filtered_history_cache();
      }
      Effect::WorkflowStateChanged { .. } => {
        self.invalidate_visible_cache();
        self.invalidate_counts_cache();
      }
      Effect::HistoryMutated | Effect::HistoryFilterChanged => {
        self.invalidate_filtered_history_cache();
      }
      Effect::LibraryFilterChanged
      | Effect::SourcesEnabledChanged
      | Effect::TagsChanged => {
        self.invalidate_visible_cache();
      }
      Effect::FiltersChanged => {
        self.invalidate_visible_cache();
        self.invalidate_filter_summary_cache();
        self.invalidate_filtered_history_cache();
      }
      Effect::ItemsChanged => {
        self.invalidate_visible_cache();
        self.invalidate_items_derived_caches();
      }
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
