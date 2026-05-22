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
//! caller as a pre/post filter (PR 2). This engine handles only the
//! fuzzy-ranked text columns.
//!
//! Standalone in PR 1: constructed and tested here, not yet wired into
//! the feed pipeline.

use std::sync::Arc;

use nucleo::pattern::{CaseMatching, Normalization};
use nucleo::{Config, Nucleo, Utf32String};

use super::Query;

const COL_TITLE: u32 = 0;
const COL_AUTHORS: u32 = 1;
const COL_ABSTRACT: u32 = 2;
const COL_COMBINED: u32 = 3;
const COLUMNS: u32 = 4;

/// Background fuzzy-ranking worker over the feed corpus. `T = u32` is the
/// item's index into the corpus slice it was [`reload`](Self::reload)ed
/// from; [`ranked_indices`](Self::ranked_indices) returns those indices in
/// best-match-first order.
pub struct FeedSearch {
  nucleo: Nucleo<u32>,
}

impl FeedSearch {
  pub fn new() -> Self {
    // No-op notify: trench's frame loop polls `tick` rather than reacting
    // to the worker's wake-up callback (PR 2 folds `Status::running` into
    // the loop cadence instead).
    let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
    // `prefer_prefix` rewards matches nearer the start of the haystack.
    // Combined with title-first ordering in column 3, this lets a title
    // hit outrank an abstract-only hit — recovering ADR-012's field
    // weighting via nucleo's own scoring rather than a separate pass.
    let mut config = Config::DEFAULT;
    config.prefer_prefix = true;
    let nucleo = Nucleo::new(config, notify, None, COLUMNS);
    Self { nucleo }
  }

  /// Replace the corpus. Clears the worker's store and re-injects one
  /// entry per item, filling the four match columns. PR 2 will switch
  /// background ingestion to incremental `injector().push` of the delta
  /// instead of a full reload.
  pub fn reload(&mut self, items: &[crate::models::FeedItem]) {
    self.nucleo.restart(true);
    let injector = self.nucleo.injector();
    for (idx, item) in items.iter().enumerate() {
      injector.push(idx as u32, |_, columns| {
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
    }
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

  /// Corpus indices for the current query, best match first.
  pub fn ranked_indices(&self) -> Vec<u32> {
    let snapshot = self.nucleo.snapshot();
    snapshot.matched_items(..).map(|item| *item.data).collect()
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

  fn item(
    title: &str,
    authors: &[&str],
    summary: &str,
  ) -> crate::models::FeedItem {
    let mut it = crate::models::fixtures::variant(0);
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
    s.reload(&items);
    s.set_query(&Query::parse("robotics"), false);
    settle(&mut s);
    let ranked = s.ranked_indices();
    assert!(ranked.contains(&0), "title match should appear");
    assert!(ranked.contains(&1), "abstract match should appear");
    assert!(!ranked.contains(&2), "non-match should be excluded");
  }

  #[test]
  fn field_scope_restricts_to_its_column() {
    let items = vec![
      item("Diffusion Models", &["A"], "image synthesis"),
      item("A Survey", &["B"], "we revisit diffusion methods"),
    ];
    let mut s = FeedSearch::new();
    s.reload(&items);
    // `ti:` targets the title column only — the abstract hit must NOT match.
    s.set_query(&Query::parse("ti:diffusion"), false);
    settle(&mut s);
    let ranked = s.ranked_indices();
    assert_eq!(ranked, vec![0], "only the title-match should survive ti:");
  }

  #[test]
  fn title_hit_outranks_abstract_only_hit() {
    let items = vec![
      item("A Model", &["A"], "we revisit attention mechanisms"),
      item("Attention Is All You Need", &["B"], "a sequence model"),
    ];
    let mut s = FeedSearch::new();
    s.reload(&items);
    s.set_query(&Query::parse("attention"), false);
    settle(&mut s);
    let ranked = s.ranked_indices();
    assert_eq!(
      ranked.first(),
      Some(&1),
      "title hit (item 1) should rank ahead of abstract-only hit (item 0)"
    );
  }
}
