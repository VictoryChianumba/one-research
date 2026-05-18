# Bench harnesses

Python wrappers around `trench`'s built-in bench flags. Each script
runs the trench binary with the right flag, captures stdout, and
aggregates a distribution across runs.

| Script | Wraps | Purpose |
|---|---|---|
| `bench_startup.py` | `trench` under pty, kill after N seconds | Baseline startup timing into `trench.log` |
| `bench_first_frame.py` | `trench --bench-startup` | Cold-start wall-clock distribution (fork → first frame ready) |
| `bench_pipeline.py` | `trench` under pty with `TRENCH_DEBUG_LOG=1`, clean `q` shutdown | Ingestion pipeline timing (env_logger drop handlers flush on clean exit) |
| `bench_render.py` | `trench --bench-render feed --n N` | TestBackend per-frame draw distribution across feed sizes |

All scripts assume `target/release/trench` exists. Run `cargo build -p
trench --release` first.

See `docs/PERFORMANCE.md` for the audit log that motivated each harness.
