# ADR-016 — Notes backend seam: the dock owns selection

- **Status:** Proposed (2026-06-11). PR 1 (this ADR + `selected` field on `NotesInstanceModel` + boundary tests) landed. PR 2 landed as a **render-only safe half** (see §S5 and the Discovery note below). PR 3 (nav inversion + editor handoff + force-`List`/dead-`Preview` removal) landed and **intentionally fixes the two latent bugs the entanglement caused** (see the Discovery note). PR 4 (tripwire + Accepted) follows.
- **Date:** 2026-06-11
- **Owner:** Victory Chianumba
- **Supersedes:** none
- **Extends:** [ADR-003](ADR-003-notes-slice.md). ADR-003 consolidated the 11 scattered `notes_*` fields into `App.notes: NotesPaneModel` but deliberately left the embedded `notes::app::App` persistence backend untouched. This ADR addresses the seam ADR-003 left open: who owns *selection*.

## Goal

Make the dock the single owner of "which note is selected" in the notes pane. Collapse the dual selection state — backend `current_note_id` + `entries_list` vs. the dock's recomputed visible set — onto one owner (`NotesInstanceModel`), and stop the dock from poking the backend's `NotesState`. Behavior-preserving throughout.

## Context

Selection currently lives in two places that don't trust each other.

- The **backend** (`crates/notes/src/app/mod.rs`) owns `current_note_id`, `entries_list` (ratatui list index + sort + multi-select), and `notes_state` (`List` / `Preview` / `PreviewScroll` / `Editor`). It has its own navigation methods (`select_next_entry`, `sync_current_note_id`).
- The **dock** (`one-research/src/keys/mod.rs`) does not use any of that as truth. It recomputes the visible set itself in `visible_note_ids` (`keys/mod.rs:308`), applying the **mode + paper-link filtering the backend has no concept of**, then writes the result back into the backend's `current_note_id` and **force-pins** `notes_state = List` on nearly every keystroke (`keys/mod.rs:1313`, `:338`, `:422`, `:479`, `:495`).

Five functions exist only to keep the two halves synchronised: `visible_note_ids`, `ensure_notes_browser_selection`, `select_notes_browser_index`, `move_notes_browser_selection`, `sync_notes_tab_selection_to_current_note`. Because the dock renders its **own** preview pane (`one-research/src/ui/layout/notes.rs:320`), the backend's `Preview` / `PreviewScroll` states and its nav methods are dead weight in the embedded path — two preview implementations, one never reached.

This was surfaced by the 2026-06-11 notes deep-dive, not by a feature pulling on it. The forcing function is the dual-ownership smell itself: every new notes feature has to fight two selection state machines. Honest framing — same shape as ADR-003's "audit-grade-alone" justification, and just as thin. The compensating discipline is identical: tight scope, short PR cadence, hard tripwire.

### Discovery during PR 2 — the seam is more entangled than first read

Tracing the seam to write PR 2 revealed that `current_note_id` is **not** cleanly "the browser selection." It is reset to the *active tab's* note at the top of every notes keystroke (`handle_notes_pane` → `sync_notes_app_to_side`, `keys/mod.rs:1307`), then overridden by specific handlers (`j`/`k`/`g`/`G`, mode switch) and by tab navigation (`sync_notes_backend_to_active` → `focus_note`). Browser-selection and the **editor target** (what `Enter` edits) are conflated through that one field, and the per-keystroke reset interacts with Library mode (where the visible list contains notes that are not open tabs) in a way that could not be verified behaviour-identical without running the app.

Consequence: the nav inversion ("delete the five helpers, nav writes the model") cannot be made *provably* behaviour-preserving in isolation, because deciding what the per-keystroke reset should do is bound up with the editor-target decision — PR 3's explicit-handoff work. PR 2 was therefore split: the **render half** (provably safe) landed; the **nav inversion** moved into PR 3, where `sync_notes_app_to_side` and the editor target can be untangled in one coherent pass.

**PR 3 outcome — two latent bugs fixed, not preserved.** Removing the per-keystroke reset (`sync_notes_app_to_side` no longer writes `current_note_id`) and routing the editor through an explicit handoff (`seed_backend_from_selection`) changed two Library-mode behaviours that the entanglement had silently broken:

1. **Selection now persists across keystrokes.** Before, the reset re-pinned the cursor to the active tab each key, so consecutive `j`/`k` could fail to advance for a non-tab note. Now `j`,`j` moves two rows.
2. **`Enter` edits the selected note, not the active tab.** Before, the reset meant `Enter` on a non-tab Library note opened the editor on whatever the active tab was. Now the handoff seeds `current_note_id` from `selected`, so the editor opens on the highlighted note.

Both are pinned by dispatch-level characterization tests (`keys::notes_selection_tests`) that drive `dispatch` end-to-end against an in-memory backend. This is why PR 3's behaviour-change cell reads "intended-equivalent" rather than "none": equivalent for the paths that already worked, corrected for the two that didn't. (Interactive run-testing was the intended manual check; it is not possible in the headless CI/sandbox — the dispatch-level tests stand in for it.)

## Decision

### Scope: selection + `NotesState` ownership only (Small)

In scope:

- Move selection identity onto `NotesInstanceModel`.
- The dock reads/writes selection on the model; render reads selection from the model.
- The dock stops writing `current_note_id` and stops poking `notes_state`.
- The backend's `Preview` path is fenced off from the dock (kept for standalone coherence, never routed into from the dock).

**Explicitly out of scope:**

- The buried backend popups (filter `f`, sort `o`, fuzzy-find `/`, export). Surfacing or cutting them is the separate "dock self-consistency" thread. This slice leaves their fall-through (`keys/mod.rs:1385`) exactly as it is.
- The latent page-jump quirk in `move_notes_browser_selection` (PageUp/PageDown pass `page_size = 8` but the `delta > 1` branch is never taken, so page keys move by one). PR 2 carries the quirk forward verbatim to stay behavior-preserving; fixing it is a deliberate follow-up, flagged here so it isn't lost.
- `LayoutMetrics`, voice, reader bottom drawer — unrelated.

### Decisions inherited unchanged from ADR-001 / ADR-002 / ADR-003

- **D1** composition root: `App.notes: NotesPaneModel`; Models never reference each other.
- **D4** renders take `&Model + &Context`, not `&mut App`.
- **D7 / ADR-002 §S5** tests at the Model boundary, inline `#[cfg(test)]`, without instantiating the `notes::app::App` backend.

### Slice-specific decisions

#### S1. Selection identity is a `note_id` on `NotesInstanceModel`, not an index

`NotesInstanceModel` gains `selected: Option<String>` — the `note_id` of the browser selection. A `note_id` (not a list index, not a borrow of the backend) because it must survive re-sorting, filtering, and deletion of *other* notes without dangling. Selection is per-instance: primary and secondary can point at different notes, exactly like `mode` and `context` (ADR-003 §S4).

The visible set stays computed by the orchestrator (`visible_note_ids`) — it depends on `mode` + `context` + the live backend index, which is genuinely orchestrator territory. The model takes the visible set as an argument and reconciles against it; it never reaches into the backend. This keeps `NotesInstanceModel` pure and testable without the persistence backend.

#### S2. The backend retreats to persistence + editor + popups

The backend keeps `current_note_id` / `entries_list` / its nav methods for its own standalone coherence, but the dock stops using them as truth. The change is **additive to the backend, subtractive in the dock**: nothing in `crates/notes` is removed, so the standalone path (if ever revived) is unaffected; the dock simply stops borrowing the backend's selection.

#### S3. The dead `Preview` path is fenced off, not deleted

`Preview` / `PreviewScroll` stay in the backend enum (standalone uses them). The dock provably never routes into them: it renders its own preview pane, and after this slice it no longer writes `notes_state` at all. PR 4's tripwire enforces that no `notes_state = ` assignment escapes `keys/`.

#### S4. Editor entry is an explicit handoff

Today the editor is entered by falling through to the backend's `go_to_editor`, which edits whatever `current_note_id` happens to be, after the dock has forced it. PR 3 replaces this with an explicit "open the editor for this `note_id`" call seeded from the model's `selected`. The dock owns *what* to edit; the backend owns *how* to edit it. On editor exit the dock reconciles titles/tabs (it already does this for freshly created notes via `last_created_note_id`).

#### S5. 4-PR cadence, every PR behavior-preserving

| # | PR | Behaviour change |
|---|---|---|
| 1 | This ADR + `selected: Option<String>` on `NotesInstanceModel` + pure `select` / `selected_note_id` / `reconcile_selection` + boundary tests. Not wired — the dock still reads the backend. | none |
| 2 | **Render-only safe half** (rescoped — see Discovery). Render reads `app.notes.<side>.selected`; render no longer reads `current_note_id`. A single mirror in `keys::dispatch` (`reconcile_notes_selection_from_backend`) keeps `selected` equal to the backend's `current_note_id` after every key dispatch. `move_selection` lands on the model (quirk preserved), staged for PR 3. The nav helpers and the backend's operational ownership are **untouched** — this is provably behaviour-identical. | none |
| 3 | The nav inversion deferred from PR 2: nav writes the model; delete the five sync helpers and the `current_note_id` mirror; untangle `sync_notes_app_to_side`'s per-keystroke reset; explicit editor handoff (S4); remove force-`List` pokes and dead-`Preview` routing. **Behaviour-risky** — needs run-testing, not a pure guarantee. | intended-equivalent |
| 4 | Tripwires I12–I14 in `scripts/check-render-purification.sh`; ADR → Accepted. | none |

The render half (PR 2) and the nav inversion (PR 3) were one PR in the original plan. The split is the honest consequence of the Discovery above: the render half is provably safe and shipped first; the nav inversion is behaviour-risky and gets its own PR with run-testing.

### Invariants for the PR 4 tripwire

- **I12** No `set_current_note(` call from `one-research/src/keys/`. The dock no longer writes backend selection.
- **I13** No `notes_state = ` assignment from `one-research/src/keys/`. The dock no longer drives backend state.
- **I14** `NotesInstanceModel` owns `selected`; render reads selection through `app.notes.<side>.selected`, not through `notes_app.current_note_id`.

## Consequences

### Positive

- One owner of selection. New notes features stop fighting two state machines.
- `NotesInstanceModel` selection is testable at the Model boundary without the backend.
- Five sync helpers and the `current_note_id` round-trips collapse to a couple of pure model methods.
- The dead-in-dock `Preview` path is provably unreachable from the dock and locked by a tripwire.

### Negative

- Proactive cleanup with thin justification (surfaced by a deep-dive, not a feature). Honest, but weak — same class as ADR-003.
- The backend keeps selection state the dock no longer uses; mild redundancy is the price of not destabilising the standalone path.

### Trade-offs explicitly accepted

- **Selection by `note_id`, not index** — slightly more lookup work each reconcile, in exchange for surviving re-sort/filter/delete.
- **Backend selection kept, not removed** — additive/subtractive split chosen over a deeper backend rewrite to keep risk low.
- **Page-jump quirk preserved** — behavior-preservation beats an opportunistic fix mid-refactor. Fix lands deliberately, later.

## Risks

1. **Editor handoff reconciliation.** The editor mutates a note's title/content; the dock must refresh the tab label and selection on exit. Mitigation: reuse the existing `last_created_note_id` reconcile path; PR 3 extends it to the edit case.
2. **Behavior drift during the sweep.** Mitigation: each PR is behavior-preserving and PR 2 carries the page-jump quirk verbatim; the tripwire locks the end state.

## Related

- [ADR-003](ADR-003-notes-slice.md) — consolidated the host fields; left this seam open.
- [ADR-001](ADR-001-render-purification.md) / [ADR-002](ADR-002-reader-slice.md) — the parent render-purification pattern.
- `one-research/src/keys/mod.rs:308–524` — the five sync helpers this slice collapses.
- `docs/CONTEXT.md` — `NotesInstanceModel` vocabulary; updated in PR 1 with selection ownership.
