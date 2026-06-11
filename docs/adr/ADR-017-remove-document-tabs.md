# ADR-017 — Remove document tabs from reader and notes (for now)

- **Status:** Accepted (2026-06-11). Notes landed first (this commit); reader follows in a sibling commit.
- **Date:** 2026-06-11
- **Owner:** Victory Chianumba
- **Supersedes:** none
- **Amends:** [ADR-002](ADR-002-reader-slice.md) and [ADR-003](ADR-003-notes-slice.md) — both described their instance model as "owning tabs + active-tab index." That ownership is removed here; the rest of those ADRs stands.

## Goal

Remove the multi-document **tab** feature from the reader and notes panes: a pane no longer holds a `Vec` of open documents with a tab bar and new/cycle/close gestures. Each pane shows one document at a time.

## Context

Tabs were added early (the historical checklist's "Reader pane tabs" / "Tabbed notes" items) before either pane was fully formed. In practice they carry weight the panes don't yet earn:

- They complicate every instance model (`tabs: Vec<T>` + `active_tab` threaded through render, navigation, persistence, and the dual→single collapse).
- For notes specifically, the ADR-016 inversion made `selected` the single source of truth for "which note" — so the tab `Vec` had become largely redundant with selection.
- The feature is half-formed: tab bars only appeared past two tabs, persistence/back-compat was an open concern, and the gestures competed with the mode/selection keys.

The decision is to remove tabs **for now** and re-introduce them deliberately when the reader and notes are mature enough to make multi-document a designed feature rather than scaffolding. Git history holds the prior implementation; re-adding is a fresh design pass, not a revert.

### Two axes — only one is "tabs"

The word "tab" was overloaded. Two orthogonal axes existed:

- **Tabs** (removed): multiple documents inside *one* pane — `Vec<T>` + `active_tab`, tab bar, cycle/close/new.
- **Dual / split / secondary** (kept): two panes *side by side* — `split_active` / `dual_active` / `focused`, primary + secondary instances, `FocusedReader`, `ReaderTarget::{Primary,Secondary}`, pane visibility, `Ldr+1/2/3` pane focus. Untouched.

The two only touched because each pane *owned a `Vec` of documents*. Removal collapses that `Vec` to a single document and leaves the side-by-side machinery intact.

## Decision

### Collapse, don't disable

Per the "remove, don't disable" philosophy: delete the tab data, methods, bars, and keybindings rather than hiding a dormant `Vec`. With tabs gone, **opening a paper/note replaces the one in that pane** (single slot).

### Scope and sequencing

Two commits, notes first (smaller; ADR-016 had just stabilised it), reader second.

| # | Pane | Change |
|---|---|---|
| 1 | Notes | Remove `NotesTab`; `NotesInstanceModel` loses `tabs`/`active_tab` (and `focus_tab_for_selection`) — it's now `{ mode, context, selected }`. Remove `next_tab`/`prev_tab`/`close_active_tab`/`add_or_focus_tab`/`prune_tabs` and `CloseTabOutcome`; `collapse_secondary_into_primary` moves the whole instance. Remove the tab bar, the `Ldr+[ / ]` notes cycle, and repurpose `Ldr+w` to hide the pane. Drop notes-tab persistence from `UiState`. |
| 2 | Reader | Collapse `ReaderInstanceModel.tabs: Vec<ReaderTab>` + `active_tab` to a single document; fold `OpenMode::{NewTab,ReplaceActive}` into "set the pane's document"; drop `fulltext_new_tab` and the `Ldr+t` new-tab path; remove the reader tab bar and the `Ldr+[ / ]` reader cycle; rewrite the `q` / `Ldr+w` close paths to keep their dual→single collapse role without a tab `Vec`. |

### Notes-specific: tabs folded into selection

Notes doesn't even need a single-doc slot. After ADR-016, `selected` *is* "the current note": the editor opens on it, the preview shows it, `Enter` edits it. So notes tabs are removed outright rather than collapsed to `Option<NotesTab>`.

### Persistence

`UiState`'s `notes_tabs` / `notes_active_tab` / `secondary_notes_*` fields are removed. Existing `ui.json` files load fine — serde drops the now-unknown fields — and saves no longer write them. The "restore open note tabs on reopen" behaviour is dropped with the feature; reopening shows the browser with selection reconciled to the first note.

## Consequences

### Positive

- Each instance model shrinks to pure single-document content; render, navigation, and persistence lose the `Vec`/`active_tab` threading.
- Notes' selection ownership (ADR-016) is no longer shadowed by a parallel tab concept.
- The dual/split/secondary axis is now visibly the *only* multi-pane mechanism.

### Negative / accepted

- Loss of multi-document-per-pane until re-added. Accepted: it was scaffolding, not a designed feature.
- `Ldr+[ / ]` and `Ldr+t` lose meaning as the panes de-tab (reader entries cleaned up in commit 2).
- Re-adding tabs later is a fresh implementation, not a flag flip. Accepted — git history is the reference.

## Related

- [ADR-002](ADR-002-reader-slice.md) / [ADR-003](ADR-003-notes-slice.md) — established the instance models this amends.
- [ADR-016](ADR-016-notes-backend-seam.md) — made `selected` the notes selection truth, which is why notes tabs fold into selection.
- `docs/CONTEXT.md` — `NotesInstanceModel` / `ReaderInstanceModel` vocabulary updated alongside each commit.
