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


# ── Slice 2 (reader pane + popup) ───────────────────────────────────────

# I4.  The 13 reader+popup fields migrated by PR 2 must stay off App's
#      top level.  ADR-002 trimmed bottom-drawer / abstract-popup /
#      narrow-feed-details state out of scope, so those reader_* fields
#      are still allowed at App top level — only the explicitly migrated
#      ones are flagged.
app_mod="trench/src/app/mod.rs"
for field in \
  "pub reader_tabs:" \
  "pub reader_active_tab:" \
  "pub reader_active:" \
  "pub reader_popup_active:" \
  "pub reader_popup_rx:" \
  "pub reader_popup_editor:" \
  "pub reader_popup_image_state:" \
  "pub reader_popup_burst:" \
  "pub reader_split_active:" \
  "pub reader_dual_active:" \
  "pub reader_secondary_tabs:" \
  "pub reader_secondary_active_tab:" \
  "pub focused_reader:" \
; do
  if grep -qF "$field" "$app_mod"; then
    echo "FAIL: ${app_mod} still declares '${field}' — slice 2 PR 2 migrated this off App (ADR-002)"
    fail=1
  fi
done

# I5.  ReaderPaneModel + ReaderPopupModel must be wired on App.
for field in "pub reader: crate::reader::ReaderPaneModel" \
             "pub reader_popup: crate::reader::ReaderPopupModel"; do
  if ! grep -qF "$field" "$app_mod"; then
    echo "FAIL: ${app_mod} missing slice-2 model field '${field}'"
    fail=1
  fi
done

# I6.  ADR-002 D6 / cadence table mentions every slice-2 PR.
adr2="docs/adr/ADR-002-reader-slice.md"
for tag in "| 1 |" "| 2 |" "| 3 |" "| 4 |" "| 5 |" "| 6 |"; do
  if ! grep -qF "$tag" "$adr2"; then
    echo "FAIL: ${adr2} cadence table missing entry matching '${tag}'"
    fail=1
  fi
done

# I7.  Layout-derived resize must live in pre_draw, not inline in render.
#      Catches regressions like `tab.last_resize != Some(new_size)` blocks
#      reappearing in the reader render paths.
for f in trench/src/ui/layout/main_row.rs trench/src/ui/layout/reader.rs; do
  if grep -qE 'last_resize\s*!=\s*Some' "$f"; then
    echo "FAIL: ${f} contains an inline last_resize check — pre_draw should own it (ADR-002 §D3)"
    fail=1
  fi
done

if [[ "$fail" -eq 0 ]]; then
  echo "OK: render-purification invariants hold (feed + reader)"
fi
exit $fail
