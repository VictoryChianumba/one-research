#!/usr/bin/env bash
#
# Subject-browser invariants for ADR-010.
#
# Sibling to scripts/check-ingestion-seam.sh — that one guards the
# `Source` trait surface (5 impls, no inline retry); this one guards
# the taxonomy table, the `FeedTab::Browse` dispatch coverage, the
# trait-non-membership (Browse is NOT a `Source`), and the deletion
# of the legacy `KNOWN_ARXIV_CATS` shortlist.
#
# Exit codes:
#   0  all invariants hold
#   1  one or more invariants violated (details printed)

set -Eeuo pipefail

cd "$(dirname "$0")/.."

fail=0

# ── Slice-10 (subject browser, ADR-010) ────────────────────────────────

# O1.  TAXONOMY const has exactly 8 top-level Group entries. arXiv has
#      not added a top-level group in over a decade; off-by-one here is
#      almost certainly an accidental edit rather than a real schema
#      change. The inline test taxonomy_has_eight_groups also anchors
#      this; the grep catches accidental const edits before the test
#      runs.
group_count=$(grep -cE '^  Group \{$' trench/src/models/arxiv_taxonomy.rs || true)
if [[ "$group_count" -ne 8 ]]; then
  echo "FAIL: expected exactly 8 'Group {' entries in trench/src/models/arxiv_taxonomy.rs, found ${group_count}"
  echo "      (Physics, Mathematics, Computer Science, Quantitative Biology, Quantitative Finance, Statistics, EESS, Economics)"
  fail=1
fi

# O2.  Every file that uses `FeedTab::Inbox =>` (match-arm syntax) should
#      also have `FeedTab::Browse =>`. The match-arm pattern is the
#      precise signal — bare `FeedTab::Inbox` as a return value (e.g.
#      WorkflowState → FeedTab mapping in app/methods/history.rs) or
#      as a test fixture (render_caches.rs) is not a dispatch site and
#      doesn't need a Browse arm.
inbox_dispatch=$(grep -lrE 'FeedTab::Inbox[[:space:]]*=>' trench/src/ 2>/dev/null | sort -u)
browse_dispatch=$(grep -lrE 'FeedTab::Browse[[:space:]]*=>' trench/src/ 2>/dev/null | sort -u)
missing=$(comm -23 <(echo "$inbox_dispatch") <(echo "$browse_dispatch") || true)
if [[ -n "$missing" ]]; then
  echo "FAIL: file(s) have 'FeedTab::Inbox =>' arms but no 'FeedTab::Browse =>' arm:"
  echo "$missing" | sed 's/^/  /'
  fail=1
fi

# O3.  Browse is NOT a Source impl. The Source trait (ADR-004 §D1) is
#      bulk-refresh-only; Browse is selection-driven and uses a worker
#      thread (browse/pipeline.rs::spawn_browse_fetch). Registering
#      Browse as a Source would silently double-fetch every browsed
#      category on each refresh.
if grep -rnE '^impl Source for (Browse|Subject)' trench/src/ 2>/dev/null; then
  echo "FAIL: 'impl Source for Browse*' found — Browse must use the worker pattern, not the Source trait (ADR-010 §D3, ADR-004 §D1)"
  fail=1
fi

# O4.  KNOWN_ARXIV_CATS stays deleted. The 7-entry shortlist was
#      replaced by the full taxonomy + Browse `p` promotion gesture
#      (ADR-010 §D5). Reviving the const would re-create the dual-
#      management ambiguity D5 explicitly removed.
if grep -rnE '\bKNOWN_ARXIV_CATS\b' trench/src/ crates/ 2>/dev/null; then
  echo "FAIL: KNOWN_ARXIV_CATS resurfaced — PR 3 deleted this const (ADR-010 §D5 O4)"
  echo "      arXiv categories are now managed exclusively in Browse."
  fail=1
fi

# O5.  ADR-010 cadence text mentions every shipped PR. Same shape as
#      J5 / I2 — guards against the Status line drifting out of sync
#      with what actually shipped.
adr10="docs/adr/ADR-010-subject-browser.md"
for tag in "PR 1 (" "PR 2 (" "PR 3 ("; do
  if ! grep -qF "$tag" "$adr10"; then
    echo "FAIL: ${adr10} status block missing reference to '${tag}'"
    fail=1
  fi
done

if [[ "$fail" -eq 0 ]]; then
  echo "OK: subject-browser invariants hold (taxonomy × 8 groups, dispatch coverage, Browse ∉ Source, KNOWN_ARXIV_CATS removed)"
fi
exit $fail
