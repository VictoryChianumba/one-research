# ADR-010 — Subject Browser surfaces arXiv's full taxonomy without forcing it into the general feed

- **Status:** Accepted (2026-05-19). All three PRs landed: PR 1 (this ADR + `FeedTab::Browse` variant + static `arxiv_taxonomy.rs` taxonomy table with 8 groups / 155 categories + `BrowseModel` skeleton + placeholder UI in `ui/layout/browse.rs` + `handle_browse_tab` navigation + CONTEXT.md vocabulary). PR 2 (`browse/pipeline.rs::spawn_browse_fetch` worker + own-channel `BrowseMessage` type instead of `FetchMessage::BrowseItems` to avoid bulk-refresh-channel lifecycle entanglement + shared `App::merge_fetched_item` dedup helper + `process_incoming_browse` + `Enter` wiring + details-pane resolution of URLs to FeedItem titles + 7 inline tests anchoring the dedup contract). PR 3 (`p` promotion gesture in `keys/feed.rs` + `★` indicator in `draw_categories_column` + sources-popup arXiv-section surgery in three files + `KNOWN_ARXIV_CATS` deleted from `config.rs` + `scripts/check-subject-browser.sh` with O1-O5 tripwires + `ci.sh` wiring + README.md keybindings/Sources/Features sections updated + status flip).
- **Date:** 2026-05-19
- **Owner:** Victory Chianumba
- **Supersedes:** none
- **Extends:** [ADR-004](ADR-004-ingestion-seam.md) §D1 (`Source` is bulk-refresh-only; on-demand fetches use worker threads).

## Goal

Let the user browse arXiv's full ~155-category taxonomy inside trench — three-level hierarchy `Group → Archive → Category` mirroring [arxiv.org's home page](https://arxiv.org/) — without forcing every category into the general feed. The existing curated feed (`config.sources.arxiv_categories`) stays small; the browser is where new subjects are discovered, with a gesture to promote a category into the feed permanently.

After the slice, **the 7-entry `KNOWN_ARXIV_CATS` shortlist disappears** and arXiv-category management lives exclusively in the browser. The general feed remains a *subset* of the taxonomy — the user's promoted choices — rather than the only window onto arXiv.

## Context

Today trench fetches arXiv papers from a tactical shortlist of three category codes (`cs.LG + cs.AI + stat.ML`, defined in `trench/src/config.rs:309-313`), toggleable from a sources popup driven by `KNOWN_ARXIV_CATS` (7 entries at `trench/src/config.rs:7-15`). Browsing anything outside the shortlist requires editing the config or routing through the discovery agent's free-text path.

The audit's framing of the shortlist — *"the seam-shaped wrong answer"* — applies. arXiv's taxonomy is a stable three-level structure: arXiv hasn't added a new top-level group in over a decade (the 8 groups in `models/arxiv_taxonomy.rs::TAXONOMY` are canonical), and the ~155 categories drift on a multi-year cadence. The shortlist exists because the codebase didn't have a typed taxonomy table; once you have one, the picker UI is just a tree-walk and the shortlist becomes redundant.

[ADR-004](ADR-004-ingestion-seam.md) — landed 2026-05-18 — established the bulk-refresh `Source` trait (`trench/src/ingestion/pipeline.rs:43`). The Subject Browser is the first feature that *uses* ADR-004's framing to its advantage: Browse is selection-driven (one fetch per user keystroke), not poll-driven, so it does **not** register as a `Source`. It mirrors `spawn_discovery` (`trench/src/discovery/pipeline.rs:11`) — the on-demand worker pattern — instead. ADR-004 §D1 anticipated this split; ADR-010 is the first ADR to act on it.

The forcing function isn't tech-debt cleanup — it's a product expansion. The user wants `physics.optics` and `q-bio.NC` and `econ.TH` accessible alongside `cs.LG` without each one polluting the daily feed.

## Decision

### Scope: feed-side surface; fetch worker; sources-popup surgery

ADR-010 is a **feed-side** ADR. The render path gains one `FeedTab` variant and one renderer; the on-demand fetch path gains one worker module; the sources popup loses its arXiv-categories section. The bulk-refresh `Source` registry is unchanged. The merge path (`app/methods/process.rs:64-108`) gains one new `FetchMessage` arm sharing the existing dedup/workflow-state-preservation helper.

### Trigger: the audit's "shortlist is the seam-shaped wrong answer" framing

`KNOWN_ARXIV_CATS` is the load-bearing wrong abstraction. Replacing it with a typed taxonomy unlocks every "show me category X" feature for free.

### Decisions

#### D1. New `FeedTab::Browse` variant; cycle order Inbox → Library → Discoveries → **Browse** → History → Inbox

The Subject Browser is a top-level feed tab, not a modal overlay. The 4-column Miller layout (Groups | Archives | Categories | Recent papers) doesn't fit a popup, and tab membership puts the surface on the same plane as Discoveries (the other "finding new stuff" tab).

`FeedTab::Browse` lands at `trench/src/app/state/feed.rs:96`. The cycle order groups the "exploratory" tabs (Discoveries, Browse) together. Tab strip at `trench/src/ui/layout/title.rs` shows `Browse 155` — the static category count communicates "you can browse this many subject categories" rather than relying on a per-session counter that would always be 0 on first frame.

#### D2. Static taxonomy in `models/arxiv_taxonomy.rs`; does not depend on `map_arxiv_category`

The full taxonomy lives in its own module as a `&'static [Group]` const (`TAXONOMY`). Three structs (`Group { archives }`, `Archive { categories }`, `Category { code, name }`); three accessors (`find_category`, `all_categories`, `group_count`). 155 entries, linear scan — no `phf` dependency.

`models/categories.rs::map_arxiv_category` (the hot-path per-row label dictionary used in feed rendering) stays as-is. The two tables serve different consumers: `map_arxiv_category` is read inside `parse_atom`'s per-item loop during fetch; `arxiv_taxonomy::TAXONOMY` is read once per frame by the Browser's render path. Conflating them would couple a render seam to a config seam — the same anti-pattern ADR-007 §D1 explicitly rejected for `ItemStore`.

#### D3. Browse fetches via a worker thread, **not** as a `Source` impl

ADR-004 §D1: the `Source` trait is bulk-refresh-only ("one `fetch` call per refresh"). Browse is selection-driven (one fetch per user keystroke, scoped to one category). Registering Browse as a `Source` would either force every refresh to enumerate the user's last browsing trail, or require a new `Source::fetch` shape with an `Option<&Scope>` input — the API churn ADR-004 §D1 rejected.

Instead, PR 2 lands `trench/src/browse/pipeline.rs::spawn_browse_fetch(category, tx)` mirroring `spawn_discovery` (`discovery/pipeline.rs:11`): its own `std::thread::spawn`, its own `panic_msg`-isolated body, calls the existing `arxiv::fetch(&[category])` free function (`ingestion/arxiv.rs:30`), and sends `FetchMessage::BrowseItems { category, items }` on the existing channel.

Tripwire O3 enforces this — no `impl Source for Browse*` may exist anywhere in `trench/src/`.

#### D4. Merge with session scope: items land in `workspace.items`, visibility is filtered by `config.sources.arxiv_categories`

Browse-fetched items merge into `workspace.items_store` via the existing dedup path (`app/methods/process.rs:64-108`) — same URL + arXiv-ID dedup, same workflow-state preservation from `persisted_states`. This is option (c) in the design discussion ("merge with session scope") and is required for the existing dedup invariants to hold.

But the *general feed* (Inbox + Library tabs) only shows items whose category is in `config.sources.arxiv_categories`. Browse-fetched items from unsubscribed categories appear in the Browser's column 4 (via `BrowseModel.loaded_categories[code] -> Vec<url>`, resolved against `workspace.items_store.url_index`) but do not show up in Inbox unless the user explicitly promotes the category.

This is the answer to "does browsing pollute my feed?": no, unless you promote.

#### D5. `BrowseModel.loaded_categories` resets on restart (session-scoped only)

`BrowseModel.loaded_categories: HashMap<String, Vec<String>>` is in-memory only — not persisted to `~/.config/trench/`. Re-launching trench shows an empty column 4 until the user re-fetches.

Persisting it would re-raise the question D4 already answered ("what does Inbox contain on launch?"). Browse stays a *session* tool; the general feed is the *persistent* surface. The two are distinct by design.

#### D6. Sources popup loses its arXiv-categories section; RSS / predefined / custom-feed parts stay

PR 3 trims `trench/src/surfaces/overlays/sources.rs` and `trench/src/app/methods/sources_popup.rs` to remove the arXiv-category chips and toggle handlers. `KNOWN_ARXIV_CATS` is deleted from `config.rs:7-15`. The popup retains its RSS / predefined-sources (HF, OpenAI, DeepMind, BAIR, MIT, OpenReview, CORE) / custom-feeds sections.

The arXiv-categories job moves into the Browser: pressing `p` on a category in column 3 toggles its membership in `config.sources.arxiv_categories` and persists via the existing `Config::save()` path. A one-line hint stays in the popup for one release ("arXiv categories now configured in Browse — Tab cycles to it").

Discoverability of the new gesture is the main trade-off — see §Risks.

### Invariants for PR 3 tripwire (`scripts/check-subject-browser.sh`)

Letter `O` continues the alphabet (I = render purification, J = ingestion seam, K = discovery slice, L = store seam, M = item-store, N = frame layout).

- **O1.** `models/arxiv_taxonomy.rs::TAXONOMY` has exactly 8 groups. arXiv hasn't added a top-level group in over a decade; off-by-one is almost certainly an accidental edit, not a real schema change. The unit test `taxonomy_has_eight_groups` plus a grep over the source file's group-count anchor catches both directions.
- **O2.** `FeedTab::Browse` is matched at every site that destructures `FeedTab`. PR 1 audits the count (today: N exhaustive matches across `app/mod.rs`, `feed/mod.rs`, `keys/reader.rs`); tripwire encodes the literal so a future contributor who adds a new dispatch site gets a heads-up.
- **O3.** No `impl Source for Browse*` anywhere in `trench/src/`. Browse is *not* a `Source` (ADR-004 §D1); a future contributor wiring Browse into the bulk-refresh registry would silently double-fetch every browsed category on each refresh.
- **O4.** `KNOWN_ARXIV_CATS` stays deleted across `trench/src/` and `crates/`. A revival would re-create the dual-management ambiguity D6 explicitly removed.
- **O5.** ADR-010's cadence table (Status line) lists every shipped PR with its `(...)` summary. Mirror of ADR-004 J5 / ADR-008 N's status hygiene.

The script wires into `scripts/ci.sh` alongside the existing five (`check-render-purification.sh`, `check-ingestion-seam.sh`, `check-store-seam.sh`, `check-item-store.sh`, `check-frame-layout.sh`).

## Consequences

### Positive

- The 7-entry shortlist disappears as a primary mechanism. Adding a category to the general feed is a `p` keystroke from any leaf in a familiar three-level tree, not a config-file edit or a sources-popup hunt.
- arXiv-category management becomes uniform across all 155 codes. The asymmetry between `cs.LG` (curated, in `KNOWN_ARXIV_CATS`) and `physics.optics` (not in any picker, requires manual config) goes away.
- Browse-fetched items reuse the existing dedup invariants. No new code path mutates `workspace.items` — the existing `FetchMessage::Items` handler is generalised to a shared helper called by both `Items` and `BrowseItems` arms.
- The `Source` trait stays sharp. ADR-004's bulk-refresh-only framing is *strengthened*, not blurred, by the explicit non-membership of Browse.

### Negative

- The Subject Browser is a new top-level surface. New tab strip span, new renderer module (`ui/layout/browse.rs`), new key shim (`keys/feed.rs::handle_browse_tab`). Maintenance footprint grows by one tab's worth.
- The `p` promotion gesture has weak discoverability — a user expecting the sources popup to manage arXiv categories will not find them there. PR 3 ships a one-line hint and a `docs/hotkeys.md` entry to mitigate.
- Browse-fetched items land in `workspace.items` regardless of whether the user promotes the category. The cache grows by ~50 items per browsed category. If cache size becomes a problem, eviction is a separate, simpler concern than splitting state.

### Trade-offs explicitly accepted

- **Worker thread per-fetch instead of a `BrowseSource` trait.** A future second corpus (e.g. Semantic Scholar's bulk paper search) would be the forcing function for a `SearchSource` trait. Until then, one free function is right-sized — same calculus ADR-004 used when consolidating five `Source` impls.
- **Static taxonomy const, not a runtime-loaded JSON.** arXiv updates the taxonomy on a multi-year cadence; the cost of a manual refresh once every 2-3 years is lower than the cost of a runtime parse-and-validate path on every launch.
- **Session-scoped per-category URL lists, not persisted.** D5 — persisting would re-raise D4's "what does Inbox contain on launch?" question.
- **Browse uses the feed-pane area, not a full-screen takeover.** Details pane on the right continues to render whatever was last there. A future PR 4 polish pass may rethink this for narrow widths, but PR 1 keeps the existing layout topology unchanged.

## Risks

- **R1. Variant-churn on `FeedTab::Browse` add.** `FeedTab` is `match`-exhaustive across ~12 sites in `app/mod.rs`, `feed/mod.rs`, `keys/reader.rs`. PR 1 audits the count with `rg 'match.*feed_tab'` and adds minimal arms (no-op or "use Inbox path") at every site in a single compile pass. Mitigation: tripwire O2 encodes the post-PR-1 count so subsequent contributors adding dispatch sites are reminded.
- **R2. Race between Browse fetches and bulk refresh on overlapping categories.** If the user starts browsing `cs.LG` mid-refresh, the bulk refresh's `FetchMessage::Items` and the worker's `FetchMessage::BrowseItems` both land in the same `workspace.items_store`. Mitigation: the existing URL/arXiv-ID dedup in `app/methods/process.rs:74-95` handles concurrent arrivals (position-stable replace; the second arrival wins). `BrowseModel.inflight` prevents duplicate Browse fetches for the same category.
- **R3. Rapid Enter mashing on a Category row.** Without debouncing, the user can fire N parallel fetches for the same category, exceeding arXiv's 1-req/3s envelope. Mitigation: `BrowseModel.inflight.contains(code)` short-circuits before `spawn_browse_fetch` (PR 2). No retry envelope on the worker itself — calls `arxiv::fetch` directly; the eventual move to `with_retry` is a refactor that happens to all `arxiv::*` callers at once, not Browse-specific.

## Related

- [ADR-004](ADR-004-ingestion-seam.md) — `Source` trait; explains why Browse does NOT implement `Source` (§D1 bulk-refresh-only).
- [ADR-005](ADR-005-discovery-slice.md) — sibling on-demand worker pattern (`spawn_discovery`); Browse mirrors its panic-isolation shape.
- [ADR-007](ADR-007-item-store.md) — `ItemStore` dedup invariants that Browse's merge path piggybacks on.
- `docs/CONTEXT.md` — vocabulary entries for `Group`, `Archive`, `Category`, `BrowseModel` added in PR 1.
