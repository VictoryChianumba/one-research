#!/usr/bin/env bash
#
# Ingestion-seam invariants for ADR-004 (C10).
#
# Sibling to scripts/check-render-purification.sh — that one enforces the
# per-pane composition-root rules (ADR-001/2/3); this one enforces the
# Source / EnrichmentSource trait surface and the retry seam.
#
# Exit codes:
#   0  all invariants hold
#   1  one or more invariants violated (details printed)

set -Eeuo pipefail

cd "$(dirname "$0")/.."

fail=0

# ── Slice-4 (ingestion seam, ADR-004) ──────────────────────────────────

# J1.  Exactly five Source impls — one per bulk-ingestion module:
#      ArxivSource, HuggingFaceSource, RssSource, OpenReviewSource,
#      CoreSource.  Catches a future drift where a new source is added
#      as a free function instead of through the trait, or where a
#      Source impl is silently deleted.
src_count=$(grep -rE '^impl Source for [A-Z]' trench/src/ingestion/ | wc -l | tr -d ' ')
if [[ "$src_count" -ne 5 ]]; then
  echo "FAIL: expected exactly 5 'impl Source for' blocks in trench/src/ingestion/, found ${src_count}"
  echo "      (ArxivSource, HuggingFaceSource, RssSource, OpenReviewSource, CoreSource)"
  grep -rnE '^impl Source for [A-Z]' trench/src/ingestion/ | sed 's/^/  /'
  fail=1
fi

# J2.  Exactly two EnrichmentSource impls — SemanticScholarEnrichment,
#      HuggingFaceRepoEnrichment.  Catches a future drift where post-fetch
#      mutation grows back into the orchestrator instead of behind the
#      trait.
enr_count=$(grep -rE '^impl EnrichmentSource for [A-Z]' trench/src/ingestion/ | wc -l | tr -d ' ')
if [[ "$enr_count" -ne 2 ]]; then
  echo "FAIL: expected exactly 2 'impl EnrichmentSource for' blocks in trench/src/ingestion/, found ${enr_count}"
  echo "      (SemanticScholarEnrichment, HuggingFaceRepoEnrichment)"
  grep -rnE '^impl EnrichmentSource for [A-Z]' trench/src/ingestion/ | sed 's/^/  /'
  fail=1
fi

# J3.  The deleted retry helper must not return.  PR 2 collapsed the
#      29-line `fetch_arxiv_with_retry` into `crates/http::with_retry` +
#      `RetryPolicy::arxiv()`.  If it reappears, the seam was bypassed.
#      Matches actual `fn` declarations only; the docstring history
#      references ("Pre-C10 this lived in a local fetch_arxiv_with_retry
#      helper") are skipped because they live in `///` comment lines.
if grep -rnE '^[[:space:]]*(pub )?fn fetch_arxiv_with_retry\b' trench/src/ crates/ 2>/dev/null; then
  echo "FAIL: 'fn fetch_arxiv_with_retry' resurfaced — PR 2 deleted this helper (ADR-004 §S5 J3)"
  echo "      Use crate::ingestion::pipeline::FetchContext::with_retry instead."
  fail=1
fi

# J4 (scoped).  Inside `impl Source for X { fn fetch ... }` and
#               `impl EnrichmentSource for X { fn enrich ... }` blocks,
#               no direct `crate::http::client()` call — must reach HTTP
#               through `ctx.http()` / `ctx.with_retry`.  Legacy free
#               functions (arxiv::fetch called from discovery,
#               rss::fetch as Source-impl delegate, etc.) retain
#               `crate::http::client()` because they have no FetchContext
#               available — the seam lives at the trait-impl boundary.
#               See ADR-004 §S5 J4 for the scope decision.
for f in trench/src/ingestion/*.rs; do
  hits=$(awk '
    /^impl (Source|EnrichmentSource) for / { in_impl = 1; brace = 0; next }
    in_impl {
      brace += gsub(/{/, "{")
      brace -= gsub(/}/, "}")
      if (/crate::http::client\(\)|super::http::client\(\)/) {
        printf("%s:%d: %s\n", FILENAME, NR, $0)
      }
      if (brace < 0) { in_impl = 0 }
    }
  ' "$f")
  if [[ -n "$hits" ]]; then
    echo "FAIL: ${f} — Source/EnrichmentSource impl body reaches around FetchContext (ADR-004 §S5 J4):"
    echo "$hits" | sed 's/^/  /'
    fail=1
  fi
done

# J5.  ADR-004 cadence table mentions every PR.  Same shape as I2/I6
#      in the render-purification script — guards against the cadence
#      table drifting out of sync with what actually shipped.
adr4="docs/adr/ADR-004-ingestion-seam.md"
for tag in "| 1 |" "| 2 |" "| 3 |"; do
  if ! grep -qF "$tag" "$adr4"; then
    echo "FAIL: ${adr4} cadence table missing entry matching '${tag}'"
    fail=1
  fi
done

if [[ "$fail" -eq 0 ]]; then
  echo "OK: ingestion-seam invariants hold (Source × 5, EnrichmentSource × 2)"
fi
exit $fail
