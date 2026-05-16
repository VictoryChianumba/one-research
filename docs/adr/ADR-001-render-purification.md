# ADR-001 — Render purification, per pane

- **Status:** Accepted (Slice 1 complete: PRs 1, 2, 3, 4a, 4b, 4c, 6 landed; PR 5 — `Action::OpenInReader` cross-pane plumbing — deferred until Slice 2 triggers it). Closes when all panes have been refactored under lazy rollout (likely years out).
- **Date:** 2026-05-15 (proposed); 2026-05-16 (accepted)
- **Owner:** Victory Chianumba
- **Supersedes:** none

## Goal

Land image rendering (Kitty graphics + sixel + ASCII fallback) and voice playback without compounding the architectural debt the codebase has been accruing. The forcing function is concrete: both features touch exactly the parts of the codebase that are already broken (render path, async state on `App`). Adding them on top of the current shape would make things worse.

This ADR is not "improve the code." It is "make the next major surfaces landable on a clean pattern."

## Context

An architectural audit on 2026-05-14 surfaced the following:

- `App` is a 1,614-line god struct with ~80 public fields and no invariant. Any combination of field values is reachable.
- `keys/mod.rs` is a 1,442-line keybinding monolith with no internal structure.
- `ui/layout/feed.rs` (1,144 lines) and `ui/layout/main_row.rs` mutate state during render. The comments `// Intentional render-time mutation` mark known-bad code that was deferred.
- The UI layer has zero tests. ~145 tests exist, all on parsing/ingestion/store. The interface the user actually exercises is untested.
- A phased migration was already in flight (`action.rs` "Phase 2", `effect.rs` "Phase 3", and Phase 4 = "hoists render-time state mutations into action handlers"). Phase 4 was never completed.
- Domain language is inconsistent (`FeedItem` / `Item` / `Paper` / `DiscoveryItem` for similar concepts).
- No `CONTEXT.md` or ADRs existed.

The pain landed concretely: every new feature ends up adding fields to `App`, adding match arms to `keys/mod.rs`, and either mutating during render or sprinkling another cache. Each is mechanical; together they have made the codebase hostile to change.

## Decision

The refactor is **per-pane vertical slices**, in lazy-rollout order. The slice for the feed pane (Slice 1) establishes the pattern; each subsequent pane copies it when a feature pulls us into it.

The seven load-bearing choices:

### D1. Per-pane composition root

Each pane becomes a *Model* owned as a field on `App`. Models never reference each other. Cross-pane communication is through `Action` only. Renders take `&Model + &Context`, never `&mut`.

Rejected alternatives:
- Full Elm/MVU rewrite — too much for a solo project mid-flight.
- Field grouping inside `App` (a nested `App.feed: FeedState` with the same fields) — solves nothing; the god-object pattern survives.

### D2. Feed pane first; lazy rollout after

Slice 1 = feed pane. Slice 2 = reader pane, triggered when image rendering needs to land. All other panes (chat, notes, repo viewer, settings) stay legacy until a feature pulls us in.

Rationale: feed has the most copy-paste and the most-touched render path. The pattern proved here is the one Slice 2 copies. Inbox, Library, Discoveries, History tabs are all in scope because they share chrome.

### D3. Action (in) / Effect (out, narrow) / pre_draw(Viewport) (layout)

- `Action` is the input vocabulary. Grows as panes migrate. Cross-pane verbs (`Action::OpenInReader`) live here.
- `Effect` stays narrow — it names *cache-invalidation events*, not commands. The existing 9 variants in `effect.rs` are roughly right.
- `Model::pre_draw(viewport: Viewport)` runs once per frame, before render. Any mutation that needs layout-derived values (auto-scroll offset, width-aware wrapping) goes here.

`Viewport` is a tiny POD: `{ rows: u16, cols: u16 }`. No ratatui types leak into model code.

### D4. Strict `&Model` renders

After a slice, `feed.rs` (and equivalent for the next pane) takes no `&mut App` arguments. Any mutation that previously happened during render either (a) moves into `pre_draw`, or (b) was a key-event handler in disguise and moves into a model method.

### D5. Workspace mutation — W3 hybrid rule

Some mutations are state-local; some are cross-pane. The rule:

- **State-local gestures** (mark read, queue, archive, toggle tag, search query change, filter chip toggle): model methods take `&mut Workspace` directly. Borrow conflict avoided by splitting the borrow once at the top of the key handler (`let (feed, ws) = app.feed_and_workspace_mut();`).
- **Cross-pane gestures** (open in reader, append history): model emits an `Action`. The orchestrator owns the mutation.

Classification table for the feed pane:

| Gesture | Shape | Mechanism |
|---|---|---|
| Mark Read / Queue / Archive (workflow state) | W1 | `&mut Workspace`; emits `Effect::WorkflowStateChanged` |
| Toggle tag on item | W1 | `&mut Workspace`; emits `Effect::TagsChanged` |
| Search query change | W1 | `&mut Workspace` (cache); emits `Effect::SearchQueryChanged` |
| Filter chip toggle | W1 | model-local; emits `Effect::FiltersChanged` |
| Enter → open reader | W2 | emits `Action::OpenInReader { item, target }` |
| Discovery search completion | W2 | emits `Action::AppendHistory(entry)` |
| Discovery fetch start | W1 | model-local loading flag; spawns thread |

### D6. 6-PR strangler-fig with feature freeze

| # | PR | Behaviour change |
|---|---|---|
| 1 | Foundations: `CONTEXT.md`, `ADR-001`, empty `FeedModel`, empty `FeedContext`, `Viewport` POD, test stub, `CLAUDE.md` pointer. | None |
| 2 | State migration: move feed-related fields from `App` top-level into `App.feed: FeedModel`. Mechanical. | None |
| 3 | Action vocabulary: gesture methods on `FeedModel`; `keys/feed.rs` calls model methods. | None |
| 4a | `pre_draw` (wide feed): auto-scroll for item-table and history-tab moves into `FeedModel::pre_draw`. | None visible |
| 4b | `pre_draw` (narrow feed): variable-row-height variant `pre_draw_narrow_feed` lands; the third deferred mutation site moves into it. | None visible |
| 4c | Render-signature flip: `feed.rs` renders take `&mut FeedModel + &FeedContext`, no `&App`. `FeedContext` carries pre-computed `visible_indices` + `filtered_history` + counts + theme + workspace/config borrows. **The load-bearing PR.** | None visible |
| 5 | Cross-pane via `Action::OpenInReader { item, target: ReaderTarget }` (forward-compat with Slice 2). **Deferred to Slice 2** — the audit on 2026-05-16 flagged this as premature in isolation; cleaner to land alongside the reader-pane slice that uses it. | None visible |
| 6 | Lock the door: `scripts/check-render-purification.sh` greps for `App` in `feed.rs` and verifies the ADR-001 D6 table mentions every slice-1 PR. Wired into `ci.sh`. ADR-001 status flips to Accepted. | None |

Feature freeze for the ~2 weeks of evening work the slice takes. Bug fixes and trivial UI tweaks are fine; no new surfaces.

**Cadence note (2026-05-16):** PR 4 was originally a single PR ("pre_draw + render flip"). In practice it split into 4a + 4b + 4c — pre_draw turned out to have a wide-feed and narrow-feed variant, and the render-signature flip wanted its own review weight. The table above records the actual cadence; the original "PR 4" framing is preserved in the slice-1 commit history (`a953261` → `4f00e80`).

### D7. Tests at the Model boundary

- Tests live in inline `#[cfg(test)] mod tests { … }` modules in `trench/src/feed/mod.rs` (and per-pane equivalents for later slices). This matches the existing codebase convention — `trench` is a binary crate with no `lib.rs`, so `tests/` integration files cannot reach internal modules. Tests construct a `FeedModel`, fire `Action`s, assert state + emitted `Effect`s. No fake terminal, no ratatui mocking.
- 8–12 tests by end of slice (default state, tab cycle, scroll, filter toggle, search filter, pre_draw scroll, Enter emits action, viewport resize, discovery isolation).
- No goldenfile snapshot tests (brittle, low value in a themed TUI).
- No characterization tests on legacy code (too expensive to harness; manual smoke instead).
- Manual smoke: `SMOKE.md` checklist run by hand at the end of each PR.
- Tests land **with** each PR, not at the end.

## Consequences

### Positive

- Image rendering and voice land on a clean pattern.
- UI becomes testable without a terminal — the Model is the test surface.
- The `// intentional render-time mutation` comments delete themselves.
- `App` shrinks per slice (feed-related fields move into `App.feed`).
- A reusable pattern proves out — Slice 2 (reader) costs ~4 PRs instead of 6.

### Negative

- 2-week feature freeze.
- `keys/mod.rs` stays 1,442 lines after Slice 1 (we touch match-arm contents, not file structure).
- `primitives/list_state.rs` and `view_models/feed_row.rs` shallow-module debt is deferred (lazy rollout decides if they survive).
- Slice 1 introduces a small forward-design (`ReaderTarget` discriminator on `Action::OpenInReader`) used by only one variant initially. Premature in isolation; correct given known Slice 2 shape.

### Trade-offs explicitly accepted

- **Workspace is not refactored in Slice 1.** It stays owned by `App` and borrowed by models. Splitting Workspace is a separate slice for later.
- **No CHANGELOG of intermediate design ideas.** They live in the grilling transcript that produced this ADR (2026-05-15).
- **No diagrams.** They rot.

## Risks

1. **`pre_draw` skipped during overlay paths.** Mitigation: gate render behind a discipline-enforcing helper that calls `pre_draw` before render.
2. **DiscoveryModel async lifecycle during PR 2.** Mitigation: move only when fully owned, not mid-flight.
3. **PR 5 stranding.** Mitigation: PR 5 preserves current `app.reader_active = true` behaviour inside the orchestrator's match arm; just relocates the source of truth.
4. **PR 3 surgery on `keys/mod.rs`.** Mitigation: changes are mechanical; `SMOKE.md` catches drift.
5. **Borrow conflicts on `(feed, ws)` split.** Mitigation: standard Rust pattern; introduce `app.feed_and_workspace_mut()` accessor.
6. **`pre_draw -> ()` contract.** Flagged: if width-resize needs cache invalidation, signature widens to `Vec<Effect>`. Not pre-solved.

## Slice 2 — Reader pane (forward design, 5 lines)

When the reader slice triggers:

- Shape: `ReaderPaneModel { primary: ReaderInstanceModel, secondary: Option<ReaderInstanceModel> }`. Each instance owns its own tabs + viewport.
- `VoiceModel` lives on `App` next to `ReaderPaneModel` (voice survives closing a paper).
- Image cache + `tread::Reader` resize state move into `ReaderInstanceModel::pre_draw`.
- `Action::OpenInReader { item, target: ReaderTarget }` already exists from Slice 1 PR 5; orchestrator learns the other `ReaderTarget` variants.
- Expected ≈ 4 PRs. Pattern lifted from Slice 1.

## Related

- `docs/CONTEXT.md` — domain language and patterns.
- `CLAUDE.md` — project-wide rules.
- Source of decisions: grilling session, 2026-05-15.
