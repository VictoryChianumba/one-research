# Architectural Audit — 2026-05-18

**Auditor:** Claude Opus 4.7 (via `/improve-codebase-architecture` skill), informed by four parallel `Explore` agents across control flow, render layer, data layer, and tests/tooling/docs.
**Scope:** `trench/` binary crate. Sibling crates out of scope.
**Vocabulary:** Skill's `LANGUAGE.md` — *module*, *interface*, *depth*, *shallow*, *seam*, *adapter*, *leverage*, *locality*. "Component", "service", "API", "boundary" do not appear.
**Prior reference:** `docs/audits/2026-05-16-architectural-audit.md` (2 days ago, graded C+).

The audit was requested for educational purposes with high standards and no flattery. Findings are honest. Where the prior audit's verdict still stands, that is itself a finding — discipline that doesn't compound is debt accumulating at a fixed rate.

---

## Scorecard

| # | Dimension | 2026-05-16 | 2026-05-18 | Δ | One-line summary |
|---|---|---|---|---|---|
| 1 | Domain vocabulary | B+ | **B+** | — | `FeedItem`/`Paper` rules followed; `Workspace` still underused as concept. |
| 2 | ADR-001 design integrity | A− | **A−** | — | ADR remains load-bearing; D6 table drift now resolved. |
| 3 | ADR-001 execution | C+ | **C+** | — | Slice 2 PR 1 landed (ReaderPopupModel); 11 of 12 prior candidates untouched. |
| 4 | `App` composition root | D+ | **D+** | — | 1,531 lines (was 1,556 — comment removal, not structural). ~100 fields. 37 raw UI flags. |
| 5 | Key dispatch (`keys/`) | C | **C** | — | 1,442 lines untouched. Flat `match key.code` per pane. No sub-gestures named. |
| 6 | Render layer | C | **C** | — | C4 (ReaderPopupModel) shipped. Four `&mut App` render fns are read-only and should flip. |
| 7 | Workspace (data) | C− | **C−** | — | 103 direct field accesses. No invariant enforcement. |
| 8 | Persistence (`store/`) | C− | **C** | ↑ | `atomic_write` now used by 5 callers (was 4). Six modules still copy path+corruption boilerplate. |
| 9 | Ingestion | C+ | **C** | ↓ | New `fetch_arxiv_with_retry` is a one-off; missed seam opportunity. |
| 10 | Discovery sub-state | C | **C** | — | Still nested in `FeedModel`. 1,016 LOC. No movement. |
| 11 | Test coverage | C | **C−** | ↓ | 172 tests (+6, none from this week's work). Bench + retry + max_results + export all untested. |
| 12 | Test surface health | B | **B** | — | Where tests exist, they cross the right seam. `bench.rs` is well-shaped but bench ≠ test. |
| 13 | Dead code / warnings | C | **C** | — | 12 compiler warnings stable; 131 clippy (+4 from bench). C11 stubs still warning post-Slice-2. |
| 14 | Doc currency | B− | **B+** | ↑ | ADR-001 D6 drift resolved. CONTEXT.md current. New `PERFORMANCE.md` is well-shaped. |
| 15 | Tooling (fmt/CI) | D | **A** | ↑↑ | `wrap_comments=true` removed; stable/nightly fmt parity. |

**Overall: C+ (unchanged).**

The three rising grades (8, 14, 15) are real wins. The two falling grades (9, 11) are anti-patterns introduced *by the same recent work* — feature velocity faster than test/seam velocity is now visible in the audit numbers. Net effect: the floor came up modestly, the ceiling came down modestly, the average is unchanged.

**Educational takeaway:** A codebase that grades the same across two audits despite intervening work is doing one of two things — running in place, or accumulating debt at the rate features arrive. The trench codebase is the latter. This is not unusual; it's the median real-world software pattern. Recognizing it explicitly is the first step.

---

## Drift since 2026-05-16

| Item | Status |
|---|---|
| ADR-001 D6 table (was 1 week stale) | **RESOLVED** — reflects PR 4a/4b/4c split |
| `rustfmt.toml` `wrap_comments=true` | **RESOLVED** — removed; stable fmt now works |
| C11 forward-design stubs ("for Slice 2") | **STALE** — Slice 2 shipped without using them. Now dead code, not forward design |
| Python bench harnesses | **NEW DRIFT** — 4 files live in `/tmp/`, undiscoverable, deletable |
| HF retry seam | **NEW DRIFT** — added inline (`fetch_arxiv_with_retry`) instead of as `crate::http::with_retry` |
| Bench `synthetic_item` factory | **NEW DRIFT** — duplicates FeedItem construction, not exposed as test fixture |

---

## Deepening candidates

Per the skill: candidates, not interfaces. Twelve carried from the prior audit (one shipped, two resolved); three new ones surfaced by the 2-day deltas.

### Carried from 2026-05-16

**C1. Collapse `App`'s tab-dispatch accessors.**
*Files:* `trench/src/app/mod.rs:681-736`.
*Problem:* Four 4-arm `match self.feed.feed_tab` getter/setter pairs at the `App` interface. Callers already know the tab context they're in.
*Solution:* Inline the matches at the ~8 call sites that have tab context in scope.
*Benefits:* Locality wins — tab-conditional logic moves where the tab is a known invariant. Deletion test concentrates complexity correctly.

**C2. Promote `keys/feed.rs` flat `match` into semantically named sub-gestures.**
*Files:* `trench/src/keys/feed.rs`, `keys/reader.rs`, `keys/popups.rs`.
*Problem:* 510-line file, multiple nested 70+ line `match key.code` blocks. No internal seams. A reader sees `KeyCode::Esc` instead of "exit_narrow_feed_or_close_details_popup".
*Solution:* Extract `handle_search_bar_input`, `handle_narrow_feed_state_2`, `handle_workflow_gesture` sub-modules.
*Benefits:* Locality (each gesture has one home). Leverage (gesture names become a vocabulary). Tests become possible — a sub-gesture module can be exercised without driving the whole event loop.

**C3. Finish W3 hybrid rule — `FeedModel` gestures take `&mut Workspace`.**
*Files:* `trench/src/feed/mod.rs`, `app/methods/library_filter.rs`, key handlers.
*Problem:* ADR-001 D5 says state-local gestures should be `FeedModel::mark_read(&mut self, w: &mut Workspace)`. Today they're `app.set_workflow_state_for_url(...)` — wrapper methods on App. Three of four Slice 1 pillars are partial.
*Solution:* Move gesture methods onto `FeedModel` with split-borrow `(feed, workspace)`. Wrapper methods on App become thin pass-throughs, then delete.
*Benefits:* Locality concentrates workspace mutation in the model that conceptually owns it. Tests exercise `FeedModel::mark_read` directly without an `App`.

**C4. ✓ SHIPPED.** `ReaderPopupModel` extracted (Slice 2 PR 1).

**C5. Consolidate primary + secondary notes into `NotesInstanceModel`.**
*Files:* `trench/src/app/mod.rs:96-105`, `ui/layout/notes.rs:384`, `app/state/notes.rs`.
*Problem:* 12 field-for-field duplicates. Render branches on `FocusedReader::{Primary,Secondary}` to pick the right 6-tuple. Every notes mutation requires either two-path code or a `for side in [Primary,Secondary]` loop.
*Solution:* `NotesInstanceModel { tabs, active_tab, mode, context }` + `notes: (NotesInstanceModel, Option<NotesInstanceModel>)`.
*Benefits:* Same depth pattern as C4. Render becomes `let inst = &app.notes[side]`. Tests exercise notes-instance behavior without picking a side.

**C6. Lift layout-derived metrics into a per-frame `LayoutMetrics` struct.**
*Files:* `trench/src/ui/layout/main_row.rs`, `details.rs`, `reader.rs:419-441`.
*Problem:* Geometry computed multiple times per frame. Scroll-state mutation discipline relies on developer-marked comments ("Intentional render-time mutation" at reader.rs:424-428).
*Solution:* One `LayoutMetrics` struct computed in `pre_draw_update`, threaded into renders.
*Benefits:* Eliminates a class of stale-scroll-max bugs. Renders become pure functions of metrics + model. Per-pane bench scenarios become possible.

**C7. Lift `DiscoveryState` into its own `DiscoveryModel`.**
*Files:* `trench/src/feed/mod.rs`, `discovery/`, `services/discovery.rs`.
*Problem:* 1,016 LOC of agent loop, intent classification, palette, session — still nested inside `FeedModel.discovery`. ADR-001 D2 said "lift when grown enough"; it has.
*Solution:* `App.discovery: DiscoveryModel`.
*Benefits:* Discovery agent thread survives feed-tab switches (latent bug today). Render seam decouples from feed pane's. Tests drive the discovery state machine without a `FeedModel`.

**C8. Extract a `Store<T: StorageEntry>` seam.**
*Files:* `trench/src/store/{cache,enrichment_cache,discovery_cache,session,history,tags}.rs`.
*Problem:* Each store reimplements path construction, atomic-write, corruption quarantine, serde error handling. `atomic_write` reuse is partial — the surrounding boilerplate still copies. Adding a seventh store repeats ~30 lines.
*Solution:* `trait StorageEntry { const KEY: &str; fn load() -> Self; fn save(&self); }` with a generic `Store<T>` providing the path/atomic/quarantine machinery.
*Benefits:* Pure depth — small interface, ~180 lines saved, future stores collapse to ~10 lines.

**C9. Give `Workspace` an interface (`ItemStore`).**
*Files:* `trench/src/data/workspace_store.rs`, ~103 call sites.
*Problem:* Six public collections, one method (`new`). No invariant enforced. Index drift between `items` / `url_index` / `arxiv_id_index` possible if caller forgets `rebuild_indices`.
*Solution:* `ItemStore` with `add_item`, `remove_by_url`, `find_by_url`, `find_by_arxiv_id` methods that maintain the index invariant.
*Benefits:* The invariant moves from "we hope callers remembered" to "the type enforces it." 80+ call sites collapse to method calls. Tests verify the invariant once.

**C10. Extract a `Source` trait + `FetchContext` for ingestion.**
*Files:* `trench/src/ingestion/{arxiv,huggingface,rss,semantic_scholar,openreview}.rs`.
*Problem:* Four sources share a message type (`FetchMessage`) but no behavior contract. No shared HTTP client, no shared retry, no shared cache handle. `fetch_arxiv_with_retry` (newly added) is a one-off.
*Solution:* `trait Source { fn fetch(&self, ctx: &FetchContext) -> Result<Vec<FeedItem>>; }` with `FetchContext` bundling HTTP client + retry policy + caches.
*Benefits:* `fetch_arxiv_with_retry` collapses to `ctx.fetch_with_retry(url, RetryPolicy::arxiv())`. Future rate-limit bugs in arxiv/openreview/RSS get the same fix automatically. Semantic Scholar splits into `trait EnrichmentSource` — enrichment-vs-source confusion resolves.

**C11. Delete forward-design stubs that Slice 2 didn't claim.**
*Files:* `trench/src/primitives/list_state.rs` (6 methods), `scroll_state.rs`, `text_input.rs`, `surfaces/overlays/modal_stack.rs::pop`, `AsyncLoad::*`.
*Problem:* Slice 2 PR 1 shipped without using these. They are no longer "forward design"; they are dead code wearing a forward-design label.
*Solution:* Delete now. If a future slice needs them, the actual usage will inform the design.
*Benefits:* The warning surface becomes trustworthy. Compiler warnings stop being noise.

**C12. ✓ SHIPPED.** `rustfmt.toml` fixed.

### New friction surfaced by 2-day commits

**C13. Push `fetch_arxiv_with_retry` into a shared `http::with_retry` seam.**
*Files:* `trench/src/ingestion/huggingface.rs:74-103`, `crates/http/`.
*Problem:* Commit `5491470` added inline 429/503 retry to one source. The other four sources have the same upstream-rate-limit risk and will need duplicated logic when they bite.
*Solution:* Lift the retry decision (retriable codes, backoff curve) into `trench-http` as `with_retry(req, RetryPolicy)`. Each ingestion source threads it through `FetchContext` (C10).
*Benefits:* Pure depth, ~30 lines saved per future retry site, one place to tune backoff. *Educationally:* this is the most common shape of architectural debt — a tactical fix that should have been a seam, written under pressure where the seam wasn't visible.

**C14. Move Python bench harnesses into the repo.**
*Files:* `/tmp/bench_render.py`, `/tmp/bench_first_frame.py`, `/tmp/bench_pipeline.py`.
*Problem:* Harnesses live in `/tmp/`. Undiscoverable, deletable, not version-controlled. `PERFORMANCE.md` references them as if permanent infrastructure but the filesystem disagrees.
*Solution:* Move to `scripts/bench/`. Update doc references.
*Benefits:* Discoverable, reviewable, survives reboot. Doc-references stop lying.

**C15. Promote `bench.rs::synthetic_item` to a public test fixture.**
*Files:* `trench/src/bench.rs:145-209`, missing `trench/src/models/fixtures.rs`.
*Problem:* `synthetic_item` is a deterministic FeedItem factory but lives inside the bench module. Five inline FeedItem construction blocks across tests duplicate the same field list. When FeedItem gains a field, all of these drift.
*Solution:* Move `synthetic_item` to `models/fixtures.rs` behind `#[cfg(any(test, debug_assertions))]`. Reuse from bench and tests.
*Benefits:* One canonical FeedItem construction site. Future fields ripple through one helper.

---

## Priority drift since prior audit

The 2026-05-16 audit's "recommended path forward" said:

1. **First — finish Slice 1.** Render-signature flip, then C3 (W3 hybrid), then PR 6.
2. **Second — quality-of-life sweep.** C12, C11.
3. **Third — pick one new slice, complete it.**
4. **Fourth — heavier lifts when forced.**

What actually happened:

- ✗ Slice 1 unfinished. C3 untouched.
- ⚠ Quality-of-life: C12 ✓, C11 ✗ (more stale, not less).
- ⚠ Slice 2 PR 1 (ReaderPopupModel) shipped — partial start on "third" step.
- ✗ Heavier lifts deferred (correct per plan).
- All session effort went to perf + correctness, not architecture.

The plan was correct; execution against it was minimal.

---

## Recommended path forward (revised 2026-05-18)

1. **Backlog hygiene as a single sub-day PR**: C11 (delete dead stubs), C14 (move bench scripts), C15 (promote fixture). ~2 hours. Audit doc bank precedes.

2. **Finish what's started**: Slice 2 PR 2+ — continue the reader-pane lift while warm. Bundle C5 (notes consolidation) since they share `FocusedReader` state.

3. **The one architecture investment worth its weight right now**: C10 (Source trait + FetchContext). Absorbs C13 (the retry seam I missed), unblocks OpenReview completion, clarifies semantic_scholar's role.

4. **C2 and C3 should slip into any keys/feed PR** that lands for another reason. Not dedicated PRs.

5. **C8 (Store<T>) and C9 (ItemStore) wait until forced**.

---

## Closing observation (educational)

The hardest part of this audit isn't identifying the candidates — most were already in the prior audit. The hardest part is the meta-finding: **a codebase with an excellent ADR, a recent audit, and active development can still go 4 days without addressing a single candidate.** That's the default behavior of every codebase that ships features. The defense is procedural — making architecture a forcing function, not an aspiration.

If I had to pick the single change with highest leverage from this audit, it would not be any of C1-C15. It would be **landing PR 6** — the CI grep check that fails the build if `feed.rs` has any `&mut App`. That converts ADR-001 from a document into a tripwire. Without that tripwire, the next architectural audit four days from now will look identical to this one.

---

## Index

Audits live in `docs/audits/YYYY-MM-DD-*.md`. Skill: `/improve-codebase-architecture`. Prior: `2026-05-16-architectural-audit.md`. CONTEXT.md should grow a pointer to the latest audit; that discoverability fix is a sub-task for whoever lands this doc.
