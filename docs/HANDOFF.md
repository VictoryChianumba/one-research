# Handoff — ADR-015 Browse paging buffer (continuation)

_Written 2026-06-01 at the end of an over-budget session. This is the "what's
left" view. It does **not** re-explain the design — read the artifacts below._

## Canonical artifacts (read these first, in order)

1. `docs/adr/ADR-015-browse-paging-buffer.md` — design, §F1–F7 decisions, the
   3-PR cadence, and tripwire definitions R1–R6.
2. Memory file `project_adr015_browse_paging.md` (auto-loaded via `MEMORY.md`)
   — per-PR status with `file:line` anchors for every change.
3. Commits: `ae40c83` (PR 1+2), `addf30b` (PR 3a). `git show <hash>` for diffs.
4. `scripts/check-subject-browser.sh` — run it; it must exit 0 (O/P/R1–R6).

## Status

- **PR 1** (per-category `CategoryBuffer` + honest rail count) — committed `ae40c83`.
- **PR 2** (scroll-tail pagination, `arxiv::fetch_page`) — committed `ae40c83`.
- **PR 3a** (arrival auto-fill on rail settle) — committed `addf30b`.
- All verified: 307 tests, fmt, clippy clean for new code, tripwire exit 0.

## Left to do

### Open decision (unresolved — do not silently pick)
- **Auto-fill gating.** `App::poll_browse_autofill` (`trench/src/app/methods/history.rs`)
  currently gates on **subject-follow ON**. Open question: keep gated, or fire on
  any category landing (unconditional, per the ADR's literal §F5)? Rationale for
  the current gate is in the method's doc comment and ADR §F5. The user has not ruled.

### Remaining build
- **PR 3b — background page-ahead.** Prefetch page 2 *while reading* page 1 so the
  buffer stays ahead of the scroll (§F4 extension). Not started. Needs the same
  idle-poll path as 3a (the loop body in `main.rs` next to `poll_browse_autofill`,
  ~line 897), reusing the worker's `start` param. Highest arXiv-request pressure —
  that's why it was split out for its own TUI soak.

### Honest partial in PR 1 (§F6 is only half-done)
- The **rail** half of the honest empty-state shipped (per-category count cell) plus
  status-bar messages. The **in-feed seam markers did NOT ship**: a "caught up — N
  papers" marker at the tail when a category is `exhausted`, and a dedicated
  "loading…" state *in the feed pane itself* (not just the status bar). The data
  exists (`exhausted`, `inflight` on `CategoryBuffer` / `BrowseModel`); it's
  render-only work, deliberately deferred to avoid touching the shared feed-render
  path early. Natural companion to PR 3b.

### Verification owed (user, per the one-change-at-a-time rule)
- TUI-verify **PR 2** (scrolling a followed Category deepens the list past 50) and
  **PR 3a** (land cursor on a Category with follow ON, wait ~½s → it auto-loads).
  This is the gate before stacking 3b.

### Public docs (standing rule: keep public docs current with code)
- Update the **README / `features.md` Subject-Browser section** to describe the new
  behavior: categories load deeper on scroll, and auto-load on arrival. This is the
  doc debt from PRs 2–3a. (No new keybindings — both behaviors are automatic.)

### Tuning knobs (all guesses until felt in the TUI)
- `BROWSE_PAGE_SIZE` = 50 (`app/state/browse.rs`), settle window = 400ms +
  page-ahead threshold = 5 rows (`app/methods/history.rs`), first-page depth.

## Explicitly NOT doing (decided — do not re-litigate)
- **Inbox/feed display cap or "Load more"** — rejected; the feed render is already
  windowed. Only conditional successor: a *cache-write cap* IF startup latency ever
  shows a real number in the `main.rs:837` debug log. Not active work.

## Environment hazard (important)
- This working tree has a **concurrent committer** (a scheduler / another agent;
  `.claude/scheduled_tasks.lock` is present). It made `db9da4e` mid-session and has
  ongoing uncommitted edits to `ui/layout/{main_row,mod,title}.rs` and a stray
  "Theme" vocab row in `docs/CONTEXT.md` — **none of which are ours**. Always
  `git add` your files **by path; never `git add -A`**, or you will sweep its work
  into your commit.

## Suggested skills for the next session
- `verify` or `run` — launch the TUI to do the owed PR 2 / PR 3a verification.
- `ratatui` / `tui-development` — for the §F6 in-feed render markers.
- `code-review` — before committing PR 3b.

## Resume boot sequence
`MEMORY.md` → `project_adr015_browse_paging.md` → ADR-015 cadence → `git log` →
run `scripts/check-subject-browser.sh` (expect exit 0).
