//! Async, incremental ranking engine (ADR-013).
//!
//! Wraps [`nucleo::Nucleo`] — the fzf/skim-class matcher Helix uses — so
//! fuzzy ranking runs on a background thread pool instead of blocking the
//! render loop on every keystroke. The UI pushes items + a query and
//! polls [`FeedSearch::tick`] each frame for the latest ranked snapshot.
//!
//! ## Grammar → columns
//!
//! `nucleo`'s [`MultiPattern`](nucleo::pattern::MultiPattern) matches one
//! atom-set per column and ANDs the columns — which is exactly our
//! conjunctive field grammar. We use four columns:
//!
//! | Col | Text | Fed by [`Query`] bucket |
//! |-----|------|-------------------------|
//! | 0   | title | `ti:` / `title:` |
//! | 1   | authors (space-joined) | `au:` / `author:` |
//! | 2   | abstract | `abs:` / `abstract:` |
//! | 3   | title ¶ authors ¶ abstract | free text |
//!
//! An empty column pattern matches everything, so unused fields impose no
//! constraint. Free text matches column 3 (everything), with `title`
//! placed first so nucleo's start-of-haystack bonus naturally ranks a
//! title hit above an abstract-only hit — recovering ADR-012's field
//! weighting implicitly.
//!
//! `cat:` and `year:` are controlled-vocabulary / numeric gates that
//! nucleo can't express; they stay [`Query`]-side and are applied by the
//! caller (`visible_indices_for`) around this engine's ranked output.
//! This engine handles only the fuzzy-ranked text columns.
//!
//! The worker is keyed on each item's **URL**, not its corpus index, so a
//! mid-search re-sort of `items_store` can't stale the ranking; the caller
//! re-maps each ranked URL to its current index (ADR-013 §D5).

use std::sync::Arc;

use nucleo::pattern::{CaseMatching, Normalization};
use nucleo::{Config, Nucleo, Utf32String};

use super::Query;

const COL_TITLE: u32 = 0;
const COL_AUTHORS: u32 = 1;
const COL_ABSTRACT: u32 = 2;
const COL_COMBINED: u32 = 3;
const COLUMNS: u32 = 4;

/// Background fuzzy-ranking worker over the feed corpus. `T = String` is
/// the item's **URL** — a stable identity, unlike the corpus index, which
/// `items_store` reshuffles whenever it re-sorts on merge. Items are
/// injected incrementally by URL; [`ranked_urls`](Self::ranked_urls)
/// returns matching URLs best-match-first, and the caller maps each back
/// to its current index via `items_store.find_index_by_url` (ADR-013 §D5).
pub struct FeedSearch {
  nucleo: Nucleo<String>,
  /// URLs already pushed to the worker — lets [`sync`](Self::sync) inject
  /// only newly-arrived items instead of rebuilding the whole corpus.
  injected: std::collections::HashSet<String>,
}

impl FeedSearch {
  pub fn new() -> Self {
    // No-op notify: one-research's frame loop polls `tick` rather than reacting
    // to the worker's wake-up callback (the loop folds `Status::running`
    // into its cadence instead).
    let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
    // `prefer_prefix` rewards matches nearer the start of the haystack.
    // Combined with title-first ordering in column 3, this lets a title
    // hit outrank an abstract-only hit — recovering ADR-012's field
    // weighting via nucleo's own scoring rather than a separate pass.
    let mut config = Config::DEFAULT;
    config.prefer_prefix = true;
    let nucleo = Nucleo::new(config, notify, None, COLUMNS);
    Self { nucleo, injected: std::collections::HashSet::new() }
  }

  /// Inject any items whose URL hasn't been pushed yet (incremental — no
  /// restart). Safe to call after a merge that re-sorted the corpus: URLs
  /// already present are skipped, and stored ranking is keyed on URL so
  /// re-ordering doesn't matter.
  pub fn sync(&mut self, items: &[crate::models::FeedItem]) {
    let injector = self.nucleo.injector();
    for item in items {
      if self.injected.contains(&item.url) {
        continue;
      }
      injector.push(item.url.clone(), |_, columns| {
        let authors = item.authors.join(" ");
        columns[COL_TITLE as usize] = Utf32String::from(item.title.as_str());
        columns[COL_AUTHORS as usize] = Utf32String::from(authors.as_str());
        columns[COL_ABSTRACT as usize] =
          Utf32String::from(item.summary_short.as_str());
        // Title first so a title hit earns nucleo's start-of-haystack
        // bonus and outranks an abstract-only hit.
        columns[COL_COMBINED as usize] = Utf32String::from(
          format!("{}\n{}\n{}", item.title, authors, item.summary_short)
            .as_str(),
        );
      });
      self.injected.insert(item.url.clone());
    }
  }

  /// Drop the entire corpus (worker store + the injected-URL set). Used
  /// when items_store is replaced wholesale (a refresh `clear`), after
  /// which the caller re-[`sync`](Self::sync)s.
  pub fn reset(&mut self) {
    self.nucleo.restart(true);
    self.injected.clear();
  }

  /// Set the query. Maps each [`Query`] bucket to its column. `append` is
  /// `true` when the new query extends the previous one (a fresh keystroke
  /// at the end), letting nucleo re-match only prior survivors.
  pub fn set_query(&mut self, query: &Query, append: bool) {
    let columns = [
      (COL_TITLE, query.title.join(" ")),
      (COL_AUTHORS, query.author.join(" ")),
      (COL_ABSTRACT, query.summary.join(" ")),
      (COL_COMBINED, query.free.join(" ")),
    ];
    for (col, text) in columns {
      self.nucleo.pattern.reparse(
        col as usize,
        &text,
        CaseMatching::Smart,
        Normalization::Smart,
        append,
      );
    }
  }

  /// Advance the worker up to `timeout_ms` and return its status.
  /// `status.running` is `true` while matching is still in flight;
  /// `status.changed` is `true` when the snapshot updated.
  pub fn tick(&mut self, timeout_ms: u64) -> nucleo::Status {
    self.nucleo.tick(timeout_ms)
  }

  /// URLs matching the current query, best match first. The caller maps
  /// each back to its current corpus index.
  pub fn ranked_urls(&self) -> Vec<String> {
    let snapshot = self.nucleo.snapshot();
    snapshot.matched_items(..).map(|item| item.data.clone()).collect()
  }
}

impl Default for FeedSearch {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // Distinct URL per item — the engine keys on URL, so test fixtures
  // must not collide (fixtures::variant(0) reuses one URL).
  fn item(
    title: &str,
    authors: &[&str],
    summary: &str,
  ) -> crate::models::FeedItem {
    let mut it = crate::models::fixtures::variant(0);
    it.url = format!("http://example.test/{title}");
    it.title = title.to_string();
    it.authors = authors.iter().map(|s| s.to_string()).collect();
    it.summary_short = summary.to_string();
    it
  }

  /// Drive the background worker to completion (tiny corpus settles fast).
  fn settle(search: &mut FeedSearch) {
    for _ in 0..1000 {
      if !search.tick(10).running {
        break;
      }
    }
  }

  #[test]
  fn free_text_matches_across_title_and_abstract() {
    let items = vec![
      item("Robotics Manipulation", &["A"], "control of arms"),
      item("A Model", &["B"], "we study robotics policies"),
      item("Unrelated", &["C"], "image synthesis"),
    ];
    let mut s = FeedSearch::new();
    s.sync(&items);
    s.set_query(&Query::parse("robotics"), false);
    settle(&mut s);
    let ranked = s.ranked_urls();
    assert!(ranked.contains(&items[0].url), "title match should appear");
    assert!(ranked.contains(&items[1].url), "abstract match should appear");
    assert!(!ranked.contains(&items[2].url), "non-match should be excluded");
  }

  #[test]
  fn field_scope_restricts_to_its_column() {
    let items = vec![
      item("Diffusion Models", &["A"], "image synthesis"),
      item("A Survey", &["B"], "we revisit diffusion methods"),
    ];
    let mut s = FeedSearch::new();
    s.sync(&items);
    // `ti:` targets the title column only — the abstract hit must NOT match.
    s.set_query(&Query::parse("ti:diffusion"), false);
    settle(&mut s);
    assert_eq!(
      s.ranked_urls(),
      vec![items[0].url.clone()],
      "only the title-match should survive ti:"
    );
  }

  #[test]
  fn title_hit_outranks_abstract_only_hit() {
    let items = vec![
      item("A Model", &["A"], "we revisit attention mechanisms"),
      item("Attention Is All You Need", &["B"], "a sequence model"),
    ];
    let mut s = FeedSearch::new();
    s.sync(&items);
    s.set_query(&Query::parse("attention"), false);
    settle(&mut s);
    assert_eq!(
      s.ranked_urls().first(),
      Some(&items[1].url),
      "title hit should rank ahead of abstract-only hit"
    );
  }

  #[test]
  fn sync_is_incremental_and_idempotent() {
    let mut items = vec![item("Diffusion Models", &["A"], "image synthesis")];
    let mut s = FeedSearch::new();
    s.sync(&items);
    s.sync(&items); // re-sync the same corpus: must not double-inject
    items.push(item("Robotics", &["B"], "control")); // a new arrival
    s.sync(&items); // pushes only the new URL
    s.set_query(&Query::parse("diffusion"), false);
    settle(&mut s);
    assert_eq!(
      s.ranked_urls(),
      vec![items[0].url.clone()],
      "exactly one diffusion match despite repeated syncs"
    );
  }
}
