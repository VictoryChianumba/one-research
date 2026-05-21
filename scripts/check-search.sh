#!/usr/bin/env bash
#
# Feed-search invariants for ADR-012 (fuzzy, field-scoped, ranked search).
#
# Sibling to scripts/check-subject-browser.sh. That one guards the
# Browse surface; this one guards the search slice: the single
# visible-set function, the relevance-ranking pass, the parser grammar,
# and the move off the lowercase-substring path.
#
# Exit codes:
#   0  all invariants hold
#   1  one or more invariants violated (details printed)

set -Eeuo pipefail

cd "$(dirname "$0")/.."

fail=0

# ── Slice-12 (fuzzy ranked search, ADR-012) ────────────────────────────

# Q1.  No inline substring search left in app/mod.rs::visible_items.
#      ADR-012 §D5 collapses every tab onto visible_indices_for; a stray
#      `title_lower.contains` in app/mod.rs would mean the duplicate
#      filter is back and selection could disagree with the rendered
#      ranked list. (History's title_lower.contains lives in
#      app/methods/history.rs and feed/mod.rs::filtered_history_for —
#      out of scope here.)
if grep -nE '\.title_lower\.contains' trench/src/app/mod.rs; then
  echo "FAIL: inline title_lower substring search found in app/mod.rs (ADR-012 §D5 — visible_items must delegate to visible_indices_for)"
  fail=1
fi

# Q2.  Relevance ranking is wired into visible_indices_for: the scoring
#      call and the stable score-sort must both be present.
if ! grep -qE 'query\.score\(' trench/src/feed/mod.rs; then
  echo "FAIL: feed/mod.rs does not call query.score(...) (ADR-012 §D4 relevance scoring missing)"
  fail=1
fi
if ! grep -qE 'scored\.sort_by' trench/src/feed/mod.rs; then
  echo "FAIL: feed/mod.rs missing the relevance sort (scored.sort_by) (ADR-012 §D4)"
  fail=1
fi

# Q3.  The parser grammar exists: all four field prefixes are recognised
#      in the search module. Catches an accidental rename/drop of a
#      prefix arm.
for prefix in '"ti"' '"abs"' '"au"' '"cat"' '"year"'; do
  if ! grep -qF "$prefix" trench/src/search/mod.rs; then
    echo "FAIL: search/mod.rs parser missing field prefix arm ${prefix} (ADR-012 §D2)"
    fail=1
  fi
done

# Q4.  The feed search path parses the raw query, not the lowercase
#      mirror. SkimMatcherV2 is smart-case (ADR-012 §D3); reintroducing
#      search_query_lower into visible_indices_for would mean someone
#      rebuilt the old substring path.
if grep -nE 'fn visible_indices_for' trench/src/feed/mod.rs >/dev/null; then
  body=$(awk '/pub fn visible_indices_for/{p=1} p; /^}/{if(p)exit}' trench/src/feed/mod.rs)
  if ! grep -qE 'Query::parse\(&feed\.search_query\)' <<<"$body"; then
    echo "FAIL: visible_indices_for does not parse feed.search_query via search::Query::parse (ADR-012 §D1)"
    fail=1
  fi
  if grep -qE 'search_query_lower' <<<"$body"; then
    echo "FAIL: visible_indices_for references search_query_lower — feed search must use the fuzzy path, not the lowercase substring mirror (ADR-012 §D3)"
    fail=1
  fi
fi

# Q5.  ADR-012 cadence text references the shipped PR. Mirror of P5/O5.
adr12="docs/adr/ADR-012-fuzzy-ranked-search.md"
if ! grep -qF "PR 1 (" "$adr12"; then
  echo "FAIL: ${adr12} status block missing reference to 'PR 1 ('"
  fail=1
fi

if [[ "$fail" -eq 0 ]]; then
  echo "OK: search invariants hold (single visible-set fn, relevance sort + query.score present, 5 field prefixes incl. cat:, raw-query fuzzy path, ADR-012 cadence intact)"
fi
exit $fail
