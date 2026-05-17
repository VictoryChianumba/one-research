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
