use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::time::Instant;

use crate::browse::BrowseMessage;
use crate::models::arxiv_taxonomy::{Archive, Category, Group, TAXONOMY};
use crate::primitives::ListState;

/// Papers fetched per Browse page (arXiv `max_results` for the Browse
/// path). The buffer uses it to tell a *full* page (more may remain)
/// from a *short* one (archive exhausted). ADR-015 PR 2 threads this
/// into `arxiv::fetch_page`; today `arxiv::fetch` hardcodes the same
/// value, so the two stay in lockstep (tripwire R1).
pub const BROWSE_PAGE_SIZE: usize = 50;

/// Per-category paging buffer (ADR-015 §F1). Supersedes the flat
/// `Vec<url>` ADR-010 recorded: alongside the fetched URLs it tracks
/// where the *next* page starts and whether the archive is exhausted.
/// PR 1 renders an honest per-category count from `urls`; PR 2 reads
/// `next_offset` / `exhausted` to page deeper on scroll.
#[derive(Debug, Default, Clone)]
pub struct CategoryBuffer {
  /// Fetched URLs in arrival order, resolved against `items_store`.
  pub urls: Vec<String>,
  /// arXiv `start` offset for the next page — equals the count fetched
  /// so far, since pages are contiguous from `start = 0`. Read by the
  /// scroll-tail pager (ADR-015 §F4).
  pub next_offset: usize,
  /// A short page (`< BROWSE_PAGE_SIZE`) came back: nothing more to pull.
  /// The pager stops once this is set.
  pub exhausted: bool,
}

impl CategoryBuffer {
  /// Build the buffer from a category's first page. `next_offset`
  /// advances past what we fetched; a short page means the archive is
  /// already exhausted. PR 2's append path extends this rather than
  /// replacing it.
  pub fn first_page(urls: Vec<String>) -> Self {
    let n = urls.len();
    Self { urls, next_offset: n, exhausted: n < BROWSE_PAGE_SIZE }
  }
}

/// Composition-root model for the Subject Browser tab (`FeedTab::Browse`).
/// Sibling to `FeedModel`, `DiscoveryModel`, etc.
///
/// ADR-011 reshape: a single right-side rail with drill-replace semantics.
/// The rail shows one taxonomy level at a time (Groups by default).
/// `Enter` on a Group pushes onto `rail_path` and the rail repaints
/// to show that Group's Archives. `Enter` on an Archive drills again
/// to Categories. `Enter` on a Category fires `spawn_browse_fetch`
/// (the ADR-010 §D3 worker pattern) and surfaces results in the
/// left-hand feed table. `Esc` / `h` / `Backspace` pops the path
/// back up one level.
///
/// The replaced state — `focused_column: u8`, parallel `archives` /
/// `categories` cursors, the `recent` cursor for a deleted details
/// pane — is gone. Only the rail cursor remains. See ADR-011 §E2.
///
/// The worker channel (`tx` / `rx`) and the session-scoped `inflight`
/// from ADR-010 PR 2 are unchanged; `loaded_categories` is now a
/// `CategoryBuffer` per code (ADR-015 §F1).
pub struct BrowseModel {
  /// Drill stack. Empty = at the Groups level. One entry = inside a
  /// Group, showing its Archives. Two entries = inside an Archive,
  /// showing its Categories. Three would be inside a Category, but
  /// leaf-level drills don't push the path — they fire a fetch
  /// instead, so the path stays at depth ≤ 2.
  pub rail_path: Vec<RailNode>,

  /// Cursor over the current rail level's rows. Reset on every push /
  /// pop because the underlying row set changes.
  pub rail_cursor: ListState,

  /// Local focus inside the Browse tab. `Tab` remains the global tab
  /// switcher; Browse uses `h` / `l` to move between the subject rail
  /// and the paper feed.
  pub focus: BrowseFocus,

  /// Per-category paging buffer fetched in this session (ADR-015 §F1).
  /// The Browse renderer resolves each buffer's URLs against
  /// `workspace.items_store`, so dedup + workflow-state preservation
  /// come for free; the rail reads the URL count for an honest
  /// per-category indicator (§F6).
  pub loaded_categories: HashMap<String, CategoryBuffer>,

  /// Category codes with an outstanding fetch. `Enter` short-circuits
  /// when the code is already in this set so rapid mashing doesn't
  /// fire parallel duplicate fetches.
  pub inflight: HashSet<String>,

  /// Arrival auto-fill settle anchor (ADR-015 §F5): the Category code the
  /// rail cursor currently rests on, plus when it arrived there. Reset
  /// whenever the cursor moves to a different category. The idle poll
  /// fires page 1 once the cursor has rested past the settle window.
  pub autofill_anchor: Option<(String, Instant)>,

  /// Categories already auto-filled this session. Prevents a failed
  /// fetch (which clears `inflight` but leaves no buffer) from re-firing
  /// every settle window — i.e. an auto-fill retry storm. `Enter` is
  /// unaffected, so a manual retry still works.
  pub autofill_attempted: HashSet<String>,

  /// Sender end of the persistent browse channel. Cloned by
  /// `spawn_browse_fetch` per spawn. Lives for the App lifetime.
  pub tx: mpsc::Sender<BrowseMessage>,

  /// Receiver end — drained per-frame by
  /// `App::process_incoming_browse()`. `Option` so tests can `.take()`
  /// it and synthesise messages directly into `loaded_categories`.
  pub rx: Option<mpsc::Receiver<BrowseMessage>>,
}

/// One step in the rail's drill stack. `Group` / `Archive` carry the
/// `id` of the selected node so the renderer can look up the next
/// level's row set from `TAXONOMY` without storing the borrowed
/// reference (lifetimes get sticky inside a model).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RailNode {
  Group(&'static str),
  Archive(&'static str, &'static str),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrowseFocus {
  Rail,
  Feed,
}

impl Default for BrowseModel {
  fn default() -> Self {
    Self::new()
  }
}

impl BrowseModel {
  /// Initialise at the Groups level with the rail cursor bounded to
  /// the taxonomy's group count. The persistent worker channel is
  /// created here — senders are cloned per `spawn_browse_fetch`.
  pub fn new() -> Self {
    let mut rail_cursor = ListState::default();
    rail_cursor.set_count(crate::models::arxiv_taxonomy::group_count());
    let (tx, rx) = mpsc::channel::<BrowseMessage>();
    Self {
      rail_path: Vec::new(),
      rail_cursor,
      focus: BrowseFocus::Rail,
      loaded_categories: HashMap::new(),
      inflight: HashSet::new(),
      autofill_anchor: None,
      autofill_attempted: HashSet::new(),
      tx,
      rx: Some(rx),
    }
  }

  /// Loaded paper count for a category code, or `None` if it has never
  /// been fetched this session. Drives the rail's honest per-category
  /// count (ADR-015 §F6): blank = unfetched, `0` = fetched-but-empty,
  /// `N` = N papers buffered.
  pub fn loaded_count(&self, code: &str) -> Option<usize> {
    self.loaded_categories.get(code).map(|b| b.urls.len())
  }

  /// Resolve the `Group` whose drill the user is currently inside, if
  /// any. Returns `None` at the Groups level.
  pub fn current_group(&self) -> Option<&'static Group> {
    let id = match self.rail_path.first()? {
      RailNode::Group(id) | RailNode::Archive(id, _) => *id,
    };
    TAXONOMY.iter().find(|g| g.id == id)
  }

  /// Resolve the `Archive` whose drill the user is currently inside,
  /// if any. Only returns `Some` when `rail_path` has depth ≥ 2.
  pub fn current_archive(&self) -> Option<&'static Archive> {
    let group = self.current_group()?;
    let RailNode::Archive(_, archive_id) = self.rail_path.get(1)? else {
      return None;
    };
    group.archives.iter().find(|a| a.id == *archive_id)
  }

  /// Total rows in the current rail level. Used for cursor-bound
  /// updates after a push / pop.
  pub fn current_level_len(&self) -> usize {
    match self.rail_path.len() {
      0 => TAXONOMY.len(),
      1 => self.current_group().map(|g| g.archives.len()).unwrap_or(0),
      _ => self.current_archive().map(|a| a.categories.len()).unwrap_or(0),
    }
  }

  /// Push a drill step onto the rail path. Resets the cursor to the
  /// top of the new level and updates its count bound. No-op if the
  /// node doesn't actually drill (e.g. trying to push a third level).
  pub fn drill_into(&mut self, node: RailNode) {
    if self.rail_path.len() >= 2 {
      // Already at the deepest navigable level (Categories). Leaf
      // selection (Category → fetch) is handled by handle_browse_tab,
      // not by the rail-path stack — see ADR-011 §E2.
      return;
    }
    self.rail_path.push(node);
    self.rail_cursor.set_selected(0);
    self.rail_cursor.set_offset(0);
    self.rail_cursor.set_count(self.current_level_len());
  }

  /// Pop one level off the rail path. No-op at the root.
  pub fn drill_back(&mut self) {
    if self.rail_path.pop().is_some() {
      self.rail_cursor.set_selected(0);
      self.rail_cursor.set_offset(0);
      self.rail_cursor.set_count(self.current_level_len());
    }
  }

  /// The `Group` at the rail's currently-highlighted row, when the
  /// user is at the Groups level. `None` if the cursor is out of
  /// bounds or the user has already drilled past Groups.
  pub fn rail_selected_group(&self) -> Option<&'static Group> {
    if !self.rail_path.is_empty() {
      return None;
    }
    TAXONOMY.get(self.rail_cursor.selected())
  }

  /// The `Archive` at the rail's currently-highlighted row, when the
  /// user has drilled into one Group. `None` otherwise.
  pub fn rail_selected_archive(&self) -> Option<&'static Archive> {
    if self.rail_path.len() != 1 {
      return None;
    }
    let archives = self.current_group()?.archives;
    archives.get(self.rail_cursor.selected()).or(archives.first())
  }

  /// The `Category` at the rail's currently-highlighted row, when the
  /// user has drilled into one Archive. `None` otherwise.
  pub fn rail_selected_category(&self) -> Option<&'static Category> {
    if self.rail_path.len() != 2 {
      return None;
    }
    let cats = self.current_archive()?.categories;
    cats.get(self.rail_cursor.selected()).or(cats.first())
  }

  /// What `Enter` / `l` / `Right` should do on the current cursor row.
  /// Returns `Some(node)` to drill into; `None` means the cursor is on
  /// a Category leaf, which the caller should treat as a fetch action.
  pub fn drill_target(&self) -> Option<RailNode> {
    match self.rail_path.len() {
      0 => self.rail_selected_group().map(|g| RailNode::Group(g.id)),
      1 => self.rail_selected_archive().map(|a| {
        let group_id = self.current_group().map(|g| g.id).unwrap_or("");
        RailNode::Archive(group_id, a.id)
      }),
      _ => None,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_starts_at_groups_level() {
    let m = BrowseModel::new();
    assert!(m.rail_path.is_empty());
    assert_eq!(m.focus, BrowseFocus::Rail);
    assert_eq!(m.rail_cursor.selected(), 0);
    assert_eq!(
      m.rail_cursor.count(),
      crate::models::arxiv_taxonomy::group_count()
    );
  }

  #[test]
  fn drill_into_group_resets_cursor_and_updates_count() {
    let mut m = BrowseModel::new();
    // Pretend the cursor was somewhere down the Groups list.
    m.rail_cursor.set_selected(3);
    m.drill_into(RailNode::Group("cs"));
    assert_eq!(m.rail_path.len(), 1);
    assert_eq!(m.rail_cursor.selected(), 0, "cursor resets on drill");
    let expected =
      TAXONOMY.iter().find(|g| g.id == "cs").unwrap().archives.len();
    assert_eq!(m.rail_cursor.count(), expected);
  }

  #[test]
  fn drill_back_pops_one_level_and_restores_root_count() {
    let mut m = BrowseModel::new();
    m.drill_into(RailNode::Group("cs"));
    m.drill_back();
    assert!(m.rail_path.is_empty());
    assert_eq!(
      m.rail_cursor.count(),
      crate::models::arxiv_taxonomy::group_count()
    );
  }

  #[test]
  fn drill_into_archive_lands_at_categories_level() {
    let mut m = BrowseModel::new();
    m.drill_into(RailNode::Group("cs"));
    m.drill_into(RailNode::Archive("cs", "cs"));
    assert_eq!(m.rail_path.len(), 2);
    // cs archive has 40 categories — cursor count should match.
    let expected = TAXONOMY
      .iter()
      .find(|g| g.id == "cs")
      .unwrap()
      .archives
      .iter()
      .find(|a| a.id == "cs")
      .unwrap()
      .categories
      .len();
    assert_eq!(m.rail_cursor.count(), expected);
  }

  #[test]
  fn third_drill_is_noop() {
    let mut m = BrowseModel::new();
    m.drill_into(RailNode::Group("cs"));
    m.drill_into(RailNode::Archive("cs", "cs"));
    let depth_before = m.rail_path.len();
    m.drill_into(RailNode::Group("math"));
    assert_eq!(m.rail_path.len(), depth_before, "no drill past Categories");
  }

  #[test]
  fn rail_selected_group_only_returns_at_root() {
    let mut m = BrowseModel::new();
    assert!(m.rail_selected_group().is_some());
    m.drill_into(RailNode::Group("cs"));
    assert!(
      m.rail_selected_group().is_none(),
      "no group selected once drilled"
    );
  }

  #[test]
  fn rail_selected_archive_only_returns_at_depth_one() {
    let mut m = BrowseModel::new();
    assert!(m.rail_selected_archive().is_none());
    m.drill_into(RailNode::Group("cs"));
    assert!(m.rail_selected_archive().is_some());
    m.drill_into(RailNode::Archive("cs", "cs"));
    assert!(m.rail_selected_archive().is_none());
  }

  #[test]
  fn rail_selected_category_only_returns_at_depth_two() {
    let mut m = BrowseModel::new();
    assert!(m.rail_selected_category().is_none());
    m.drill_into(RailNode::Group("cs"));
    assert!(m.rail_selected_category().is_none());
    m.drill_into(RailNode::Archive("cs", "cs"));
    assert!(m.rail_selected_category().is_some());
  }

  #[test]
  fn loaded_state_starts_empty() {
    let m = BrowseModel::new();
    assert!(m.loaded_categories.is_empty());
    assert!(m.inflight.is_empty());
  }
}
