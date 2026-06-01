# ADR-015 — Browse is a per-category paging buffer with windowed render, not an on-Enter snapshot

- **Status:** Proposed (2026-06-01). PR 1 + PR 2 committed (`ae40c83`). PR 3 split into **3a** (arrival auto-fill — implemented + verified locally, uncommitted) and **3b** (background page-ahead — pending); the split mirrors slice-1's 4a/4b, keeping each rate-limit-affecting change independently verifiable in the TUI. Cadence table below.
- **Date:** 2026-06-01
- **Owner:** Victory Chianumba
- **Supersedes:** [ADR-011](ADR-011-browse-scoped-feed.md) §E1's *on-Enter snapshot* flow — a Category drill fetched exactly one 50-item page and stopped. Replaced by §F4 (scroll-driven pagination) + §F5 (first page on rail-arrival). ADR-011's rail UI (§E2), sort modes (§E3), subject-follow (§E4), and Subject column (§E5) remain in force.
- **Extends:** [ADR-010](ADR-010-subject-browser.md) §D3 (worker thread, not a `Source` impl — unchanged) and §D5 (session-scoped, not persisted — **explicitly retained**, see §F7).

## Goal

Make the Browse feed *feel* like the Inbox feed — open instantly, scroll bottomlessly — while honestly reflecting that its items come from arXiv over a rate-limited wire rather than from an already-downloaded corpus.

Today a Category drill fetches one page of 50 recent papers and stops. The user hits the bottom of those 50 almost immediately and there is nothing behind them, even though arXiv's listing for that category has thousands of papers going back years. The fix is to turn Browse's flat per-category URL list into a **paging buffer** that the existing windowed render draws from, and to keep that buffer deeper than the scroll position.

## Context

Two costs scale with feed length on a website — DOM nodes and network payload — and "Load more" exists to dodge both. In trench's TUI neither applies the way it does on the web:

1. **Render is already windowed.** `draw_item_table` (`ui/layout/feed.rs`) slices to `offset .. offset + viewport_rows + 2` and only builds `Row` objects for that visible window. Whether the underlying pool is 50 items or 5,000, per-frame render cost is constant. The Inbox feed feels bottomless not because rendering is special but because its pool was downloaded deep up front (`store::cache::load()` at startup).

2. **Browse pays a fetch cost the Inbox does not.** Each category's items live on arXiv's servers until something pulls them. `spawn_browse_fetch` (`browse/pipeline.rs:21`) calls `arxiv::fetch(&[category])`, which hardcodes `&max_results=50` with no `start` offset (`ingestion/arxiv.rs:30-44`). One page, most-recent-first, then nothing.

So the half of the feed's behaviour that's free — windowed render — *already* transfers to Browse. What's missing is depth in the pool it draws from. The only genuinely scarce resource is arXiv's request budget (~1 req / 3s); "fetch everything eagerly" is slow because of *that*, not memory (a fully-loaded ~155-category corpus is tens of MB) or compute (the filter scan over even 15k items is sub-millisecond).

The forcing function is direct user feedback (2026-06-01): drilling into a populated arXiv category and seeing only ~50 items, or a silent empty list, when the website shows a daily firehose.

## Decision

### Scope: data-model + fetch-pagination + empty-state; render path reused as-is

ADR-015 is a **Browse-data-flow** ADR. It does *not* introduce a new render path — the existing windowed `draw_item_table` is the whole point of leverage. What changes:

- `BrowseModel.loaded_categories` gains structure: a per-category buffer carrying `next_offset` + `exhausted`, replacing the bare `Vec<String>`.
- `ingestion/arxiv.rs` gains a paginating `fetch_page(categories, start, page_size)`; the existing `fetch` delegates to it with `(0, 50)` so bulk refresh is byte-for-byte unchanged.
- `browse/pipeline.rs` and `BrowseMessage` carry the page offset so the consumer can append and advance the buffer.
- The Browse rail/feed render gains honest seam states (loading / count / caught-up) read from the buffer + `inflight`.

The bulk-refresh `Source` registry, the dedup/merge path, and the rail UI from ADR-011 are untouched.

### Trigger: the pool is shallow, not the renderer

The Inbox feels bottomless because its pool is deep. Browse's render is identical; only its pool is shallow (one 50-item page). Deepen the pool — and keep it ahead of the scroll — and Browse inherits the feel for free.

### Decisions

#### F1. Per-category paging buffer replaces the flat URL list

`loaded_categories: HashMap<String, Vec<String>>` becomes `HashMap<String, CategoryBuffer>`:

```
struct CategoryBuffer {
  urls: Vec<String>,   // accumulated in fetch order; resolved against items_store
  next_offset: usize,  // arXiv `start` for the next page (0 before any fetch)
  exhausted: bool,     // a short page (< page_size) came back → no more to pull
}
```

`urls` keeps the existing role — URLs resolved against `workspace.items_store` so dedup + workflow-state preservation stay free (ADR-010 §D4 mechanics). `next_offset` / `exhausted` are the new state the other decisions read. `inflight: HashSet<String>` is unchanged and now doubles as the "loading" signal for §F6.

This is additive to `BrowseModel`'s shape — it does not reintroduce any ADR-011 P1-forbidden field (`focused_column` / `archives` / `categories`).

#### F2. `arxiv::fetch_page(categories, start, page_size)`; `fetch` delegates with `(0, 50)`

The arXiv Atom API already supports `&start=N&max_results=M`. The new free function threads both through; the existing `fetch(categories)` becomes `fetch_page(categories, 0, 50)`. Every current caller — `ArxivSource::fetch` (bulk), the discovery agent, `fetch_search` — keeps its exact behaviour. Only Browse calls `fetch_page` with a non-zero `start`.

Surgical by construction: bulk refresh's 50-most-recent contract is preserved precisely because `fetch` keeps its signature and delegates.

#### F3. The windowed render is the leverage; the buffer only changes what's *available*

No new render code for "more items." The feed already draws `offset .. offset + viewport`. Pagination's only job is to make sure `urls` has entries past `offset` by the time the user scrolls there. Render stays as snappy as Inbox because it *is* Inbox's render path — constant per-frame cost regardless of buffer depth.

#### F4. Pagination is scroll-driven; exhaustion is a short page

When the feed cursor reaches within a threshold of the tail of the current category's `urls` (subject-follow on, drilled into a Category), and `!exhausted` and the code is not `inflight`, fire `fetch_page(code, next_offset, page_size)`. On arrival, append the deduped URLs, set `next_offset += page_size`, and if the returned page was shorter than `page_size`, set `exhausted = true`. The `inflight` guard (ADR-010 R3) prevents a fast scroll from firing duplicate page fetches.

This is "infinite scroll done right": user-driven, so it never outruns arXiv's budget on its own, and it terminates honestly at the archive's end rather than looping.

#### F5. First page on rail-arrival, debounced — supersedes the on-Enter-only fetch

Landing the rail cursor on a Category (not just pressing `Enter`) schedules its first `fetch_page(code, 0, page_size)` after a short settle debounce (~400ms of cursor stillness), skipped if the category is already buffered or `inflight`. Arrowing through ten categories fires *one* fetch — the one you stop on — never ten. `Enter` remains a valid explicit trigger (fires immediately, same guard).

This supersedes ADR-011 §E1/§E2's "Enter on a Category fires the fetch" as the *only* path. The debounce is the rate-limit discipline that makes auto-fill safe; without it, navigation would hammer arXiv. (Background prefetch of page 2 while reading page 1 is a deliberate §F4 extension, deferred to PR 3 so the scroll-driven path is verified in isolation first.)

#### F6. Honest seam states, rendered from the buffer + `inflight`

The empty/edge states stop being silent. Read from `CategoryBuffer` + `inflight`:

- not buffered, not inflight → `press Enter to load` (or nothing yet, pre-debounce)
- `inflight` → `loading…`
- buffered, `!exhausted` → the feed, with the count available for the rail/footer
- buffered, `exhausted`, scrolled to tail → a quiet `caught up — N papers` marker

This is the only place the user ever sees that Browse talks to a network. It's render-only — no new fetch behaviour — which is why it ships first (PR 1) and de-risks the rest.

#### F7. Session scope retained — ADR-010 §D5 stays in force

`CategoryBuffer` is in-memory only; the buffer (offsets and all) resets on relaunch. The user explicitly declined cross-launch persistence in the 2026-06-01 discussion. Persisting would re-raise ADR-010 §D4's "what does Inbox contain on launch?" question; Browse stays a *session* tool. Recorded here so a future contributor doesn't read pagination as a reason to persist.

### Cadence — one ADR, three PRs (maps to the user's three asks)

- **PR 1 — buffer + honest empty-state (no new fetch volume).** `CategoryBuffer` type (§F1) + seam-state render (§F6) + tripwires R1-R5 + `ci.sh` wiring + CONTEXT.md vocab. First fetch is still one page of 50; the buffer simply records `next_offset = 50, exhausted = false`. User-visible win: honest states replace silent empty. *(the "honest empty-state" ask)*
- **PR 2 — deeper results via scroll-driven pagination.** `fetch_page` (§F2) + scroll-tail trigger + append/advance/exhaust (§F4) + `BrowseMessage` offset plumbing. *(the "deeper / paginate" ask)*
- **PR 3 — first-page-on-arrival + background prefetch.** Split for independent TUI verification: **3a** rail-settle auto-fill (§F5) — fires page 1 once the cursor rests on a Category past the settle window, gated on subject-follow, polled every loop iteration so the timer advances while idle, attempted-guarded against retry storms (tripwire R6); **3b** page-ahead while reading (§F4 extension) — pending. *(the "auto-fill on navigate" ask)*

### Invariants for tripwire (`scripts/check-subject-browser.sh`, letters R1-R5)

Letter `R` continues the alphabet past `Q` (search). The checks extend the existing Browse script rather than forking a new one.

- **R1.** `arxiv::fetch` still constructs `&max_results=50&start=0` for the bulk path — i.e. `fetch_page(_, 0, 50)` equivalence holds. Guards against the pagination refactor silently changing bulk-refresh depth.
- **R2.** Exactly one `fetch_page` definition, and `fetch` delegates to it (no second hand-rolled URL builder). Prevents query-construction drift between bulk and Browse.
- **R3.** `CategoryBuffer` carries `next_offset` and `exhausted`. A revert to a bare `Vec<String>` loses pagination state and silently caps Browse at one page again.
- **R4.** The scroll-tail fetch is `inflight`-guarded and `exhausted`-guarded. A grep anchors both short-circuits so a future edit can't reintroduce duplicate-page storms or post-exhaustion looping.
- **R5.** ADR-015's cadence table (Status line) lists every shipped PR with its `(...)` summary. Mirror of O5 / P5 / J5 status hygiene.

## Consequences

### Positive

- Browse inherits the Inbox feed's feel — instant first paint, bottomless scroll — without a new render path. The windowed `draw_item_table` is reused verbatim.
- The 50-item ceiling is gone: any category can be read to its archive depth, one rate-limited page at a time, terminating honestly at the end.
- Silent empty lists become legible states (`loading…` / `caught up — N papers`), removing the "is it broken or just empty?" ambiguity that triggered this ADR.
- Bulk refresh, discovery, and search are untouched — `fetch` keeps its signature and delegates, so the blast radius is Browse-only.

### Negative

- `BrowseModel` grows a richer per-category type and PR 3 adds a debounce timer — more session state to reason about. Mitigated by R3/R4 anchoring the invariants.
- More arXiv requests over a session (pagination + auto-fill). The §F4 scroll-gating and §F5 debounce keep this within the ~1-req/3s envelope, but a user who scrolls fast through a deep category will issue more calls than today's one-and-done. No retry envelope is added here (that's a cross-cutting `with_retry` refactor for all `arxiv::*` callers, out of scope).
- Memory grows with browsed depth — tens of MB at the extreme of fully paging many categories. Bounded, session-scoped (§F7), and far below any rendering or filter-scan concern.

### Trade-offs explicitly accepted

- **Scroll-driven, not eager full-category prefetch.** Pulling a whole category up front would blow the rate budget and mostly fetch papers the user never scrolls to. Page-on-demand + page-ahead is the right depth/cost balance.
- **Session-scoped buffer (§F7).** Persistence is declined per the user; re-fetching on relaunch is the accepted cost of keeping Inbox's launch contract simple.
- **Debounce on arrival rather than fetch-on-every-keystroke.** Trades a ~400ms delay before a category fills for not hammering arXiv while the user arrows past it.

## Risks

- **R1. Rate-limit throttling under fast scroll / fast rail navigation.** Mitigation: `inflight` + `exhausted` gating (§F4), arrival debounce (§F5). If throttling still appears, the page size and debounce window are the tuning knobs; the `with_retry` envelope is the escalation path.
- **R2. `fetch` / `fetch_page` divergence.** A future edit could fork the URL builder and drift the bulk query. Mitigation: tripwire R2 (single definition, delegation) + R1 (bulk depth equivalence).
- **R3. Off-by-one in `next_offset` causing duplicate or skipped papers across pages.** arXiv `start` is 0-based and the dedup path collapses exact-URL repeats, so duplicates are absorbed; skips are the real hazard. Mitigation: PR 2 tests that lock two consecutive pages of a fixture category to a contiguous, gap-free URL sequence.

## Related

- [ADR-010](ADR-010-subject-browser.md) — Subject Browser foundations; §D3 worker pattern (reused), §D5 session scope (retained, §F7).
- [ADR-011](ADR-011-browse-scoped-feed.md) — Browse-as-feed + rail + subject-follow; §E1's on-Enter snapshot is what §F4/§F5 supersede.
- [ADR-004](ADR-004-ingestion-seam.md) §D1 — `Source` is bulk-refresh-only; Browse stays a worker-thread consumer, tripwire O3 still applies.
- `docs/CONTEXT.md` — `CategoryBuffer` vocabulary entry added in PR 1.
