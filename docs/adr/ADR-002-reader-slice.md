# ADR-002 — Reader-pane slice (Slice 2 of render purification)

- **Status:** Accepted (2026-05-16). All six slice-2 PRs landed (PR 1 foundations, PR 2 state migration, PR 3 gesture methods, PR 4 `Action::OpenInReader`, PR 5 `pre_draw` for the layout-derived resize, PR 6 tripwire + this status flip). PR 5 was scoped down from "full render-signature flip" to "pre_draw landing" — see §S6 cadence note for the reasoning.
- **Date:** 2026-05-16
- **Owner:** Victory Chianumba
- **Supersedes:** none
- **Extends:** [ADR-001](ADR-001-render-purification.md). All decisions there hold for Slice 2 unless noted.

## Goal

Apply the per-pane composition-root pattern (ADR-001) to the reader pane and its tightly-coupled popup. Land a state surface where the 13 reader+popup fields scattered on `App` live behind one shape, renders take `&Model + &Context`, and tests can construct a `ReaderInstanceModel` without an `App`.

## Context

The 2026-05-16 architectural audit (`docs/audits/2026-05-16-architectural-audit.md`) graded App composition root as D+ and reader-pane state cohesion as none. Twenty fields scattered across `App` lines 94–160 cover three "reader modes" (embedded, popup, dual-split) with no shared structure, no invariant, and three separate notes-related field clusters.

ADR-001's Slice 2 forward design was specifically narrow: `ReaderPaneModel { primary, secondary }` + `VoiceModel`. The audit then offered three adjacent deepening candidates that could fold in: **C4** (`ReaderPopupModel`), **C5** (`NotesInstanceModel`), **C6** (`LayoutMetrics`).

## Decision

### Scope: reader + popup (Medium)

Slice 2 covers exactly two model surfaces:

- `ReaderPaneModel { primary: ReaderInstanceModel, secondary: Option<ReaderInstanceModel>, split_active, dual_active, focused }` — the embedded reader pane in its three layout states.
- `ReaderPopupModel { active, rx, editor, image_state, burst }` — the floating popup reader.

**Out of scope (deferred to future slices):**

- `NotesInstanceModel` (audit C5) — notes dock alongside reader and would consolidate the `notes_*` + `secondary_notes_*` field bifurcation. Deferred because Slice 2 doesn't change notes' relationship to the reader; the bifurcation predates this work.
- `LayoutMetrics` (audit C6) — per-frame geometry cache. Useful but invasive to wire across panes; better as its own cross-cutting slice.
- `VoiceModel` — voice survives closing a paper, so its lifecycle is independent of `ReaderInstanceModel`. Deferred until ElevenLabs credits + a feature trigger.
- Reader bottom drawer (`reader_bottom_*`, 7 fields) — its own visual surface; deferred.

### Trigger: proactive cleanup (deliberate departure from ADR-001 D2)

ADR-001 D2 ("Feed pane first; lazy rollout after") prescribed waiting for a feature to pull us into each pane. Slice 2 explicitly violates this: there is no concrete feature blocking; the trigger is *the audit's grade for state scatter*.

The departure is intentional, not accidental. The argument:

- The audit found 20+ reader fields on `App` with no invariant — the worst single state-scatter site in the codebase. The lazy-rollout rule is a coordination heuristic, not a moral law; when an audit identifies that *the lack of refactor is itself the problem*, lazy rollout backfires.
- Both image-rendering correctness (figure tiling, the image-load regression) and voice readiness eventually need this work. Doing it once now is cheaper than doing it twice when each forcing function lands.
- Slice 1 proved the pattern. Apply it.

**The constraint that comes with proactive scope**: "done" is defined by *invariants held*, not by a feature working. PR 6 is therefore stricter than Slice 1's: tripwire covers not just `&mut App` references but also the App-field-count invariant (no `reader_*` or `reader_popup_*` field at App top level).

### Decisions inherited unchanged from ADR-001

- **D1** composition root: each Model is a field on `App`; Models never reference each other.
- **D3** Action in / Effect out / `pre_draw(Viewport)` for layout-derived mutation.
- **D4** strict `&Model` renders post-flip; no `&mut App` in `reader.rs` or popup render paths.
- **D5** W3 hybrid rule for Workspace mutation (state-local via `&mut Workspace`, cross-pane via `Action`).
- **D7** tests at the Model boundary, inline `#[cfg(test)]`.

### Slice 2-specific decisions

#### S1. Two models, not one

`ReaderPaneModel` and `ReaderPopupModel` are siblings on `App`, not parent/child. The popup is *not* a third `ReaderInstanceModel` because its lifecycle and state differ enough (channel-based async load, no tab bar, dismissible-on-Esc) that nesting them would create a "ReaderInstanceModel that pretends to be a popup" anti-pattern.

Both models *use* `tread::Reader` as their underlying editor. That's the common substrate; the orchestration around it differs.

#### S2. `ReaderInstanceModel` owns one editor + its tabs

Shape:

```
ReaderInstanceModel {
  tabs: Vec<ReaderTab>,        // each ReaderTab carries its own tread::Reader
  active_tab: usize,
}
```

A single editor in *one tab* is the unit of reader work. Primary and secondary readers each have their own `ReaderInstanceModel`; both can have multiple tabs open.

`reader_active: bool` (currently on App) becomes implicit: the pane is "active" iff `ReaderPaneModel::primary.tabs.is_empty()` is false AND the orchestrator's view state says reader is the focused surface. This collapses one boolean onto the data it was already mirroring.

#### S3. `ReaderPopupModel` is the C4 candidate landed

Shape:

```
ReaderPopupModel {
  active: bool,
  rx: Option<Receiver<Result<PaperData, String>>>,
  editor: Option<tread::Reader>,
  image_state: tread::ImageState,
  burst: tread::BurstTracker,
}
```

The five popup fields scattered on `App` lines 116–124 collapse into this struct. Invariant: `active` is true iff `editor.is_some()` OR `rx.is_some()`. Tests can assert this.

#### S4. `Action::OpenInReader { item, target: ReaderTarget }`

The cross-pane Action variant that was forward-designed in Slice 1 PR 5 (deferred to Slice 2) lands as Slice 2 PR 4. Variants on `ReaderTarget`:

```
enum ReaderTarget {
  Primary,            // open in primary reader
  Secondary,          // open in secondary reader (when dual/split active)
  Popup,              // floating popup
}
```

The orchestrator's match arm replaces the existing inline `app.reader_active = true` mutations across `app/methods/reader.rs`.

#### S5. Tests-without-tread strategy

`tread::Reader` constructors require parsed paper content (Pandoc output, arXiv source, etc.). Constructing one in a unit test would be expensive and brittle.

**Solution:** `ReaderInstanceModel::default()` produces an empty `tabs: Vec::new()`. Tests exercise tab navigation gestures (add/remove/cycle/focus) on synthetic `ReaderTab` values that wrap *whatever shape passes the type system*. If `tread::Reader` exposes a cheap `Reader::empty()` or `Default`, use that; otherwise, the slice introduces a thin `ReaderTab::new_for_test()` test helper rather than mocking the whole `tread::Reader` surface.

This means **some reader gestures will not be testable at the Model boundary** because they're inherently about the editor's behavior, not the pane's. That's fine — those tests live in `tread` already.

#### S6. 6-PR cadence (same shape as Slice 1)

| # | PR | Behaviour change |
|---|---|---|
| 1 | Foundations: `ADR-002`, `CONTEXT.md` reader vocabulary, empty `ReaderPaneModel` + `ReaderInstanceModel` + `ReaderPopupModel` + `ReaderContext`, smoke tests. | None |
| 2 | State migration: move 13 reader+popup fields from `App` into `App.reader: ReaderPaneModel` and `App.reader_popup: ReaderPopupModel`. Mechanical. | None |
| 3 | Gesture methods: tab cycling, focus toggle, dual/split state machine, popup open/close — all become Model methods. | None |
| 4 | Cross-pane `Action::OpenInReader { target: ReaderTarget }` actually wired (was Slice 1 PR 5). | None visible |
| 5 | `pre_draw` landing: `ReaderInstanceModel::pre_draw` + `ReaderPopupModel::pre_draw` own the once-per-frame `tread::Reader::resize` call. The inline `last_resize != Some(new_size)` blocks come out of 5 render sites. **Scoped down from the original "full render-signature flip" — see cadence note below.** | None visible |
| 6 | Lock the door: extended `scripts/check-render-purification.sh` covers (I4) 13 migrated fields gone from App top level, (I5) `App.reader` + `App.reader_popup` present, (I6) ADR-002 cadence table complete, (I7) no inline `last_resize` checks in reader render paths. ADR-002 → Accepted. | None |

### Cadence note (2026-05-16, post-mortem)

PR 5 originally specified the full `&Model + &Context` render-signature flip (mirror of ADR-001 PR 4c).  In practice the flip was deliberately scoped down to "pre_draw landing only" for two reasons:

1.  **The audit's honest assessment.**  Slice 1 PR 4c introduced per-frame `Vec` allocations (visible_indices, filtered_history, ItemCounts clone) as the price of strict `&Model` render purity.  Reader renders are hotter than feed renders (they call `tread::draw` + `tread::after_draw_guarded` against a heavy editor state), and a strict flip would multiply that cost.
2.  **The user explicitly flagged perf as the next priority** after slice 2.  Doing the full flip would have introduced exactly the kind of regression the perf work is meant to fix.

The pre_draw landing captures the real architectural win — layout-derived mutation lives in one named place, no longer scattered across 5 render sites — without the per-frame regression.  A full signature flip remains *available* as a follow-up if a testability driver forces it (per ADR-002 §S5, model-boundary tests would require `tread::Reader` construction which is currently deferred).

Feature freeze for the slice. Bug fixes and trivial UI tweaks are fine; no new reader surfaces.

## Consequences

### Positive

- 13 reader+popup fields collapse from `App` top level into 2 Models with invariants.
- Reader rendering becomes testable at the Model boundary (with the S5 caveat).
- Future image-pipeline work + voice integration land on a clean state surface.
- Slice 1's pattern proves out a second time, making Slice 3+ (notes, voice, layout metrics) easier to scope.

### Negative

- Departure from ADR-001 D2 (lazy rollout) requires explicit justification — captured here.
- ~2 weeks of evening work in feature freeze.
- `tread::Reader` test mocking is awkward; some gesture tests will be thin or absent.
- Bottom-drawer fields (7 of them) and notes bifurcation (11 fields) remain on `App` — Slice 2 doesn't touch them.

### Trade-offs explicitly accepted

- **Two models for reader + popup, not one bigger model.** Bundling popup into `ReaderPaneModel` was considered and rejected (S1). The cost is two `pre_draw` calls per frame; the benefit is each Model has a tight invariant.
- **No `VoiceModel` in this slice.** Voice survives closing a paper; coupling it to `ReaderPaneModel` would mean voice state goes with the model, which is wrong. Defer.
- **`reader_active` boolean collapses into data.** A small backwards-compat hit during PR 2 (call sites that read `app.reader_active` get a method that derives it).

## Risks

1. **`tread::Reader` mocking.** Mitigation: S5 — accept that some tests don't exist.
2. **State migration churn.** 13 fields × ~30 call sites. Mitigation: compiler-driven sweep (worked for Slice 1 PR 2).
3. **Popup async load lifecycle.** The popup's `rx` channel + `editor` Option dance has subtle timing. Mitigation: PR 2 preserves the exact ordering; PR 5's `pre_draw` will encode the state machine.
4. **PR 4 widens the Action vocabulary.** ReaderTarget discriminator forward-designs three variants when only one (Primary) is initially constructed. Same trade-off as Slice 1 PR 5 — accepted.
5. **Proactive trigger means scope creep.** Mitigation: PR 6's tripwire enforces the boundary. If a field doesn't fit, it stays on App for a later slice rather than getting added to the wrong Model.

## Related

- [ADR-001](ADR-001-render-purification.md) — the parent decision.
- `docs/audits/2026-05-16-architectural-audit.md` — candidates C4 (folded in), C5 / C6 / C7 (deferred).
- `docs/CONTEXT.md` — domain vocabulary; updated in PR 1 with reader-specific terms.
