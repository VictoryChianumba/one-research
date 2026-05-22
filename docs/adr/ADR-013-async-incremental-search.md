# ADR-013 — Feed search runs off-thread via nucleo (async, incremental, ranked)

- **Status:** Accepted (2026-05-22). All three PRs landed. PR 1: `nucleo` dependency + standalone `search::engine::FeedSearch` + engine tests. PR 2: wired into the feed pipeline — lazy `App.feed_search` (created on first search char, dropped on clear), pattern driven from the search-bar chokepoints (`push`/`pop`/`clear`), `visible_indices_for` consumes the ranked snapshot with `cat:`/`year:` gates + substring fallback, frame-loop `tick` invalidates the visible cache while results stream, `fuzzy-matcher` + synchronous `Query::score` removed. PR 3: re-keyed the worker on the stable **URL** instead of the corpus index (`Nucleo<String>`) — `items_store` re-sorts in place on merge, so an index-keyed snapshot went stale and could point a result row at the wrong paper; the worker now stores URLs and `visible_indices_for` re-maps URL→current index via `items_store.find_index_by_url` each frame. That same change makes injection **incremental** (`sync` pushes only not-yet-seen URLs; full rebuild only on a corpus `clear`), retiring the per-merge full reload. Loop cadence was already correct from PR 2 (the loop marks dirty only while `Status::running`, idling at 250 ms once the worker settles), so no cadence change was needed.
- **Date:** 2026-05-22
- **Owner:** Victory Chianumba
- **Supersedes:** [ADR-012](ADR-012-fuzzy-ranked-search.md) §D3-D4 (synchronous `SkimMatcherV2` scoring on the render thread). ADR-012's query **grammar** and parser (§D1-D2) are retained and reused; only the matching/ranking *mechanism* changes.

## Goal

Make feed search behave like a mature fuzzy finder (fzf / skim / Helix): the
search bar echoes instantly and ranked results stream in, regardless of corpus
size, because **matching never runs on the UI thread**.

## Context

ADR-012 shipped a synchronous matcher: `SkimMatcherV2` scored the entire corpus
(title + authors + abstract) on the render thread, on every keystroke *and* every
frame (`visible_indices_for` is recomputed per frame in `dispatch_feed_pane`). On
a few-thousand-item corpus this makes the event loop's drain-then-draw cycle
(`main.rs`) back up: keystrokes queue behind slow frames, so typed text lands in
chunks and post-search navigation stutters.

Investigation (2026-05-22) confirmed the cost is trench's own — `tread`'s
per-frame `tick()` only forces redraws during voice playback, and the feed pane
draws without calling `tread` at all. The fix is architectural, matching what
every serious finder does: run matching on a background thread pool, match
**incrementally** (a longer query only re-checks prior survivors), and let the UI
**poll** for the latest snapshot.

`nucleo` (the matcher Helix uses) provides exactly this with a poll-based
`tick()`/`snapshot()` API — no `async` runtime, so it fits trench's
`std::sync::mpsc` + blocking-I/O model (CLAUDE.md) the same way the ingestion
drain does.

## Decision

### D1 — `nucleo` worker, polled from the frame loop

`search::engine::FeedSearch` wraps `Nucleo<u32>` (data = corpus index). Items are
injected into its background thread pool; the frame loop calls `tick(10)` and
reads `snapshot()` for ranked indices. `Status::running` folds into
`has_active_animation()` so the loop stays at the interactive cadence until
results settle (PR 2/3).

### D2 — Grammar maps onto nucleo's multi-column AND model

`MultiPattern` matches one atom-set per column and ANDs the columns — our exact
conjunctive grammar. Four columns:

| Col | Text | Fed by |
|-----|------|--------|
| 0 | title | `ti:` / `title:` |
| 1 | authors (space-joined) | `au:` / `author:` |
| 2 | abstract | `abs:` / `abstract:` |
| 3 | title ¶ authors ¶ abstract | free text |

Empty column patterns match everything, so unused fields impose no constraint.
Free text matches column 3; **title is placed first** in that column so a title
hit earns the start-of-haystack bonus.

### D3 — Field weighting via `Config::prefer_prefix`, not a second pass

ADR-012 weighted title > author > abstract with an explicit scoring sum. nucleo
recovers this natively: `prefer_prefix = true` rewards matches nearer the start of
the haystack, so with title-first column 3, a title hit outranks an abstract-only
hit. Verified by the `title_hit_outranks_abstract_only_hit` engine test. No
synchronous re-scoring pass survives.

### D4 — `cat:` and `year:` stay query-side gates

nucleo can't express controlled-vocabulary (`cat:`) or numeric-range (`year:`)
constraints. The ADR-012 parser still produces those buckets; they're applied by
the caller as a pre/post filter around the nucleo result set (PR 2). The taxonomy
resolver (`arxiv_taxonomy::item_matches_category`) is unchanged.

### D5 — Corpus scope: main item store only (option A)

`FeedSearch` indexes the shared `items_store` corpus (Inbox / Library / Browse).
Discoveries and History keep their existing simple substring filters — they're
smaller and not where the latency is felt. Revisit if uniformity is wanted later.

## Phasing

- **PR 1 (this):** dependency + standalone, tested `FeedSearch` engine. No wiring, no behavior change.
- **PR 2:** wire the engine — inject corpus on `ItemsChanged`, drive the pattern from the search bar, route `visible_indices` from the snapshot when a query is active, apply `cat:`/`year:` gates, remove `fuzzy-matcher`.
- **PR 3:** frame-loop cadence (`tick`/`Status::running`/notify), help/README finalize, `scripts/check-search.sh` invariants (matching is off-thread; no synchronous full-corpus scan in the render path), tests.

## Trade-offs

- nucleo stores its own copy of the four column strings per item — megabytes for a few-thousand-item corpus; acceptable.
- Two stores (`items_store` ↔ nucleo injector) must stay in sync; the `ItemsChanged` effect is the single sync point.
- nucleo's thread pool is created lazily on first search and can be dropped when search clears, avoiding idle threads.
- Ranking order shifts slightly vs `SkimMatcherV2` (nucleo is the fzf-class algorithm) — acceptable, arguably better.
