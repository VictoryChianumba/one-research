#!/usr/bin/env bash
#
# Design-language invariants for the minimal, header-less surfaces.
# Locks in the 2026-06-01 refinement that retired the `─── Title ───`
# divider-title pattern from the feed, details, browse, and filter
# surfaces. Dividers are now plain `─` rules; a divider may carry a
# label ONLY when it is dynamic wayfinding (notes-pane mode indicator,
# reader-mode workspace panes — a separate long-form design pass).
#
# Sibling to scripts/check-frame-layout.sh and the other ADR tripwires.
# The rule lives in AGENTS.md (no ADR slice), so R3/R4 keep the doc
# rationale travelling with the code guard. CLAUDE.md is a gitignored
# local mirror — checked too when present, but never required, so the
# guard holds in a clean CI checkout where only AGENTS.md exists.
#
# Exit codes:
#   0  all invariants hold
#   1  one or more invariants violated (details printed)

set -Eeuo pipefail

cd "$(dirname "$0")/.."

fail=0

# R1.  The centered `─── Activity ───` divider title stays gone from the
#      details panel. The divider survives as a plain full-width rule;
#      only the label was removed. (The unrelated "Activity log" comment
#      in data/workspace_store.rs is outside ui/layout and untouched.)
hits=$(grep -rnF '" Activity "' one-research/src/ui/layout/ 2>/dev/null || true)
if [[ -n "$hits" ]]; then
  echo "FAIL: '\" Activity \"' divider title reappeared — dividers are plain rules (CLAUDE.md design language):"
  echo "$hits" | sed 's/^/  /'
  fail=1
fi

# R2.  No "Browse" / "Filters" string literal is passed as a section
#      title in the feed-layout dispatcher. main_row.rs is the only file
#      that drives the feed / details / browse / narrow arms; after the
#      refinement these arms pass `""` to the split-box helpers. Comment
#      lines (`// Browse (ADR-011)…`) and the `FeedTab::Browse` enum
#      variant carry no quotes, so the quoted-literal grep skips them.
#      (filter.rs legitimately uses the bare `"Browse"` value as a
#      subject-follow toggle label — that is NOT a divider title, hence
#      this check is scoped to main_row.rs only.)
hits=$(grep -nE '"(Browse|Filters)"' one-research/src/ui/layout/main_row.rs 2>/dev/null \
  | grep -vE '^[0-9]+:[[:space:]]*//' || true)
if [[ -n "$hits" ]]; then
  echo "FAIL: '\"Browse\"' / '\"Filters\"' section title reappeared in main_row.rs (CLAUDE.md design language):"
  echo "$hits" | sed 's/^/  /'
  fail=1
fi

# AGENTS.md is the tracked design-language doc and is always present.
# CLAUDE.md is a gitignored local mirror — guard it too when it exists,
# but never require it (a clean CI checkout has only AGENTS.md).
docs="AGENTS.md"
[[ -f CLAUDE.md ]] && docs="$docs CLAUDE.md"

# R3.  The retired prescriptive doc lines do not creep back into the
#      design-language docs. These exact phrasings told contributors to
#      embed `─── Title ───` titles; their return would re-license the
#      pattern this script exists to forbid.
for f in $docs; do
  bad=$(grep -nF -e 'Section titles in the divider line format' \
                 -e 'Section titles embedded into divider' "$f" 2>/dev/null || true)
  if [[ -n "$bad" ]]; then
    echo "FAIL: retired design-rule phrasing returned to ${f}:"
    echo "$bad" | sed 's/^/  /'
    fail=1
  fi
done

# R4.  The replacement rule is stated in the docs — the word "retired"
#      anchors the rationale so a future contributor reads WHY before
#      reverting. Mirrors the ADR-cadence checks in sibling scripts.
for f in $docs; do
  if ! grep -qiF 'retired' "$f"; then
    echo "FAIL: ${f} no longer marks the divider-title pattern 'retired' — design rationale lost"
    fail=1
  fi
done

if [[ "$fail" -eq 0 ]]; then
  echo "OK: design-language invariants hold (plain dividers; no static section titles on feed surfaces)"
fi
exit $fail
