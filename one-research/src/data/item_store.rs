//! `ItemStore` — coordinated triple of items + dedup indices.
//!
//! Owns the invariant that `url_index` and `arxiv_id_index` map into
//! `items` at all times. Mutation goes through methods (`push`,
//! `replace_at`, `sort_by`, `rebuild_indices`); reads go through
//! methods (`find_by_url`, `get`, `iter`). Field access is module-
//! private, so foreign code cannot accidentally produce torn state.
//!
//! Introduced by [ADR-007](../../../docs/adr/ADR-007-item-store.md). PR 1
//! lands the type and its tests; PR 2 wires it into `Workspace`.

use std::collections::HashMap;

use crate::models::{FeedItem, arxiv_id_from_url};

/// See module-level docs.
#[derive(Default)]
pub struct ItemStore {
  items: Vec<FeedItem>,
  url_index: HashMap<String, usize>,
  arxiv_id_index: HashMap<String, usize>,
}

impl ItemStore {
  // ── Reads ───────────────────────────────────────────────────────────

  pub fn len(&self) -> usize {
    self.items.len()
  }

  pub fn is_empty(&self) -> bool {
    self.items.is_empty()
  }

  pub fn get(&self, idx: usize) -> Option<&FeedItem> {
    self.items.get(idx)
  }

  pub fn get_mut(&mut self, idx: usize) -> Option<&mut FeedItem> {
    self.items.get_mut(idx)
  }

  pub fn iter(&self) -> std::slice::Iter<'_, FeedItem> {
    self.items.iter()
  }

  pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, FeedItem> {
    self.items.iter_mut()
  }

  /// Slice escape hatch for hot loops that need to index repeatedly
  /// without re-running `get` bounds checks. Returns the underlying
  /// `Vec<FeedItem>` as a borrowed slice — reads only.
  pub fn items(&self) -> &[FeedItem] {
    &self.items
  }

  pub fn find_index_by_url(&self, url: &str) -> Option<usize> {
    self.url_index.get(url).copied()
  }

  pub fn find_by_url(&self, url: &str) -> Option<&FeedItem> {
    self.find_index_by_url(url).and_then(|i| self.items.get(i))
  }

  pub fn find_index_by_arxiv_id(&self, aid: &str) -> Option<usize> {
    self.arxiv_id_index.get(aid).copied()
  }

  pub fn find_by_arxiv_id(&self, aid: &str) -> Option<&FeedItem> {
    self.find_index_by_arxiv_id(aid).and_then(|i| self.items.get(i))
  }

  // ── Mutation ────────────────────────────────────────────────────────

  /// Append `item` and update both indices. Returns the new index.
  ///
  /// `url_index` and `arxiv_id_index` overwrite any prior entry for the
  /// same URL or arxiv id — mirrors the pre-encapsulation
  /// `process_incoming` behaviour for the "should never collide on
  /// push because the caller already deduped" path.
  pub fn push(&mut self, item: FeedItem) -> usize {
    let idx = self.items.len();
    self.url_index.insert(item.url.clone(), idx);
    if let Some(aid) = arxiv_id_from_url(&item.url) {
      self.arxiv_id_index.insert(aid.to_string(), idx);
    }
    self.items.push(item);
    idx
  }

  /// Replace the item at `idx`. Position does not change, so both
  /// indices stay valid — but if the replacement's URL or arxiv id
  /// differ from the original, stale entries would be left behind.
  /// In practice every caller in `process_incoming` replaces with an
  /// item whose URL matches; this method preserves that invariant by
  /// re-inserting the index entries for the new item's keys.
  ///
  /// No-op if `idx >= len()`.
  pub fn replace_at(&mut self, idx: usize, item: FeedItem) {
    if idx >= self.items.len() {
      return;
    }
    // Drop any stale index entries that pointed at this slot under
    // the *old* keys. Re-insert under the new keys (which usually
    // match — callers replace same-URL).
    let old_url = std::mem::take(&mut self.items[idx].url);
    self.url_index.remove(&old_url);
    if let Some(aid) = arxiv_id_from_url(&old_url) {
      self.arxiv_id_index.remove(aid);
    }
    self.url_index.insert(item.url.clone(), idx);
    if let Some(aid) = arxiv_id_from_url(&item.url) {
      self.arxiv_id_index.insert(aid.to_string(), idx);
    }
    self.items[idx] = item;
  }

  /// Sort items by `cmp`, then rebuild both indices.
  pub fn sort_by<F>(&mut self, cmp: F)
  where
    F: FnMut(&FeedItem, &FeedItem) -> std::cmp::Ordering,
  {
    self.items.sort_by(cmp);
    self.rebuild_indices();
  }

  /// Build an `ItemStore` from an existing `Vec<FeedItem>`. Used by
  /// cache-load paths and by tests that want to populate the store
  /// from a fixture in one step.
  pub fn from_items(items: Vec<FeedItem>) -> Self {
    let mut s = Self { items, ..Self::default() };
    s.rebuild_indices();
    s
  }

  /// Drop every item and clear both indices. Used by `App::reset_items`
  /// for the "blow away the corpus on user-triggered hard reload" path.
  pub fn clear(&mut self) {
    self.items.clear();
    self.url_index.clear();
    self.arxiv_id_index.clear();
  }

  /// Rebuild both indices from `items`. Use after a bulk mutation
  /// (cache load, manual delete sweep) where invariant maintenance
  /// across the operation isn't practical.
  pub fn rebuild_indices(&mut self) {
    self.url_index.clear();
    self.arxiv_id_index.clear();
    self.url_index.reserve(self.items.len());
    for (idx, item) in self.items.iter().enumerate() {
      self.url_index.insert(item.url.clone(), idx);
      if let Some(aid) = arxiv_id_from_url(&item.url) {
        self.arxiv_id_index.insert(aid.to_string(), idx);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::{
    ContentType, SignalLevel, SourcePlatform, WorkflowState,
  };

  fn item(url: &str) -> FeedItem {
    FeedItem {
      id: url.to_string(),
      title: "t".to_string(),
      source_platform: SourcePlatform::ArXiv,
      source_name: "test".to_string(),
      content_type: ContentType::Paper,
      domain_tags: Vec::new(),
      signal: SignalLevel::Primary,
      published_at: "2026-01-01".to_string(),
      authors: Vec::new(),
      summary_short: String::new(),
      workflow_state: WorkflowState::Inbox,
      url: url.to_string(),
      upvote_count: 0,
      github_repo: None,
      github_owner: None,
      github_repo_name: None,
      benchmark_results: Vec::new(),
      full_content: None,
      title_lower: "t".to_string(),
      authors_lower: Vec::new(),
    }
  }

  #[test]
  fn default_is_empty() {
    let s = ItemStore::default();
    assert_eq!(s.len(), 0);
    assert!(s.is_empty());
    assert!(s.find_by_url("anything").is_none());
  }

  #[test]
  fn push_updates_url_index() {
    // Core invariant: push must register the URL in url_index *and*
    // return the index that find_index_by_url will see.
    let mut s = ItemStore::default();
    let idx = s.push(item("https://example.com/a"));
    assert_eq!(idx, 0);
    assert_eq!(s.find_index_by_url("https://example.com/a"), Some(0));
    assert!(s.find_by_url("https://example.com/a").is_some());
  }

  #[test]
  fn push_indexes_arxiv_url_in_both_maps() {
    // ArXiv URLs land in both indices; non-arXiv URLs only in url_index.
    let mut s = ItemStore::default();
    s.push(item("https://arxiv.org/abs/2401.12345"));
    assert!(s.find_by_url("https://arxiv.org/abs/2401.12345").is_some());
    assert!(s.find_by_arxiv_id("2401.12345").is_some());
  }

  #[test]
  fn push_does_not_index_non_arxiv_urls_by_arxiv_id() {
    let mut s = ItemStore::default();
    s.push(item("https://example.com/blog"));
    assert!(s.find_by_arxiv_id("blog").is_none());
  }

  #[test]
  fn replace_at_keeps_indices_pointing_to_same_slot() {
    // Replace must not shift other items' positions. After replacing
    // item 0, find_by_url still points at 0 and item 1 is untouched.
    let mut s = ItemStore::default();
    s.push(item("https://a"));
    s.push(item("https://b"));
    let mut replacement = item("https://a");
    replacement.title = "new title".to_string();
    s.replace_at(0, replacement);
    assert_eq!(s.len(), 2);
    assert_eq!(s.find_index_by_url("https://a"), Some(0));
    assert_eq!(s.find_index_by_url("https://b"), Some(1));
    assert_eq!(s.get(0).unwrap().title, "new title");
  }

  #[test]
  fn replace_at_out_of_bounds_is_noop() {
    let mut s = ItemStore::default();
    s.push(item("https://a"));
    s.replace_at(99, item("https://ghost"));
    assert_eq!(s.len(), 1);
    assert!(s.find_by_url("https://ghost").is_none());
  }

  #[test]
  fn sort_by_rebuilds_indices() {
    // After sort, url_index must point to the new positions.
    // Regression guard against "sort but forget to rebuild" — exactly
    // the bug class C9 closes.
    let mut s = ItemStore::default();
    s.push(item("https://b"));
    s.push(item("https://a"));
    s.push(item("https://c"));
    s.sort_by(|x, y| x.url.cmp(&y.url));
    assert_eq!(s.get(0).unwrap().url, "https://a");
    assert_eq!(s.get(1).unwrap().url, "https://b");
    assert_eq!(s.get(2).unwrap().url, "https://c");
    assert_eq!(s.find_index_by_url("https://a"), Some(0));
    assert_eq!(s.find_index_by_url("https://b"), Some(1));
    assert_eq!(s.find_index_by_url("https://c"), Some(2));
  }

  #[test]
  fn rebuild_indices_is_idempotent() {
    // Calling rebuild twice in a row produces the same state.
    let mut s = ItemStore::default();
    s.push(item("https://arxiv.org/abs/2401.00001"));
    s.push(item("https://example.com/post"));
    s.rebuild_indices();
    let urls: Vec<usize> =
      ["https://arxiv.org/abs/2401.00001", "https://example.com/post"]
        .iter()
        .filter_map(|u| s.find_index_by_url(u))
        .collect();
    s.rebuild_indices();
    let urls2: Vec<usize> =
      ["https://arxiv.org/abs/2401.00001", "https://example.com/post"]
        .iter()
        .filter_map(|u| s.find_index_by_url(u))
        .collect();
    assert_eq!(urls, urls2);
    assert_eq!(urls, vec![0, 1]);
  }

  // ── Invariant witnesses (N2, ADR-007) ───────────────────────────────
  //
  // The per-operation tests above check specific behaviours. These tests
  // lock the *whole-state invariant* that ItemStore exists to enforce:
  //
  //   "Every entry in url_index / arxiv_id_index points to a live item
  //    in items whose key matches the index key."
  //
  // Bug class C9 was *named after* — items_store + url_index + arxiv_id_index
  // drifting apart silently. The check_invariants helper below is the
  // witness; every mutation path runs through it after a sequence of ops.

  /// Walks every (key, idx) entry in both indices and asserts the matching
  /// item exists at that position with the expected URL / arxiv-id key.
  /// Also asserts no items in `items` are *missing* from `url_index`.
  fn check_invariants(s: &ItemStore) {
    // url_index → items: every entry resolves to a live item with
    // matching url.
    for (k, &idx) in &s.url_index {
      let it = s.items.get(idx).unwrap_or_else(|| {
        panic!("url_index entry {k:?}→{idx} points past end of items")
      });
      assert_eq!(
        &it.url, k,
        "url_index key {k:?} points to item with url {:?} at idx {idx}",
        it.url
      );
    }
    // arxiv_id_index → items: every entry resolves to an item whose URL
    // really is an arXiv URL with the matching id.
    for (aid, &idx) in &s.arxiv_id_index {
      let it = s.items.get(idx).unwrap_or_else(|| {
        panic!("arxiv_id_index entry {aid:?}→{idx} points past end of items")
      });
      let derived = arxiv_id_from_url(&it.url).unwrap_or_else(|| {
        panic!(
          "arxiv_id_index key {aid:?} points at non-arxiv item {:?}",
          it.url
        )
      });
      assert_eq!(
        derived, aid,
        "arxiv_id_index key {aid:?} disagrees with derived id {derived:?}"
      );
    }
    // items → url_index: every live item is reachable via url_index.
    for (idx, it) in s.items.iter().enumerate() {
      let found = s.url_index.get(&it.url).copied();
      assert_eq!(
        found,
        Some(idx),
        "item at idx {idx} url={:?} not findable via url_index (got {found:?})",
        it.url
      );
    }
  }

  #[test]
  fn n2_invariants_hold_after_push_sequence() {
    // Witness: a stream of mixed arxiv/non-arxiv pushes leaves both
    // indices consistent and complete.
    let mut s = ItemStore::default();
    for url in [
      "https://arxiv.org/abs/2401.00001",
      "https://example.com/a",
      "https://arxiv.org/abs/2401.00002",
      "https://example.com/b",
      "https://arxiv.org/abs/2401.00003",
    ] {
      s.push(item(url));
      check_invariants(&s);
    }
    assert_eq!(s.len(), 5);
    assert_eq!(s.arxiv_id_index.len(), 3);
    assert_eq!(s.url_index.len(), 5);
  }

  #[test]
  fn n2_replace_at_with_different_url_drops_stale_index_entry() {
    // The replace_at branch at lines 107-111 cleans up the old key.
    // Without that, the old URL would still resolve to the slot — a
    // stale entry. This test exercises the cleanup branch directly.
    let mut s = ItemStore::default();
    s.push(item("https://old"));
    s.push(item("https://keep"));
    s.replace_at(0, item("https://new"));
    check_invariants(&s);
    // Old URL no longer resolves anywhere.
    assert!(s.find_by_url("https://old").is_none(), "stale url_index entry");
    // New URL resolves to slot 0.
    assert_eq!(s.find_index_by_url("https://new"), Some(0));
    // Sibling untouched.
    assert_eq!(s.find_index_by_url("https://keep"), Some(1));
  }

  #[test]
  fn n2_replace_at_arxiv_to_non_arxiv_drops_arxiv_entry() {
    // Cross the arxiv boundary: replacing an arxiv item with a non-arxiv
    // item must remove the arxiv_id_index entry for the old item.
    let mut s = ItemStore::default();
    s.push(item("https://arxiv.org/abs/2401.99999"));
    s.replace_at(0, item("https://blog.example.com/post"));
    check_invariants(&s);
    assert!(s.find_by_arxiv_id("2401.99999").is_none(), "stale arxiv entry");
    assert!(s.find_by_url("https://blog.example.com/post").is_some());
  }

  #[test]
  fn n2_replace_at_non_arxiv_to_arxiv_inserts_arxiv_entry() {
    // Other direction: starting non-arxiv, replacement is arxiv. The
    // arxiv_id_index must gain the new entry. Previously there'd be no
    // arxiv_id_index entry at all for slot 0, so this catches "forgot to
    // insert on the new-key side" bugs.
    let mut s = ItemStore::default();
    s.push(item("https://blog.example.com/post"));
    s.replace_at(0, item("https://arxiv.org/abs/2402.55555"));
    check_invariants(&s);
    assert_eq!(s.find_index_by_arxiv_id("2402.55555"), Some(0));
  }

  #[test]
  fn n2_invariants_hold_through_mixed_sequence() {
    // The hard test: push some, replace some (mixing same-URL and
    // different-URL), sort, push more, replace again. Invariant must
    // hold after every step — bugs that only show up in sequences are
    // why this exists separately from the per-op tests.
    let mut s = ItemStore::default();
    s.push(item("https://arxiv.org/abs/2401.00001"));
    s.push(item("https://b"));
    s.push(item("https://arxiv.org/abs/2401.00002"));
    check_invariants(&s);

    // Same-URL replace (title update, common case).
    let mut upd = item("https://b");
    upd.title = "updated".into();
    s.replace_at(1, upd);
    check_invariants(&s);

    // Different-URL replace (rare but legal).
    s.replace_at(0, item("https://arxiv.org/abs/2401.00009"));
    check_invariants(&s);
    assert!(s.find_by_arxiv_id("2401.00001").is_none());
    assert_eq!(s.find_index_by_arxiv_id("2401.00009"), Some(0));

    // Sort: positions move; rebuild_indices runs internally.
    s.sort_by(|x, y| x.url.cmp(&y.url));
    check_invariants(&s);

    // Push and replace after sort — exercises the post-sort indexing
    // path and proves the prior sort didn't leak any entries.
    s.push(item("https://example.com/late"));
    check_invariants(&s);
    s.replace_at(s.len() - 1, item("https://arxiv.org/abs/2403.00000"));
    check_invariants(&s);
  }

  #[test]
  fn n2_clear_drops_all_indices() {
    let mut s = ItemStore::default();
    s.push(item("https://arxiv.org/abs/2401.00001"));
    s.push(item("https://b"));
    s.clear();
    check_invariants(&s);
    assert_eq!(s.len(), 0);
    assert!(s.url_index.is_empty());
    assert!(s.arxiv_id_index.is_empty());
  }

  #[test]
  fn n2_from_items_builds_invariants_from_scratch() {
    // Witness for the cache-load path: from_items takes an arbitrary Vec
    // and must produce an invariant-clean ItemStore via rebuild_indices.
    let v = vec![
      item("https://arxiv.org/abs/2401.00001"),
      item("https://blog/post"),
      item("https://arxiv.org/abs/2401.00002"),
    ];
    let s = ItemStore::from_items(v);
    check_invariants(&s);
    assert_eq!(s.len(), 3);
    assert_eq!(s.find_index_by_arxiv_id("2401.00001"), Some(0));
    assert_eq!(s.find_index_by_arxiv_id("2401.00002"), Some(2));
  }
}
