# ADR-005 — Discovery-pane slice (Slice 5 of render purification)

- **Status:** Accepted (2026-05-18). All 4 PRs landed: PR 1 = ADR + rename + smoke tests, PR 2 = field migration (`App.feed.discovery` → `App.discovery`) + threaded `&mut DiscoveryModel` through 8 feed-render fn signatures, PR 3 = gesture methods on `DiscoveryModel` + 4 App wrappers deleted, PR 4 = K1-K4 tripwires in `scripts/check-render-purification.sh` + this status flip.
- **Date:** 2026-05-18
- **Owner:** Victory Chianumba
- **Supersedes:** none
- **Extends:** [ADR-001](ADR-001-render-purification.md), [ADR-002](ADR-002-reader-slice.md), [ADR-003](ADR-003-notes-slice.md). All decisions in those hold for Slice 5 unless noted.

## Goal

Apply the per-pane composition-root pattern (ADR-001) to the discovery sub-pane. Lift `DiscoveryState` out of `App.feed.discovery` and onto `App.discovery: DiscoveryModel` as a sibling to `FeedModel`, `ReaderPaneModel`, and `NotesPaneModel`.

## Context

The 2026-05-18 architectural audit (`docs/audits/2026-05-18-architectural-audit.md`) named candidate **C7**:

> *Problem:* 1,016 LOC of agent loop, intent classification, palette, session — still nested inside `FeedModel.discovery`. ADR-001 D2 said "lift when grown enough"; it has.
> *Solution:* `App.discovery: DiscoveryModel`.
> *Benefits:* Discovery agent thread survives feed-tab switches (latent bug today). Render seam decouples from feed pane's. Tests drive the discovery state machine without a `FeedModel`.

Discovery has grown into a substantial sub-system:

- `app/state/discovery.rs` — 15 fields on `DiscoveryState`: search query + lowercased mirror, search-focus flag, loading flag, agent receiver, status line, items + indices, list cursor, palette state, multi-turn session, intent + forced-intent, force-new flag.
- `discovery/` — 6 files, ~980 LOC: `agent.rs` (background agent loop), `ai_query.rs` (LLM call), `intent.rs` (query classifier), `pipeline.rs` (multi-step search), `tools.rs` (Claude tool definitions), `mod.rs` (DiscoveryMessage + SessionHistory).
- `services/discovery.rs` — 280 LOC orchestrator.
- 118 call sites of `app.feed.discovery.*` across the codebase.

Discovery's relationship to the feed pane is the same shape as notes' relationship to the reader pane (ADR-003) — *adjacent, not nested*. Discovery has its own search bar, its own list, its own state machine, its own background thread. It just happens to live under `feed` today because it originated as a feature inside the feed pane. The size + the audit's "lift when grown enough" verdict are the forcing function.

## Decision

### Scope: discovery only (Medium)

Slice 5 lifts one struct:

- `DiscoveryModel { /* the 15 existing fields */ }` — the composition root.

**Explicitly out of scope** (deferred):

- The `discovery/` background-thread modules (`agent`, `ai_query`, `intent`, `pipeline`, `tools`) — they're already well-isolated and don't carry App state. They keep emitting `DiscoveryMessage` over the channel; the `DiscoveryModel` receives.
- `services/discovery.rs` orchestration — wraps the background-thread spawn. Untouched.
- The `App::push_discovery_char` / `pop_discovery_char` / `clear_discovery_query` / `set_discovery_query` wrappers in `app/caches.rs` — they migrate to `DiscoveryModel` methods in PR 3, not PR 2.
- A `LayoutMetrics` lift for the discovery palette (forward-design from ADR-001 §D6) — separate slice (candidate C6).

### Trigger: audit-grade-alone + size-forcing-function

The audit explicitly named the size threshold (~1,000 LOC of agent-related code) as crossed. Slice 3 set the precedent for audit-grade-alone justification (ADR-003). Slice 5 carries the same shape with one stronger argument: the size threshold is concrete (lines counted), not just "the audit said so."

### Decisions inherited unchanged from ADR-001 / ADR-002 / ADR-003

- **D1** composition root: `App.discovery: DiscoveryModel`. Models never reference each other.
- **D3** Action in / Effect out / `pre_draw(Viewport)` if layout-derived mutation appears (today: none).
- **D4** renders take `&Model + &Context`, not `&mut App`.
- **D5** W3 hybrid rule for shared mutations.
- **D7** tests at the Model boundary, inline `#[cfg(test)]`.
- **ADR-002 §S5 tests-without-backend** — discovery model tests instantiate `DiscoveryModel::default()` and exercise gesture methods against synthetic queries, without invoking the agent / LLM / Claude API.

### Slice 5-specific decisions

#### S1. Rename `DiscoveryState` → `DiscoveryModel`

The naming convention from ADRs 002/003 is `<Pane>Model` for composition roots. `DiscoveryState` predates the convention. PR 1 renames it. Mechanical; the struct keeps the same 15 fields.

#### S2. Field moves from `App.feed.discovery` to `App.discovery`

Today: `App.feed: FeedModel`, where `FeedModel.discovery: DiscoveryState`.
After: `App.feed: FeedModel`, `App.discovery: DiscoveryModel`.

The `discovery: DiscoveryModel` field on `FeedModel` goes away. Discovery becomes a true sibling of feed, reader, notes.

#### S3. `rx: Option<Receiver<DiscoveryMessage>>` stays on `DiscoveryModel`

The agent thread channel `rx` is *per-session* state (each `spawn_ai_discovery` creates a fresh channel). It belongs on the model, not on infrastructure. Same shape as `App.reader_popup.rx` lives on `ReaderPopupModel` per ADR-002.

#### S4. The `discovery/` background modules are not lifted

`discovery/agent.rs` et al. take owned data (config, query) over channels; they don't hold App state. PR 2's sweep does NOT touch these files. The trait-and-context pattern from ADR-004 (Source/EnrichmentSource) is not required here — there's only one agent kind today.

#### S5. 4-PR cadence

| # | PR | Behaviour change |
|---|---|---|
| 1 | ADR-005 + rename `DiscoveryState` → `DiscoveryModel` + CONTEXT.md vocabulary + smoke tests. | None |
| 2 | Move field from `App.feed.discovery` to `App.discovery`. Compiler-driven sweep across ~118 call sites. | None |
| 3 | Gesture methods on `DiscoveryModel` — pull `push_discovery_char`/`pop_discovery_char`/`clear_discovery_query`/`set_discovery_query` from `App` wrappers in `app/caches.rs` onto the model. | None |
| 4 | Lock the door — extend `scripts/check-render-purification.sh` with K1/K2/K3 tripwires; ADR-005 status → Accepted. | None |

### Invariants for PR 4 tripwire

`scripts/check-render-purification.sh` gets three new invariants:

- **K1** No `pub discovery:` field on `FeedModel` (i.e., no `discovery: DiscoveryState` or `DiscoveryModel` inside `FeedModel`).
- **K2** `App` declares `pub discovery: DiscoveryModel`.
- **K3** No `app.feed.discovery` reads anywhere in `trench/src/` (every render path goes through `app.discovery.*`). Scoped to render paths in `trench/src/ui/`; gesture orchestration in `trench/src/keys/` may also be checked depending on how the gesture migration in PR 3 settles.

## Consequences

### Positive

- 15 fields move from a nested `FeedModel.discovery` into a top-level `App.discovery`. The composition root pattern from ADR-001 holds for a fourth pane.
- Render paths in `ui/title.rs`, `ui/details.rs`, etc. that touch discovery state get a one-step path (`app.discovery.*`) instead of a two-step path (`app.feed.discovery.*`). Locality wins.
- Tests can exercise `DiscoveryModel::default()` + gesture methods without instantiating a full `FeedModel`.
- The latent observation that "the discovery sub-system feels nested where it should be a peer" gets resolved at the type level.
- Slice 1/2/3/4 pattern proves out a fourth time. The next audit's discovery-related candidates (none currently) would estimate cleanly against this slice's actual cost.

### Negative

- 118 call sites need updating. Mechanical sweep via the compiler-driven loop (worked for Slice 1 PR 2 ~280 sites, Slice 3 PR 2 ~30 sites).
- Departure from ADR-001 D2 (lazy rollout) — same shape as ADR-003's audit-grade-alone justification. Documented honestly here.
- The latent-bug claim from the audit ("discovery agent thread doesn't survive feed-tab switches") may not actually be a bug — the `rx` is owned by `DiscoveryState` whose ownership is unaffected by tab switches. C7 still improves the architecture but the specific bug claim may not materialise. Recorded for honesty.

### Trade-offs explicitly accepted

- **`DiscoveryModel` is named per the ADR-002/003 convention** (`<Pane>Model`), departing from the existing `DiscoveryState`. One file rename + 1 import line in `feed/mod.rs`; no functional change.
- **The `discovery/` background modules stay untouched.** They're not state; they're infrastructure. PR 2's sweep doesn't touch them.
- **`SessionHistory` stays on the model.** It's per-session state, not shared infrastructure.

## Risks

1. **118-site sweep is large.** Mitigation: compiler-driven loop. Slice 1 PR 2 swept ~280 sites cleanly with the same approach.

2. **Render paths read discovery state from many places** (`ui/title.rs`, `ui/details.rs`, `ui/layout/feed.rs`, `main.rs`). PR 2 must update every site. Mitigation: the type system catches any miss as a compile error.

3. **The agent thread is mid-flight when tabs switch.** Today this is fine (the rx stays alive). Post-C7 same: rx is still on the model, just at a different path. No behavior change expected.

## Related

- [ADR-001](ADR-001-render-purification.md) — parent per-pane refactor.
- [ADR-002](ADR-002-reader-slice.md) — reader-pane slice; same shape.
- [ADR-003](ADR-003-notes-slice.md) — notes-dock slice; same shape, asymmetric secondary.
- [ADR-004](ADR-004-ingestion-seam.md) — sibling architecture work (Source trait + FetchContext), different layer of the system.
- `docs/audits/2026-05-18-architectural-audit.md` — candidate **C7**.
- `docs/CONTEXT.md` — vocabulary updated in PR 1.
- `scripts/check-render-purification.sh` — extended with K1/K2/K3 in PR 4.
