# Architectural Audit — 2026-05-16

**Auditor:** Claude Opus 4.7 (via /improve-codebase-architecture skill), informed by four parallel `Explore` agents across control flow, render layer, data layer, and tests/docs.
**Scope:** `trench/` binary crate. Sibling crates (`tread`, `hygg-reader`, `cli-text-reader`, `cli-justify`, etc.) out of scope.
**Vocabulary:** `docs/adr/improve-codebase-architecture-language.md` is not vendored; this audit uses the skill's LANGUAGE.md terms — *module*, *interface*, *depth*, *shallow*, *seam*, *adapter*, *leverage*, *locality*. "Component", "service", "API", "boundary" are not used here.

The audit was requested for educational purposes with high standards and no flattery. Findings are honest and unsparing where the friction is real.

---

## Scorecard

Letter grades, A (excellent) to F (failing). Grades are relative to *what the codebase is trying to be* (a research-feed TUI mid-refactor under ADR-001), not against a hypothetical ideal.

| # | Dimension | Grade | One-line summary |
|---|---|---|---|
| 1 | Domain vocabulary | **B+** | `FeedItem`/`Paper` rules followed; legacy terms nearly purged; `Workspace` is named but underused as a concept. |
| 2 | ADR-001 design integrity | **A−** | The decision is sound, well-grilled, well-documented. Forward design for Slice 2 is deliberately under-specified — that's discipline, not a gap. |
| 3 | ADR-001 execution (in-flight) | **C+** | 5 of 6 PRs in for the feed pane; one render-signature flip remains; six other panes untouched. Trajectory is good, surface is still 80% legacy. |
| 4 | `App` composition root | **D+** | 1,556-line struct, ~80 public fields, no invariant. Slice 1 carved out `App.feed`; reader/notes/popup/voice fields still scatter across `App` with no struct boundary. |
| 5 | Key dispatch (`keys/`) | **C** | The 1,442-line `keys/mod.rs` is a legitimately deep modal dispatcher. Per-pane handlers underneath are flat 100+ line `match` trees — shallow, no semantic sub-gestures named. |
| 6 | Render layer | **C** | Feed pane mutation hoisted into `pre_draw` (PRs 4a/4b). Reader, notes, chat, settings, repo viewer, popup, sources-popup still take `&mut App` at render time. |
| 7 | Workspace (data) | **C−** | A struct of six public collections, one method (`new`). No invariants enforced; index drift is possible. 128+ direct field accesses across the codebase. |
| 8 | Persistence (`store/`) | **C−** | Six autonomous store modules each reimplementing path + atomic-write + corruption-recovery boilerplate. `atomic_write` is genuinely useful as a utility but there is no module seam. |
| 9 | Ingestion | **C+** | `FetchMessage` is a real seam (four sources behind it). Each adapter is independent prose; no shared `FetchContext`. Semantic Scholar is an enrichment masquerading as a source. |
| 10 | Discovery sub-state | **C** | 1,016 LOC of agent loop, intent classification, palette, session — still nested inside `FeedModel::discovery`. Has grown past the "Q4 lazy lift" threshold. |
| 11 | Test coverage | **C** | 166 tests, all on parsing/ingestion/store/`FeedModel`. ~98 modules with zero tests. UI render and 1,442 lines of key dispatch are entirely untested. |
| 12 | Test surface health | **B** | Where tests exist, they cross the right seam (`FeedModel` is the test surface for slice 1). `pre_draw_narrow_feed_*` tests verify *intent*, not just behavior. |
| 13 | Dead code / warnings | **C** | 13 compiler warnings + 127 clippy. Most dead methods are intentional forward-design stubs for Slice 2 (`ListState::page_down`, `AsyncLoad::*`). Some are mechanical PR-2 leftovers. |
| 14 | Doc currency | **B−** | ADR-001 D6 table is one-week stale (PR 4 split into 4a + 4b + pending flip). CONTEXT.md slice-status table is current. CLAUDE.md TODO has 2 stale `[-]` items. |
| 15 | Tooling (fmt/CI) | **D** | `rustfmt.toml` requires `wrap_comments = true` (nightly-only). Stable `cargo fmt` cannot apply it; widespread drift across 20+ files. Hostile to pre-commit hooks. |

**Overall: C+.** This is a real grade, not a flattering one. The codebase has an *excellent* design document (ADR-001) and a *partially executed* refactor against it. The gap between the two — six panes still legacy, one render-signature flip pending, six store modules un-abstracted, ~80 fields still on `App` — is where the friction lives.

The trajectory is improving: PR 4b landed today, the model-test discipline holds, and the term hygiene is genuinely good. Sustained, the grade will move toward B+ inside a year of evening work. Abandoned mid-flight, the grade collapses toward D+ because partially-refactored codebases are worse than either fully-refactored or fully-legacy ones (callers can't predict which pattern applies).

---

## Drift: ADR-001 vs reality

Worth fixing in the same pass as the next slice-1 commit:

| Decision | ADR-001 says | Reality on 2026-05-16 |
|---|---|---|
| D6 — PR 4 | "pre_draw + render flip as one PR" | Split into 4a (landed) + 4b (landed today) + render-signature flip (pending). |
| D6 — PR count | 6 PRs total | 7 PRs likely (4a, 4b, 4c-or-renamed). |
| D7 — tests with each PR | "Tests land with each PR, not at the end" | PR 3 (gesture methods) added zero tests — mechanical rewrite was the justification, but PR 3's methods have no model-side tests today. |
| W3 hybrid rule (D5) | "model methods take `&mut Workspace` directly" | No `FeedModel` method takes `&mut Workspace` yet. Workflow-state mutations still go through `App::set_workflow_state_for_url`. |
| `Action` vocabulary growth | "Grows as panes migrate" | Stuck at 2 variants (`DismissTopModal`, `OpenSettings`) — no slice-1 gesture has been promoted to an Action yet. |

D5 and the `Action` vocabulary point at the same gap: PR 5 (cross-pane Action::OpenInReader) hasn't started, and PR 3 (gesture methods) didn't push gestures all the way to `Action` emission. Two of the three Slice 1 pillars (composition root ✓, pre_draw ~, `Action` ✗, render purity ~) are still partial.

---

## Deepening candidates

The skill says: present candidates, do not propose interfaces yet. The user picks; the grilling-loop step designs the interface.

Twelve candidates surfaced across the four agent slices. Listed in roughly descending leverage. Each has a deletion-test verdict.

### Slice A — control flow & state ownership

**C1. Collapse `App`'s tab-dispatch accessors back into callers.**
*Files:* `trench/src/app/mod.rs:681–736` (the `active_selected_index` / `active_list_offset` / `set_*` family).
*Today:* Four 4-arm `match self.feed.feed_tab` getter/setter pairs. Callers already know the tab context they're acting in.
*Deletion test:* Complexity vanishes into context-specific mutation at ~8 call sites. The match-on-tab moves to where the tab is already a known invariant. **Verdict: shallow — earns no leverage.**

**C2. Promote `keys/feed.rs`'s flat match into semantically named sub-gestures.**
*Files:* `trench/src/keys/feed.rs`, `trench/src/keys/reader.rs`, `trench/src/keys/popups.rs` (each is one giant `match key.code`).
*Today:* `handle_feed_view` is ~170 lines, one match, no internal seams. Library visual-mode logic, search-bar logic, workflow gestures all inline.
*Deletion test:* Extracting `handle_search_bar_input`, `toggle_library_visual_mode`, `handle_workflow_gesture` etc. **concentrates complexity into named gestures**. Locality wins; leverage gains too because gesture names become a vocabulary the rest of the codebase can match on. **Verdict: real depth available.**

**C3. Finish W3 hybrid rule for the feed pane (`FeedModel` gestures take `&mut Workspace`).**
*Files:* `trench/src/feed/mod.rs`, `trench/src/app/methods/*.rs` (workspace mutators).
*Today:* `FeedModel::mark_read(&mut self, w: &mut Workspace)` etc. don't exist; key handlers call `app.set_workflow_state_for_url(...)` directly. ADR-001 D5 said this is how state-local gestures should flow.
*Deletion test:* If `FeedModel::mark_read` existed, the App-level wrapper would be a thin pass-through (delete it, complexity goes into callers using `(feed, ws)` split borrow). If wrapper goes, `FeedModel` *owns* the gesture. **Verdict: the missing half of PR 3.**

### Slice B — render & UI layer

**C4. Extract `ReaderPopupModel` from `App`.**
*Files:* `trench/src/app/mod.rs:116–124` (7 scattered popup fields), `trench/src/ui/layout/main_row.rs`, `trench/src/ui/layout/reader.rs`.
*Today:* Reader popup state (rx, editor, image_state, burst, active flag) is 7 free fields on `App`. Three call sites must keep them in sync; no invariant enforced.
*Deletion test:* Inlining is small (~80 lines spread); the cost is **invariant scatter** — code reviewers can't tell which fields must move together. **Verdict: a Slice 2 prerequisite, smaller than the full reader-pane lift.**

**C5. Consolidate primary + secondary notes into `NotesInstanceModel`.**
*Files:* `trench/src/app/mod.rs:94–105` (a 12-field bifurcation), `trench/src/ui/layout/notes.rs`, `trench/src/app/state/notes.rs`.
*Today:* Primary and secondary notes are field-for-field duplicates. Render branches on `FocusedReader::{Primary, Secondary}` to pick the right 6-tuple.
*Deletion test:* Collapsing into `notes_primary: NotesInstanceModel, notes_secondary: Option<NotesInstanceModel>` localizes the invariant. Render code's branching simplifies. **Verdict: this should land alongside Slice 2's reader lift, not on its own — they share the focused-reader concept.**

**C6. Lift layout-derived metrics into a per-frame `LayoutMetrics` struct.**
*Files:* `trench/src/ui/layout/main_row.rs`, `trench/src/ui/layout/details.rs`, `trench/src/ui/layout/reader.rs`.
*Today:* Geometry (list visible rows, details panel height, scroll maxima) is computed at multiple points per frame, sometimes redundantly, sometimes with stale scroll state.
*Deletion test:* One `LayoutMetrics` per frame, computed in `pre_draw` or layout-orchestrator, threaded to renders. Eliminates redundant arithmetic and ends a class of "stale scroll max" bugs. **Verdict: pairs with the render-signature flip; consider bundling.**

### Slice C — data, workspace, ingestion, store

**C7. Lift `DiscoveryState` out of `FeedModel` into its own `DiscoveryModel`.**
*Files:* `trench/src/feed/mod.rs` (the nested `DiscoveryState`), `trench/src/discovery/*`, `trench/src/services/discovery.rs`.
*Today:* 15 fields nested inside `FeedModel.discovery` driving 1,016 LOC of agent + intent + palette + session. ADR-001 D2 said "lift when grown enough" — it has grown.
*Deletion test:* Promoting to `App.discovery: DiscoveryModel` lets the agent thread survive feed-tab switches (a latent bug today), and decouples discovery's render seam from the feed pane's. **Verdict: deserves its own slice, parallel to Slice 2; small enough to be a 2-PR slice.**

**C8. Extract a `Store<T: StorageEntry>` seam over the six persistence modules.**
*Files:* `trench/src/store/{cache,enrichment_cache,discovery_cache,session,history,tags}.rs`.
*Today:* Each store reimplements path construction, atomic-write, corruption quarantine, serde error handling — ~30 lines of boilerplate × 6 modules.
*Deletion test:* If the seam existed, each store collapses to `struct X; impl StorageEntry for X { const KEY: &str = "x"; }`. Today, adding a new store (e.g., highlights) repeats the boilerplate. **Verdict: pure depth — small interface, big locality win, ~180 lines saved.**

**C9. Give `Workspace` an interface (`ItemStore` for items + indices).**
*Files:* `trench/src/data/workspace_store.rs` (search the repo to confirm path), `trench/src/app/mod.rs` (call sites).
*Today:* `Workspace` exposes six public collections and one constructor. 128 direct field accesses across the codebase. Index drift between `items` / `url_index` / `arxiv_id_index` is possible if a caller forgets to call `rebuild_indices`.
*Deletion test:* An `ItemStore { items, url_index, arxiv_id_index }` enforces the invariant via `add_item` / `remove_by_url` / `find_by_url`. Field access in callers shrinks to method calls. Tests can verify the invariant once instead of "we hope callers remembered to rebuild." **Verdict: high leverage, also unblocks future Phase 4 splitting.**

**C10. Extract a `FetchContext` and `Source` trait for ingestion.**
*Files:* `trench/src/ingestion/{arxiv,huggingface,rss,semantic_scholar}.rs`.
*Today:* Four sources share a message type but not a behavior contract. Semantic Scholar is structurally an enrichment, lives in `ingestion/`. No shared HTTP client, no shared cache handle, no shared arxiv-id dedup.
*Deletion test:* `trait Source { fn fetch(&self, ctx: &FetchContext) -> Result<Vec<FeedItem>>; }` unifies the contract. Adding a new source (e.g., OpenReview, which is half-wired today) becomes a one-struct impl. **Verdict: real depth, also untangles the enrichment-in-ingestion confusion.**

### Slice D — tests, dead code, tooling

**C11. Delete forward-design stubs that Slice 2 hasn't claimed yet.**
*Files:* `trench/src/primitives/list_state.rs` (`page_down`, `page_up`, `count`, `go_to_top`, `go_to_bottom`, `viewport_size`), `trench/src/primitives/scroll_state.rs`, `trench/src/primitives/text_input.rs`, `trench/src/surfaces/overlays/modal_stack.rs::pop`, `AsyncLoad` associated items.
*Today:* ~15 methods compile-flagged dead. Most labelled "for Slice 2." Two-week-old dead code is forward design; six-month-old dead code is fiction.
*Deletion test:* Delete now; if Slice 2 needs them, the design-pressure that re-introduces them will be informed by the actual usage, not the speculative API. **Verdict: small but high-signal — keeps the warning surface trustworthy.**

**C12. Fix `rustfmt.toml` (drop `wrap_comments = true` or move CI to nightly).**
*Files:* `rustfmt.toml`, `ci.sh`.
*Today:* `wrap_comments = true` is nightly-only. Stable `cargo fmt` silently ignores it and reformats files differently. 20+ files drifted; every `cargo fmt` run rewrites the same lines, then a developer reverts because they didn't intend that. Pre-commit hooks impossible.
*Deletion test:* Either remove the setting (accept wider comments) or run `cargo +nightly fmt` in CI and locally. **Verdict: not architectural, but quality-of-life critical and a precondition for any "lock the door" CI check (PR 6).**

---

## Recommended path forward

Ordering matters because some candidates unblock others. Numbers below match the candidate IDs.

1. **First — finish Slice 1.** Render-signature flip (the deferred half of PR 4), then **C3** (W3 hybrid for feed), then PR 6 (lock the door + ADR-001 status flip from Proposed to Accepted). This closes the open slice before opening a new one — the discipline ADR-001 is built around.

2. **Second — quality-of-life sweep.** **C12** (fmt config), **C11** (dead-code deletion). Both are sub-day. Doing them before the next slice means PR 6's grep check is meaningful (the file isn't full of stubs PR 6 can't reason about).

3. **Third — pick one new slice, complete it.** ADR-001 D2 says lazy rollout. Options:
   - **Slice 2: reader pane** (the original next slice) — bundles **C4** + **C5** + **C6** under one ADR. Forcing function: image rendering, voice.
   - **Slice 1.5: discovery pane** (**C7**) — promote `DiscoveryState` to `DiscoveryModel`. Smaller (~2 PRs), independent forcing function (the discovery agent thread bug).
   - **Cross-cutting: persistence** (**C8**) — `Store<T>` seam. Independent of any pane; clears boilerplate; lays groundwork for future highlights/progress stores.

4. **Fourth — heavier lifts when forced.** **C9** (`Workspace` interface), **C10** (ingestion `Source` trait). Neither is forced by image rendering or voice; defer until a feature needs them.

5. **Always — backlog hygiene.** **C1** (collapse shallow accessors) and **C2** (named sub-gestures) are small enough to slip into any slice that touches their files. Don't make them their own PRs; make them subtractions inside the slice that's already there.

---

## How to avoid the "mixup" recurrence

The "mixup" the user named is: PR 4 was meant to be one PR; it became 4a + 4b + a pending flip; the ADR table didn't reflect it; future-readers (including future-you) would have been confused.

The pattern is universal: **commit-level reality drifts from ADR-level intent within days.** The fix is procedural, not architectural:

- **When a PR splits or merges in flight, update ADR-001's D6 table in the same commit.** The table is normative. If reality diverges, either rewrite the table or rewrite the PR.
- **PR 6's "lock the door" check should include an ADR-table consistency check** — does every committed slice-1 PR appear in D6, and vice versa? A dozen-line script.
- **This audit doc itself is a tripwire.** A future audit run that produces a *different* drift table is the alarm bell. Run this audit before each new slice begins.

---

## Index for future audits

Audits live in `docs/audits/YYYY-MM-DD-*.md`. The skill that generates them is `/improve-codebase-architecture`. CONTEXT.md should grow a one-line pointer to the latest audit; that's the simplest discoverability fix and is left as a sub-task for whoever lands this doc.
