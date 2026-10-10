#!/usr/bin/env bash
# Record real API responses into one-research/tests/fixtures/ for the
# offline parser tests. Run by .github/workflows/record-fixtures.yml (the
# sandbox and CI test jobs are offline by design); can also be run locally.
#
# Each request mirrors what the app sends (same endpoint and query shape),
# with result counts reduced to keep fixtures small. A response is only
# written if it is HTTP 200 and passes a basic shape check, so an error
# page, a rate-limit body or an empty result can never become a fixture.
#
# CORE needs an API key: set CORE_API_KEY, otherwise CORE is skipped and
# reported (never faked).
set -euo pipefail

out="${1:-one-research/tests/fixtures}"
mkdir -p "$out"
ua="one-research-fixture-recorder (+https://github.com/VictoryChianumba/one-research)"
recorded_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
manifest_entries=()
failed=0

# Print a failure; in GitHub Actions also emit it as an annotation so it
# shows on the run summary / PR without opening the log.
fail() {
  echo "FAIL $*" >&2
  if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    local msg="${*//'%'/'%25'}"; msg="${msg//$'\n'/'%0A'}"
    echo "::error title=record-fixtures::${msg}"
  fi
  failed=1
}

# fetch NAME URL SHAPE_REGEX [extra curl args...]
fetch() {
  local name="$1" url="$2" shape="$3"
  shift 3
  local tmp status
  tmp="$(mktemp)"
  # curl prints 000 itself when the request fails outright.
  status="$(curl -sS -L --max-time 60 -A "$ua" -o "$tmp" -w '%{http_code}' "$@" "$url" 2>/dev/null)" || true
  if [[ "$status" != "200" ]]; then
    fail "$name: HTTP $status from $url"$'\n'"$(head -c 300 "$tmp")"
    rm -f "$tmp"; return
  fi
  if ! grep -Eq "$shape" "$tmp"; then
    fail "$name: response does not look right (expected /$shape/)"$'\n'"$(head -c 300 "$tmp")"
    rm -f "$tmp"; return
  fi
  mv "$tmp" "$out/$name"
  local size sha
  size="$(wc -c < "$out/$name" | tr -d ' ')"
  sha="$(sha256sum "$out/$name" | cut -d' ' -f1)"
  echo "ok   $name ($size bytes)"
  manifest_entries+=("$(jq -n --arg f "$name" --arg u "$url" --arg t "$recorded_at" \
    --arg s "$sha" --argjson b "$size" \
    '{file: $f, source_url: $u, recorded_at: $t, sha256: $s, bytes: $b}')")
}

# arXiv listing — arxiv::fetch_page(["cs.LG"], 0, n).
fetch arxiv_cs_lg.atom.xml \
  "https://export.arxiv.org/api/query?search_query=cat:cs.LG&sortBy=submittedDate&sortOrder=descending&start=0&max_results=5" \
  '<entry>'
sleep 4 # arXiv asks for >= 3s between requests

# arXiv id_list — the batched abstract lookup in huggingface::fetch_abstracts.
fetch arxiv_id_list.atom.xml \
  "https://export.arxiv.org/api/query?id_list=1706.03762,2005.14165,2312.00752&max_results=3" \
  '<entry>'

# Hugging Face daily papers page — huggingface::fetch parses this HTML.
fetch huggingface_papers.html "https://huggingface.co/papers" 'href="/papers/[0-9]'

# OpenReview v2 notes — openreview::fetch_venue for the first DEFAULT_VENUES entry.
fetch openreview_iclr2025.json \
  "https://api2.openreview.net/notes?invitation=ICLR.cc%2F2025%2FConference%2F-%2FSubmission&limit=5&offset=0" \
  '"notes": ?\[ ?\{'

# CORE v3 search — core::fetch (Bearer key in a header, as the app sends it).
if [[ -n "${CORE_API_KEY:-}" ]]; then
  fetch core_search.json \
    "https://api.core.ac.uk/v3/search/works?q=artificial+intelligence+machine+learning&limit=5&sort=recency" \
    '"results": ?\[ ?\{' \
    -H "Authorization: Bearer ${CORE_API_KEY}"
else
  echo "SKIP core_search.json: CORE_API_KEY not set" >&2
  if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    echo "::warning title=record-fixtures::CORE skipped: CORE_API_KEY secret not set"
  fi
fi

# Merge into the manifest, keeping entries for files not re-recorded
# this run (e.g. CORE when no key is available).
manifest="$out/manifest.json"
[[ -f "$manifest" ]] || echo '[]' > "$manifest"
new="$(printf '%s\n' "${manifest_entries[@]:-}" | jq -s 'map(select(. != null))')"
jq --argjson new "$new" \
  '(map(select(.file as $f | ($new | map(.file) | index($f)) | not)) + $new) | sort_by(.file)' \
  "$manifest" > "$manifest.tmp" && mv "$manifest.tmp" "$manifest"

exit "$failed"
