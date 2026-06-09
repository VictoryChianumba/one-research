# One Research — backlog

The tracked, forward-looking list of things we want to improve or implement.
Committed (unlike the gitignored `V2.md`) so it survives a fresh checkout and is
visible to anyone reading the repo.

**Scope vs. neighbours:**
- **This file** — open improvements and ideas, not yet started. The forward
  complement to the changelog.
- **`V2.md`** (gitignored) — the v2 release-gate checklist: things that must ship
  before v2 goes out officially. A subset of intent, release-focused.
- **`docs/PERFORMANCE.md`** — deferred performance-audit items specifically.
- **`docs/adr/`** — decisions already *accepted* (not a wishlist).

**Convention:** newest items first within each section. When an item is picked
up, note the commit/ADR that resolved it and remove it (the changelog/ADR then
carries the record). Keep each entry self-contained — name the key files so a
reader can find the code without this context.

---

## UI / UX

- **Narrow-feed column language not yet unified (deferred 2026-06-09).** The wide
  feed (`draw_item_table`) and History were normalized to a shared column spec
  (`FEED_META_W` / `FEED_COL_SPACING` in `one-research/src/ui/layout/feed.rs`):
  Title flush-left, `Subject`/`Date`/`Viewed` at the shared metadata width, and
  the signal + `Src` + `Kind` columns removed (all already shown in the details
  pane). The *narrow* renderer (`draw_narrow_feed` → `reader.rs` `n_line`, used
  when the feed pane drops below 70 cols — e.g. the reader-drawer strip) still
  shows `Src Kind Title Date`.
  **Open question:** match it to the wide language, or keep the compact
  `Src`/`Kind` for the thin reader strip where the details pane may not be
  visible? No decision yet.

## Features

_(none recorded yet)_

## Tech debt

_(none recorded yet)_
