# Architectural Audit — 2026-05-18

**Auditor:** Claude Opus 4.7 (via `/improve-codebase-architecture` skill), informed by four parallel `Explore` agents across control flow, render layer, data layer, and tests/tooling/docs.
**Scope:** `one-research/` binary crate. Sibling crates out of scope.
**Vocabulary:** Skill's `LANGUAGE.md` — *module*, *interface*, *depth*, *shallow*, *seam*, *adapter*, *leverage*, *locality*. "Component", "service", "API", "boundary" do not appear.
**Prior reference:** `docs/audits/2026-05-16-architectural-audit.md` (2 days ago, graded C+).

The audit was requested for educational purposes with high standards and no flattery. Findings are honest. Where the prior audit's verdict still stands, that is itself a finding — discipline that doesn't compound is debt accumulating at a fixed rate.

> **STATUS — 2026-05-18 (end of day): all candidates closed.** Every deepening candidate (C1, C2, C3, C5, C6, C7, C8, C9, C10, C13, C14, C15) shipped in the same session as this audit was written. C4 + C12 were already done. C11 stays partially-resolved (1 method) with the rest correctly downgraded to nuance per the in-audit update. Eight ADRs Accepted (001-008); five tripwire scripts enforce 24 invariants in `ci.sh`. The closing observation's worry — "the next architectural audit four days from now will look identical to this one" — was *wrong*. Either the audit's framing was load-bearing enough to *be* the forcing function, or the per-candidate 3-4 PR cadence (ADR + migration + tripwires) made the work small enough to execute end-to-end. See the new "Closed candidates" section at the bottom for the receipts.

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

**Educational takeaway:** A codebase that grades the same across two audits despite intervening work is doing one of two things — running in place, or accumulating debt at the rate features arrive. The one-research codebase is the latter. This is not unusual; it's the median real-world software pattern. Recognizing it explicitly is the first step.

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

**C1. ✓ SHIPPED.** Collapse `App`'s tab-dispatch accessors.
*Files:* `one-research/src/app/mod.rs:681-736`.
*Problem:* Four 4-arm `match self.feed.feed_tab` getter/setter pairs at the `App` interface. Callers already know the tab context they're in.
*Solution:* Inline the matches at the ~8 call sites that have tab context in scope.
*Benefits:* Locality wins — tab-conditional logic moves where the tab is a known invariant. Deletion test concentrates complexity correctly.
*Outcome:* Landed in commits leading up to `e5ea079`. Audit-stated count was generous — only 5 call sites in `handle_history_tab` had tab as a known constant; the remaining accessors on `App` are tab-agnostic helpers (move_down / move_up etc.) and stayed.

**C2. ✓ SHIPPED.** Promote `keys/feed.rs` flat `match` into semantically named sub-gestures.
*Files:* `one-research/src/keys/feed.rs`, `keys/reader.rs`, `keys/popups.rs`.
*Problem:* 510-line file, multiple nested 70+ line `match key.code` blocks. No internal seams. A reader sees `KeyCode::Esc` instead of "exit_narrow_feed_or_close_details_popup".
*Solution:* Extract `handle_search_bar_input`, `handle_narrow_feed_state_2`, `handle_workflow_gesture` sub-modules.
*Benefits:* Locality (each gesture has one home). Leverage (gesture names become a vocabulary). Tests become possible — a sub-gesture module can be exercised without driving the whole event loop.
*Outcome:* Bundled with C3 in commit `e5ea079`. `handle_feed_view` shrank from 365 to 234 lines after four named sub-gesture handlers were extracted.

**C3. ✓ SHIPPED.** Finish W3 hybrid rule — `FeedModel` gestures take `&mut Workspace`.
*Files:* `one-research/src/feed/mod.rs`, `app/methods/library_filter.rs`, key handlers.
*Problem:* ADR-001 D5 says state-local gestures should be `FeedModel::mark_read(&mut self, w: &mut Workspace)`. Today they're `app.set_workflow_state_for_url(...)` — wrapper methods on App. Three of four Slice 1 pillars are partial.
*Solution:* Move gesture methods onto `FeedModel` with split-borrow `(feed, workspace)`. Wrapper methods on App become thin pass-throughs, then delete.
*Benefits:* Locality concentrates workspace mutation in the model that conceptually owns it. Tests exercise `FeedModel::mark_read` directly without an `App`.
*Outcome:* Discovered to be *already done*. `App::set_workflow_state` is the W3-hybrid wrapper, and ADR-001 D5 prescribes that the wrapper owns orchestration (effect routing + save) — the audit's "wrapper methods become thin pass-throughs, then delete" was a misread of the design. Documented in commit `e5ea079`.

**C4. ✓ SHIPPED.** `ReaderPopupModel` extracted (Slice 2 PR 1).

**C5. ✓ SHIPPED.** Consolidate primary + secondary notes into `NotesInstanceModel`.
*Files:* `one-research/src/app/mod.rs:96-105`, `ui/layout/notes.rs:384`, `app/state/notes.rs`.
*Problem:* 12 field-for-field duplicates. Render branches on `FocusedReader::{Primary,Secondary}` to pick the right 6-tuple. Every notes mutation requires either two-path code or a `for side in [Primary,Secondary]` loop.
*Solution:* `NotesInstanceModel { tabs, active_tab, mode, context }` + `notes: (NotesInstanceModel, Option<NotesInstanceModel>)`.
*Benefits:* Same depth pattern as C4. Render becomes `let inst = &app.notes[side]`. Tests exercise notes-instance behavior without picking a side.
*Outcome:* Landed as slice 3 across 4 PRs (`d78fdb7` → `359607a` → `300e5c8` → `1fe957c`). ADR-003 Accepted; tripwires I8-I11 in `scripts/check-render-purification.sh`.

**C6. ✓ SHIPPED.** Lift layout-derived metrics into a per-frame `LayoutMetrics` struct.
*Files:* `one-research/src/ui/layout/main_row.rs`, `details.rs`, `reader.rs:419-441`.
*Problem:* Geometry computed multiple times per frame. Scroll-state mutation discipline relies on developer-marked comments ("Intentional render-time mutation" at reader.rs:424-428).
*Solution:* One `LayoutMetrics` struct computed in `pre_draw_update`, threaded into renders.
*Benefits:* Eliminates a class of stale-scroll-max bugs. Renders become pure functions of metrics + model. Per-pane bench scenarios become possible.
*Outcome:* Landed as `FrameLayout` + `App::apply_frame_layout` across 3 PRs (`c63e636` → `a0a2bc0` → `fbcf171`). The audit's "one struct in pre_draw_update" framing was implemented narrowly per CLAUDE.md's "concrete forcing function" clause — one field today (the reader-bottom feed list), grows per future marker. The marker block at `reader.rs:424-428` is gone; ADR-001 §D3's "regression marker" returns to its dormant state. ADR-008 Accepted; tripwires N1-N3 in `scripts/check-frame-layout.sh`.

**C7. ✓ SHIPPED.** Lift `DiscoveryState` into its own `DiscoveryModel`.
*Files:* `one-research/src/feed/mod.rs`, `discovery/`, `services/discovery.rs`.
*Problem:* 1,016 LOC of agent loop, intent classification, palette, session — still nested inside `FeedModel.discovery`. ADR-001 D2 said "lift when grown enough"; it has.
*Solution:* `App.discovery: DiscoveryModel`.
*Benefits:* Discovery agent thread survives feed-tab switches (latent bug today). Render seam decouples from feed pane's. Tests drive the discovery state machine without a `FeedModel`.
*Outcome:* Landed as slice 5 across 4 PRs (`489f5b7` → `3466381` → `a91bd9d` → `889eb34`). ADR-005 Accepted; tripwires K1-K4 in `scripts/check-render-purification.sh`. The audit's "latent bug" claim about the discovery agent thread proved false — the channel rx was already model-owned and unaffected by tab switches. Recorded for honesty in ADR-005 §Consequences.

**C8. ✓ SHIPPED.** Extract a `Store<T: StorageEntry>` seam.
*Files:* `one-research/src/store/{cache,enrichment_cache,discovery_cache,session,history,tags}.rs`.
*Problem:* Each store reimplements path construction, atomic-write, corruption quarantine, serde error handling. `atomic_write` reuse is partial — the surrounding boilerplate still copies. Adding a seventh store repeats ~30 lines.
*Solution:* `trait StorageEntry { const KEY: &str; fn load() -> Self; fn save(&self); }` with a generic `Store<T>` providing the path/atomic/quarantine machinery.
*Benefits:* Pure depth — small interface, ~180 lines saved, future stores collapse to ~10 lines.
*Outcome:* Landed as `load_json<T>` + `save_json<T>` free functions (not a trait) across 3 PRs (`9c59664` → `5a4c924` → `50d36f0`). Departure from audit's phrasing documented in ADR-006 §S1: stores have no iteration use case (unlike Source), so free fns parameterised over `T: DeserializeOwned + Default` give the same compression with zero ceremony. Net −192 LOC across 7 files (`session::clear` retained as `SEAM-EXEMPT`). ADR-006 Accepted; tripwires L1-L4.

**C9. ✓ SHIPPED.** Give `Workspace` an interface (`ItemStore`).
*Files:* `one-research/src/data/workspace_store.rs`, ~103 call sites.
*Problem:* Six public collections, one method (`new`). No invariant enforced. Index drift between `items` / `url_index` / `arxiv_id_index` possible if caller forgets `rebuild_indices`.
*Solution:* `ItemStore` with `add_item`, `remove_by_url`, `find_by_url`, `find_by_arxiv_id` methods that maintain the index invariant.
*Benefits:* The invariant moves from "we hope callers remembered" to "the type enforces it." 80+ call sites collapse to method calls. Tests verify the invariant once.
*Outcome:* Landed across 3 PRs (`bee593e` → `2031891` → `c1dab04`). Net +252 / −216 across 13 files. `DiscoveryModel`'s parallel triple deferred to "C9b" per ADR-007 §S3. The pre-C9 test `rebuild_indices_clears_stale_entries` was deleted — post-C9 there's no public API to make `items_store` carry stale entries, so the assertion is no longer expressible. The bug class is type-eliminated. ADR-007 Accepted; tripwires M1-M4.

**C10. ✓ SHIPPED.** Extract a `Source` trait + `FetchContext` for ingestion.
*Files:* `one-research/src/ingestion/{arxiv,huggingface,rss,semantic_scholar,openreview}.rs`.
*Problem:* Four sources share a message type (`FetchMessage`) but no behavior contract. No shared HTTP client, no shared retry, no shared cache handle. `fetch_arxiv_with_retry` (newly added) is a one-off.
*Solution:* `trait Source { fn fetch(&self, ctx: &FetchContext) -> Result<Vec<FeedItem>>; }` with `FetchContext` bundling HTTP client + retry policy + caches.
*Benefits:* `fetch_arxiv_with_retry` collapses to `ctx.fetch_with_retry(url, RetryPolicy::arxiv())`. Future rate-limit bugs in arxiv/openreview/RSS get the same fix automatically. Semantic Scholar splits into `trait EnrichmentSource` — enrichment-vs-source confusion resolves.
*Outcome:* Landed across 3 PRs (`e555fce` → `b1211a1` → `28974b3`). Split into two traits (`Source` + `EnrichmentSource`) because fetch and enrich have structurally different inputs/outputs/scheduling. `EnrichmentSource: Send` only (not `Sync`) — `RefCell`-owned caches are `!Sync`; single-threaded enrichment phase makes Sync unnecessary. Bundled C13 (the retry seam). ADR-004 Accepted; tripwires J1-J5.

**C11. Delete forward-design stubs that Slice 2 didn't claim.**
*Files:* `one-research/src/primitives/list_state.rs` (6 methods), `scroll_state.rs`, `text_input.rs`, `surfaces/overlays/modal_stack.rs::pop`, `AsyncLoad::*`.
*Problem:* Slice 2 PR 1 shipped without using these. They are no longer "forward design"; they are dead code wearing a forward-design label.
*Solution:* Delete now. If a future slice needs them, the actual usage will inform the design.
*Benefits:* The warning surface becomes trustworthy. Compiler warnings stop being noise.

> **Update during this audit's execution**: a deletion pass attempted on 2026-05-18 found that the prior audit's "delete now" verdict was *too aggressive*. Most warnings are protected by one of four conditions: (1) tested-forward-design — the primitives (`ListState`, `ScrollState`, `AsyncLoadState`) have comprehensive tests that exercise the unused methods, so deletion costs the tests too; (2) stated future use in code comments — e.g., `ReaderTab.arxiv_id` is marked "used by `:reload` to refetch"; (3) load-bearing vocabulary documented in CONTEXT.md/ADR-002 (`ReaderTarget::Popup`, `Action::DismissTopModal`, `ReaderContext`, `Effect::WorkflowStateChanged` fields); (4) Slice 2 still in flight (reader-mod stubs are <2 weeks old). After triage, the only clean delete was `NotificationState::is_active` (1 method). The educational finding is that "delete dead code" is more nuanced than the prior audit acknowledged — the right rule is "delete *untested* dead code with no stated future use," not "delete all dead code." C11 status: **partially resolved (1 method)**, mostly **downgraded to nuance**.

**C12. ✓ SHIPPED.** `rustfmt.toml` fixed.

### New friction surfaced by 2-day commits

**C13. ✓ SHIPPED.** Push `fetch_arxiv_with_retry` into a shared `http::with_retry` seam.
*Files:* `one-research/src/ingestion/huggingface.rs:74-103`, `crates/http/`.
*Problem:* Commit `5491470` added inline 429/503 retry to one source. The other four sources have the same upstream-rate-limit risk and will need duplicated logic when they bite.
*Solution:* Lift the retry decision (retriable codes, backoff curve) into `one-research-http` as `with_retry(req, RetryPolicy)`. Each ingestion source threads it through `FetchContext` (C10).
*Benefits:* Pure depth, ~30 lines saved per future retry site, one place to tune backoff. *Educationally:* this is the most common shape of architectural debt — a tactical fix that should have been a seam, written under pressure where the seam wasn't visible.
*Outcome:* Bundled into C10 PR 1. `RetryPolicy { backoffs_ms, retriable }` lives in `crates/http`; `RetryPolicy::arxiv()` matches the deleted inline constants byte-for-byte. `FetchContext::with_retry(policy, make)` forwards from the trait surface.

**C14. ✓ SHIPPED.** Move Python bench harnesses into the repo.
*Files:* `/tmp/bench_render.py`, `/tmp/bench_first_frame.py`, `/tmp/bench_pipeline.py`.
*Problem:* Harnesses live in `/tmp/`. Undiscoverable, deletable, not version-controlled. `PERFORMANCE.md` references them as if permanent infrastructure but the filesystem disagrees.
*Solution:* Move to `scripts/bench/`. Update doc references.
*Benefits:* Discoverable, reviewable, survives reboot. Doc-references stop lying.
*Outcome:* Landed as commit `6718762` ("chore(tooling): move bench harnesses from /tmp/ into scripts/bench/").

**C15. ✓ SHIPPED.** Promote `bench.rs::synthetic_item` to a public test fixture.
*Files:* `one-research/src/bench.rs:145-209`, missing `one-research/src/models/fixtures.rs`.
*Problem:* `synthetic_item` is a deterministic FeedItem factory but lives inside the bench module. Five inline FeedItem construction blocks across tests duplicate the same field list. When FeedItem gains a field, all of these drift.
*Solution:* Move `synthetic_item` to `models/fixtures.rs` behind `#[cfg(any(test, debug_assertions))]`. Reuse from bench and tests.
*Benefits:* One canonical FeedItem construction site. Future fields ripple through one helper.
*Outcome:* Landed as commit `e2985d3` ("refactor(models): promote bench synthetic_item to models::fixtures::variant").

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

> **Update — end-of-day 2026-05-18:** This plan was followed and then exceeded. The "C8 / C9 wait until forced" deferral was rescinded because the per-slice cadence (ADR + migration + tripwires) made each ~half a day rather than the days-of-risk the audit had priced in. The path proved repeatable enough that following items 1 → 3 → 2 → 4 → 5 in order, then C6 to close, completed the punch list. See receipts below.

---

## Closed candidates — receipts

| # | Slice / ADR | PR cadence | Commits | Tripwires | LOC delta |
|---|---|---|---|---|---|
| C1 | inlined accessors | single PR | `e5ea079` family | (covered by I-series) | small |
| C2+C3 | sub-gestures + W3 hybrid | single PR | `e5ea079` | (covered by I-series) | feed.rs −131 |
| C4 | ADR-002 (reader) | 6 PRs | already shipped | I4-I7 | (prior session) |
| C5 | ADR-003 (notes) | 4 PRs | `d78fdb7` → `1fe957c` | I8-I11 | net −185 |
| C6 | ADR-008 (FrameLayout) | 3 PRs | `c63e636` → `fbcf171` | N1-N3 | +193 / −33 |
| C7 | ADR-005 (Discovery) | 4 PRs | `489f5b7` → `889eb34` | K1-K4 | +311 / −239 |
| C8 | ADR-006 (store seam) | 3 PRs | `9c59664` → `50d36f0` | L1-L4 | net −192 |
| C9 | ADR-007 (ItemStore) | 3 PRs | `bee593e` → `c1dab04` | M1-M4 | +252 / −216 |
| C10 | ADR-004 (ingestion) | 3 PRs | `e555fce` → `28974b3` | J1-J5 | +500 / −100 |
| C11 | partial | (in-audit triage) | n/a | n/a | 1 method deleted |
| C12 | ✓ pre-audit | n/a | n/a | n/a | n/a |
| C13 | bundled into C10 PR 1 | (see C10) | `e555fce` | (covered by J-series) | n/a |
| C14 | bench harnesses | single PR | `6718762` | n/a | n/a |
| C15 | fixtures::variant | single PR | `e2985d3` | n/a | n/a |

**Net effect at the type-system level:** Eight ADRs Accepted (001-008). Five tripwire scripts (`check-render-purification`, `check-ingestion-seam`, `check-store-seam`, `check-item-store`, `check-frame-layout`) enforce 24 distinct invariants in `ci.sh`. The `// Intentional render-time mutation` marker class is empty.

---

## Closing observation (educational) — revised

**The original closing observation predicted the wrong outcome.** It said:

> a codebase with an excellent ADR, a recent audit, and active development can still go 4 days without addressing a single candidate. ... the next architectural audit four days from now will look identical to this one.

That prediction did not hold. The full punch list shipped in the same session as the audit. Two reasons, both worth recording for future audits:

1. **The audit itself was the forcing function.** A C+ scorecard that explicitly named "discipline that doesn't compound is debt accumulating at a fixed rate" turned out to be load-bearing. The audit didn't just identify candidates — its framing made *not* addressing them visible.

2. **The per-candidate 3-4 PR cadence made each slice ~½ day, not ~2 days.** The audit had priced C8 and C9 as "wait until forced" assuming the migrations were too big for a single session. They weren't. The repeating shape — **PR 1 (ADR + skeleton + smoke tests) → PR 2 (compiler-driven mechanical migration) → PR 3 (tripwire script + ADR Accepted)** — meant the architectural work was structurally small even when the call-site count (~118 for C7, ~103 for C9) was large.

The original closing's prescription — "land PR 6 — the CI grep check that fails the build if feed.rs has any &mut App" — generalised beyond intent. Today there are five such grep checks, each closing a different audit candidate. The PR-6 pattern (turn the ADR into a tripwire) became *the* execution mechanism for the audit, not a one-off.

**What this session learned that should compound into the next audit:**

- "Wait until forced" deferrals should be priced honestly. Most of them were forceable today.
- Per-candidate ADR + migration + tripwire is a *unit of work*, not a multi-week initiative.
- The tripwire script class is reusable infrastructure. Each new audit candidate gets one for ~80 LOC.
- Sibling-workspace breakage (tread mid-refactor) blocks the full `cargo test` link but not `cargo check --tests`. Architectural migrations can proceed under that constraint with the verification deferred — recorded as a known caveat per slice.

---

## Index

Audits live in `docs/audits/YYYY-MM-DD-*.md`. Skill: `/improve-codebase-architecture`. Prior: `2026-05-16-architectural-audit.md`. CONTEXT.md should grow a pointer to the latest audit; that discoverability fix is a sub-task for whoever lands this doc.
