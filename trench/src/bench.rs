// In-process render benchmark harness, gated behind the `--bench-render` flag
// on the main binary. Same precedent as `--bench-startup` (main.rs:604):
// a flag-driven path that constructs a synthetic state, exercises a hot path,
// and prints `key=value` timing data to stdout for an external Python harness
// (`/tmp/bench_render.py`) to aggregate across iterations.
//
// Scope (MVP): one scenario, `feed`. Constructs N synthetic FeedItems, drives
// `ui::draw` against a ratatui TestBackend for F frames, reports the
// post-warmup distribution. The intent is to unblock the heavy-path
// scenarios that closed PERFORMANCE.md axis 7 as "deferred" — additional
// scenarios (search-active, resize cycle, repo viewer, notes) layer in on
// the same harness.

use std::time::Instant;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::App;
use crate::models::fixtures;

const WARMUP_FRAMES: usize = 5;

pub struct BenchOptions {
  pub scenario: String,
  pub n: usize,
  pub frames: usize,
  pub width: u16,
  pub height: u16,
}

impl Default for BenchOptions {
  fn default() -> Self {
    Self {
      scenario: "feed".to_string(),
      n: 1000,
      frames: 200,
      width: 160,
      height: 48,
    }
  }
}

/// Parse `--bench-render <scenario> [--n N] [--frames F] [--width W] [--height H]`
/// from `std::env::args()`. Returns `Some(opts)` if `--bench-render` is
/// present, `None` otherwise. Unknown flags fall through to defaults rather
/// than erroring — the harness is a developer tool, not a public CLI.
pub fn parse_bench_args() -> Option<BenchOptions> {
  let args: Vec<String> = std::env::args().collect();
  let pos = args.iter().position(|a| a == "--bench-render")?;
  let mut opts = BenchOptions::default();
  if let Some(s) = args.get(pos + 1) {
    if !s.starts_with("--") {
      opts.scenario = s.clone();
    }
  }
  let take_u = |flag: &str| -> Option<usize> {
    args
      .iter()
      .position(|a| a == flag)
      .and_then(|i| args.get(i + 1))
      .and_then(|s| s.parse().ok())
  };
  if let Some(n) = take_u("--n") {
    opts.n = n;
  }
  if let Some(f) = take_u("--frames") {
    opts.frames = f;
  }
  if let Some(w) = take_u("--width") {
    opts.width = w as u16;
  }
  if let Some(h) = take_u("--height") {
    opts.height = h as u16;
  }
  Some(opts)
}

pub fn run(opts: BenchOptions) -> Result<(), Box<dyn std::error::Error>> {
  match opts.scenario.as_str() {
    "feed" => run_feed(opts),
    other => {
      eprintln!("unknown bench scenario: {other}");
      std::process::exit(2);
    }
  }
}

fn run_feed(opts: BenchOptions) -> Result<(), Box<dyn std::error::Error>> {
  let mut app = App::new();
  app.workspace.items = (0..opts.n).map(fixtures::variant).collect();
  app.rebuild_indices();

  let backend = TestBackend::new(opts.width, opts.height);
  let mut terminal = Terminal::new(backend)?;

  let mut samples_us: Vec<u128> = Vec::with_capacity(opts.frames);
  for frame_idx in 0..opts.frames {
    // The dirty flag is read-cleared inside the main loop; we sidestep that
    // gate by calling `terminal.draw` directly. Each draw runs the full
    // pre_draw_update + ui::draw pipeline.
    let t = Instant::now();
    terminal.draw(|frame| crate::ui::draw(frame, &mut app))?;
    let elapsed_us = t.elapsed().as_micros();
    if frame_idx >= WARMUP_FRAMES {
      samples_us.push(elapsed_us);
    }
  }

  emit_summary(&opts, &mut samples_us);
  Ok(())
}

fn emit_summary(opts: &BenchOptions, samples: &mut [u128]) {
  samples.sort_unstable();
  let n = samples.len();
  let pct = |p: f64| -> u128 {
    if n == 0 {
      return 0;
    }
    let idx = ((p * n as f64).ceil() as usize).saturating_sub(1).min(n - 1);
    samples[idx]
  };
  let max = samples.last().copied().unwrap_or(0);
  println!("scenario={}", opts.scenario);
  println!("n_items={}", opts.n);
  println!("frames_measured={}", n);
  println!("width={}", opts.width);
  println!("height={}", opts.height);
  println!("p50_us={}", pct(0.50));
  println!("p95_us={}", pct(0.95));
  println!("p99_us={}", pct(0.99));
  println!("max_us={}", max);
}

// Synthetic FeedItem factory lives in `crate::models::fixtures` so the
// bench and future tests can share it. See that module for the variant
// shape (deterministic, spreads enum coverage).
