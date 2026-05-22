# ADR-011 — Browse is the feed surface itself, scoped by a right-rail subject filter

- **Status:** Accepted (2026-05-19). The shipped Browse surface is a left feed
  with a Details-width right companion pane: the subject rail is shown by
  default and Filters replace it while filter focus is active.
- **Date:** 2026-05-19
- **Owner:** Victory Chianumba
- **Supersedes:** [ADR-010](ADR-010-subject-browser.md) §D4 ("merge with session scope") — replaced by §E1 ("Browse is the feed, with a subject-filter overlay"). ADR-010's D2 (typed taxonomy table), D3 (worker thread, not `Source` impl), and D5 (`KNOWN_ARXIV_CATS` stays deleted) remain in force.
- **Extends:** [ADR-010](ADR-010-subject-browser.md) §D2-D3, §D5-D6.

## Goal

Reshape the Browse tab so it stops being *"a taxonomy navigator that loads papers into a side panel"* and becomes *"the main reading surface itself, with an optional subject-scope filter."* The user's framing from 2026-05-19: *"It's not exactly doing anything for you"* — referring to the 3-column Miller layout consuming the full pane width without earning that real estate.

After the slice, the Browse tab presents:
1. A narrow right rail that shows one taxonomy level at a time and replaces — not stacks — when you drill in. Breadcrumb up top.
2. The actual feed table on the left, taking the rest of the pane. Same shape as Inbox/Library, plus a new **Subject** column.
3. New sort modes (`dated`/`random`/`popular`/`trending`) selectable from the filter pane.
4. A subject-follow toggle that, when ON, narrows the feed to whatever subject the rail is currently drilled into.

## Context

ADR-010 landed three PRs (2026-05-19, all on the local working tree, uncommitted) that built the Subject Browser as a 3-column Miller layout: `Groups | Archives | Categories` left-to-right, with a side details pane on the right rendering metadata + recent papers for the selected category.

After ADR-010 PR 3 landed, the user reviewed and surfaced two design concerns:

1. **Vertical real-estate waste.** All three columns persist at full height regardless of how deep the user has drilled. After selecting `Physics → astro-ph → astro-ph.GA`, the Groups column is still visible at full width but no longer doing navigation work — the user has already chosen. The "history of navigation" is interesting context but doesn't earn 8 visible rows per column.

2. **Indirection between taxonomy and feed.** Recent papers landed in a side details pane separate from the main feed surface. To browse those papers you had to look at a *different* table than the one you'd been reading in Inbox/Library. Two presentation contexts for the same thing (FeedItem rows) created cognitive friction.

The fix is structural, not cosmetic: instead of a taxonomy navigator that *produces* a list of papers, make the taxonomy a *filter* on the main feed itself.

[ADR-010](ADR-010-subject-browser.md) §D2's typed taxonomy table, §D3's worker-thread pattern (`spawn_browse_fetch`, not a `Source` impl), and §D5's `KNOWN_ARXIV_CATS` deletion all remain load-bearing. What changes is the *presentation* shape and the *feed-data flow* in the Browse tab.

## Decision

### Scope: rail UI + sort modes + subject-follow + Subject column

ADR-011 is a **presentation-layer ADR**. The data model from ADR-010 (`BrowseModel.loaded_categories`, `workspace.items_store.merge_fetched_item`, the existing dedup invariants) is unchanged. The fetch pipeline (`browse/pipeline.rs::spawn_browse_fetch`) is unchanged. What changes is:

- `BrowseModel`'s navigation state (rail-path stack instead of 3 parallel cursors).
- The `draw_browse_tab` renderer (single-column rail).
- The `main_row.rs` Browse layout (feed area + Details-width right rail instead of feed area + details pane).
- The feed-sort behavior (new `FeedSortMode` enum; new `subject_follow` toggle).
- The feed table for Browse only (gains a Subject column).

### Trigger: user-surfaced design feedback after ADR-010 shipped

The 3-column Miller layout is removed because the user found it isn't *doing anything* for them at the current taxonomy depth. The fix is the rail.

### Decisions

#### E1. Browse is the feed surface itself; the rail is a scope filter

The left-hand area of the Browse tab is the actual `draw_item_table` (or a Browse-specific variant of it), populated from `workspace.items_store.items()` and filtered by the rail's current subject scope. The right companion pane matches the normal Details width; it shows the subject rail by default and swaps to Filters while filter focus is active. There is no separate "Recent papers" pane anymore.

Subject-follow off (the default): the feed area shows the mixed view across every subscribed category plus any browse-fetched items. The rail navigates the taxonomy independently — it does not scope the feed.

Subject-follow on: the rail's current drill point becomes a *filter predicate* over `workspace.items_store.items()`. Drilled to `Mathematics → math` archive: feed shows only items whose `domain_tags` overlap with any `math.*` code. Drilled to `math.NT` leaf: feed shows only items tagged `math.NT`.

#### E2. Rail is replace-mode, not push-stack

When the user presses `Enter` on a Group, the rail's content *replaces* with that Group's Archives. The breadcrumb up top updates to show the current path. `Esc` / `h` / `Backspace` pops back up one level. This is the file-explorer-detail-view pattern, not the Miller-column-cascade pattern.

`BrowseModel`'s state becomes a stack:

```
rail_path: Vec<RailNode>      // [] = at Groups level
                              // [Group("math")] = at Math's archives
                              // [Group("math"), Archive("math")] = at math.* categories
rail_cursor: ListState        // single cursor over the current level's rows
```

The old `focused_column: u8` / `groups` / `archives` / `categories` triple is deleted. `loaded_categories` / `inflight` / `tx` / `rx` from ADR-010 PR 2 remain unchanged.

#### E3. Four sort modes: `dated` (default), `random`, `popular`, `trending`

Selectable from the filter pane (`f` opens it; the sort mode is one new line of the panel). Mutually exclusive — exactly one sort mode is active at any time. Stacks on top of the existing source / workflow-state / signal filters.

- **`dated`** — `published_at` descending. Current default; behaviour unchanged.
- **`random`** — deterministic shuffle keyed off a per-session seed. Stable within a session so re-renders don't reorder; reshuffles on next launch (or on an explicit `r` in the filter pane).
- **`popular`** — `upvote_count` descending. Works for HuggingFace items natively. arXiv items get their `upvote_count` filled from Semantic Scholar's `citation_count` field where enriched; unenriched arXiv items sort to the bottom.
- **`trending`** — items published in the last 14 days, sorted by `upvote_count` descending. Arxiv items with no upvote signal fall back to their `compute_signal()` tier (Primary > Secondary > Tertiary). Older items are filtered out entirely under this mode.

The sort mode applies *across every tab*, not just Browse. A user who sets `popular` in Browse and then cycles to Inbox sees Inbox sorted by popularity too. This is consistent with how the existing feed filters work today.

#### E4. Subject-follow toggle: filter pane line + `F` quick-toggle, default OFF

`subject_follow: bool` lives on `FeedModel` (sibling to `active_filters`). Default `false`.

Two access paths:
- **Filter pane (`f`)** — swaps the right companion pane from the rail to Filters. Its Browse section includes `[ ] Follow rail subject`; toggle with `Space`.
- **Quick-toggle `x` / `F`** — in the Browse rail, flips `subject_follow` without opening Filters. The rail footer renders the current follow state.

Default OFF rationale: the user enters Browse expecting the firehose, opts in to scoping when they want it. Reversing the default would surprise users who Tab into Browse and find their feed mysteriously narrowed.

#### E5. Subject column scope: Browse tab only

The Browse feed table gains a leading **Subject** column (e.g., `cs.LG` or `Math` depending on the rail's current depth — at Groups level, the Subject column shows the Group name; at Archive level, the Archive code; at Category level, the Category code). The column is omitted from Inbox / Library / Discoveries / History to keep their layouts untouched.

This is a *render-only* decision. The Subject data already exists on every `FeedItem` via `domain_tags`. The Browse renderer prepends the column; other tabs don't.

#### E6. Tab order: Inbox first, Browse second

`cycle_tab` order becomes `Inbox → Browse → Library → Discoveries → History → Inbox`. Default landing tab stays **Inbox** — the user wants their curated feed on launch, with Browse adjacent as the corpus surface. `Tab` from Inbox cycles to Browse; `Shift-Tab` from Inbox cycles to History.

The updated reasoning: Inbox remains the day-to-day first surface; Browse is the next step out from the curated feed when the user wants the broader corpus. Library is what you've engaged with, Discoveries is AI-driven search, and History is the audit trail.

### Invariants for PR 3 tripwire (extends `scripts/check-subject-browser.sh` from O1-O5 → adds P1-P5)

- **P1.** `BrowseModel` no longer has `focused_column`, `archives`, or `categories` fields. The rail-path refactor must keep the old state out.
- **P2.** `draw_browse_detail_panel` is deleted. The metadata side pane is gone in the new design; if it returns it must be renamed and re-documented.
- **P3.** Exactly one `FeedSortMode::Random`/`Popular`/`Trending`/`Dated` match — i.e., these are the only four variants, and a fifth would be an unplanned mode addition.
- **P4.** The Subject column renders in Browse tab only. A `grep` for the Subject-column rendering inside the generic `draw_item_table` (used by Inbox/Library) would fail the check.
- **P5.** ADR-011 cadence table lists every shipped PR. Same shape as O5 / J5 / I2.

## Consequences

### Positive

- The Browse tab's vertical real estate stops being wasted on 3 always-visible columns. Its right rail uses the same fixed companion-pane width as Details, so the feed + side-pane rhythm stays consistent with Inbox.
- Recent papers and Inbox papers now live in the *same* presentation context (the feed table), so the user's eye doesn't have to context-switch between two tables.
- Sort modes are a long-overdue feature for the existing feed. Browse drives the requirement, but every tab benefits.
- The subject-follow toggle gives the user explicit control over scope. Today there's no way to say "show me all arXiv physics papers I have cached, but nothing else" — after this, that's two keystrokes (`F` + drill).

### Negative

- Significant churn on a UI surface that just shipped (ADR-010 was Accepted today, 2026-05-19, on the same local working tree). Every PR-1 → PR-3 file from ADR-010 is touched again. The work isn't wasted — the data model, worker pipeline, dedup helper, and tripwires from ADR-010 are all reused — but the *render path* is rewritten.
- The `subject_follow` toggle's default-OFF state means new users won't discover the rail-scoping mechanism until they read the docs or stumble onto `F`. Mitigation: README + the rail's footer always shows the toggle state.
- Sort modes introduce a per-session piece of state that the user might forget they set. Setting `random` and then quitting / relaunching → the mode resets to `dated` by default; we explicitly do **not** persist sort mode to disk because surprise-on-launch is worse than re-selection.

### Trade-offs explicitly accepted

- **Replace, not push-stack.** Some users prefer Miller-column-cascade for visual context of the navigation history. We chose replace + breadcrumb because the user explicitly said the cascade *"isn't doing anything for you."*
- **Sort mode applies across all tabs.** Could have made it Browse-only for purity, but applying globally is more useful and matches how existing feed filters behave.
- **Sort mode resets on launch.** Persistence would create launch-time surprise; the cost of re-selection is low.
- **Subject column in Browse only.** Could be in every tab for consistency, but Browse is the only tab where the feed might span subjects you're not scoped to — the column earns its place there but is noise elsewhere.

## Risks

- **R1. Field-removal churn on `BrowseModel`.** Deleting `focused_column` / `archives` / `categories` will break every site that touches them (the renderer, `handle_browse_tab`, the BrowseModel tests). PR 1 must audit + update all in one pass. Mitigation: tripwire P1 anchors the post-refactor state.
- **R2. Sort modes risk silent miscompare with existing filter logic.** `visible_indices_for` already filters; sort applies after filter. Insertion point: after the filter pass, before returning. Mitigation: tests that lock the four sort modes' behaviours against fixture items.
- **R3. The Subject column in the Browse feed needs a width budget that compresses other columns.** The existing table already has Time/Title/Source/Signal columns; adding Subject pushes Title narrower. Mitigation: PR 3 makes the Subject column narrow (≤12 chars, ellipsised) and only renders when the Browse tab is active.

## Related

- [ADR-010](ADR-010-subject-browser.md) — Subject Browser foundations. ADR-011 explicitly supersedes §D4 and extends D2-D3, D5-D6.
- [ADR-001](ADR-001-render-purification.md) §D3 — `pre_draw` discipline. The rail's drill state lives in `BrowseModel`; render reads it. No new pre-draw mutation introduced.
- [ADR-004](ADR-004-ingestion-seam.md) §D1 — `Source` trait bulk-refresh-only. Browse remains a worker-thread consumer, not a `Source` impl. Tripwire O3 from ADR-010 stays active.
