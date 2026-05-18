# ADR-007 — `ItemStore` encapsulates Workspace's item triple

- **Status:** Accepted (2026-05-18). All 3 PRs landed: PR 1 = ADR + `ItemStore` skeleton + 8 smoke tests + CONTEXT.md vocabulary, PR 2 = Workspace migration (~13 files, +252 / −216), PR 3 = M1-M4 tripwires in `scripts/check-item-store.sh` + ci.sh wired + this status flip.
- **Date:** 2026-05-18
- **Owner:** Victory Chianumba
- **Supersedes:** none
- **Extends:** none directly. Sibling to [ADR-006](ADR-006-store-seam.md) (persistent-state seam) — same audit, different layer.

## Goal

Encapsulate the `items + url_index + arxiv_id_index` triple on `Workspace` behind a typed `ItemStore` that owns the *invariant* "both indices are in sync with items". Mutation goes through methods (`push`, `replace_at`, `sort_by`, `rebuild_indices`); reads go through methods (`find_by_url`, `get`, `iter`). Direct field access disappears.

Today every call site that adds, replaces, or sorts items takes on the burden of updating both indices in lockstep — a pattern that's been written correctly in `process::process_incoming` (where the dedup logic lives) and `app::rebuild_indices` (the bulk rebuild after sort), but is one careless `workspace.items.push(...)` away from torn state.

## Context

The 2026-05-18 architectural audit (`docs/audits/2026-05-18-architectural-audit.md`) named candidate **C9**:

> *Problem:* `Workspace` exposes `items: Vec<FeedItem>`, `url_index: HashMap<...>`, and `arxiv_id_index: HashMap<...>` as three `pub` fields. The contract that both indices map into `items` is maintained by convention across 5 mutation paths (process_incoming Items + EnrichedItems dedup branches, the post-sort rebuild, the manual cache-load path, and the workflow-state-update fall-through to discovery). Adding a sixth mutation path that forgets either index is a class of bug Rust can't currently catch.
> *Solution:* `ItemStore` type owning the triple, with a single mutation chokepoint.

Mutation paths today (Workspace only, scoped to this slice):

| Site | What it does | Index maintenance |
|---|---|---|
| `process::process_incoming` Items branch | URL/arxiv dedup or push new | Maintained inline |
| `process::process_incoming` EnrichedItems branch | URL/arxiv dedup with field-merge logic | Maintained inline |
| `app::rebuild_indices` | Bulk rebuild after sort | Clears + rebuilds from scratch |
| `process::process_incoming` after fetch | Sort items by `published_at` desc then call rebuild | Indirect: relies on `rebuild_indices` after |
| `app/methods/library_filter.rs` (multi-select workflow) | Iterates `workspace.items.iter_mut()` for `workflow_state` writes | No index touched (correct — workflow_state isn't indexed) |

The pattern's *correctness today* is encouraging. The pattern's *fragility going forward* is what C9 addresses: a future "delete archived items" feature, or a "merge duplicates manually" UX, has to re-derive the invariant from scratch by reading `process.rs` carefully — or it'll ship a torn state.

## Decision

### The type (`trench/src/data/item_store.rs`)

```rust
/// Coordinated triple: items + url-index + arxiv-id-index. Owns the
/// invariant that both indices map into `items`. Mutation goes through
/// methods; raw fields are pub(super) so workspace_store.rs can
/// serde-derive over them but no foreign code can mutate the indices
/// out of band.
#[derive(Default)]
pub struct ItemStore {
  items: Vec<FeedItem>,
  url_index: HashMap<String, usize>,
  arxiv_id_index: HashMap<String, usize>,
}

impl ItemStore {
  // ── Reads ──
  pub fn len(&self) -> usize;
  pub fn is_empty(&self) -> bool;
  pub fn get(&self, idx: usize) -> Option<&FeedItem>;
  pub fn get_mut(&mut self, idx: usize) -> Option<&mut FeedItem>;
  pub fn iter(&self) -> impl Iterator<Item = &FeedItem>;
  pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut FeedItem>;
  pub fn items(&self) -> &[FeedItem];   // slice escape hatch for hot loops
  pub fn find_index_by_url(&self, url: &str) -> Option<usize>;
  pub fn find_by_url(&self, url: &str) -> Option<&FeedItem>;
  pub fn find_index_by_arxiv_id(&self, aid: &str) -> Option<usize>;
  pub fn find_by_arxiv_id(&self, aid: &str) -> Option<&FeedItem>;

  // ── Mutation ──
  /// Append `item` and update both indices. Returns the index.
  pub fn push(&mut self, item: FeedItem) -> usize;
  /// Replace the item at `idx` (in-place — indices stay valid because
  /// position doesn't change). Caller has verified `idx < len()`.
  pub fn replace_at(&mut self, idx: usize, item: FeedItem);
  /// Sort items by `cmp`, then rebuild indices.
  pub fn sort_by(&mut self, cmp: impl FnMut(&FeedItem, &FeedItem) -> std::cmp::Ordering);
  /// Rebuild both indices from `items`. Use after a bulk mutation
  /// (cache load, manual delete sweep) where invariant maintenance
  /// across the operation is not practical.
  pub fn rebuild_indices(&mut self);
}
```

### Workspace shape after PR 2

```rust
pub struct Workspace {
  pub items_store: ItemStore,           // was: items + url_index + arxiv_id_index
  pub history: Vec<HistoryEntry>,
  pub item_tags: ItemTags,
  pub persisted_states: HashMap<String, WorkflowState>,
}
```

The three fields collapse into one. `persisted_states` *could* live inside `ItemStore` (workflow_state on each item is denormalised from persisted_states[url]), but it has independent disk-persistence semantics — kept on `Workspace` to avoid scope creep. ADR-007 §S2 explicitly defers.

### Slice 7-specific decisions

#### S1. Methods, not pub field-access

`items_store.items` is not pub-readable from outside the module. Callers use `items_store.items()` (slice borrow) or `items_store.iter()` (iterator). This costs ~158 call sites a one-token edit (`items` → `items()`); the benefit is that the invariant gets a type-level home.

The audit's literal phrasing was "ItemStore for Workspace" — interpreted here as "encapsulate, don't just rename." A type with all-pub fields would compress nothing.

#### S2. `persisted_states` stays on `Workspace`

`persisted_states` maps URL → WorkflowState, persisted to `~/.config/trench/state.json` independently of `cache.json`. Folding it into `ItemStore` would couple the item-triple invariant to a disk-persistence concern. ADR-007 keeps the scope tight: `ItemStore` owns *only* the triple.

#### S3. `DiscoveryModel`'s parallel triple is out of scope

`DiscoveryModel` has the same shape (`items: Vec<FeedItem>` + `url_index` + `arxiv_id_index`). The structural symmetry is real, but:

1. Discovery has agent-message-driven mutation (different lifecycle).
2. Migrating it doubles PR 2's call-site count from ~103 to ~158.
3. The audit only named Workspace.

Discovery's triple stays. A future "C9b" or opportunistic refactor can unify, once the Workspace migration has soaked.

#### S4. 3-PR cadence

| # | PR | Behaviour change |
|---|---|---|
| 1 | ADR-007 + `ItemStore` skeleton (`trench/src/data/item_store.rs`) + 8 inline smoke tests + CONTEXT.md vocabulary. | None — the type is unused. |
| 2 | Migrate Workspace: collapse `items + url_index + arxiv_id_index` → `items_store: ItemStore`. ~103 call sites + 2 dedup branches + `rebuild_indices`. | None — invariant: same items, same indices, same observable behaviour. |
| 3 | `scripts/check-item-store.sh` with M1-M4 tripwires; ci.sh wired; ADR-007 → Accepted. | None |

### Invariants for PR 3 tripwire

- **M1** `trench/src/data/workspace_store.rs` declares `pub items_store: ItemStore` and does NOT declare `pub items:`, `pub url_index:`, or `pub arxiv_id_index:`. Awk-scoped to the Workspace struct body.
- **M2** Outside `trench/src/data/item_store.rs`, no expression matches `\.items_store\.items` (raw vec access) or `\.items_store\.url_index` / `\.items_store\.arxiv_id_index` (raw map access). Reads go through methods.
- **M3** No `workspace\.url_index\.insert` / `workspace\.url_index\.remove` / `workspace\.arxiv_id_index\.(insert|remove)` calls anywhere in `trench/src/`. Index mutation is `ItemStore`-internal.
- **M4** ADR-007 cadence table lists every committed PR (1, 2, 3).

## Consequences

### Positive

- Index-invariant lives at the type. Adding a 6th mutation path is "call `push`/`replace_at`" — not "remember to maintain both indices."
- `find_by_url(url)` reads more clearly than `url_index.get(url).map(|&i| &items[i])` (which the codebase has in multiple places today).
- A future change like switching `arxiv_id_index` to a different key shape is one-file.
- Free unit tests at the type — what the triple's invariant *is* gets written down.

### Negative

- ~103 call sites get a mechanical edit. Compiler-driven sweep handles this in the now-familiar shape from slices 1 / 2 / 5.
- Method dispatch instead of direct field reads. The optimizer inlines all of it; no perf concern. But readers familiar with the pre-C9 shape will need one pass through the new method names.

### Trade-offs explicitly accepted

- **`persisted_states` stays separate.** Could have been folded into `ItemStore` for a stronger invariant ("workflow_state on an item agrees with persisted_states[url]"), but that's a different audit candidate (workflow-state caching). Out of scope.
- **`DiscoveryModel`'s parallel triple is NOT migrated.** Honest scope-cut. The symmetry is real; the architectural pressure to unify is not (yet).
- **No `delete_by_url` method in PR 1.** Today's codebase doesn't delete items individually (only bulk via cache reload). Adding a delete API speculatively grows the surface; the type is open for extension when a feature needs it.

## Risks

1. **Migration touches ~103 sites.** Mitigation: compiler-driven loop. Slice 5 PR 2 swept ~118 cleanly with the same pattern.
2. **Method-vs-field dispatch may surface a perf regression in `items[idx]` hot loops.** Mitigation: `items()` returns `&[FeedItem]`, so `&items_store.items()[idx]` is equivalent. The escape hatch covers any hot path that needs zero-overhead access.
3. **Borrow-checker friction on simultaneous borrows.** `items_store.find_by_url(url)` borrows `&self`; `items_store.iter_mut()` borrows `&mut self`. Today's code splits these by stepping out of borrow scopes; the migration keeps the same shape.

## Related

- [ADR-001](ADR-001-render-purification.md) — parent per-pane refactor (audit-grade-alone justification shape).
- [ADR-006](ADR-006-store-seam.md) — sibling C8; same audit, different layer of the system.
- `docs/audits/2026-05-18-architectural-audit.md` — candidate **C9**.
- `docs/CONTEXT.md` — vocabulary updated in PR 1.
- `scripts/check-item-store.sh` — created in PR 3.
