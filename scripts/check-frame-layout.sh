#!/usr/bin/env bash
#
# FrameLayout invariants for ADR-008. Locks in the post-layout hook so
# the `// Intentional render-time mutation` marker class stays gone.
#
# Sibling to scripts/check-render-purification.sh,
# scripts/check-ingestion-seam.sh, scripts/check-store-seam.sh,
# and scripts/check-item-store.sh.
#
# Exit codes:
#   0  all invariants hold
#   1  one or more invariants violated (details printed)

set -Eeuo pipefail

cd "$(dirname "$0")/.."

fail=0

# N1.  No `// Intentional render-time mutation` comment block remains
#      in any source file.  Doc-comment narrative references (the
#      ADR-008 commit + apply_frame_layout doc + frame_layout.rs
#      module-level doc) live behind `//!` or `///` and explicitly
#      mention the marker by name; the strict regex catches the
#      double-slash form only.
hits=$(grep -rnE '^\s*// Intentional render-time mutation' one-research/src/ 2>/dev/null || true)
if [[ -n "$hits" ]]; then
  echo "FAIL: '// Intentional render-time mutation' comment block reappeared (ADR-008 / ADR-001 §D3):"
  echo "$hits" | sed 's/^/  /'
  fail=1
fi

# N2.  No `set_max(` or `set_offset(` call inside one-research/src/ui/layout/
#      outside explicit SEAM-EXEMPT sites.  The post-layout state
#      mutation lives in `App::apply_frame_layout`; renders should
#      only read.  Two known exemptions today: help popup's in-pane
#      scroll bound + feed-pane `pre_draw_*` math (ListState's set_*
#      family on the model side, not the render side).
hits=$(grep -rnE '\.set_max\(|\.set_offset\(' one-research/src/ui/layout/ \
  2>/dev/null | grep -v 'SEAM-EXEMPT' || true)
filtered=""
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  file=$(echo "$line" | cut -d: -f1)
  lineno=$(echo "$line" | cut -d: -f2)
  # 5-line lookback for SEAM-EXEMPT annotation.
  start=$((lineno > 5 ? lineno - 5 : 1))
  window=$(awk -v s="$start" -v e="$lineno" 'NR >= s && NR <= e' "$file")
  if ! echo "$window" | grep -q "SEAM-EXEMPT:"; then
    filtered+="$line"$'\n'
  fi
done <<< "$hits"
if [[ -n "${filtered// }" ]]; then
  echo "FAIL: render-path set_max/set_offset call without SEAM-EXEMPT (ADR-008 §S2):"
  echo "$filtered" | sed 's/^/  /'
  fail=1
fi

# N3.  ADR-008 cadence table mentions every committed PR.  Mirrors
#      I2 / I6 / K4 / L4 / M4.
adr8="docs/adr/ADR-008-frame-layout.md"
for tag in "| 1 |" "| 2 |" "| 3 |"; do
  if ! grep -qF "$tag" "$adr8"; then
    echo "FAIL: ${adr8} cadence table missing entry matching '${tag}'"
    fail=1
  fi
done

if [[ "$fail" -eq 0 ]]; then
  echo "OK: frame-layout invariants hold (marker class gone, hooks centralised)"
fi
exit $fail
