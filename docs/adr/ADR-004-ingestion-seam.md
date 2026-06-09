# ADR-004 — Ingestion seam (Source + EnrichmentSource + FetchContext)

- **Status:** Accepted (2026-05-18). All three PRs landed: PR 1 (skeletons + this ADR + `RetryPolicy` + vocabulary), PR 2 (5 `Source` impls + 2 `EnrichmentSource` impls + registry + `spawn_fetch` rewrite + `fetch_arxiv_with_retry` deleted), PR 3 (tripwires J1-J5 in `scripts/check-ingestion-seam.sh`, wired into `ci.sh`).
- **Date:** 2026-05-18
- **Owner:** Victory Chianumba
- **Supersedes:** none
- **Relates to:** Stands alongside [ADR-001](ADR-001-render-purification.md) / [ADR-002](ADR-002-reader-slice.md) / [ADR-003](ADR-003-notes-slice.md) (per-pane composition-root refactors) — same shipping discipline (ADR + cadence + tripwire), different layer of the system. The composition-root ADRs address how *rendered state* flows; this ADR addresses how *ingested data* enters.

## Goal

Introduce a uniform contract for one-research's bulk-refresh ingestion path. Today six modules (`arxiv`, `huggingface`, `rss`, `openreview`, `core`, plus the enrichment-not-source `semantic_scholar`) share a channel type (`FetchMessage`) but no behavior contract. After this ADR:

- A `Source` trait expresses *what every bulk-ingestion source does* (one `fetch(&FetchContext) -> Result<Vec<FeedItem>>` call plus `name()` + `host_group()` for the orchestrator's scheduling).
- A sibling `EnrichmentSource` trait expresses the structurally different enrichment phase (`enrich(&mut [FeedItem], &FetchContext)` — best-effort, no `Result`, post-fetch).
- A `FetchContext` carries the shared infrastructure each source needs: HTTP client access, retry policy support, config, cache directory.
- A `RetryPolicy` + `with_retry(client, policy, make)` in `crates/http` generalises the inline `fetch_arxiv_with_retry` helper into a reusable seam.

## Context

The 2026-05-18 architectural audit (`docs/audits/2026-05-18-architectural-audit.md`) named candidate **C10** as *"the one architecture investment worth its weight right now"* after the backlog-hygiene items (C11/C14/C15) and Slice 2 / Slice 3 closure. All four prerequisites have shipped.

The forcing example is commit `5491470` (2 days before this ADR), which introduced `fetch_arxiv_with_retry` *inline* inside `huggingface.rs` as a tactical fix for a 429 incident. The audit's framing:

> *Educationally:* this is the most common shape of architectural debt — a tactical fix that should have been a seam, written under pressure where the seam wasn't visible.

The same retry pattern will be needed again the next time arxiv / openreview / RSS hits 429 or 503. Without C10, the next incident becomes another 29-line inline helper. With C10, it becomes one line at the call site: `ctx.with_retry(&RetryPolicy::arxiv(), |c| c.get(url))`.

The audit also called out a second seam issue: `semantic_scholar::enrich(items: &mut Vec<FeedItem>, ...)` is *structurally* enrichment (mutates in place, no `Result`, runs post-fetch) but lives alongside the fetch modules. The role confusion shows up in `services/ingestion.rs:262-278` where s2 enrichment is special-cased outside the source-runner abstraction. The split into two traits resolves this.

Source count has grown 50% since the prior audit on 2026-05-16 (added: `openreview`, `core`, custom RSS feeds). The seam gets harder to add per new source. The cost of *not* doing C10 is no longer hypothetical.

## Decision

### Scope: bulk-ingestion path only

C10 covers the modules invoked from `services::ingestion::spawn_fetch`:

- `Source` impls: `ArxivSource`, `HuggingFaceSource`, `RssSource`, `OpenReviewSource`, `CoreSource`.
- `EnrichmentSource` impls: `SemanticScholarEnrichment`, `HuggingFaceRepoEnrichment`.
- Orchestrator rewrite in `services/ingestion.rs` around `Vec<Box<dyn Source>>` + `Vec<Box<dyn EnrichmentSource>>` built by a small `ingestion::registry` module.
- `crates/http` grows `RetryPolicy` + `with_retry`.

**Explicitly out of scope** — items that look related but stay free-functions:

- `arxiv::search_query` and `arxiv::fetch_by_ids` — query-shaped, called by the discovery agent and HF's abstract-backfill batch, not the bulk-refresh loop. Different contract; stays as free fns.
- `crossref::lookup(query) -> Option<FeedItem>` — dead in bulk path, alive in discovery. Stays untouched.
- `C8` (`Store<T>` trait for the persistence layer) — separate refactor, separate ADR if/when forced.
- `ContentType` / `SourcePlatform` enum cleanup — touching them would balloon C10.
- OpenReview venue parameterization in config — venues lift to construction-time constants in `OpenReviewSource::default()`; making them user-configurable is a separate feature.
- Backgrounding the enrichment phase — today it blocks `spawn_fetch`'s background thread. Performance-tuning, not seam-cutting.

### Trigger: proactive cleanup with a concrete forcing example

ADR-001 D2 prescribed lazy rollout — refactor when a feature pulls. This ADR follows that rule, with `5491470` (the inline retry helper) as the documented pull. Unlike ADR-003's audit-grade-alone justification, here the next retry incident *will* duplicate `fetch_arxiv_with_retry` if the seam isn't built first. Recording the justification strength here so future readers don't over- or under-claim it.

### Decisions

#### D1. Two traits, not one — same module

The audit phrased C10 as *"a single `Source` trait."* This ADR departs: two traits live in `one-research/src/ingestion/pipeline.rs`.

```rust
pub trait Source: Send + Sync {
    fn name(&self) -> &str;          // for logs + FetchMessage::SourceComplete
    fn host_group(&self) -> &str;    // for orchestrator scheduling
    fn fetch(&self, ctx: &FetchContext) -> Result<Vec<FeedItem>, String>;
}

pub trait EnrichmentSource: Send {
    fn name(&self) -> &str;
    fn enrich(&self, items: &mut [FeedItem], ctx: &FetchContext);
}
```

**Why split:** the two traits have different inputs (`fetch` returns items; `enrich` takes and mutates a slice), different output semantics (`Result` vs best-effort), and different scheduling (parallel inside `thread::scope` vs sequential post-scope). A single trait would force `Option<Vec<FeedItem>>` returns or a marker enum and the orchestrator would branch on it anyway. Two traits make the orchestrator's two phases self-documenting.

**Why same module:** both traits live in `pipeline.rs` so the full contract is readable in one sitting.

#### D2. `FetchContext` carries shared infrastructure

```rust
pub struct FetchContext<'a> {
    pub config: &'a Config,
    pub cache_dir: &'a Path,
}

impl FetchContext<'_> {
    pub fn http(&self) -> &'static reqwest::blocking::Client { crate::http::client() }
    pub fn with_retry<F>(&self, policy: &RetryPolicy, make: F) -> Result<Response, String>
    where F: Fn(&reqwest::blocking::Client) -> reqwest::blocking::RequestBuilder { … }
}
```

- The trait methods take `&FetchContext`. The trait itself has no lifetime parameter — `'a` is on the struct and the method binding only.
- `http()` re-exports the existing `crates/http::client()` singleton. Sources never reach `crate::http::client()` directly post-migration (tripwire J4 enforces this).
- `with_retry` is a convenience that forwards to the `crates/http::with_retry` free function. Closure form (rather than passing a `RequestBuilder`) because `.send()` consumes the builder — each retry attempt needs to construct a fresh one.

#### D3. `RetryPolicy` lives in `crates/http`

```rust
pub struct RetryPolicy {
    pub backoffs_ms: Vec<u64>,
    pub retriable: fn(u16) -> bool,
}
impl RetryPolicy {
    pub fn arxiv() -> Self { Self { backoffs_ms: vec![3_000, 6_000], retriable: |c| c == 429 || c == 503 } }
    pub fn none() -> Self  { Self { backoffs_ms: vec![],            retriable: |_| false } }
}
```

The constants in `arxiv()` are byte-equivalent to the deleted `fetch_arxiv_with_retry`'s `BACKOFFS_MS: &[3_000, 6_000]` and 429|503 retriable predicate. A test in `crates/http` asserts this so future changes can't silently shift the envelope.

#### D4. Asymmetric `Send + Sync` bounds (the !Sync compromise on EnrichmentSource)

`Source: Send + Sync` because the orchestrator's `std::thread::scope` captures source references across threads (`spawn(move || src.fetch(&ctx))` requires `&Source: Send` which requires `Source: Sync`).

`EnrichmentSource: Send` only — **no `Sync` bound**. Reason: the enrichment impls own their per-source caches (`SemanticScholarEnrichment { cache: RefCell<EnrichmentCache> }` and `HuggingFaceRepoEnrichment { cache: RefCell<HfRepoCache> }`), and `RefCell<T>: !Sync` by design. The enrichment phase runs single-threaded post-scope in the orchestrator, so `Sync` is provably unnecessary. Recording the asymmetry here so a future reader doesn't try to "fix" it by removing the `RefCell` (which would force threading `&mut EnrichmentContext` through every call site for no real benefit).

#### D5. Source impls are tagged structs

| Struct | Fields | `name()` | `host_group()` |
|---|---|---|---|
| `ArxivSource` | `categories: Vec<String>` | `"arxiv"` | `"arxiv"` |
| `HuggingFaceSource` | — | `"huggingface"` | `"arxiv"` (same envelope as arxiv) |
| `RssSource` | `name, url, platform, content_type` | `&self.name` | `"rss"` |
| `OpenReviewSource` | `venues: Vec<String>` (lifted from hardcode) | `"openreview"` | `"openreview"` |
| `CoreSource` | — (reads `ctx.config.core_api_key`) | `"core"` | `"core"` |

Each impl wraps its existing parser as-is. `ArxivSource::fetch` calls `arxiv::parse_atom`; `RssSource::fetch` calls `rss::parse_feed`. Only the IO entry point moves behind the trait; the parsing free functions stay (they're pure, well-tested, and used from non-trait paths too).

#### D6. Orchestrator owns gating; sources are unaware

Today's enabled-source map check and api-key gating (`services/ingestion.rs:74-76`, `:184-194`, `:258-261`) stay at the orchestrator boundary, encoded in a small `ingestion::registry` module:

```rust
pub fn build_sources(config: &Config) -> Vec<Box<dyn Source>> { … }
pub fn build_enrichments(config: &Config) -> Vec<Box<dyn EnrichmentSource>> { … }
```

The registry decides "does this source exist this run?" — sources don't gate themselves. This preserves the existing log distinctions ("skipped — disabled", "skipped — no API key") without polluting source impls.

#### D7. 3-PR cadence

| # | PR | Behavior change |
|---|---|---|
| 1 | ADR-004 + empty `Source`/`EnrichmentSource` traits in `ingestion/pipeline.rs` + `FetchContext` skeleton + `RetryPolicy` in `crates/http` + `CONTEXT.md` vocabulary + smoke tests. | None |
| 2 | All 5 `Source` impls + 2 `EnrichmentSource` impls + orchestrator rewrite + delete `fetch_arxiv_with_retry`. Registry builds the `Vec<Box<dyn …>>` with existing gating. | None — invariant: byte-equivalent network behavior (same URLs, same headers, same retry constants) |
| 3 | `scripts/check-ingestion-seam.sh` with J1-J5 tripwires; wired into `ci.sh`; ADR-004 → Accepted. | None |

Shorter than slices 1/2 (6 PRs) and one PR shorter than slice 3 (4 PRs) because:

- No state migration (sources keep their own internal shapes; only the entry seam changes).
- No gesture extraction (ingestion has no user-facing gestures).
- Per-source change is *more* invasive than a field rename, so splitting fetch/enrich migrations into separate PRs would leave a transient half-trait orchestrator that's awkward to review.

If reviewer feedback during PR 2 wants a split, fall back to slice-3's 4-PR shape (fetch sources first, then enrichments). Not a redesign — a split along a clean line.

### Invariants for PR 3 tripwire

`scripts/check-ingestion-seam.sh` (sibling to `scripts/check-render-purification.sh`):

- **J1** Exactly five `impl Source for` blocks under `one-research/src/ingestion/` — one per bulk-ingestion module (Arxiv, HuggingFace, Rss, OpenReview, Core). Off-by-one in either direction is a regression.
- **J2** Exactly two `impl EnrichmentSource for` blocks under `one-research/src/ingestion/` — `SemanticScholarEnrichment` and `HuggingFaceRepoEnrichment`.
- **J3** No `fn fetch_arxiv_with_retry` declaration anywhere in `one-research/src/` or `crates/`. The PR-2 deletion stays deleted; docstring references in `///` comments are skipped because they're historical pointers, not symbols.
- **J4 (scoped)** Inside `impl Source for X { fn fetch ... }` and `impl EnrichmentSource for X { fn enrich ... }` bodies, no direct `crate::http::client()` or `super::http::client()` call — must reach HTTP through `FetchContext::http()` / `FetchContext::with_retry`. **Scoped to trait-impl bodies, not transitively** — legacy free functions (e.g., `arxiv::fetch`, called from the discovery agent which has no `FetchContext`) retain direct `crate::http::client()` calls because the alternative would balloon C10's scope into the discovery layer. The seam lives at the trait-impl boundary; bypass detection is structural, not transitive.
- **J5** This file's PR cadence table mentions every PR (analogous to I2/I6 ADR cadence checks).

Both scripts wire into `ci.sh` alongside each other.

## Consequences

### Positive

- `fetch_arxiv_with_retry` (29 lines, one-source) collapses to `ctx.with_retry(&RetryPolicy::arxiv(), …)` (one line). Future retry needs in any source are a one-line addition, not a 29-line copy.
- The enrichment-vs-source role confusion that complicated `services/ingestion.rs:250-278` resolves into a structural distinction between two traits.
- Adding a future source becomes: define the struct, impl `Source`, register it. The orchestrator's host-grouping + scheduling logic stays untouched.
- The audit's J4 tripwire forces ingestion to use the shared HTTP infrastructure — `crates/http`'s hardened defaults (15s timeout, 2-redirect limit, uniform user-agent) can't be bypassed by a future source.
- Splits enrichment into a place where backgrounding (a deferred performance follow-up) becomes a small change to the orchestrator's enrichment loop rather than a refactor.

### Negative

- ~+500 / -100 LOC in PR 2 — atomic migration is one large reviewer load. Mitigation: detailed plan upfront, fallback to slice-3's split if requested.
- Adds a `Box<dyn Source>` indirection (dyn-dispatch + heap alloc per source registration). Cost is ~7 vtable calls per refresh against 15-second HTTP timeouts; documented in case anyone asks.
- The `EnrichmentSource: Send` without `Sync` asymmetry is a real surprise compared to `Source: Send + Sync`. Documented in D4; tripwire J2 doesn't enforce bounds (they're a compile-time concern, not a regex one).

### Trade-offs explicitly accepted

- **Two traits over one** — see D1.
- **Registry module beyond the audit's phrasing** — see D6; avoids polluting source impls with gating logic.
- **`Send` without `Sync` on EnrichmentSource** — see D4; lets enrichments own their caches via `RefCell` without forcing a single-threaded-anyway phase to look multi-threaded.
- **Free functions stay for query-shaped APIs** (`arxiv::search_query`, `crossref::lookup`) — the trait covers the bulk-refresh path only.
- **3 PRs not 4** — see D7; per-source change is structurally atomic.

## Risks

1. **Hidden `Send`/`Sync` blockers in source-internal state.** All planned source impls satisfy `Send + Sync` trivially (no `Rc`, no `RefCell`, no thread-local state). Mitigation: PR 2's first sub-task is to declare the impls and let the compiler surface any blocker.

2. **Byte-equivalent network behavior is the load-bearing PR 2 invariant.** A renamed query parameter, a missed `User-Agent` header, or a different request body shape will look like a refactor but change observed behavior. Mitigation: PR 2 verification step diffs ingestion logs (item counts per source, pipeline total time) against a pre-C10 baseline captured before the PR. ≤5% pipeline-total drift, exact item-count match per source.

3. **Atomic PR 2 review burden.** Mitigation as recorded in D7 — split available if requested, not a redesign.

4. **OpenReview's hardcoded venues lift might inadvertently change scheduling.** `OpenReviewSource::default()` carries the same three venues today. Tested by item-count comparison in PR 2 verification.

5. **`SemanticScholarEnrichment`'s cache load timing.** Today the cache is loaded inside `spawn_fetch` immediately before the enrich call. Post-C10 it loads in `SemanticScholarEnrichment::new()` at registry-build time. Same file, same TTL contract; the only difference is "when in the spawn_fetch thread" — no observable difference.

## Related

- [ADR-001](ADR-001-render-purification.md) — per-pane composition root (feed slice). Same shipping discipline (ADR + PR cadence + tripwire script).
- [ADR-002](ADR-002-reader-slice.md), [ADR-003](ADR-003-notes-slice.md) — sibling slices applying the same pattern to reader and notes.
- `docs/audits/2026-05-18-architectural-audit.md` — candidate **C10** + **C13** (the `http::with_retry` seam folded in here).
- `docs/CONTEXT.md` — vocabulary section updated in PR 1 with `Source`, `EnrichmentSource`, `FetchContext`, `RetryPolicy`, *host group*.
- `crates/http/src/lib.rs` — home for `RetryPolicy` and `with_retry`.
- `scripts/check-render-purification.sh` — template for the sibling `scripts/check-ingestion-seam.sh`.
