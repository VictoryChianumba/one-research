# CLAUDE.md — 12-rule template

These rules apply to every task in this project unless explicitly overridden.
Bias: caution over speed on non-trivial work. Use judgment on trivial tasks.

## Rule 1 — Think Before Coding
State assumptions explicitly. If uncertain, ask rather than guess.
Present multiple interpretations when ambiguity exists.
Push back when a simpler approach exists.
Stop when confused. Name what's unclear.

## Rule 2 — Simplicity First
Minimum code that solves the problem. Nothing speculative.
No features beyond what was asked. No abstractions for single-use code.
Test: would a senior engineer say this is overcomplicated? If yes, simplify.

## Rule 3 — Surgical Changes
Touch only what you must. Clean up only your own mess.
Don't "improve" adjacent code, comments, or formatting.
Don't refactor what isn't broken. Match existing style.

## Rule 4 — Goal-Driven Execution
Define success criteria. Loop until verified.
Don't follow steps. Define success and iterate.
Strong success criteria let you loop independently.

## Rule 5 — Use the model only for judgment calls
Use me for: classification, drafting, summarization, extraction.
Do NOT use me for: routing, retries, deterministic transforms.
If code can answer, code answers.

## Rule 6 — Token budgets are not advisory
Per-task: 4,000 tokens. Per-session: 30,000 tokens.
If approaching budget, summarize and start fresh.
Surface the breach. Do not silently overrun.

## Rule 7 — Surface conflicts, don't average them
If two patterns contradict, pick one (more recent / more tested).
Explain why. Flag the other for cleanup.
Don't blend conflicting patterns.

## Rule 8 — Read before you write
Before adding code, read exports, immediate callers, shared utilities.
"Looks orthogonal" is dangerous. If unsure why code is structured a way, ask.

## Rule 9 — Tests verify intent, not just behavior
Tests must encode WHY behavior matters, not just WHAT it does.
A test that can't fail when business logic changes is wrong.

## Rule 10 — Checkpoint after every significant step
Summarize what was done, what's verified, what's left.
Don't continue from a state you can't describe back.
If you lose track, stop and restate.

## Rule 11 — Match the codebase's conventions, even if you disagree
Conformance > taste inside the codebase.
If you genuinely think a convention is harmful, surface it. Don't fork silently.

## Rule 12 — Fail loud
"Completed" is wrong if anything was skipped silently.
"Tests pass" is wrong if any were skipped.
Default to surfacing uncertainty, not hiding it.

# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Commands

### /checkpoint
Saves current progress with a commit message.
Usage: /checkpoint "description of what was done"
Command: git add -A && git commit -m "$1"

```sh
# Build and run (primary development workflow)
cargo run -p trench --release

# Build all crates
cargo build --release

# Run all tests
cargo test

# Run tests for a specific crate
cargo test -p trench

# Run a single test
cargo test -p trench test_name

# Check formatting
cargo fmt --check

# Lint
cargo clippy --workspace --all-targets

# Build trench release binary
cargo build -p trench --release
```

Rust edition: 2024, MSRV: 1.88. The `ci.sh` script uses the nightly toolchain for `cargo fix`, `cargo udeps`, and `cargo audit`.

## Workspace Structure

```
trench/            → main binary: AI research feed aggregator TUI
crates/http        → shared HTTP client + RetryPolicy + with_retry
crates/notes       → notes-pane backend
crates/chat        → chat-pane backend
crates/ui-theme    → theme system
```

The reader logic now lives in the sibling `tread` repo at
`../../tread/crates/tread`, consumed through the path dependency in
`trench/Cargo.toml`. Reader internals belong to tread's docs.

## Workspace-wide Clippy Allowances

`needless_return`, `unused_imports`, `implicit_saturating_sub`, `single_component_path_imports` are allowed workspace-wide.

## Reader (tread) — out of repo

The reader boundary in this repo is the tread integration: `tread::Reader`,
`tread::PaperData`, `tread::ImageState`, and `tread::BurstTracker` are used
from trench, but reader rendering and input internals are documented in tread.

## trench Architecture

A separate TUI binary (`trench/src/main.rs`) that aggregates AI research feeds. No async — uses `std::sync::mpsc` and `reqwest::blocking` throughout.

### Data model (`src/models/`)

`FeedItem` is the central type: `id`, `title`, `source_platform`, `content_type`, `domain_tags`, `signal` (Primary/Secondary/Tertiary), `published_at`, `authors`, `summary_short`, `workflow_state` (Inbox/Queued/DeepRead/Archived), `url`, `upvote_count`. `upvote_count` has `#[serde(default)]` for cache backward-compatibility.

`FeedItem::compute_signal()` derives signal from platform and upvote count. `map_arxiv_category()` and `detect_subtopics()` live in `src/models/categories.rs`.

### Ingestion pipeline (`src/ingestion/`)

The background refresh builds `Source` and `EnrichmentSource` registries:

1. Bulk `Source`s: arXiv, HuggingFace, OpenReview, CORE when configured, built-in RSS feeds, and custom RSS feeds.
2. Sources with the same `host_group()` run serially in one scoped thread; different host groups run in parallel.
3. Post-fetch `EnrichmentSource`s run sequentially over the accumulated items: Semantic Scholar when configured, then HuggingFace repo enrichment.

Each source sends `FetchMessage::Items(Vec<FeedItem>)` plus completion/error messages over mpsc. After enrichments, the worker sends the enriched batch and `AllComplete`.

### App state and merge logic (`src/app.rs`)

`App::process_incoming()` drains the channel each frame (non-blocking `try_recv` loop):
- **URL dedup**: overwrites cached item with fresh data; workflow state comes from `persisted_states` (keyed by URL).
- **ArXiv ID dedup**: collapses HF and arXiv entries for the same paper — arXiv entry wins. The HF entry's `workflow_state` is preserved onto the arXiv entry when replacing.

Items are sorted by `published_at` descending after each batch. Cache is written to `~/.config/trench/cache.json` immediately.

### Store (`src/store/`)

- `store::load()` / `store::save()` — workflow states, keyed by URL, at `~/.config/trench/state.json`.
- `store::cache` — full `Vec<FeedItem>` cache, loaded at startup so the TUI is populated before network fetches complete.
- `store::enrichment_cache` — Semantic Scholar results, 7-day TTL via Julian Day Number arithmetic (no chrono).

### UI (`src/ui/layout.rs`)

Single `draw(frame, app)` entry point. Feed view: tab bar → search row → item table + details panel → status bar with braille spinner during loading. Reader view: full-screen content with header bar. Details panel shows upvote count for HuggingFace items.

### Visual Design Language

Tentative should use a quiet research-interface design language:
- Shared frames and split containers over independently boxed widgets.
- Muted slate borders and separators.
- Section titles embedded into divider/header lines.
- Baby blue for primary accent/actionable content.
- Darker luminous blue for section and column headers.
- Selection should use row/background treatment, not bright borders.
- Footers should be calm command text.
- Repo viewer, chat, and notes should feel structurally consistent with feed/details.
- Reader mode is a separate long-form reading design pass.
