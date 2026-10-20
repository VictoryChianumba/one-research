#!/usr/bin/env bash
# Run one build for 120s with network, screenshots along the way.
set -euo pipefail
bin="$1" label="$2"
home="$RUNNER_TEMP/home-$label"; out="results/$label"
rm -rf "$home"; mkdir -p "$home" "$out"
python3 tmp-apptest/run_app.py "$bin" "$home" "$out" 120 \
  "15:shot:t15" "45:shot:t45" "90:shot:t90"
python3 tmp-apptest/summarize.py "$home" "$label" > "$out/summary.json"
cp "$home/.config/one-research/one-research.log" "$out/" 2>/dev/null || true
ls -la "$home/.config/one-research" > "$out/files.txt" 2>&1 || true
