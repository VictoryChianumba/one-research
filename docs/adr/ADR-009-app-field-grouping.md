# ADR-009 — Cluster flat `App` fields into named state structs

- **Status:** Accepted (2026-05-19). Six PRs landed in one session. PR 1 ships ADR + `DebounceState` + 5 smoke tests + 4-field migration. PR 2 ships `LeaderState` + 7 smoke tests + 3-field migration + 6 call-site updates. PR 3 ships `ReaderBottomState` + 2 smoke tests + 5-field migration + ~104 call-site renames (sed-driven mechanical). PR 4 ships `ViewFlags` + 2 smoke tests + 3-field migration; the audit's original 5-field grouping is corrected — the two `fulltext_*` routing flags reclassify into the future `AsyncJobs` cluster. PR 5 ships `RenderCaches` + 11 behavioural tests (one per `Effect` variant) + 5-field migration + the effect-observer protocol moves onto the struct. PR 6 ships `AsyncJobs` + 3 smoke tests + 13-field migration + ~112 call-site renames; closes the punch list. App shrank 108 → 80 fields net across the series.
- **Date:** 2026-05-19
- **Owner:** Victory Chianumba
- **Supersedes:** none
- **Extends:** [ADR-001](ADR-001-render-purification.md) §"composition root" — `App` is the composition root; this ADR is about its internal shape.

## Goal

Shrink `App`'s flat field surface (today: ~108 fields, ~1556 LOC in `trench/src/app/mod.rs`) by clustering tightly-related fields into named state structs living under `trench/src/app/state/`. Per cluster: one struct, focused methods that encapsulate the cluster's protocol, smoke tests for the protocol.

After the slice, `App` continues to be the composition root — the goal is **not** to lift state out of `App`, but to give the state inside it shape. A reader of the `App` struct should be able to infer field groupings without reading 100+ field names.

## Context

The 2026-05-18 audit graded `App` composition root **D+**:

> 1,556 lines (was 1,556 — comment removal, not structural). ~100 fields. 37 raw UI flags.

Eight ADRs landed across 2026-05-16 → 2026-05-18 (ADR-001…ADR-008). Each ADR extracted a *sub*-model (`FeedModel`, `ReaderPaneModel`, `NotesPaneModel`, `DiscoveryModel`, `ReaderPopupModel`). After all eight slices, the field *count* on `App` did not shrink — the audit explicitly noted "shrinkage post-slicing = 0." The structural debt is real but is a different shape than what the prior ADRs addressed.

The 108 fields break into ~10 logical categories (debounce, leader-key, view-state booleans, async receivers, RefCell caches, UI overlays, …). Today they sit flat, with category boundaries only documentable via inline comments — see the `// Scroll debounce …`, `// Leader key`, `// Help overlay` comment lines in `trench/src/app/mod.rs`.

Two specific symptoms of the flat shape:

1. **Open-coded protocols.** The debounce cluster's read-compare-update logic lives twice in `main.rs` (`kbd_scroll_ok`, `mouse_scroll_ok`), once per gate. The cluster has a protocol — "ask whether you may scroll, the gate updates its timer atomically" — but no type to own that protocol.

2. **Initialisation lists drift from declaration order.** The `App::new()` constructor today re-lists all ~108 fields literally. Adding a field requires editing two sites (declaration + init); forgetting one is a compile error today but informational debt every time.

This ADR's pattern — `pub debounce: DebounceState` + `DebounceState::default()` — replaces 4 declaration lines + 4 init lines with 1 + 1, and centralises the protocol with type-enforced method-only access (the inner fields are module-private).

## Decision

Cluster `App` fields by *protocol cohesion*: when N fields share a read-update protocol or a single lifecycle, group them. Each cluster becomes a struct in `trench/src/app/state/<cluster>.rs`, owns its fields (private), and exposes methods that encode the protocol.

### The pilot — `DebounceState` (this PR)

```rust
// trench/src/app/state/debounce.rs
pub struct DebounceState {
  last_kbd: Option<Instant>,
  kbd_cooldown_ms: u64,
  last_mouse: Option<Instant>,
  mouse_cooldown_ms: u64,
}

impl DebounceState {
  pub fn try_kbd_scroll(&mut self) -> bool { … }
  pub fn try_mouse_scroll(&mut self) -> bool { … }
}
```

`App` carries one `pub debounce: DebounceState` field. The 4 flat fields are gone. `main.rs::kbd_scroll_ok` becomes a one-line delegator: `app.debounce.try_kbd_scroll()`. Existing 8 call sites of `kbd_scroll_ok(app)` / `mouse_scroll_ok(app)` are unchanged — the helper signature stays.

**Why debounce as the pilot:** smallest tight cluster (4 fields, 6 internal call sites), open-coded protocol that obviously benefits from method encapsulation, zero render-path involvement (low risk).

### Future clusters (each its own PR, opportunistic)

| Cluster | Fields | Struct | Status |
|---|---|---|---|
| Leader key | `leader_active`, `leader_activated_at`, `leader_timeout_ms` (3) | `LeaderState` | ✓ PR 2 (2026-05-19) |
| Reader-bottom drawer | `reader_bottom_open`, `reader_bottom_focused`, `reader_bottom_details`, `reader_feed_popup_selected`, `reader_bottom_scroll` (5) | `ReaderBottomState` | ✓ PR 3 (2026-05-19) |
| Async fetch receivers | `fetch_rx`, `fulltext_rx`, `tread_fetch_rx`, `repo_fetch_rx`, `is_loading`, `loading_sources`, `loaded_sources`, `spinner_frame`, `fulltext_loading`, `pending_fulltext_context`, `pending_tread_fetch`, `fulltext_new_tab`, `fulltext_for_secondary` (13) | `AsyncJobs` | ✓ PR 6 (2026-05-19) |
| Popup flags | `tab_window_prompt_active`, `abstract_popup_active`, `narrow_feed_details_open` (3) | `ViewFlags` | ✓ PR 4 (2026-05-19) |
| RefCell caches | `visible_cache`, `counts_cache`, `filter_source_names_cache`, `filter_summary_cache`, `filtered_history_cache` (5) | `RenderCaches` | ✓ PR 5 (2026-05-19) |

Each future cluster lands its own PR with the same shape: ADR section update + struct + smoke tests + migration. **Lazy rollout** — a cluster is migrated when a feature pulls us into it, not on a schedule. Same cadence as ADR-001's slice rollout.

### What stays flat

Not every field clusters. `needs_redraw`, `should_quit`, `status_message`, `notification`, `kitty_supported`, `details_last_item_url`, `last_read`, `last_read_source` are independent scalars with no shared protocol. They stay as top-level fields on `App`.

The 6 sub-model composition roots (`feed`, `discovery`, `reader`, `reader_popup`, `notes`, `chat`) are already encapsulated by ADR-001/2/3/5; they stay as-is.

## Alternatives considered

### Lift state out of `App` into a separate `AppState` struct

ADR-001 explicitly chose to keep `App` as the composition root; the per-pane sub-models are children. Introducing an intermediate `AppState` between `App` and the sub-models violates that decision. The right move per ADR-001 is to *shrink the children* (this ADR), not to add a layer.

### Use bitflags / a single state-enum for view booleans

The 5 view-state popups could be modelled as `enum ActivePopup { None, TabWindowPrompt, AbstractPopup, ... }`. Rejected for the pilot — debounce is the simpler cluster — but kept as an option for the `ViewFlags` cluster's eventual PR. Bitflags will be considered when that PR is written.

### Inline migration only — no ADR

Per the codebase's established cadence (8 ADRs in 3 days, each with a tripwire), a structural refactor of this kind earns an ADR. The ADR's main load-bearing function is the future-clusters table — it makes the broader plan inspectable and prevents the per-cluster PRs from re-litigating the pattern.

## Consequences

### Positive

- **Locality:** the debounce protocol lives in one file (`debounce.rs`). A future bug ("the kbd cooldown is too short on Linux") has one place to be fixed.
- **Leverage:** `DebounceState::try_*` is now reusable from anywhere that gets `&mut App.debounce`. Today's two call sites are the only consumers, but the method-style API is the right shape for future "should I accept this input?" gates.
- **Test surface:** five smoke tests lock the cooldown constants, the gate-independence invariant, and the "after cooldown elapses, scrolls are accepted" path. The pre-grouping behaviour was untested.
- **Future shrink:** each subsequent cluster PR drops 3–11 fields off the `App` declaration. After all five clusters listed in §"Future clusters" land, `App` shrinks by ~25 fields — from ~108 to ~83. Not zero; not a god object's worth of difference; but a real, defensible step.

### Negative

- **Two-level access:** `app.debounce.try_kbd_scroll()` is one hop deeper than `app.last_scroll_time`. Acceptable — the call sites are wrapped in helpers that hide the hop.
- **More files in `app/state/`:** the directory grows from 12 modules to ~17 across the full rollout. Tolerable; the existing 12 set the precedent.
- **No invariant tripwire yet.** Future clusters may want one (e.g., "no flat `pub last_scroll_time` reappears on `App`"). The pilot doesn't ship a tripwire because the flat fields are simply gone — they can't reappear without a compile error somewhere obvious. A tripwire becomes worthwhile only if a cluster's fields could plausibly leak back. Revisit per-cluster.

## PR cadence

Following the ADR-006/007/008 rhythm:

| PR | Scope | LOC |
|---|---|---|
| 1 | ADR + `DebounceState` struct + 5 smoke tests + 4-field migration + CONTEXT.md vocabulary | small |
| 2+ | One cluster per PR (opportunistic, no schedule) | small per cluster |
| Final | ADR Accepted flip after all clusters in §"Future clusters" land, or after the audit's next pass declares the goal met | trivial |

This ADR moves to **Accepted** when the audit's `App` composition-root grade reaches **C+** or better, or when the next architectural audit explicitly closes the candidate.

**Flipped to Accepted on 2026-05-19** when the AsyncJobs PR 6 closed the Future-clusters table. App shrank 108 → 80 fields net. The grade-progression test (does the next audit close the candidate?) is deferred to that audit; this status flip reflects the *punch list* being complete, not the audit grade being recalculated.
