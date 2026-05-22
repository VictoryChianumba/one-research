# Benchmarking

[Back to docs](../README.md)

The active bench harnesses live in [`../../scripts/bench/`](../../scripts/bench/).
They wrap trench's startup, ingestion, and render bench modes.

Build the release binary before running them:

```sh
cargo build -p trench --release
python3 scripts/bench/bench_first_frame.py
python3 scripts/bench/bench_pipeline.py
python3 scripts/bench/bench_render.py
```

Use [`../PERFORMANCE.md`](../PERFORMANCE.md) as the measurement checklist and
audit log when investigating regressions.
