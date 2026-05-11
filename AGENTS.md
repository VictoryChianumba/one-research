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
cargo run -p hygg-reader --release -- test-data/pdf/pdfreference1.7old.pdf

# Build all crates
cargo build --release

# Run tests
cargo test

# Run tests for a specific crate
cargo test -p cli-text-reader

# Run a single test
cargo test -p cli-text-reader test_name

# Check formatting
cargo fmt --check

# Lint
cargo clippy

# Build trench separately
cargo build -p trench --release
```

Rust edition: 2024, MSRV: 1.88. The `ci.sh` script uses the nightly toolchain for `cargo fix`, `cargo udeps`, and `cargo audit`.

## Workspace Structure

```
hygg-reader/       → main reader binary: arg parsing, doc conversion → cli-text-reader
cli-text-reader/   → the TUI reader (all editor logic lives here)
cli-pdf-to-text/   → PDF → plain text conversion
cli-epub-to-text/  → EPUB → plain text conversion
cli-justify/       → text justification/wrapping
hygg-shared/       → shared utilities
redirect-stderr/   → stderr redirection helper
trench/            → separate binary: AI research feed aggregator TUI
```

**hygg-reader pipeline**: arg parsing → OCR (optional, via `ocrmypdf`) → format conversion (PDF/EPUB/pandoc) → `cli-justify::justify()` → `cli-text-reader::run_cli_text_reader()`.

## Workspace-wide Clippy Allowances

`needless_return`, `unused_imports`, `implicit_saturating_sub`, `single_component_path_imports` are allowed workspace-wide.

## cli-text-reader Architecture

This crate is the core. Everything is implemented as `impl Editor` blocks spread across many files. The `Editor` struct is defined in `src/core_state.rs` and re-exported via `src/editor/core.rs`.

**Main loop** (`src/editor/display_loop.rs`): polls voice status, handles crossterm events, triggers redraws. Uses `needs_redraw` flag — call `self.mark_dirty()` to request a redraw.

**Event routing** (`src/editor/event_handler.rs` → `src/editor/normal_mode.rs`): `handle_event` dispatches to mode-specific handlers. Normal mode calls handlers in priority order: tmux prefix → voice keys → control keys → operator pending → search/visual → navigation.

**Modes** (`src/core_types.rs`): `EditorMode` — Normal, VisualChar, VisualLine, Search, ReverseSearch, Command, CommandExecution, Tutorial. Mode is stored per-buffer in `BufferState`; use `get_active_mode()` / `set_active_mode()`.

**Voice/TTS** (`src/voice/`, `src/editor/voice_control.rs`):
- `PlaybackController` owns a background thread (`playback_loop`) that receives `PlaybackCommand` over an mpsc channel and drives rodio audio playback.
- Text is split into ≤4500-char chunks via `chunk_paragraphs()` in `src/voice/mod.rs`.
- `VoicePlayingInfo` (shared via `Arc<Mutex>`) tracks which doc lines are playing and timing for word-highlight animation.
- `sync_voice_status()` is called each tick in the display loop — this is the hook point for detecting playback completion.
- TTS uses ElevenLabs API. Config (`ELEVENLABS_API_KEY`, `VOICE_ID`, `PLAYBACK_SPEED`) lives in `~/.config/hygg-reader/.env`.

**Config** (`src/config.rs`): loaded from `~/.config/hygg-reader/.env` via `dotenvy`. Call `load_config()` at startup; `save_config()` persists changes.

**Persistence**: Progress saved per-document using a hash of the document content (`src/progress.rs`). Bookmarks and highlights also keyed by document hash (`src/bookmarks.rs`, `src/highlights.rs`). All files live under `~/.config/hygg-reader/`.

**Buffers**: The editor supports multiple `BufferState` buffers (used for split-view command output). Buffer 0 is always the document. Active buffer accessed via `self.active_buffer` index.

**Display**: `draw_content_buffered` renders to a `Vec<u8>` then flushes in one write to minimize flicker. Status line rendered separately by `draw_status_line` / `draw_status_line_buffered`.

**Key conventions**:
- All editor methods are `impl Editor` — no separate structs for subsystems.
- Handler functions return `Result<Option<bool>, ...>`: `Some(true)` = quit, `Some(false)` = handled (stop propagation), `None` = not handled (continue to next handler).
- `self.offset` = first visible line index; `self.cursor_y` = cursor row on screen; `self.offset + self.cursor_y` = current doc line.

## trench Architecture

A separate TUI binary (`trench/src/main.rs`) that aggregates AI research feeds. No async — uses `std::sync::mpsc` and `reqwest::blocking` throughout.

### Data model (`src/models/`)

`FeedItem` is the central type: `id`, `title`, `source_platform`, `content_type`, `domain_tags`, `signal` (Primary/Secondary/Tertiary), `published_at`, `authors`, `summary_short`, `workflow_state` (Inbox/Skimmed/Queued/DeepRead/Archived), `url`, `upvote_count`. `upvote_count` has `#[serde(default)]` for cache backward-compatibility.

`FeedItem::compute_signal()` derives signal from platform and upvote count. `map_arxiv_category()` and `detect_subtopics()` live in `src/models/categories.rs`.

### Ingestion pipeline (`src/ingestion/`)

Background thread in `main.rs` fetches all sources sequentially then runs enrichment:

1. `arxiv::fetch()` — arXiv Atom API (cs.LG + cs.AI + stat.ML). Maps category codes via `map_arxiv_category()`, detects subtopics via `detect_subtopics()`.
2. `huggingface::fetch()` — Scrapes HF daily papers page (two-pass: h3 for titles, entity-encoded JSON for upvotes/authors), then makes one batched arXiv API call to fill `summary_short` for all items.
3. `rss::fetch()` — Generic RSS 2.0 / Atom parser for OpenAI blog, DeepMind blog, Import AI, The Batch. Handles CDATA via `Event::CData`. Anthropic has no RSS feed and is intentionally skipped.
4. `semantic_scholar::enrich()` — Enriches arXiv items with citation counts and fields of study. 7-day TTL cache at `~/.config/trench/enrichment_cache.json`. Entries with empty `fields_of_study` are invalidated on load.

Each source sends `FetchMessage::Items(Vec<FeedItem>)` + `FetchMessage::SourceComplete(name)` over mpsc. After all sources, sends enriched batch + `AllComplete`.

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
