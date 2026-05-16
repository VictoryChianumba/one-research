#!/usr/bin/env bash
#
# Slice-1 render-purification invariants (ADR-001).
#
# Fails CI if any of the locked-down properties regress.  Currently
# checks the feed pane (slice 1).  As more panes complete their slice,
# they extend this script with their own checks.
#
# Exit codes:
#   0  all invariants hold
#   1  one or more invariants violated (details printed)

set -Eeuo pipefail

cd "$(dirname "$0")/.."

fail=0

# ── Slice 1 (feed pane) ─────────────────────────────────────────────────

# I1.  feed.rs takes no `&App` / `&mut App` and imports no `App` type.
#      Renders cross the model + context seam only (ADR-001 D1, D4).
feed_rs="trench/src/ui/layout/feed.rs"
violations=$(grep -nE '&\s*mut\s+App\b|&\s*App\b|\bapp:\s*&' "$feed_rs" || true)
if [[ -n "$violations" ]]; then
  echo "FAIL: ${feed_rs} references App at the render seam (ADR-001 D1/D4):"
  echo "$violations" | sed 's/^/  /'
  fail=1
fi

# I2.  ADR-001 D6 table mentions every committed slice-1 PR.  Cheap
#      consistency tripwire so the table can't drift from history again
#      (the "mixup" PR 4 → 4a/4b/4c that motivated this script).
adr="docs/adr/ADR-001-render-purification.md"
for tag in "| 1 |" "| 2 |" "| 3 |" "| 4a |" "| 4b |" "| 4c |" "| 5 |" "| 6 |"; do
  if ! grep -qF "$tag" "$adr"; then
    echo "FAIL: ${adr} D6 table missing entry matching '${tag}'"
    fail=1
  fi
done

# I3.  Slice-1 commit titles all reference `slice 1/`.  Catches the
#      class of "lost track of which PR belongs to the slice" mistakes.
slice_commits=$(git log --oneline --grep='refactor(feed): slice 1/' -- "$feed_rs" "trench/src/feed/" 2>/dev/null | wc -l | tr -d ' ')
if [[ "$slice_commits" -lt 6 ]]; then
  echo "WARN: only ${slice_commits} slice-1 commits visible (expected ≥6: PR 1, 2, 3, 4a, 4b, 4c)"
  # WARN, not FAIL — the count is informational; squash/rebase can change it.
fi

if [[ "$fail" -eq 0 ]]; then
  echo "OK: render-purification invariants hold (feed pane)"
fi
exit $fail
