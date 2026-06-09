# Bench harnesses

Python wrappers around `one-research`'s built-in bench flags. Each script
runs the one-research binary with the right flag, captures stdout, and
aggregates a distribution across runs.

| Script | Wraps | Purpose |
|---|---|---|
| `bench_startup.py` | `one-research` under pty, kill after N seconds | Baseline startup timing into `one-research.log` |
| `bench_first_frame.py` | `one-research --bench-startup` | Cold-start wall-clock distribution (fork → first frame ready) |
| `bench_pipeline.py` | `one-research` under pty with `ONE_RESEARCH_DEBUG_LOG=1`, clean `q` shutdown | Ingestion pipeline timing (env_logger drop handlers flush on clean exit) |
| `bench_render.py` | `one-research --bench-render feed --n N` | TestBackend per-frame draw distribution across feed sizes |

All scripts assume `target/release/one-research` exists. Run `cargo build -p
one-research --release` first.

See `docs/PERFORMANCE.md` for the audit log that motivated each harness.
