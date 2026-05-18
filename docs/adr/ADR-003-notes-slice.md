# ADR-003 — Notes-pane slice (Slice 3 of render purification)

- **Status:** Accepted (2026-05-18). All four PRs have landed: PR 1 (skeletons + this ADR + vocabulary), PR 2 (state migration, 11 fields → `App.notes`), PR 3 (gesture methods on `NotesPaneModel`), PR 4 (tripwires I8/I9/I10/I11 in `scripts/check-render-purification.sh`).
- **Date:** 2026-05-18
- **Owner:** Victory Chianumba
- **Supersedes:** none
- **Extends:** [ADR-001](ADR-001-render-purification.md) and [ADR-002](ADR-002-reader-slice.md). All decisions in those hold for Slice 3 unless noted.

## Goal

Apply the per-pane composition-root pattern (ADR-001) to the notes dock. Collapse 11 bifurcated `notes_*` / `secondary_notes_*` fields scattered on `App` lines 95-105 into a single `App.notes: NotesPaneModel` with verifiable invariants.

## Context

The 2026-05-18 architectural audit (`docs/audits/2026-05-18-architectural-audit.md`) re-graded App composition as D+ for the second consecutive audit. ADR-002 explicitly deferred candidate C5 (`NotesInstanceModel`) to a future slice, on the basis that Slice 2 doesn't change notes' relationship to the reader. The bifurcation has been load-bearing in the codebase for several weeks: 11 fields, ~30 read-and-write call sites, an accessor (`notes_mode_for_side`) that exists *specifically to dispatch on a side dimension the data doesn't yet carry*.

The audit's vocabulary: an accessor that dispatches on a discriminator is a pre-existing seam waiting for the right shape behind it. `notes_mode_for_side(side: FocusedReader)` is exactly that — the seam was discovered before the data structure caught up. Slice 3 completes the move.

## Decision

### Scope: notes only (Small)

Slice 3 covers two model surfaces, no more:

- `NotesInstanceModel { tabs, active_tab, mode, context }` — one notes context (primary or secondary).
- `NotesPaneModel { app, primary, primary_visible, secondary: Option, secondary_visible }` — the composition root.

**Explicitly out of scope** (deferred from the audit, not in this slice):

- `LayoutMetrics` (C6) — cross-cutting per-frame geometry cache. Independent slice.
- Reader bottom drawer (audit's `reader_bottom_*` cluster, 7 fields) — its own bifurcation pattern, separate slice.
- `VoiceModel` — lifecycle-independent of any pane; needs ElevenLabs/voice trigger.
- The two `app.notes_mode` direct-read sites in `main_row.rs:338,406` that ignore the side dimension. The type system will surface them in PR 2 as compile errors; fixing them lands in PR 2 as part of the migration sweep.

### Trigger: proactive cleanup (same justification shape as ADR-002)

ADR-001 D2 prescribed lazy rollout — refactor when a feature pulls. Slice 2 already departed from this; Slice 3 makes the same departure for the same reason: *the audit identified the lack of refactor as the problem itself*.

**The justification is thin.** No concrete feature requires notes consolidation. No user-reported friction triggered it. The forcing function is the audit-grade-alone signal, which is the weakest justification on the table. Honest framing recorded here so future-readers don't infer a stronger case than exists.

The compensating discipline: **scope is tight, PR cadence is short, and PR 4 is a hard tripwire**. If invariants aren't verifiable, the slice doesn't close. No mission creep.

### Decisions inherited unchanged from ADR-001 / ADR-002

- **D1** composition root: `App.notes: NotesPaneModel`; Models never reference each other.
- **D3** Action in / Effect out / `pre_draw(Viewport)` if layout-derived mutation appears.
- **D4** renders take `&Model + &Context`, not `&mut App` (or the existing pre_draw discipline holds).
- **D5** W3 hybrid rule for Workspace mutation.
- **D7** tests at the Model boundary, inline `#[cfg(test)]`.
- **ADR-002 §S5 tests-without-backend** — `NotesInstanceModel::default()` produces an empty model. Tests on tab navigation / mode cycling run against synthetic `NotesTab` values without instantiating the heavy `notes::app::App` persistence backend.

### Slice 3-specific decisions

#### S1. Two structs, asymmetric secondary

`NotesPaneModel` owns the pane; `NotesInstanceModel` owns one notes context (primary or secondary). They're parent/child, not siblings — only the pane is on `App`.

Shape:

```rust
pub struct NotesPaneModel {
    /// Persistence backend, shared across instances. Lazy-loaded.
    pub app: Option<notes::app::App>,

    pub primary: NotesInstanceModel,
    /// "Primary notes pane is rendered on screen right now."
    /// Independent of tab population — toggling off preserves tabs.
    pub primary_visible: bool,

    /// None = never opened (no Vec<NotesTab> allocated).
    /// Some(_) = ever opened; visibility controlled separately.
    pub secondary: Option<NotesInstanceModel>,
    /// Only meaningful when `secondary.is_some()`.
    pub secondary_visible: bool,
}

pub struct NotesInstanceModel {
    pub tabs: Vec<NotesTab>,
    pub active_tab: usize,
    pub mode: NotesMode,
    pub context: Option<NotesContext>,
}
```

#### S2. Why secondary is `Option`, not always-present (departure from ADR-002 §S2)

ADR-002 chose always-present-secondary for reader. Slice 3 chose `Option` for secondary. The two divergences:

1. **Reader's secondary is first-class.** Dual / split reader is a deliberate layout state with its own enum (`split_active`/`dual_active`). The audit said reader's secondary is "an instance the layout already iterates over uniformly via `FocusedReader`."

2. **Notes' secondary is a tacked-on extra.** The bifurcation pattern in current code reads like a copy-paste pass: every `notes_*` field has a `secondary_notes_*` twin, but there's no first-class concept of "notes split mode." Secondary notes appear *because* reader is dual-split, not as their own feature. Two adapters for the same seam (LANGUAGE.md): reader uses both side-by-side; notes uses one-and-a-bit.

Going `Option` matches the asymmetric reality. The cost: layout code that iterates "both notes sides" must check `secondary.is_some()`. The benefit: memory savings when secondary has never been opened, and the type system enforces "secondary_visible is meaningful only when secondary exists."

#### S3. Why visibility lives at the pane root, not inside each instance

Visibility is a layout-level concern: "should the renderer allocate a rect for this pane?" Tied to today's `app.notes_active` bool semantics, which the call-site evidence (10/10 sites in `ui/layout/`) confirms is unambiguously visibility.

`NotesInstanceModel` stays pure-content: tabs + active_tab + mode + context. Tests of NotesInstanceModel are about content behavior (tab cycling, mode switching) without layout concerns leaking in.

#### S4. NotesMode is per-instance

Existing code already has the `notes_mode_for_side(side: FocusedReader) -> NotesMode` accessor — a pre-existing seam that this slice collapses onto the data it was already abstracting. Mode moves into `NotesInstanceModel`. The accessor becomes `app.notes.primary.mode` or `app.notes.secondary.as_ref().map(|s| s.mode)`.

#### S5. 4-PR cadence (shorter than Slice 1 / Slice 2's 6)

| # | PR | Behaviour change |
|---|---|---|
| 1 | ADR-003 + empty `NotesPaneModel`/`NotesInstanceModel` skeletons + CONTEXT.md vocabulary + smoke tests | None |
| 2 | State migration — 11 fields move from `App` into `App.notes`. Compiler-driven sweep across ~30 call sites. The two `app.notes_mode` direct-read bugs in `main_row.rs` surface as compile errors and get fixed in this PR. Accessor `notes_mode_for_side` collapses onto the data. | None |
| 3 | Gesture methods on `NotesPaneModel` — mode cycle, tab open/close, secondary spawn/hide — pull logic out of `keys/` flat matches into named model methods. | None |
| 4 | Lock the door — extend `scripts/check-render-purification.sh` with I8/I9/I10/I11 tripwires; ADR-003 status → Accepted. | None |

No separate PR for cross-pane `Action::OpenNotes` — not on the table yet, no caller needs it, defer until forced. No `pre_draw` PR — notes has no layout-derived mutation today (no resize state machine equivalent to `tread::Reader::resize`), so nothing to hoist.

This is shorter than ADR-001/002 deliberately. Cutting PRs that don't apply isn't laziness; PR 4 / PR 5 of those slices addressed reader-specific problems (cross-pane Action::OpenInReader, pre_draw for tread::Reader::resize). Notes has neither.

### Invariants for PR 4 tripwire

The `scripts/check-render-purification.sh` script gets four new invariants enforced by grep at CI time:

- **I8** No `notes_*` or `secondary_notes_*` field at App top level. After PR 2 the 11 fields are gone; PR 4 catches regressions.
- **I9** `App.notes: NotesPaneModel` present.
- **I10** `NotesInstanceModel` has no visibility field. Visibility is a pane concern; if a future PR adds `visible: bool` to the instance, it fails the check.
- **I11** Every render path reads notes state through `app.notes.*`. No `app.notes_tabs` direct access escapes.

## Consequences

### Positive

- 11 App top-level fields collapse to 1 (`App.notes: NotesPaneModel`).
- `NotesInstanceModel` is testable at the Model boundary without instantiating the `notes::app::App` persistence backend.
- The `notes_mode_for_side` accessor (pre-existing seam) collapses onto data — one fewer indirection in render paths.
- The two `app.notes_mode` direct-read bugs in `main_row.rs` surface as compile errors and get fixed during migration.
- Slice 1/2's pattern proves out a third time, making future slices (bottom drawer, voice, layout-metrics) easier to estimate.

### Negative

- Departure from ADR-001 D2 (lazy rollout) requires explicit justification — captured here as "audit-grade-alone," which is the weakest justification on the table. Honest, but thin.
- ~1 evening of mechanical migration (30 call sites + 11 fields).
- The asymmetric secondary (`Option<NotesInstanceModel>` + separate visibility flag) is more complex than reader's always-present-secondary; layout code that iterates "both sides" has more branching.

### Trade-offs explicitly accepted

- **`Option` for secondary, against reader's pattern**. Reader chose always-present; notes diverges. Reason: notes' secondary is asymmetric in real usage (rarely opened). One adapter and a half, not two adapters.
- **Visibility at pane root, not inside instance**. Layout is a pane concern; instances stay pure content. Tests of instances don't touch visibility.
- **NotesMode is per-instance, not pane-level**. Today's behavior preserved: primary can be in Library while secondary is in PaperNotes.
- **Slice 3 is proactive cleanup with thin justification**. Audit-grade-alone. Compensating discipline: tight scope, short cadence, hard tripwire.

## Risks

1. **`notes::app::App` mocking.** Some gesture tests may need the persistence backend. Mitigation: the audit-002 §S5 strategy — `NotesInstanceModel::default()` returns an empty model; tests that need the backend are thin or absent.
2. **State migration churn.** 11 fields × ~30 call sites. Mitigation: compiler-driven sweep (worked for Slice 1 PR 2 and Slice 2 PR 2).
3. **Asymmetric Option<secondary>.** Layout iterates "both sides" via `FocusedReader`; when secondary is None, render must skip cleanly. Mitigation: PR 2 includes a `NotesPaneModel::side(focused: FocusedReader) -> Option<&NotesInstanceModel>` accessor that returns None when secondary is unset.
4. **`app.notes_mode` direct-read bugs.** Two latent bugs surface as compile errors in PR 2. Mitigation: fix them in the same PR; they're cheap (2-line edits).

## Related

- [ADR-001](ADR-001-render-purification.md) — the parent decision.
- [ADR-002](ADR-002-reader-slice.md) — Slice 2 reader-pane extension; this slice mirrors its shape with two named divergences.
- `docs/audits/2026-05-18-architectural-audit.md` — candidate C5; the audit doc that motivated this slice.
- `docs/CONTEXT.md` — domain vocabulary; updated in PR 1 with `NotesPaneModel` + `NotesInstanceModel`.
