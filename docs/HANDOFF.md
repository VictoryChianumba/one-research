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

### Resolved decisions
- **Auto-fill gating — SETTLED 2026-06-02: keep gated on subject-follow ON.** Fire
  only while follow is on (with follow off the fetched items aren't shown, so an
  unconditional fetch wastes arXiv budget). Matches the shipped PR 3a code, so no
  code change. Recorded in ADR-015 §F5.

### Remaining build
- **PR 3b — background page-ahead.** Prefetch page 2 *while reading* page 1 so the
  buffer stays ahead of the scroll (§F4 extension). Not started. Needs the same
  idle-poll path as 3a (the loop body in `main.rs` next to `poll_browse_autofill`,
  ~line 897), reusing the worker's `start` param. Highest arXiv-request pressure —
  that's why it was split out for its own TUI soak.

### §F6 in-feed markers — DONE 2026-06-03
- The **in-feed seam markers shipped**: `loading…` while the followed category is
  `inflight`, and `caught up — N papers` (or `no recent papers`) once `exhausted`
  and the tail is on screen. `BrowseSeamState` + `browse_seam_state_for`
  (`feed/mod.rs`), rendered by `draw_feed_seam` on the feed pane's bottom padding
  row (`ui/layout/feed.rs`); set via `FeedContext.browse_seam` in `main_row.rs`.
  Tripwire R7; unit test `browse_seam_state_reflects_buffer_edges`. The shared
  `draw_item_table` stays Inbox/Library-safe (gated `match ctx.browse_seam`).
  TUI-verified: loading shows/clears with the fetch; caught-up appears at the
  cs.GL tail (239 papers) and hides when scrolled off-tail; Inbox shows nothing.
  Deliberate cut: no `press Enter to load` empty hint (would flicker for ~400ms
  under follow-on auto-fill).

### Verification owed (user, per the one-change-at-a-time rule)
- ~~TUI-verify PR 2 + PR 3a~~ — DONE 2026-06-02 (PASS; loading/pagination/inflight
  guard/follow-off gate all confirmed live). §F6 also TUI-verified 2026-06-03.
  Remaining unverified work is **PR 3b** only.

### Public docs — DONE 2026-06-02
- README.md "Subject Browser" section and `docs/pages/reference.md` "Browse tab"
  section now describe scroll-driven deepening + follow-gated auto-load on arrival.
  (FEATURES.md had no stale Browse load description; left untouched. No new
  keybindings — both behaviors are automatic.)

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
