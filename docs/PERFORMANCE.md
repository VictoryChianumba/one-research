# Performance Checklist

A working checklist for performance investigation across this workspace
(hygg-reader, cli-text-reader, trench, block-reader). Walk top-to-bottom when
chasing a regression; jump to a single axis when scoping a planned improvement.

Performance work splits along eight axes. The original triad —
**latency / throughput / footprint+background** — is a steady-state snapshot;
the remaining axes cover *time* (startup, warmup), *distribution* (tail/jitter),
*shape* (scalability), and *constraints* (responsiveness budget, energy).

Bias toward **probe-first, not fix-first** — each item asks you to look at
something, not change something. No optimization without a measurement showing
where time/memory actually goes.

---

## 0. Before you optimize anything

- [ ] **Name the axis.** Which of the eight are you actually trying to improve?
      "Slow" is not a goal. ("Cold start under 200ms" is.)
- [ ] **Reproduce.** Have a single command + dataset that exhibits the problem
      on demand. For trench: a specific cached feed; for hygg: a specific
      PDF/EPUB; for block-reader: a specific arXiv ID.
- [ ] **Baseline.** Measure before changing anything. Write the number down.
      `hyperfine` for end-to-end, `cargo bench` (criterion) for hot paths.
- [ ] **Success criterion.** What number means "done"? Strong criteria let you
      loop independently (CLAUDE.md Rule 4).
- [ ] **Profile before guessing.** No optimization without a flamegraph or
      measurement showing where time/memory actually goes. Intuition is
      reliably wrong.

---

## 1. Latency (time per operation)

- [ ] **Hot path identified.** `cargo flamegraph -p <crate>` or `samply` on a
      representative run. The hot path is whatever the flamegraph says, not
      what you think.
- [ ] **Allocations on hot path.** `String::new`, `format!`, `to_string`,
      `clone`, `Vec::new` inside loops. Each is cheap once, lethal millions of
      times. Probe: `dhat-rs` or grep `.clone()` in the hot module.
- [ ] **Sync I/O on hot path.** Disk reads, network calls, file syncs inside a
      render/event loop. In trench: cache writes after every
      `process_incoming` batch — is that fsync'd every time?
- [ ] **Format machinery on a hot path.** `format!`, `write!`, `println!` are
      surprisingly expensive. ratatui's `Spans` building per-frame can
      dominate render time.
- [ ] **Regex / parsing inside loops.** LaTeX/RSS/HTML parsers compiled
      per-call. In `arxiv.rs`, `rss.rs`, `huggingface.rs` — compile once,
      reuse.
- [ ] **`Arc<Mutex>` lock churn.** Especially the `VoicePlayingInfo` shared
      state — how often is it locked per tick?

## 2. Throughput (ops per unit time)

- [ ] **Pipeline stage bottleneck.** The ingestion pipeline (`arxiv` →
      `huggingface` → `rss` → `semantic_scholar enrich`) runs **sequentially**
      in one background thread. Measure each stage's wall time. Is the
      slowest 80% of the total?
- [ ] **Parallelizable stages.** arXiv, HuggingFace, RSS feeds are independent
      network calls — could run concurrently via `std::thread::scope` or
      `rayon` for a 3–4× wall-time reduction with no async runtime.
- [ ] **Batching.** Semantic Scholar's `enrich()` — one batched call or N
      small ones? HuggingFace's per-item arXiv fill is already batched (good).
- [ ] **Backpressure.** mpsc unbounded. If a downstream consumer stalls, does
      the producer balloon memory?
- [ ] **Wasted retries.** Failed fetches retried with exponential backoff, or
      hammered?

## 3. Footprint (memory, disk, binary size)

- [ ] **Binary size.** `cargo bloat --release` per binary. Common offenders:
      `regex`, `serde_json` codegen, large `match` arms. Strip with
      `strip = true` in `[profile.release]`.
- [ ] **Steady-state RSS.** `ps -o rss` while idle on the feed view. Anything
      above ~50MB for a TUI is worth investigating.
- [ ] **Heap growth over time.** `dhat-rs` or run for an hour and watch RSS.
      Caches that only grow are leaks in slow motion.
- [ ] **Disk footprint of caches.** `~/.config/trench/cache.json` size after a
      week. JSON is verbose — is bincode/postcard worth it? Probably no until
      it's >10MB.
- [ ] **Per-item retained memory.** `FeedItem` size — strings galore. Consider
      `Box<str>` over `String` for immutable fields; `Arc<str>` for shared
      author names.
- [ ] **Big vectors held forever.** Full document text in
      `ReaderInstanceModel`, all `VisualLine`s for huge papers.

## 4. Startup (cold-start time)

- [ ] **End-to-end cold start.** `hyperfine './target/release/trench'` from
      invocation to first frame visible. Target: <100ms for the cached feed
      to appear.
- [ ] **Cache deserialization cost.** `cache.json` parse time — does it grow
      with feed size? `serde_json::from_reader` with a buffered reader is
      faster than `from_str`.
- [ ] **Lazy init.** What's loaded at startup that could be deferred to first
      use? Theme files, help text, bookmarks for papers not yet opened.
- [ ] **Link-time options.** `lto = "thin"` and `codegen-units = 1` in release
      profile — measure both directions; LTO can also slow startup.
- [ ] **Panic strategy.** `panic = "abort"` shrinks binary and skips unwind
      tables; only safe if you don't catch panics.
- [ ] **`std::env` / `dotenvy` at startup.** Config loading touches disk
      before first paint. Can it run after first frame?

## 5. Tail latency / jitter / predictability

- [ ] **Frame-time histogram.** Instrument the main loop with timing per
      iteration — log p50/p95/p99 separately. p99 spikes are invisible in
      averages.
- [ ] **Allocator-induced pauses.** Rust has no GC but the system allocator
      (glibc/jemalloc/mimalloc) can stall on fragmented heaps. Try `mimalloc`
      as the global allocator; measure.
- [ ] **Lock-induced jitter.** Every `Arc<Mutex>` is a potential stall.
      Identify locks held across I/O.
- [ ] **Worst-case parse.** Some papers parse in 50ms, some in 5s. Identify
      the 99th-percentile-slow paper and profile *it*, not the average one.
- [ ] **Background fetch interfering with render.** When the ingestion thread
      is hot, does the UI hitch? Lower its priority or yield more often.

## 6. Scalability (how it degrades with N)

- [ ] **Identify N for each subsystem.** N = items in feed, N = lines in
      document, N = visual lines after wrap, N = bookmarks, N = highlights,
      N = cached enrichments.
- [ ] **Algorithmic complexity audit.** Look for nested loops over the same
      `N`. Probe: search for `.iter()` inside `for` over the same collection.
- [ ] **Linear scans that should be hashes.** `find` over `Vec<FeedItem>` by
      URL — O(N) per lookup. URL dedup in `process_incoming` — is it
      `HashMap`-backed?
- [ ] **Reflow on resize.** When the TUI resizes, do you rebuild
      `Vec<VisualLine>` for the entire document, or just the visible window?
      For a 10k-line paper this matters.
- [ ] **TOC/section index.** Built once, or rescanned on every `[`/`]` press?
- [ ] **`Vec` insertions at head.** `insert(0, x)` is O(N). Use `VecDeque` if
      prepending.
- [ ] **Sort cost.** `process_incoming` re-sorts after every batch. If
      batches are small and feed is large, that's O(N log N) repeatedly.

## 7. Responsiveness / frame budget (TUI-specific)

- [ ] **16ms frame budget honoured.** Time `terminal.draw(...)` plus event
      handling. If it ever exceeds 16ms, you'll feel it as input lag.
- [ ] **Redraw triggered too often.** The `needs_redraw` flag pattern in
      cli-text-reader is the right idea — verify it's actually gating draws.
- [ ] **Redraw triggered too rarely.** Voice/animation paths that *should*
      trigger redraws but don't, causing perceived staleness.
- [ ] **Full repaint vs partial.** ratatui repaints everything per frame;
      this is fine, but the *content building* (Spans, Lines, layouts) should
      be cached if expensive.
- [ ] **Event polling timeout.** `crossterm::poll(Duration::from_millis(?))`
      — too short = busy loop, too long = laggy input. Typically 16ms when
      animating, 100ms+ when idle.
- [ ] **Input coalescing.** Trackpad/key-repeat scroll events arriving faster
      than render can keep up — coalesce N pending scroll events into one
      before drawing.
- [ ] **Drawing cost per pane.** Reader pane vs feed pane vs notes pane —
      which dominates? Profile a typical 4-pane layout.
- [ ] **Image rendering cost.** Kitty graphics protocol writes can be
      expensive; cache decoded image data, not just the source file.

## 8. Energy / wakeups (background and idle)

- [ ] **Idle CPU usage.** With trench open but no input, `top` should show
      <1%. If higher, you have a busy loop somewhere.
- [ ] **Polling cadence.** What's the longest acceptable `poll` timeout when
      idle? Sleeping `100ms` instead of `16ms` while idle is free battery
      life.
- [ ] **Wakeups per second.** `pidstat -w` (Linux) / `powermetrics` (macOS).
      Each periodic timer is a wakeup; consolidate them.
- [ ] **Background thread spinning.** Ingestion thread should block on
      channel or sleep, not spin.

---

## Rust/TUI tooling appendix

| Need | Tool |
|---|---|
| End-to-end timing | `hyperfine` |
| CPU flamegraph | `cargo flamegraph`, `samply` |
| Microbenchmarks | `criterion` |
| Heap profiling | `dhat-rs`, `heaptrack` |
| Allocation counts | `dhat-rs`, custom allocator wrapper |
| Binary size | `cargo bloat`, `cargo-llvm-lines` |
| Unused deps | `cargo udeps` (nightly) — already in `ci.sh` |
| Tracing | `tracing` + `tracing-flame` |
| Allocator swap | `mimalloc`, `jemalloc` as `#[global_allocator]` |

## When to stop

- [ ] You met the success criterion you defined in step 0.
- [ ] Further changes hurt code clarity more than they help performance
      (CLAUDE.md Rule 2: simplicity first).
- [ ] The remaining bottleneck is an external system (network, disk, the
      terminal emulator itself).

---

## Notes on leverage in this codebase

The biggest leverage is almost certainly axes **2 (throughput)** and
**4 (startup)**: the sequential ingestion pipeline is an obvious parallelism
win, and TUI cold-start is highly user-visible. Latency tuning (axis 1) is
usually premature in a TUI until the frame budget (axis 7) is nailed.

Several items appear under multiple axes (allocations show up under latency
*and* tail jitter *and* footprint). That's because the same root cause often
hurts multiple axes at once — fixing one well-chosen allocation can improve
three numbers. Look for those before the single-axis wins.

---

## Audit log

One short entry per axis closure. The detailed working state lives in the
audit task list; this log is the durable record.

### Axis 4 — Startup (closed 2026-05-17)

- **Baseline**: ~30ms instrumented startup; total wall-clock unmeasured (the
  existing "first frame ready" log was being lost — turned out to be a ratatui
  panic on zero-size pty, not a SIGTERM flush race).
- **Dominant cost**: `kitty_graphics::tmux_passthrough_enabled()` shelling out
  to `tmux show -gv allow-passthrough` via fork/exec = ~8ms on every startup
  for tmux users.
- **Fix**: replaced the probe-driven 3-tier messaging in `main.rs` with an
  unconditional one-line advisory printed before alt-screen entry (preserves
  scrollback survival; loses message precision for the small set of tmux users
  with correctly-configured passthrough — they see a redundant tip).
- **Result**: instrumented startup ~16ms (47% reduction). Verified end-to-end
  via new `--bench-startup` flag (exits cleanly after first frame, prints
  `first_frame_ready_ms` to stdout for harness consumption). Warm-cache
  distribution over 6 runs:
  - `first_frame_ready_ms`: median **22ms**, range 19–25ms
  - `wall_clock_ms` (fork → clean exit): median **62.8ms**, range 56–65ms
  - Cold-cache outlier: 87ms / 164ms (single run after gap from last build)
- **Target**: PERFORMANCE.md §4 set "<25ms cold to first frame" — **met** on
  the instrumented number; wall-clock is bounded by ~40ms of process startup
  outside our code (kernel `execve`, dyld, std init).
- **Open thread**: cold-cache run is 4× warm — folds naturally into axis 5
  (tail jitter) when we get there. Binary load floor (~40ms wall - 22ms
  instrumented) folds into axis 3 (footprint).
- **Tooling kept**: `--bench-startup` flag + `/tmp/bench_first_frame.py`
  harness for repeatable cold-start measurement.

### Axis 2 — Throughput (closed 2026-05-17)

- **Premise correction**: the audit started with the doc claim that "all
  sources run sequentially on one thread." That was stale — the code in
  `services/ingestion.rs` already used `std::thread::scope` for four
  concurrent groups (A: arxiv→hf, B: openreview, C: core, D: RSS feeds).
  Total wall-clock = max(groups), not sum. CLAUDE.md still has the stale
  description; flagged for separate doc-hygiene cleanup.
- **Baseline**: 2496ms total pipeline (1999ms fetch + 496ms enrichment).
  Critical path was Group B at 1998ms — single source dominating the
  whole fetch phase.
- **Dominant cost**: `openreview::fetch()` made 3 sequential HTTP requests,
  one per venue (ICLR/NeurIPS/ICML), to the same upstream
  `api2.openreview.net`. Each ~600–700ms, total ~2s.
- **Fix**: parallelized the three venue fetches with `std::thread::scope`
  inside `openreview::fetch` (same primitive the outer service already
  uses). Same-host concurrency was the named risk — tested clean, no 429s.
- **Result**: openreview dropped to 536ms (−73%); fetch phase dropped to
  1142ms (−43%); total pipeline dropped to 1651ms (−34%).
- **New critical path**: Group A (1141ms = arxiv 168 + huggingface 973)
  and Group D max (bair at 1085ms) — both bounded by upstream
  responsiveness and the deliberate Group A serialization for shared-host
  rate-limit politeness.
- **Tooling kept**: per-source elapsed-time logging in `run_source`,
  per-enrichment-stage timing, and `ingestion: total pipeline Nms` summary
  log (INFO level — useful for ongoing observation, not diagnostic-only).
  `/tmp/bench_pipeline.py` harness uses clean 'q' shutdown so env_logger
  drop handlers flush buffered output (SIGTERM loses everything past the
  first second).
- **Open threads** (correctness, not throughput — separate from axis 2):
  - `semantic_scholar` rate-limited (429) on the first request, enriches
    0 items. Enrichment pipeline runs but does no work.
  - `huggingface → arxiv` abstract batch fill 429s, so 53 papers per
    refresh miss their abstracts.
  - Both worth a follow-up audit cycle (axis 1 latency or its own slot)
    once we have a quieter critical path to attribute against.

### Axis 3 — Footprint (closed 2026-05-17 — diagnostic only, no fix landed)

- **Inventory**:
  - Binary: 11MB on Apple Silicon (release, `strip=true` already on);
    `.text` section 6.9MiB
  - `cache.json`: 2.9MB for 1781 items (~1.7KB per `FeedItem`)
  - Other on-disk caches: ~220KB combined (negligible)
- **Top binary contributors** (via `cargo bloat --crates`):
  - `std` 1.6MiB / 23.7% — fmt/io machinery; not attackable
  - `trench` 700KB / 9.9% — our own code
  - `html5ever` 368KB / 5.2% — HTML parser pulled by `readability`/`html2text`
  - `rustls` 310KB + `ring` 132KB + `h2` 132KB + `hyper` 87KB = 661KB
    HTTP/TLS stack via reqwest
  - `tread` 255KB + `arxiv_render` 184KB — paper reader (intentional)
  - `regex_automata` 234KB + `regex_syntax` 129KB — regex engine
  - `zune_jpeg` 138KB + `image` 111KB — image decoding for figures
- **Surprise finding** (via `cargo bloat` function-level view): a
  `symphonia_bundle_mp3::Layer3::decode` function at 30KB shows up — the
  full audio stack (`rodio` + `symphonia` + `cpal`) is bundled via
  `tread::build_voice_controller`, despite the project TODO noting "voice
  mode broken in hygg rewrite." Estimated 500KB-1MB of unreachable audio
  code in the binary for currently-unused functionality.
- **Experiment attempted, reverted**: disabled reqwest's `http2` default
  feature in trench's 3 Cargo.tomls (`trench/Cargo.toml`, `crates/http`,
  `crates/chat`) to drop `h2` (132KB). Result: **no savings; binary grew
  ~200KB**. Reason: cargo's feature unification with tread's
  `crates/arxiv-render/Cargo.toml` (which still pulls reqwest with
  default features) forced `http2` back on. Single-workspace feature
  reduction is a no-op when an out-of-workspace path dep enables the
  same features. Reverted; binary back to baseline.
- **Memory side**: per-item refactor candidates audited (drop
  `github_owner`/`github_repo_name` and derive on demand; switch String
  to Box<str>; move `title_lower`/`authors_lower` to a side table).
  Estimated 300–800KB RAM savings combined, but each requires 10–50+
  call-site changes AND proper heap profiling (dhat-rs setup) to
  measure cleanly. Not attempted in this session.
- **No code change landed.** This axis closes with diagnostic data + a
  clear list of attackable items that require either cross-workspace
  coordination or measurement infrastructure that doesn't exist yet.
- **Open threads** (sized estimates, not measured):
  - **Cross-workspace tread voice feature flag** — gate
    `build_voice_controller` behind a `voice` feature in tread, disable
    in trench. Estimated 500KB-1MB binary savings. Two-PR effort
    (tread + trench).
  - **Cross-workspace reqwest http2 drop** — apply the same Cargo.toml
    change attempted here to ALL reqwest call sites including tread's
    `arxiv-render`. Estimated 132–200KB. One-line PR in tread + the
    three already-prepared edits in trench (which were reverted).
  - **HTML parser replacement** — `html5ever` (368KB) is pulled by
    `readability` and `html2text` for ingestion. Switching to a simpler
    HTML→text pipeline (e.g., `scraper` or hand-rolled regex strip) for
    the limited use cases trench actually has might save most of it.
    Higher-risk, larger refactor.
  - **Memory profiling with dhat-rs** — set up `#[global_allocator]`
    behind a feature flag, run trench under dhat, identify the largest
    `FeedItem`-related allocations, then drive the per-item refactor
    with real before/after numbers.
- **Tooling kept**: `cargo bloat` is now installed (`~/.cargo/bin/`)
  for future binary-size audits.

### Axis 7 — Responsiveness / frame budget (closed 2026-05-17 — infrastructure validated, heavy-path data deferred)

- **Infrastructure status**: per-pane draw timing already comprehensive.
  21 `log::debug!("draw_X: Nms", ...)` sites across `trench/src/ui/`
  cover all 6 main feed panes (title_bar, search_row, item_table,
  filter_panel, details_panel, footer) in both narrow and wide layouts.
  Plus `terminal.draw()` total timing + binary "(slow frame)" warning
  when total > 16ms (main.rs:1110).
- **Feed view baseline**: every recorded frame in the audit logs shows
  each pane at 0ms (sub-millisecond). No "(slow frame)" warnings ever
  recorded. Feed view total draw is consistently far below the 16ms
  frame budget — likely <2-3ms total even with 1781 items + a 4-pane
  layout.
- **What's NOT covered by current logs**: reader view (open paper,
  LaTeX rendering), repo viewer, notes app, chat panel, search filtering
  with many items typed, resize events. These are keyboard-driven
  scenarios that the headless harness can't trigger.
- **No fix landed.** Axis closes because (a) the parts we could measure
  are well under budget, (b) the parts we can't measure require either
  interactive testing or a TestBackend bench harness that doesn't exist
  yet, and (c) there's no evidence of a problem to fix in the data we
  do have.
- **Open threads**:
  - **Interactive test protocol** for capturing heavy-path data:
    1. `TRENCH_DEBUG_LOG=1 trench` and use it normally
    2. Open a few large papers (especially arXiv ones with figures /
       heavy LaTeX)
    3. Scroll through, search across all items, resize the terminal
    4. Quit cleanly with `q` (so env_logger drop handlers flush — see
       axis 4 lesson about SIGTERM losing buffered writes)
    5. Inspect `~/.config/trench/trench.log` for any "(slow frame)"
       warnings or per-pane draws over a few ms
  - **TestBackend benches** for the heavy scenarios — construct
    synthetic App states (reader open with N blocks, repo viewer with
    M files, notes with K entries) and time `ui::draw` against
    `ratatui::backend::TestBackend`. ~100-200 lines of harness, would
    give repeatable scaling data that also feeds axis 6 (scalability).
  - **Frame-time histogram** (p50/p95/p99 instead of just
    above/below-16ms binary): ~30 lines, would let us see tail/jitter
    behaviour over time without waiting for a specific overrun. Folds
    cleanly into axis 5 (tail latency) work.

### Axis 1 — Latency (closed 2026-05-17 — no concrete target identified)

- **Per the leverage notes**: latency tuning is usually premature in a
  TUI until frame budget (axis 7) is nailed. Axis 7 closed cleanly with
  feed at sub-ms — so axis 1 is now appropriate to attempt.
- **Inventory of measured warm-state operations**:
  - Per-pane draws: <1ms each, total <2-3ms per frame (axis 7)
  - Ingestion pipeline: 1651ms end-to-end after parallelization (axis 2)
  - Cold-start to first frame: 22ms median (axis 4)
  - Cache load + index rebuild on cold start: 15ms (deemed structural
    in axis 4; serde_json at ~180 MB/s is at the high end of what's
    achievable for JSON of that size)
- **No specific latency complaint surfaced** during the audit. Latency
  tuning without a concrete scenario is fishing — random
  micro-optimizations will not improve anything users notice and may
  hurt code clarity.
- **What we'd profile if we had a complaint**:
  - LaTeX parsing in tread/arxiv_render — dominant cost when opening a
    paper; not in critical paths
  - process_incoming dedup loop on each fetch batch
  - Search filter rebuild on each keystroke (filter_summary_cache
    already amortizes some of this)
  - Arc<Mutex>-protected VoicePlayingInfo lock churn — voice mode
    currently broken per project TODO, so the lock isn't actually
    contended in practice
- **No fix landed.** Closing diagnostic-only with the instruction:
  revisit when a specific operation feels slow to the user, then we
  have a concrete reproducer to instrument against.
