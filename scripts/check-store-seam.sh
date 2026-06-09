#!/usr/bin/env bash
#
# Store-seam invariants for ADR-006. Locks in the load_json<T> /
# save_json<T> envelope so future persistent-state additions can't
# silently bypass the quarantine + atomic-write defence-in-depth.
#
# Sibling to scripts/check-render-purification.sh and
# scripts/check-ingestion-seam.sh — same fail-loud-on-regression
# shape, different layer of the system.
#
# Exit codes:
#   0  all invariants hold
#   1  one or more invariants violated (details printed)

set -Eeuo pipefail

cd "$(dirname "$0")/.."

fail=0

# Every per-module wrapper should reference the seam. Whitelisted
# exemptions are documented inline via `// SEAM-EXEMPT:` comments
# (e.g. session::clear which writes literal `{}` to erase, not save).
seam_modules=(
  "one-research/src/store/cache.rs"
  "one-research/src/store/discovery_cache.rs"
  "one-research/src/store/enrichment_cache.rs"
  "one-research/src/store/history.rs"
  "one-research/src/store/session.rs"
  "one-research/src/store/tags.rs"
)

# L1.  Every store submodule references `super::load_json` and
#      `super::save_json` (or annotates the absence with SEAM-EXEMPT).
for f in "${seam_modules[@]}"; do
  has_load=$(grep -c "super::load_json" "$f" || true)
  has_save=$(grep -c "super::save_json" "$f" || true)
  exempt=$(grep -c "SEAM-EXEMPT:" "$f" || true)
  if [[ "$has_load" -eq 0 && "$exempt" -eq 0 ]]; then
    echo "FAIL: ${f} does not reference super::load_json and has no SEAM-EXEMPT note (ADR-006 §S1)"
    fail=1
  fi
  if [[ "$has_save" -eq 0 && "$exempt" -eq 0 ]]; then
    echo "FAIL: ${f} does not reference super::save_json and has no SEAM-EXEMPT note (ADR-006 §S1)"
    fail=1
  fi
done

# L2.  No `serde_json::from_slice` or `serde_json::to_vec` inside
#      one-research/src/store/ outside store/mod.rs (which owns the seam
#      itself).  Test-fixture uses inside `#[cfg(test)]` blocks are
#      exempt — the L2 grep filters them by line context.
hits=$(grep -rnE 'serde_json::(from_slice|to_vec)' one-research/src/store \
  | grep -v 'mod\.rs:' \
  | grep -v 'SEAM-EXEMPT' \
  || true)
# Filter out lines that are inside a `#[cfg(test)]` block by checking
# the file region.  Cheap awk pass: only flag matches whose enclosing
# function isn't under a `#[cfg(test)]` attribute.
filtered=""
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  file=$(echo "$line" | cut -d: -f1)
  lineno=$(echo "$line" | cut -d: -f2)
  # Walk backward from the matched line looking for the nearest
  # `#[cfg(test)]` or `mod tests` attribute (test-context) vs a regular
  # `pub fn` / `fn` declaration (production-context).  The first one
  # wins.
  context=$(awk -v ln="$lineno" '
    NR <= ln {
      if ($0 ~ /#\[cfg\(test\)\]/) ctx = "test"
      else if ($0 ~ /^mod tests/) ctx = "test"
      else if ($0 ~ /^(pub )?fn /) ctx = "prod"
    }
    END { print ctx }
  ' "$file")
  if [[ "$context" != "test" ]]; then
    filtered+="$line"$'\n'
  fi
done <<< "$hits"
if [[ -n "${filtered// }" ]]; then
  echo "FAIL: serde_json::{from_slice,to_vec} found outside store/mod.rs without SEAM-EXEMPT or test context (ADR-006 §S1):"
  echo "$filtered" | sed 's/^/  /'
  fail=1
fi

# L3.  No direct `super::atomic_write` inside one-research/src/store/
#      submodules — the seam is the choke point.  session::clear() is
#      the documented exception, annotated SEAM-EXEMPT.
for f in "${seam_modules[@]}"; do
  hits=$(grep -nE 'super::atomic_write\(' "$f" \
    | awk -F: -v path="$f" '
      NR == FNR { lines[$2] = $0; next }
    ' "$f" "$f" \
    || true)
  raw_hits=$(grep -nE 'super::atomic_write\(' "$f" || true)
  if [[ -n "$raw_hits" ]]; then
    while IFS= read -r line; do
      [[ -z "$line" ]] && continue
      lineno=$(echo "$line" | cut -d: -f1)
      # Walk backward up to 5 lines looking for SEAM-EXEMPT comment.
      start=$((lineno > 5 ? lineno - 5 : 1))
      window=$(awk -v s="$start" -v e="$lineno" 'NR >= s && NR <= e' "$f")
      if ! echo "$window" | grep -q "SEAM-EXEMPT:"; then
        echo "FAIL: ${f}:${lineno} calls super::atomic_write without SEAM-EXEMPT (ADR-006 §S1)"
        echo "  $line" | sed 's/^/  /'
        fail=1
      fi
    done <<< "$raw_hits"
  fi
done

# L4.  ADR-006 cadence table mentions every committed PR.  Mirrors I2 /
#      I6 / K4 — the table can't silently drop a row.
adr6="docs/adr/ADR-006-store-seam.md"
for tag in "| 1 |" "| 2 |" "| 3 |"; do
  if ! grep -qF "$tag" "$adr6"; then
    echo "FAIL: ${adr6} cadence table missing entry matching '${tag}'"
    fail=1
  fi
done

if [[ "$fail" -eq 0 ]]; then
  echo "OK: store-seam invariants hold (load_json / save_json × ${#seam_modules[@]} modules)"
fi
exit $fail
