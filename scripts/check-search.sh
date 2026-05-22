#!/usr/bin/env bash
#
# Feed-search invariants for ADR-012 (grammar) + ADR-013 (mechanism:
# async, off-thread, incremental ranking via nucleo).
#
# Sibling to scripts/check-subject-browser.sh. This one guards the search
# slice: the single visible-set function, the off-thread nucleo engine,
# the parser grammar, and that the synchronous SkimMatcherV2 path stays
# removed.
#
# Exit codes:
#   0  all invariants hold
#   1  one or more invariants violated (details printed)

set -Eeuo pipefail

cd "$(dirname "$0")/.."

fail=0

# ── Search slice (ADR-012 grammar, ADR-013 mechanism) ──────────────────

# Q1.  No inline substring search left in app/mod.rs::visible_items.
#      Every tab collapses onto visible_indices_for; a stray
#      `title_lower.contains` in app/mod.rs would mean the duplicate
#      filter is back and selection could disagree with the rendered
#      ranked list. (History's title_lower.contains lives in
#      app/methods/history.rs and feed/mod.rs::filtered_history_for.)
if grep -nE '\.title_lower\.contains' trench/src/app/mod.rs; then
  echo "FAIL: inline title_lower substring search found in app/mod.rs (visible_items must delegate to visible_indices_for)"
  fail=1
fi

# Q2.  Ranking runs OFF-THREAD via nucleo (ADR-013 §D1): the dependency
#      exists and the feed consumes the engine's ranked snapshot.
if ! grep -qE '^nucleo *=' trench/Cargo.toml; then
  echo "FAIL: nucleo dependency missing from trench/Cargo.toml (ADR-013 §D1)"
  fail=1
fi
if ! grep -qE 'ranked_indices\(\)' trench/src/feed/mod.rs; then
  echo "FAIL: visible_indices_for does not consume the nucleo snapshot (engine.ranked_indices()) (ADR-013 §D1)"
  fail=1
fi

# Q3.  The synchronous fuzzy matcher stays removed (ADR-013 supersedes
#      ADR-012 §D3-D4). No fuzzy-matcher dependency, no SkimMatcherV2,
#      no per-item Query::score in the render path.
if grep -qE '^fuzzy-matcher *=' trench/Cargo.toml; then
  echo "FAIL: fuzzy-matcher dependency is back — synchronous matching must stay off the render thread (ADR-013)"
  fail=1
fi
if grep -rqE 'SkimMatcherV2|fuzzy_matcher' trench/src; then
  echo "FAIL: SkimMatcherV2 / fuzzy_matcher reference found in trench/src (ADR-013 removed the synchronous matcher)"
  fail=1
fi

# Q4.  The parser grammar exists: all five field prefixes are recognised.
for prefix in '"ti"' '"abs"' '"au"' '"cat"' '"year"'; do
  if ! grep -qF "$prefix" trench/src/search/mod.rs; then
    echo "FAIL: search/mod.rs parser missing field prefix arm ${prefix} (ADR-012 §D2)"
    fail=1
  fi
done

# Q5.  The feed search path parses the raw query (the engine + gates read
#      feed.search_query directly; the lowercase mirror belongs to History).
if grep -nE 'fn visible_indices_for' trench/src/feed/mod.rs >/dev/null; then
  body=$(awk '/pub fn visible_indices_for/{p=1} p; /^}/{if(p)exit}' trench/src/feed/mod.rs)
  if ! grep -qE 'Query::parse\(&feed\.search_query\)' <<<"$body"; then
    echo "FAIL: visible_indices_for does not parse feed.search_query via search::Query::parse"
    fail=1
  fi
fi

# Q6.  ADR-013 cadence text references the shipped PRs. Mirror of P5/O5.
adr13="docs/adr/ADR-013-async-incremental-search.md"
for tag in "PR 1" "PR 2"; do
  if ! grep -qF "$tag" "$adr13"; then
    echo "FAIL: ${adr13} status block missing reference to '${tag}'"
    fail=1
  fi
done

if [[ "$fail" -eq 0 ]]; then
  echo "OK: search invariants hold (single visible-set fn, off-thread nucleo ranking, no SkimMatcherV2, 5 field prefixes, raw-query path, ADR-013 cadence intact)"
fi
exit $fail
