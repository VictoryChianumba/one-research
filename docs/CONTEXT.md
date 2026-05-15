# CONTEXT — trench domain and architecture vocabulary

Last reviewed: 2026-05-15

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
| **Feed** | Slice 1 in progress | See ADR-001. 6 PRs. Foundations (PR 1) ships the empty `FeedModel` + `Viewport`. |
| **Reader** | Pending (slice 2) | Triggered when image rendering needs to land. Expected ~4 PRs. Will introduce `ReaderPaneModel` (primary + optional secondary), `ReaderInstanceModel`, `VoiceModel`. |
| **Voice** | Pending | Lands alongside or shortly after slice 2. Separate `VoiceModel` on `App`. |
| **Chat** | Legacy | Lazy. No pressure to refactor. |
| **Notes** | Legacy | Lazy. May ride along with reader slice since notes dock alongside the reader. |
| **Repo Viewer** | Legacy | Lazy. |
| **Settings overlay** | Legacy | Lazy. Already partly migrated to `Action::DismissTopModal` / `Action::OpenSettings`. |
| **Title bar / search row** | Stateless | No model needed. Reads from app counters. |

---

## Related

- `docs/adr/ADR-001-render-purification.md` — the decision and rationale behind the per-pane refactor.
- `CLAUDE.md` — project-wide rules. CONTEXT.md is the *language* layer above those.
