//! Subject Browser worker pipeline (ADR-010 §D3).
//!
//! Browse fetches are selection-driven — one fetch per `Enter` keystroke
//! on a Category. They use their own message type and channel rather
//! than reusing the bulk-refresh `FetchMessage` envelope: `fetch_rx` has
//! a refresh-scoped lifecycle (created at refresh start, dropped at
//! `AllComplete`), but Browse needs a long-lived receiver that survives
//! across many fetches. The discovery pipeline (`discovery/mod.rs`)
//! uses the same own-type / own-channel pattern.
//!
//! `BrowseModel.rx` consumes [`BrowseMessage`]s drained per-frame by
//! `App::process_incoming_browse()`. The merge into `workspace.items`
//! piggybacks on the shared dedup helper extracted from
//! `process.rs::process_incoming` so workflow-state preservation and
//! URL / arxiv-ID dedup behave identically to the bulk-refresh path.

pub mod pipeline;

use crate::models::FeedItem;

/// Messages emitted by [`pipeline::spawn_browse_fetch`].
///
/// Every variant carries the originating `category` code so the
/// consumer can route the outcome back to the right slot in
/// `BrowseModel.loaded_categories` / `BrowseModel.inflight`.
pub enum BrowseMessage {
  /// Successful fetch — items will be merged into `workspace.items` by
  /// the shared dedup helper, then folded into the category's
  /// `CategoryBuffer` in `BrowseModel.loaded_categories`. `start` is the
  /// arXiv page offset this batch was fetched at (ADR-015 §F4): `0`
  /// replaces the buffer (first page), `> 0` appends and advances it.
  Items { category: String, start: usize, items: Vec<FeedItem> },
  /// Fetch failed (HTTP / parse error). The category is removed from
  /// `inflight` and a status notification is shown. No partial items
  /// land in the cache.
  Error { category: String, error: String },
  /// Free-text arXiv search results (see `pipeline::spawn_arxiv_search`).
  /// Reuses the long-lived browse channel rather than its own — search,
  /// like a category fetch, is "async arXiv items → merge into the
  /// store." Items merge via the shared dedup helper; unlike `Items`
  /// these are *not* recorded in `loaded_categories` (a free-text query
  /// isn't a taxonomy node). On error the worker sends an empty `items`
  /// and logs the cause, so the search-loading flag always clears.
  SearchResults { query: String, items: Vec<FeedItem> },
}
