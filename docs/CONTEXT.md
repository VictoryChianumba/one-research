# CONTEXT — trench domain and architecture vocabulary

Last reviewed: 2026-05-18

This file is the onboarding document. It captures the terms a future contributor (or future-you) must know to navigate the codebase, and the architectural patterns every new pane is expected to follow.

When you change behaviour described here, update this file in the same commit.

---

## 1. Domain Vocabulary

| Term | Meaning |
|------|---------|
| **FeedItem** | The canonical record for one piece of content (paper, article, repo, digest, thread). Lives in `trench/src/models/`. Carries source, signal, workflow state, summary, URL. |
| **Workspace** | The owned store of `FeedItem`s plus history and tag indices. One instance, lives on `App`. Mutated by both the feed pane (workflow changes, search) and the reader pane (history append, progress). |
| **WorkflowState** | The lifecycle position of a `FeedItem`: `Inbox → Queued → DeepRead → Archived`. Drives the Library tab filters and the Inbox/Library counts. |
| **SourcePlatform** | Where a `FeedItem` came from: `ArXiv`, `HuggingFace`, `Rss`. Drives source labels and signal derivation. |
| **SignalLevel** | The relevance heuristic on a `FeedItem`: `Primary` / `Secondary` / `Tertiary`. Derived from platform + upvote count in `FeedItem::compute_signal`. |
| **Pane** | A logical TUI region with its own state and renderer. Current panes: feed, reader, chat, notes, repo viewer, settings, title bar, search row. |
| **Model** | The composition-root state owner for a pane. Models live as fields on `App` and never reference each other. Renders take `&Model`, never `&mut Model`. Introduced by slice 1 (feed). |
| **Viewport** | The `{ rows, cols }` POD passed to `Model::pre_draw` before each render. Carries no ratatui types — models stay layout-toolkit-agnostic. |
| **Action** | The input vocabulary. Keystrokes are translated into `Action` variants by `keys/`, then routed to the relevant model (or to the orchestrator for cross-pane verbs). Grows as panes migrate. |
| **Effect** | The cache-invalidation receipt emitted by mutations. Narrow vocabulary — `Effect` names a semantic event (`WorkflowStateChanged`, `SearchQueryChanged`, …), not a cache to invalidate. The cache layer translates each event into the right invalidations. |
| **Paper** | A `FeedItem` from arXiv as seen *inside the reader*. Reserved term for reader-side rendering. Outside the reader, the term is **FeedItem**. |
| **ReaderInstanceModel** | One embedded reader pane (primary or secondary), owning its tabs + active-tab index. Each `ReaderTab` wraps a `tread::Reader`. Introduced by slice 2 (`ADR-002`). |
| **ReaderPaneModel** | The composition-root model for the reader pane *as a region of the layout* — owns a primary `ReaderInstanceModel`, an optional secondary one, and the split/dual/focus state. |
| **ReaderPopupModel** | The floating popup reader (`Ldr+Enter`), a sibling Model to `ReaderPaneModel`. Lifecycle is async-load + dismissible; deliberately distinct from `ReaderInstanceModel`. |
| **ReaderContext** | The per-frame read-only context for reader renders (analogue of `FeedContext`). Carries `&Workspace`, the active theme, `Viewport`, and any pre-computed data the orchestrator owes the renderer. |
| **ReaderTarget** | Discriminator on `Action::OpenInReader` naming which reader surface receives an item: `Primary`, `Secondary`, `Popup`. |
| **NotesPaneModel** | Composition-root model for the notes dock. Owns the shared `notes::app::App` persistence backend, the primary `NotesInstanceModel` (always allocated), an optional secondary, and two visibility flags. Sibling to `ReaderPaneModel`. Introduced by slice 3 (`ADR-003`). |
| **NotesInstanceModel** | One notes context (primary or secondary). Owns tabs, active tab, `NotesMode`, and the optional `NotesContext` (paper anchoring). Per-instance — primary and secondary can be in different modes and tied to different papers. Pure content; visibility lives on the parent `NotesPaneModel`. |
| **DiscoveryModel** | Composition-root model for the discovery sub-pane. Owns the agent search bar (`query`, `query_lower`, `search_focused`), the discovered-items list + dedup indices (`items`, `url_index`, `arxiv_id_index`, `list`), the multi-turn agent session (`session`, `intent`, `forced_intent`, `force_new`), the slash-command palette (`palette`), and the background agent's `Receiver<DiscoveryMessage>`. Sibling to `FeedModel` / `ReaderPaneModel` / `NotesPaneModel`. Introduced by slice 5 (`ADR-005`); was `DiscoveryState` pre-rename. Lives on `App.discovery` after C7 PR 2 (previously nested at `App.feed.discovery`). |
| **Source** | Trait implemented by bulk-ingestion modules (`ArxivSource`, `HuggingFaceSource`, `RssSource`, `OpenReviewSource`, `CoreSource`). One `fetch(&FetchContext) -> Result<Vec<FeedItem>>` method plus `name()` + `host_group()` for the orchestrator's scheduling. Lives in `trench/src/ingestion/pipeline.rs`. Introduced by C10 (`ADR-004`). |
| **EnrichmentSource** | Sibling trait for the post-fetch enrichment phase (`SemanticScholarEnrichment`, `HuggingFaceRepoEnrichment`). One `enrich(&mut [FeedItem], &FetchContext)` method — best-effort, no `Result`. Runs single-threaded after the parallel fetch scope joins. `Send` only (not `Sync`) — see ADR-004 §D4. |
| **FetchContext** | Per-refresh context passed to every `Source::fetch` / `EnrichmentSource::enrich` call. Carries `&Config`, `&Path` (cache_dir), and convenience methods `http()` and `with_retry(policy, make)` that forward to `trench_http`. |
| **RetryPolicy** | HTTP retry envelope (`backoffs_ms` + `retriable: fn(u16) -> bool`). Lives in `crates/http`. `RetryPolicy::arxiv()` matches the deleted inline `fetch_arxiv_with_retry` constants (3000ms / 6000ms backoff, retries 429 \| 503). `RetryPolicy::none()` for one-shot reads. |
| **host group** | Scheduling tag returned by `Source::host_group()`. Sources sharing a tag run serially within one thread; different groups run in parallel. Today's groups: `"arxiv"` (arxiv + huggingface — same `export.arxiv.org` envelope), `"openreview"`, `"core"`, `"rss"`. |
| **`load_json<T>` / `save_json<T>`** | Typed envelope for the persistent-state IO pattern (`trench/src/store/mod.rs`). `load_json` reads JSON or returns `T::default()` (quarantining corrupted files via `quarantine_corrupted`); `save_json` atomically serialises via `atomic_write`, logging on failure. Each store wrapper (`cache`, `discovery_cache`, `enrichment_cache`, `history`, `session`, `tags`, plus `state.json` / `ui.json`) reduces to a 3-5 line shell around these — post-load transforms (sanitize, sort, backfill) stay in the per-module wrapper. Introduced by C8 (`ADR-006`). |
| **ItemStore** | Coordinated triple `items: Vec<FeedItem>` + `url_index: HashMap<String, usize>` + `arxiv_id_index: HashMap<String, usize>`, encapsulating the invariant that both indices map into `items`. Lives at `trench/src/data/item_store.rs`. Mutation goes through methods (`push`, `replace_at`, `sort_by`, `rebuild_indices`); reads go through methods (`find_by_url`, `find_by_arxiv_id`, `get`, `iter`). Field access is module-private. Replaces `Workspace`'s three `pub` fields after C9 PR 2. Introduced by `ADR-007`. |
| **FrameLayout** | Per-frame struct (`trench/src/ui/layout/frame_layout.rs`) carrying post-layout `Rect`s that `App::apply_frame_layout` needs to size scroll bounds and viewport caps. Empty-body PR-1 scaffolding; PR 2 wires the last `// Intentional render-time mutation` site (reader-bottom feed-mode auto-scroll). Sibling to `pre_draw_update` — pre runs before layout, `apply_frame_layout` runs after. Introduced by `ADR-008`. |
| **DebounceState** | Cluster struct (`trench/src/app/state/debounce.rs`) holding the keyboard- and mouse-scroll rate gates. Owns the read-update protocol via `try_kbd_scroll` / `try_mouse_scroll`. Pilot cluster for `ADR-009`'s App-field grouping; replaces 4 flat `App` fields (`last_scroll_time` + 3 siblings). |
| **LeaderState** | Cluster struct (`trench/src/app/state/leader.rs`) holding the Ctrl+T leader-key gate. Four-method protocol: `activate`, `deactivate`, `expire_if_timed_out`, `is_active` — read and expire are deliberately separate so footer/log reads stay pure. ADR-009 cluster #2; replaces 3 flat `App` fields (`leader_active`, `leader_activated_at`, `leader_timeout_ms`). |
| **ReaderBottomState** | Cluster struct (`trench/src/app/state/reader_bottom.rs`) holding the State-3 reader-bottom drawer's UI state: `open`, `focused`, `details` (view mode), `feed_popup_selected`, `scroll`. Public fields, no protocol methods — matches `ReaderPaneModel`'s convention (ADR-002). ADR-009 cluster #3; replaces 5 flat `App` fields (`reader_bottom_open` + 4 siblings). |
| **ViewFlags** | Cluster struct (`trench/src/app/state/view_flags.rs`) holding three transient popup visibility flags: `tab_window_prompt_active`, `abstract_popup_active`, `narrow_feed_details_open`. Public fields, no protocol methods. ADR-009 cluster #4; replaces 3 flat `App` fields. The audit's original 5-field grouping reclassified the two `fulltext_*` routing flags into the future `AsyncJobs` cluster since they're consumed by the async-fetch resolution path, not popup state. |
| **RenderCaches** | Cluster struct (`trench/src/app/state/render_caches.rs`) holding the 5 RefCell-backed memoization caches consulted by the render layer (`visible`, `counts`, `filter_source_names`, `filter_summary`, `filtered_history`) plus the [`Effect`] observer that knows which caches each semantic event invalidates. `App::route_effects` delegates to `RenderCaches::observe`. ADR-009 cluster #5; replaces 5 flat `*_cache` `App` fields + the 6 invalidator methods that lived in `caches.rs`. PR 5 adds the first behavioural tests for the effect→cache routing — one witness per `Effect` variant. |
| **AsyncJobs** | Cluster struct (`trench/src/app/state/async_jobs.rs`) holding every in-flight background fetch grouped by job class — bulk fetch (`fetch_rx`, `is_loading`, `loading_sources`, `loaded_sources`, `spinner_frame`), fulltext (`fulltext_rx`, `fulltext_loading`, `pending_fulltext_context`), tread (`tread_fetch_rx`, `pending_tread_fetch`), repo (`repo_fetch_rx`), plus the two `fulltext_*` routing flags reclassified from `ViewFlags` per ADR-009 PR 4. Public fields, no protocol methods yet — extraction of `start_*`/`finish_*` per job class is future work per ADR-009's lazy rollout. Cluster #6 of ADR-009; replaces 13 scattered flat `App` fields and closes the ADR's punch list. |

### Term hygiene

The audit found inconsistencies (`FeedItem` vs `Item` vs `Paper` vs `DiscoveryItem`). The rules:

- `FeedItem` is the universal name in the feed pane, ingestion, store, workspace.
- `Paper` is only used inside the reader for arXiv-rendered content.
- `Item` (bare) and `DiscoveryItem` are legacy — prefer `FeedItem` in new code.

---

## 2. Architectural Patterns

Every pane that has been refactored under ADR-001 (render purification) follows these rules. Slices migrate panes one at a time.

### Composition root

- A pane's `Model` is a field on `App` (e.g. `App.feed: FeedModel`).
- Models never reference each other. `FeedModel` does not know `ReaderModel` exists.
- Cross-pane communication is through `Action` only.

### Render is read-only

- Renders take `&Model + &Context`. They never receive `&mut Model` or `&mut App`.
- Any mutation that needs layout-derived values (viewport size, auto-scroll, width-aware wrapping) lives in `Model::pre_draw(viewport)`, which runs once per frame before render.
- The `// intentional render-time mutation` comments are a regression marker; their disappearance is how we know a slice is done.

### Input → Action → mutation

- `keys/` translates `KeyEvent` → `Action`.
- The orchestrator (currently a `match` on `App`) routes each `Action` to the right model method or handles it directly for cross-pane verbs.
- Model methods return `Vec<Effect>`. The orchestrator drains effects into the cache-invalidation observer.

### Workspace mutation — W3 hybrid rule

- **State-local gestures** (mark read, queue, toggle tag): model methods take `&mut Workspace` and mutate it directly. Example: `FeedModel::mark_read(&mut self, w: &mut Workspace) -> Vec<Effect>`.
- **Cross-pane or shared-write gestures** (open in reader, append history): model emits an `Action` variant; the orchestrator owns the mutation.
- The classification table for the feed pane lives in ADR-001.

### Sub-models

When a pane has substantial internal state that deserves its own seam (e.g. Discovery inside the feed pane, Voice next to the reader pane), it becomes a sub-model owned by the parent. Sub-models follow the same rules.

---

## 3. Slice Status

The refactor is incremental. Lazy rollout — a pane is refactored when a feature pulls us into it, not on a schedule.

| Pane | Status | Trigger / next step |
|------|--------|---------------------|
| **Feed** | Slice 1 accepted (2026-05-16) | PRs 1, 2, 3, 4a, 4b, 4c, 6 landed. PR 5 (`Action::OpenInReader`) deferred to slice 2 where it's actually used. See ADR-001. |
| **Reader** | Slice 2 accepted (2026-05-16) | ADR-002. 6 PRs landed. PR 5 scoped down from "full signature flip" to "pre_draw landing" to avoid the per-frame allocation regression slice 1 PR 4c introduced — full flip remains available if a testability driver forces it. |
| **Popup reader** | Slice 2 accepted | Folded into ADR-002 as `ReaderPopupModel`. |
| **Voice** | Pending | Separate slice after slice 2. `VoiceModel` on `App`. Trigger: ElevenLabs credits + feature ask. |
| **Notes** | Slice 3 accepted (2026-05-18) | ADR-003. 4 PRs landed: PR 1 skeletons + ADR + vocabulary, PR 2 state migration (11 fields → `App.notes`), PR 3 gesture methods on `NotesPaneModel`, PR 4 tripwires I8-I11 in `scripts/check-render-purification.sh`. |
| **Ingestion seam** | C10 accepted (2026-05-18) | ADR-004. 3 PRs landed: Source/EnrichmentSource traits + FetchContext + RetryPolicy (PR 1); 5 Source impls + 2 EnrichmentSource impls + orchestrator + `fetch_arxiv_with_retry` deleted (PR 2); J1-J5 tripwires + ADR Accepted (PR 3). |
| **Store seam** | C8 accepted (2026-05-18) | ADR-006. 3 PRs landed: PR 1 (`load_json<T>` / `save_json<T>` + ADR + 3 smoke tests), PR 2 (8-site migration, net −192 LOC), PR 3 (L1-L4 tripwires in `scripts/check-store-seam.sh` + ci.sh wired + ADR Accepted). Trigger: audit candidate C8 — 7 store files repeated the same 5-step load shape. |
| **ItemStore** | C9 accepted (2026-05-18) | ADR-007. 3 PRs landed: PR 1 (`ItemStore` skeleton + 8 smoke tests + ADR + vocabulary), PR 2 (Workspace migration, +252 / −216 across 13 files), PR 3 (M1-M4 tripwires in `scripts/check-item-store.sh` + ci.sh wired + ADR Accepted). Trigger: audit candidate C9 — index invariant maintained by convention across 5 mutation paths. `DiscoveryModel`'s parallel triple stays raw (§S3). |
| **FrameLayout** | C6 accepted (2026-05-18) | ADR-008. 3 PRs landed: PR 1 (`FrameLayout` + empty `apply_frame_layout` + 3 smoke tests + ADR), PR 2 (wired the hook, marker block deleted, 5 files +112 / −30), PR 3 (N1-N3 tripwires in `scripts/check-frame-layout.sh` + ci.sh wired + ADR Accepted). Trigger: audit candidate C6 — the marker block at `reader.rs:424-428` was the forcing function. |
| **Discovery** | Slice 5 accepted (2026-05-18) | ADR-005. 4 PRs landed: PR 1 (rename `DiscoveryState`→`DiscoveryModel` + ADR + smoke tests), PR 2 (field migration + 8 method signatures threaded `&mut DiscoveryModel`), PR 3 (gesture methods on `DiscoveryModel` + 4 App wrappers deleted), PR 4 (K1-K4 tripwires in `scripts/check-render-purification.sh` + ADR Accepted). Trigger: audit candidate C7 + size threshold (~1,000 LOC of agent code crossed the "lift when grown enough" line). |
| **Chat** | Legacy | Lazy. No pressure to refactor. |
| **Repo Viewer** | Legacy | Lazy. |
| **Settings overlay** | Legacy | Lazy. Already partly migrated to `Action::DismissTopModal` / `Action::OpenSettings`. |
| **Title bar / search row** | Stateless | No model needed. Reads from app counters. |

---

## Related

- `docs/adr/ADR-001-render-purification.md` — the parent per-pane refactor decision (slice 1, feed).
- `docs/adr/ADR-002-reader-slice.md` — slice 2 reader-pane extension.
- `docs/adr/ADR-003-notes-slice.md` — slice 3 notes-dock consolidation (Accepted 2026-05-18; 4 PRs landed).
- `docs/adr/ADR-004-ingestion-seam.md` — C10 `Source` + `EnrichmentSource` + `FetchContext` (Accepted 2026-05-18; 3 PRs landed).
- `docs/adr/ADR-005-discovery-slice.md` — slice 5 discovery-pane lift (Accepted 2026-05-18; 4 PRs landed).
- `docs/adr/ADR-006-store-seam.md` — C8 `load_json<T>` + `save_json<T>` (Accepted 2026-05-18; 3 PRs landed).
- `docs/adr/ADR-007-item-store.md` — C9 `ItemStore` encapsulating Workspace's item triple (Accepted 2026-05-18; 3 PRs landed).
- `docs/adr/ADR-008-frame-layout.md` — C6 `FrameLayout` + `apply_frame_layout` hook (Accepted 2026-05-18; 3 PRs landed).
- `docs/adr/ADR-009-app-field-grouping.md` — N5 cluster flat `App` fields into named state structs (Accepted 2026-05-19; 6 PRs landed: `DebounceState`, `LeaderState`, `ReaderBottomState`, `ViewFlags`, `RenderCaches`, `AsyncJobs`). App shrank 108 → 80 fields.
- `docs/audits/` — periodic architectural audits with letter-graded scorecards. Latest: `2026-05-19-architectural-audit.md` (B−, up from C+ after the ADR-009 series + N1/N2/N8 witnesses landed). Run `/improve-codebase-architecture` to produce a new one.
- `CLAUDE.md` — project-wide rules. CONTEXT.md is the *language* layer above those.
