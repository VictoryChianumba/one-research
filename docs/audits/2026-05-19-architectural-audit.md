# Architectural Audit — 2026-05-19

**Auditor:** Claude Opus 4.7 (via `/improve-codebase-architecture` skill), informed by one parallel `Explore` agent for measurements + friction hunt.
**Scope:** `trench/` binary crate. Sibling crates out of scope (`crates/http`, `crates/notes`, `crates/chat`, `crates/ui-theme`).
**Vocabulary:** Skill's `LANGUAGE.md` — *module*, *interface*, *depth*, *shallow*, *seam*, *adapter*, *leverage*, *locality*.
**Prior reference:** `docs/audits/2026-05-18-architectural-audit.md` (2 days ago, graded C+). The morning conversational audit (in session memory, not on disk) graded the same baseline harshly and named candidates N1–N8.

> **STATUS — 2026-05-19 (end of day): grade lifts from C+ to B−.** Eleven commits in one continuous session shipped today: pre-flight cleanup (warnings 14→0), three witnesses (N1 Source contract behaviour, N2 ItemStore invariant sweep, N8 I12 render-purity tripwire), six structural PRs comprising the entire ADR-009 punch list (DebounceState → LeaderState → ReaderBottomState → ViewFlags → RenderCaches → AsyncJobs). App field count: 108 → 77. Tests: 235 → 338. Warnings: 14 → 0. ADR-009 Accepted. The audit's framing is now load-bearing twice over — both the 2026-05-18 audit and the morning's harsh re-grade became forcing functions executed within hours of being named.

---

## Scorecard

| # | Dimension | 2026-05-18 | 2026-05-19 | Δ | One-line summary |
|---|---|---|---|---|---|
| 1 | Domain vocabulary | B+ | **A−** | ↑ | Six new cluster terms in CONTEXT.md (`DebounceState` … `AsyncJobs`); each with a one-line semantic anchor. |
| 2 | ADR-001 design integrity | A− | **A−** | — | Unchanged. ADR-001 still load-bearing; I12 tripwire is now a witness, not just a rule. |
| 3 | ADR-001 execution | C+ | **B** | ↑ | I12 tripwire locks the 9-fn `&mut App` baseline as a shrinking ratchet. Future regressions caught at CI. |
| 4 | `App` composition root | D+ | **C+** | ↑↑ | 1,556 → 1,494 LOC; 108 → 77 fields (−28%). Six clusters add navigability without lifting children. Still a god object — smaller and legible. |
| 5 | Key dispatch (`keys/`) | C | **C** | — | 1,409 LOC untouched. N6 deferred — needs help-screen autogen as value driver. |
| 6 | Render layer | C | **B−** | ↑ | All 9 I12 baseline fns are *already* read-only — flipping them to `&App` is a 30-min-each follow-up (C17 below). |
| 7 | Workspace (data) | C− | **C−** | — | ItemStore Accepted yesterday; no further deepening. |
| 8 | Persistence (`store/`) | C | **C** | — | Unchanged. |
| 9 | Ingestion | C | **C+** | ↑ | N1 contract test for `Source` + `with_retry` behavioural tests now cover the seam end-to-end. |
| 10 | Discovery sub-state | C | **C** | — | N4 (dispatch decoupling) still deferred — opportunistic. |
| 11 | Test coverage | C− | **B−** | ↑↑ | 235 → 338 tests (+44%). N1 + N2 + RenderCaches witness suite are the load-bearing additions. |
| 12 | Test surface health | B | **B+** | ↑ | Contract tests for `Source` (4) + `ItemStore` invariant sweep (7) + Effect→cache routing (11). Behavioural, not just structural. |
| 13 | Dead code / warnings | C | **A** | ↑↑↑ | 14 compiler + 131+ clippy → 0 + 0. Pre-flight pass + post-migration cleanup. |
| 14 | Doc currency | B+ | **A−** | ↑ | CLAUDE.md de-stale'd (30 lines about deleted `cli-text-reader` removed); ADR-009 Accepted with full receipts. |
| 15 | Tooling (fmt/CI) | A | **A** | — | Five tripwires now (was four); I12 added to render-purification. |

**Overall: B− (was C+, +1.0 letter).**

The composite lift is real but uneven. Three dimensions jumped two grades (4, 11, 13); five lifted one (1, 3, 6, 9, 12, 14); seven stayed put (2, 5, 7, 8, 10, 15). The unchanged-grade dimensions are the deferred-with-reason ones (N3/N4/N6/N7 from the morning audit) — discipline that hasn't been broken, but hasn't yet compounded either.

**Educational takeaway:** A composite jump of this size in one session is unusual. Two factors made it possible — (a) the audit's framing (both the prior day's doc and the morning's harsh re-grade) was load-bearing; (b) the per-cluster ADR+migration+test cadence had been rehearsed across the eight prior ADRs and ran without friction. Neither factor would have worked alone.

---

## Drift since 2026-05-18

| Item | Status |
|---|---|
| Warning surface (14 compiler + 131+ clippy) | **RESOLVED** — pre-flight pass; now 0 + 0. |
| CLAUDE.md stale ~30 lines about deleted `cli-text-reader` | **RESOLVED** — replaced with current crate map + pointer to sibling `tread` repo. |
| Source / EnrichmentSource seam had no contract test | **RESOLVED** — N1 ships TCP-stub-server behavioural tests for `with_retry` + method-shape tests for `Source`/`EnrichmentSource`. |
| ItemStore invariant only checked by 1 test (`rebuild_indices_is_idempotent`) | **RESOLVED** — N2 ships `check_invariants` helper + 7 sweep-based witnesses covering push/replace_at/clear/from_items sequences. |
| ADR-001 D4 (render purity) had no automated guard | **RESOLVED** — I12 tripwire locks today's 9-fn `&mut App` baseline as a shrinking ratchet. |
| `App` god object (108 fields) | **PARTIALLY RESOLVED** — 77 fields, six logical clusters. Still a god object by absolute count. |
| ADR-009 Future-clusters table (6 entries) | **CLOSED** — all six clusters shipped. ADR Accepted. |

---

## Deepening candidates

Per the skill: candidates, not interfaces. Four new candidates surfaced by the Explore agent's friction hunt; four prior deferrals carry forward.

### New

**C16. Extract protocol methods onto `AsyncJobs`.**
*Files:* `trench/src/app/state/async_jobs.rs`, `trench/src/main.rs` (6 spawn sites), `trench/src/app/methods/process.rs` (4 polling patterns).
*Problem:* The start→poll→resolve lifecycle for bulk/fulltext/tread/repo fetches is open-coded across dispatch sites. Setup repeats twice in `main.rs:343,357`; polling repeats four times (one per job class) at `main.rs:~560`; resolution routes through match arms in `process_incoming` without a keeper. The cluster is grouped (ADR-009 ✓), but the protocol is scattered — same shape as the `DebounceState` PR 1 pattern at 3× the scope.
*Solution:* `AsyncJobs::start_fetch(&mut self, rx, sources)`, `poll_fetch(&mut self) -> Option<...>`, `finish_fetch(&mut self, result)`. Three methods, six call sites collapsed, the protocol becomes inspectable.
*Benefits:* Locality (the protocol lives in one place). Leverage (methods are reusable from any `&mut app.async_jobs`). Test surface (a new `async_jobs::tests::lifecycle` module can drive the protocol without `main.rs`).

**C17. Flip the 9 I12 baseline render fns from `&mut App` to `&App`.**
*Files:* `trench/src/ui/layout/popups/help.rs:draw_help_overlay`, `trench/src/ui/layout/details.rs:draw_details_panel`, `trench/src/ui/layout/main_row.rs:draw_main` (sampled — all 3 read-only). Plus 6 more in the baseline.
*Problem:* The Explore agent sampled all 9 baseline fns and confirmed each performs zero mutations — they take `&mut App` by accident, not by necessity. The I12 tripwire correctly guards future regressions but the floor is artificially high. Type-enforced purity is one signature flip away.
*Solution:* Change signature to `&App` for each; sed-friendly; reduce the I12 baseline array as each name clears. Tighten the ratchet from 9 → 0 over 2–3 PRs.
*Benefits:* Compiler-enforced render purity (not just grep). Render panes can no longer accidentally mutate state. The pre_draw/render boundary becomes legible from the type signature alone.

**C18. Behavioural tests for the field-only clusters.**
*Files:* `trench/src/app/state/{reader_bottom,view_flags,leader}.rs` (smoke tests only) vs. `state/{render_caches,debounce}.rs` (behavioural).
*Problem:* RenderCaches (11 tests) and DebounceState (5) ship with behavioural witnesses. LeaderState (3), ReaderBottomState (2), ViewFlags (2) ship with smoke tests only. Future regressions in the smoke-test clusters land silently.
*Solution:* Add per-cluster behavioural suites: leader-timeout-expiry edge cases, reader-bottom drawer mode transitions, popup interaction scenarios.
*Benefits:* Regression surface for clusters whose protocol is currently hand-eyeballed. Executable specification. ~1 hour per cluster.
*Caveat:* Low-ROI until protocol extraction (C16) creates more protocol surface to test. Worth doing once C16 lands.

**C19. Consolidate inline `FeedItem` factories across test files.**
*Files:* `trench/src/models/fixtures.rs` (1 factory from C15), inline `FeedItem { … }` blocks across ~8 test files.
*Problem:* C15 promoted one factory from `bench.rs`. ~8 other test files still inline `FeedItem { url: …, title: …, … }` blocks. When `FeedItem` gains a field, all 8 drift simultaneously.
*Solution:* Grow `fixtures::variant`-style helpers (`from_arxiv`, `from_hf`, `with_abstract`) and replace the inline construction sites.
*Benefits:* Single mutation point for FeedItem schema changes. Discoverability for new test contributors.

### Carried (deferred per their forcing-function gates)

**C20 (= morning's N3). FrameLayout deepening.** Today's `FrameLayout` carries one field. Forcing function: a second layout-derived state needs the hook. Not yet present.

**C21 (= morning's N4). FeedModel/DiscoveryModel dispatch decoupling.** Tab dispatch still lives in `FeedModel::active_list(&self, discovery: &DiscoveryModel)`. Lift to orchestrator. Opportunistic — fold into next PR touching `feed::active_list`.

**C22 (= morning's N6). `keys/mod.rs` gesture registry.** 1,409-LOC dispatch wall. Real refactor; needs help-screen autogeneration as value driver. ~300 LOC of work.

**C23 (= morning's N7). Cross-crate cohesion decision.** `crates/notes` + `crates/chat` extracted but earning no reuse. Decide: inline as modules or define `PaneStorage` trait. Not blocked on capacity; blocked on a choice.

---

## Recommended path forward

1. **First — C17 (flip I12 to `&App`).** Lowest risk, highest signal. 30 min per fn × 9 = ~4.5 hours total, splittable across 2–3 micro-PRs. Each flip removes a name from the I12 baseline. When the baseline hits zero, the tripwire becomes a hard rule rather than a ratchet. **Strong candidate for next-session warm-up** because each PR is independent.

2. **Second — C16 (AsyncJobs protocol extraction).** The structural debt named by the AsyncJobs PR 6 commit. ~4 hours; ADR-010 is plausible if the scope feels architecture-shaped (the protocol surface affects how callers think about async). Bundling with C18-LeaderState tests would give the new methods immediate coverage.

3. **Third — C21 (dispatch decoupling) when next touching `feed::active_list`.** Opportunistic, not a dedicated PR.

4. **Fourth — C20, C22, C23 wait for forcing functions.** Each has a clear gate (second metric, help autogen, reuse pressure). Don't refactor without one — yesterday's audit's "wait until forced" lesson held for half of its predictions.

5. **The honest non-architecture work the codebase needs.** Integration / property / E2E tests. Today's 338 unit tests are strong but the cross-module pipeline (ingest → ItemStore → render → keystroke → reader open → notes anchor → persistence) has zero coverage. One property test pushing 1,000 random `FeedItem`s through would find bugs an architectural audit cannot. **Recommended: one property test for the ingestion → ItemStore deduplication path.** ~3 hours, would shift dimension 11 to B+.

---

## Closed candidates — receipts

| # | Cluster / Witness | PR cadence | Commits | Tripwires |
|---|---|---|---|---|
| N1 | Source contract behaviour (ADR-004) | single PR | `3f1c50f` | (covered by J-series + behavioural stub-server) |
| N2 | ItemStore invariant sweep (ADR-007) | single PR | `4c80485` | (covered by M-series + check_invariants helper) |
| N8 | I12 render-purity tripwire (ADR-001 D4) | single PR | `755b990` | I12 added to `check-render-purification.sh` |
| ADR-009 PR 1 | `DebounceState` | single PR | `c405f93` | (cluster grouping; tripwire deferred) |
| ADR-009 PR 2 | `LeaderState` | single PR | `f540c3e` | (cluster grouping; tripwire deferred) |
| ADR-009 PR 3 | `ReaderBottomState` | single PR | `4e5cfa2` | (cluster grouping; tripwire deferred) |
| ADR-009 PR 4 | `ViewFlags` (+ scope correction) | single PR | `c162374` | (cluster grouping; tripwire deferred) |
| ADR-009 PR 5 | `RenderCaches` (+ first behavioural tests) | single PR | `d914fd3` | (cluster grouping; tripwire deferred) |
| ADR-009 PR 6 | `AsyncJobs` (closes punch list) | single PR | `dc667a3` | ADR-009 Accepted |
| pre-flight | warnings 14 → 0, fmt clean, CLAUDE.md de-stale | single PR | `3c205ed` | n/a |
| fmt | two N1/N2 lines I missed | micro-PR | `a4f48d6` | n/a |

**Net effect at the type-system level:** Nine ADRs Accepted (001–009). Five tripwire scripts enforcing 25 invariants in `ci.sh` (I12 added today). App composition root shrank 28% by field count, 4% by line count, with six logical clusters making the residual surface legible. The morning audit's eight candidates (N1–N8) closed three (N1, N2, N8) and re-classified the rest as deferred with explicit forcing functions.

---

## Closing observation (educational)

The prior audit's closing observation said: *"discipline that doesn't compound is debt accumulating at a fixed rate."* That framing held — both audits today became forcing functions.

But a second, sharper observation lives in this session's deltas: **structure that enables rapid iteration is not debt; it's leverage.** ADR-009's "Future clusters" table — written in PR 1 with five rows mostly speculative — made each subsequent PR scope inspectable and prevented re-litigating the grouping decision. The per-candidate tripwire pattern (introduced by C14 in the 2026-05-18 session) generalised: today's I12 is the eighth tripwire-shaped invariant, written in 48 lines, slotted into the existing CI machinery with zero new infrastructure.

The codebase has built a *cadence*. PR 1 (ADR + skeleton + smoke tests) → PR 2+ (compiler-driven mechanical migration) → PR N (tripwire + ADR Accepted) — this shape executed eleven times today without friction. The question the *next* audit should ask is not "did the candidates ship?" (they did) but **"is the cadence producing the right work, or just producing work efficiently?"** Today's 338 unit tests are real. The zero integration tests are also real. A codebase that ships eleven structural PRs in a session while staying at zero integration tests is producing efficiently — but the *kind* of work the cadence privileges has a shape, and the work it doesn't privilege is the work that the next ceiling needs.

**What this session learned that should compound into the next audit:**

- The cadence works across scope. ADR-009 was 6 PRs deep before AsyncJobs landed; the pacing didn't slow. (PR 6 was actually the smoothest.)
- "Pub fields, no methods" is the right shape for clusters with no protocol; "methods + behavioural tests" is right for clusters with a protocol. Both are documented in ADR-009; future clusters can pick the right shape from the table.
- Sed-driven migrations have preconditions (no destructuring, no string literals, no fn-name collisions, no method-name suffix overlap). The C16 extraction will violate the last precondition (start_fetch shares prefix with start_*). Plan for it.
- `cargo fmt --check` after every commit is non-negotiable. Two missed lines today became a separate `chore(fmt)` commit — preventable by running fmt as part of the per-PR ritual.
- The morning's "harsh audit" worked because it was *predictive*, not historical. It named friction the doc-bank audit hadn't quantified. Reserving one conversational audit per multi-PR session keeps the doc-bank audit honest.

---

## Index

Audits live in `docs/audits/YYYY-MM-DD-*.md`. Skill: `/improve-codebase-architecture`. Prior: `2026-05-18-architectural-audit.md`. CONTEXT.md's "Related" section grew a pointer to ADR-009 today; the next audit's pointer entry will land alongside this doc.
