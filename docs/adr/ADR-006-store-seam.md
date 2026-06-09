# ADR-006 — Store seam (`load_json<T>` / `save_json<T>`)

- **Status:** Accepted (2026-05-18). All 3 PRs landed: PR 1 = ADR + seam + 3 smoke tests + CONTEXT.md vocabulary, PR 2 = 8-site migration (net −192 LOC), PR 3 = L1-L4 tripwires in `scripts/check-store-seam.sh` + ci.sh wired + this status flip.
- **Date:** 2026-05-18
- **Owner:** Victory Chianumba
- **Supersedes:** none
- **Extends:** none directly. Sibling to [ADR-004](ADR-004-ingestion-seam.md) (ingestion `Source` trait) — same shape of work, different layer.

## Goal

Collapse the load/save boilerplate that's been copy-pasted across `one-research/src/store/{cache, discovery_cache, enrichment_cache, history, session, tags}.rs` (and the `state.json` / `ui.json` paths inside `mod.rs`) into a single typed seam: two free functions, `load_json<T>` and `save_json<T>`, that own the read-bytes → parse-or-quarantine and serialize → atomic-write → log envelope.

After this slice, a new persistent-state file gets a 3-line wrapper around the seam, not a 50-line file. Custom post-load transforms (sanitize, sort, backfill `title_lower`, invalidate empty-fields entries) stay in the per-module wrapper as a *single visible step*, not buried inside copy-pasted IO boilerplate.

## Context

The 2026-05-18 architectural audit (`docs/audits/2026-05-18-architectural-audit.md`) named candidate **C8**:

> *Problem:* 7 store files repeat the same five-step shape: resolve path, read bytes, parse JSON (quarantine on failure, fall back to default), apply optional transform, return. The save side repeats four steps. ~60% of each file is structurally identical to every other.
> *Solution:* `Store<T>` seam.
> *Benefits:* New persistent state cheap to add. Defense-in-depth (atomic write, quarantine) lives in *one* place — easier to harden, easier to test.

Pattern repetition today, per-module:

| Module | LOC | Load shape | Save shape | Post-load extras |
|---|---|---|---|---|
| `cache.rs` | 290 | bytes→json→quarantine | atomic_write+set_private | `sanitize_in_place` |
| `discovery_cache.rs` | 60 | bytes→json→quarantine | atomic_write | `sanitize_in_place` |
| `enrichment_cache.rs` | 88 | bytes→json→quarantine | atomic_write+set_private | invalidate empty `fields_of_study` |
| `history.rs` | 47 | bytes→json→quarantine | atomic_write | sort + truncate + backfill `title_lower` |
| `session.rs` | 57 | bytes→json→quarantine | atomic_write | none |
| `tags.rs` | 37 | bytes→json→quarantine | atomic_write | none |
| `state.json` (in mod.rs) | ~50 | bytes→json (per-key parse)→quarantine | atomic_write+set_private | tolerant per-key fallback to `Inbox` |
| `ui.json` (in mod.rs) | ~30 | bytes→json→quarantine | atomic_write+set_private | none |

The IO envelope is the same byte-for-byte across 8 sites. The post-load transforms are genuinely per-module.

## Decision

### The seam (`store/mod.rs`)

```rust
/// Load JSON from `path`, returning `T::default()` on read or parse error.
/// Parse errors quarantine the corrupted file via `quarantine_corrupted`
/// so the next save doesn't clobber the only recovery copy.
pub fn load_json<T>(path: &Path, label: &str) -> T
where
  T: serde::de::DeserializeOwned + Default,
{ /* see implementation in PR 1 */ }

/// Atomically serialize `value` to `path`. mkdir -p parent first; log on
/// any failure. The 0o600 mode is inherited from `atomic_write`.
pub fn save_json<T>(value: &T, path: &Path, label: &str)
where
  T: serde::Serialize,
{ /* see implementation in PR 1 */ }
```

Per-module wrappers become 3-5 lines:

```rust
// discovery_cache.rs after PR 2
pub fn load() -> Vec<FeedItem> {
  let Some(path) = path() else { return Vec::new() };
  let mut items: Vec<FeedItem> = super::load_json(&path, "one-research/discovery_cache");
  for item in &mut items { item.sanitize_in_place(); }
  items
}

pub fn save(items: &[FeedItem]) {
  if let Some(path) = path() {
    super::save_json(&items, &path, "one-research/discovery_cache");
  }
}
```

### Free fns, not a trait

C10's `Source` is a trait because the orchestrator iterates `Vec<Box<dyn Source>>`. Stores have no analogous iteration — each store has exactly one caller (App startup + the corresponding save site). Free functions parameterized over `T: DeserializeOwned + Default` give the same compression with zero ceremony.

A trait `Store<T>` would force every module to be a unit struct that exists only to host an `impl`, which is conversion ceremony without payoff. Rejected.

### `set_private` stays as an explicit post-save step

`atomic_write` already produces 0o600 files on Unix (the mode is set on the `.tmp` sidecar before the rename). The `set_private` calls in `cache.rs`, `enrichment_cache.rs`, `state.rs`, and `ui.rs` are defense-in-depth — kept verbatim. Migration does not touch these.

### Decisions inherited unchanged

- **D1** Atomic write semantics — `atomic_write` stays the substrate, called from `save_json`.
- **D2** Quarantine semantics — `quarantine_corrupted` stays the substrate, called from `load_json`.
- **D3** Per-module path resolution — `fn path() -> Option<PathBuf>` stays per-module. The seam takes `&Path`, not a path resolver.

### Slice 6-specific decisions

#### S1. The label is informational, not key

`label: &str` is the prefix used in log lines and quarantine sidecar messages. It's not used as a dedup key — two modules using the same label would be wrong but not broken. The audit's "single owner per file" property is enforced by the path, not the label.

#### S2. `cache.rs`'s background writer thread is out of scope

`cache.rs` has a `WriterMsg` channel + writer thread for debounced saves. The thread calls `save()` internally. Migration shrinks `save()` itself but leaves the writer thread untouched.

#### S3. 3-PR cadence

| # | PR | Behaviour change |
|---|---|---|
| 1 | ADR-006 + `load_json` + `save_json` skeletons + 3 smoke tests + CONTEXT.md vocabulary. | None |
| 2 | Migrate 8 sites (5 submodules + state + ui) to the seam. | None — invariant: byte-equivalent file contents on round-trip. |
| 3 | `scripts/check-store-seam.sh` with L1-L3 tripwires; ci.sh wired; ADR-006 → Accepted. | None |

### Invariants for PR 3 tripwire

`scripts/check-store-seam.sh` gets three new invariants:

- **L1** Every module in `one-research/src/store/{cache, discovery_cache, enrichment_cache, history, session, tags}.rs` references `super::load_json` and `super::save_json` *or* explains in a `// SEAM-EXEMPT:` comment why it doesn't. Catches "this new store skipped the seam" mistakes.
- **L2** No `serde_json::from_slice` or `serde_json::to_vec` call inside `one-research/src/store/` other than in `mod.rs`'s implementation of `load_json`/`save_json`. Forces the seam to be the choke point.
- **L3** No direct `super::atomic_write` call inside `one-research/src/store/` other than from `mod.rs`'s `save_json`. Same forcing function.
- **L4** ADR-006 cadence table lists every committed PR (mirrors I2 / I6 / K4).

## Consequences

### Positive

- ~150 LOC of structural duplication collapses into one seam.
- Adding a new persistent-state file becomes a 5-line wrapper around `load_json` / `save_json`.
- Hardening the IO envelope (e.g. extending the quarantine algorithm, swapping serialization format) is a one-place change.
- Round-trip tests on the seam cover the IO envelope for *every* store site, not just the ones whose authors happened to write tests.

### Negative

- Generic `<T: DeserializeOwned + Default>` requires `T: Default`. Today every store's success type satisfies this (Vec/HashMap/UiState/SessionHistory). If a future store wants a non-Default type, it needs to pass an `Option<T>` or wrap.
- Re-reading a per-module file no longer shows the IO logic inline; readers have to know the seam exists. The single-line `super::load_json(&path, "...")` is short enough that this is mild.

### Trade-offs explicitly accepted

- **`cache.rs`'s background writer thread is not refactored.** It calls `save()` internally; PR 2 shrinks `save()`'s body but leaves the threading model alone.
- **`state.json`'s two-pass parse (per-key tolerant fallback to `Inbox`) stays.** `load_json` returns the raw `HashMap<String, Value>`; the per-key map happens in the caller. The seam doesn't try to absorb the tolerant-parse policy.
- **`set_private` redundancy is preserved.** Existing `set_private` calls after `save_json` stay because `atomic_write` already produces 0o600 (the calls are belt-and-suspenders).

## Risks

1. **`T: Default` constraint may catch future types out.** Mitigation: the constraint surfaces at the call site, not as runtime drift. If a new store needs `Option<T>`, it falls back to using `atomic_write` directly with a `// SEAM-EXEMPT:` comment per L1.

2. **Migration is mechanical but covers 8 files.** Mitigation: each is small (≤290 LOC, mostly the cache.rs writer thread that PR 2 doesn't touch). Round-trip behaviour is verified at the seam in PR 1.

3. **Closure of the IO surface might mask a latent bug.** Mitigation: PR 2's verification is "every store's `load()` returns equivalent values for the same on-disk bytes" — a tempfile round-trip per module.

## Related

- [ADR-001](ADR-001-render-purification.md) — parent per-pane refactor decisions (the "audit-grade-alone" justification shape).
- [ADR-004](ADR-004-ingestion-seam.md) — sibling architecture work; trait-based because of iteration. Free fns chosen here because no iteration.
- `docs/audits/2026-05-18-architectural-audit.md` — candidate **C8**.
- `docs/CONTEXT.md` — vocabulary updated in PR 1.
- `scripts/check-store-seam.sh` — created in PR 3.
