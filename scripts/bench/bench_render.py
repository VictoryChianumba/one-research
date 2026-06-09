#!/usr/bin/env python3
"""Sweep `one-research --bench-render` across feed sizes (N) and report the
distribution of per-frame draw times at each N.

Unlike bench_first_frame.py this harness does NOT need a pty — the bench
flag exits before any terminal setup, so stdout is plain. It also runs
multiple frames per invocation (the binary self-aggregates), so we only
need one process per N.

Usage:
  scripts/bench/bench_render.py                # default sweep
  scripts/bench/bench_render.py 100 5000       # custom N values
"""
import os
import subprocess
import sys

BIN = "/Users/temp/Desktop/projects/pproject-forks/one-research/target/release/one-research"
DEFAULT_NS = [100, 500, 1000, 2500, 5000, 10000]
FRAMES = 200
WIDTH = 160
HEIGHT = 48


def one_run(n: int) -> dict:
    res = subprocess.run(
        [
            BIN,
            "--bench-render",
            "feed",
            "--n",
            str(n),
            "--frames",
            str(FRAMES),
            "--width",
            str(WIDTH),
            "--height",
            str(HEIGHT),
        ],
        capture_output=True,
        text=True,
        timeout=60,
    )
    if res.returncode != 0:
        print(f"  ERROR (rc={res.returncode}): {res.stderr.strip()}", file=sys.stderr)
        return {}
    out = {}
    for line in res.stdout.splitlines():
        if "=" in line:
            k, v = line.split("=", 1)
            out[k.strip()] = v.strip()
    return out


def main():
    ns = [int(a) for a in sys.argv[1:]] if len(sys.argv) > 1 else DEFAULT_NS
    print(f"scenario=feed  frames={FRAMES}  viewport={WIDTH}x{HEIGHT}\n")
    header = f"{'N':>7}  {'p50_us':>8}  {'p95_us':>8}  {'p99_us':>8}  {'max_us':>8}"
    print(header)
    print("-" * len(header))
    for n in ns:
        d = one_run(n)
        if not d:
            print(f"{n:>7}  (failed)")
            continue
        print(
            f"{n:>7}  "
            f"{d.get('p50_us', '?'):>8}  "
            f"{d.get('p95_us', '?'):>8}  "
            f"{d.get('p99_us', '?'):>8}  "
            f"{d.get('max_us', '?'):>8}"
        )


if __name__ == "__main__":
    main()
