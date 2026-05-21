# ADR-012 — Feed search is fuzzy, field-scoped, and relevance-ranked

- **Status:** Accepted (2026-05-21). Shipped as PR 1 (this ADR + new `trench/src/search/mod.rs` query parser + field-weighted fuzzy scorer + `fuzzy-matcher` dependency + `visible_indices_for` reworked into a non-search filter pass followed by a scoring pass with relevance ordering + `app/mod.rs::visible_items` collapsed onto `visible_indices_for` for every tab + search-syntax hint in `ui/layout/title.rs` + Search section in `ui/layout/popups/help.rs` + README Search section + `scripts/check-search.sh` invariants Q1-Q5 + 9 inline parser/scorer tests).
- **Date:** 2026-05-21
- **Owner:** Victory Chianumba
- **Extends:** [ADR-011](ADR-011-browse-scoped-feed.md) §E3 (`apply_sort_mode`) — the sort mode still governs the no-query case; a live query overrides it with relevance order.

## Goal

Make the feed search behave like arXiv's: search by keyword, author, or year,
tolerate typos, and **surface the most relevant paper first** instead of merely
hiding non-matches in published-date order.

## Context

Before this slice the search bar was a single case-insensitive substring filter
over `title_lower` + `authors_lower` (`feed/mod.rs`), duplicated as an inline
closure in `app/mod.rs::visible_items`. It could not search the abstract, had no
author/year/field syntax, and never ranked: a survivor with a title hit sorted
identically to one matched only on an author surname, because ordering was always
the global `FeedSortMode` (`Dated`/`Popular`/`Trending`/`Random`).

The user's framing (2026-05-21): *"I don't think it has all the natural search
capabilities that the search bar in arXiv has … and we don't return the most
relevant paper either."*

The data was already present on `FeedItem` (`title`, `authors`, `summary_short`,
`published_at`); the gap was purely the matching and ordering logic.

## Decision

### D1 — A dedicated `search` module owns parsing and scoring

`trench/src/search/mod.rs` parses the raw bar text once into a `Query`
{ free, title, author, summary, year } and exposes `score(item, matcher) ->
Option<i64>`. Parsing and matching are pure and unit-tested in isolation; the
feed pipeline only calls `Query::parse` + `Query::score`.

### D2 — Field-prefix grammar with conjunctive terms

`ti:`/`title:`, `abs:`/`abstract:`, `au:`/`author:`, `year:`/`yr:`, plus free
text. Double quotes group a value with spaces (`author:"Yann LeCun"`). Year
accepts `2024`, `2020-2024`, `>2020`, `>=2020`, `<2024`, `<=2024`. All terms are
AND'd; an unknown prefix (`http://…`) is treated as one free term, never split.
An unparseable `year:` is dropped rather than demoted to free text, so a bad year
can't silently start matching titles.

### D3 — Fuzzy matching via `fuzzy-matcher` (SkimMatcherV2), smart-case

Typo tolerance and subsequence ranking come from `SkimMatcherV2`, whose default
smart-case means we match the original-case `title`/`summary_short`/`authors`
directly — the precomputed `title_lower`/`authors_lower` fields are no longer on
the feed search path (they remain in service of History search). Year is a hard
filter (no fuzz), checked before any text scoring.

### D4 — Relevance ordering overrides the sort mode while searching

Score weights are title > author > abstract (`3:2:1`). When a query is active,
`visible_indices_for` orders by score descending via a **stable** sort, so ties
keep `items_store`'s published-date-desc order (newest wins ties). When the query
is empty, `apply_sort_mode` runs unchanged. "When you're searching, match quality
is the ordering."

### D5 — One visible-set function for every tab

Relevance ranking is only correct if the rendered list and the
selection/navigation list agree on order. `app/mod.rs::visible_items` previously
kept its own inline filter for non-Browse tabs while `main_row.rs` rendered via
`visible_indices_for`; they agreed only because the default `Dated` sort is a
no-op. This slice collapses `visible_items` onto `visible_indices_for` for all
tabs, making `visible_indices_for` the single source of truth for membership and
order. (History continues to use `filtered_history_for`.)

## Tripwires

`scripts/check-search.sh` (Q1-Q5): no inline substring search left in
`visible_items`; the relevance sort + `Query::score` are present in
`visible_indices_for`; the four field prefixes exist in the parser; the feed path
parses raw `feed.search_query` (not the lowercase mirror); and this status block
references the shipped PR.
