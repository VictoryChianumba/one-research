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

# ── Slice-11 (rail-shaped browse, ADR-011) ─────────────────────────────

# P1.  BrowseModel must not carry the deleted ADR-010 fields. The rail
#      refactor (ADR-011 PR 1) replaced three parallel cursors with a
#      single rail_path + rail_cursor. Reviving any of the three would
#      mean the rail reshape was silently regressed.
if grep -nE '^\s*pub (focused_column|archives|categories):' \
  trench/src/app/state/browse.rs 2>/dev/null
then
  echo "FAIL: BrowseModel resurfaced a deleted ADR-010 field (ADR-011 §E2 P1)"
  echo "      Expected fields: rail_path, rail_cursor, recent, loaded_categories, inflight, tx, rx."
  fail=1
fi

# P2.  draw_browse_detail_panel was deleted in ADR-011 PR 1. The
#      metadata side pane is gone; the feed table on the right shows
#      the items. A revival would mean the rail reshape's central
#      simplification was undone.
if grep -nE 'fn draw_browse_detail_panel' \
  trench/src/ui/layout/browse.rs 2>/dev/null
then
  echo "FAIL: draw_browse_detail_panel resurfaced (ADR-011 P2)"
  echo "      The details side pane is gone in the rail design."
  fail=1
fi

# P3.  FeedSortMode has exactly four variants: Dated, Random, Popular,
#      Trending. A fifth variant is an unplanned mode addition that
#      needs ADR-level review.
sort_variants=$(grep -cE '^\s*(Dated|Random|Popular|Trending),' \
  trench/src/feed/mod.rs || true)
if [[ "$sort_variants" -ne 4 ]]; then
  echo "FAIL: FeedSortMode expected 4 variants (Dated, Random, Popular, Trending), found ${sort_variants} (ADR-011 P3)"
  fail=1
fi

# P4.  The Subject column renders in Browse only. A reference to
#      `Subject` cell rendering inside the *non-Browse* branch of
#      draw_item_table would mean the column has leaked into Inbox /
#      Library. The guard is structural: `show_subject_col` must be
#      derived from `feed_tab == FeedTab::Browse`.
if ! grep -qE 'show_subject_col.*=.*feed_tab.*Browse' \
  trench/src/ui/layout/feed.rs
then
  echo "FAIL: Subject column not gated on feed_tab == Browse (ADR-011 §E5 P4)"
  echo "      Expected: 'let show_subject_col = model.feed_tab == FeedTab::Browse;'"
  fail=1
fi

# P5.  ADR-011 cadence text mentions every shipped PR. Mirror of O5 / J5.
adr11="docs/adr/ADR-011-browse-scoped-feed.md"
for tag in "PR 1 (" "PR 2 (" "PR 3 ("; do
  if ! grep -qF "$tag" "$adr11"; then
    echo "FAIL: ${adr11} status block missing reference to '${tag}'"
    fail=1
  fi
done

if [[ "$fail" -eq 0 ]]; then
  echo "OK: subject-browser invariants hold (taxonomy × 8 groups, dispatch coverage, Browse ∉ Source, KNOWN_ARXIV_CATS removed, rail reshape locked: 4 sort modes, Subject col Browse-only, no draw_browse_detail_panel)"
fi
exit $fail
