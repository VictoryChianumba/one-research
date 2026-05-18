#!/usr/bin/env bash
#
# ItemStore invariants for ADR-007. Locks in the Workspace
# encapsulation so future PRs can't re-expose items + url_index +
# arxiv_id_index as raw fields.
#
# Sibling to scripts/check-render-purification.sh,
# scripts/check-ingestion-seam.sh, and scripts/check-store-seam.sh.
#
# Exit codes:
#   0  all invariants hold
#   1  one or more invariants violated (details printed)

set -Eeuo pipefail

cd "$(dirname "$0")/.."

fail=0

workspace_rs="trench/src/data/workspace_store.rs"
app_mod="trench/src/app/mod.rs"

# M1.  Workspace must declare `pub items_store: ItemStore` and must NOT
#      declare `pub items:` / `pub url_index:` / `pub arxiv_id_index:`
#      at the struct level.  Awk-scoped to the Workspace struct body —
#      the raw fields still exist inside ItemStore (which is correct).
workspace_body=$(awk '/^pub struct Workspace \{/,/^\}/' "$workspace_rs")
if ! echo "$workspace_body" | grep -qE '\bpub\s+items_store\s*:\s*ItemStore\b'; then
  echo "FAIL: ${workspace_rs} Workspace missing 'pub items_store: ItemStore' (ADR-007 §S2)"
  fail=1
fi
for field in "pub items:" "pub url_index:" "pub arxiv_id_index:"; do
  if echo "$workspace_body" | grep -qE "\b${field}"; then
    echo "FAIL: ${workspace_rs} Workspace still declares '${field}' — should be encapsulated in items_store (ADR-007 §S2)"
    fail=1
  fi
done

# M2.  No expression matches `\.items_store\.items` (raw vec access),
#      `\.items_store\.url_index`, or `\.items_store\.arxiv_id_index`
#      outside item_store.rs.  Reads go through methods — get, iter,
#      find_by_url, find_by_arxiv_id, etc.  `.items_store.items()` is
#      the slice escape hatch and is allowed (note the `()`).
hits=$(grep -rnE '\.items_store\.items[^(]' trench/src/ \
  | grep -v "^trench/src/data/item_store.rs:" \
  | grep -v 'SEAM-EXEMPT' \
  || true)
if [[ -n "$hits" ]]; then
  echo "FAIL: raw items_store.items vec access found outside item_store.rs (ADR-007 §S1):"
  echo "$hits" | sed 's/^/  /'
  fail=1
fi
for field in "url_index" "arxiv_id_index"; do
  hits=$(grep -rnE "\.items_store\.${field}\b" trench/src/ \
    | grep -v "^trench/src/data/item_store.rs:" \
    | grep -v 'SEAM-EXEMPT' \
    || true)
  if [[ -n "$hits" ]]; then
    echo "FAIL: raw items_store.${field} access found outside item_store.rs (ADR-007 §S1):"
    echo "$hits" | sed 's/^/  /'
    fail=1
  fi
done

# M3.  No `workspace.url_index.(insert|remove)` or
#      `workspace.arxiv_id_index.(insert|remove)` calls anywhere in
#      trench/src.  Index mutation is ItemStore-internal.  The legacy
#      field names should produce E0609 at the type level too, but
#      this catches docs/comments that leak old patterns and any
#      future-typo'd field name re-introduction.
for op in "url_index" "arxiv_id_index"; do
  hits=$(grep -rnE "workspace\.${op}\.(insert|remove)\(" trench/src/ \
    | grep -v 'SEAM-EXEMPT' \
    || true)
  if [[ -n "$hits" ]]; then
    echo "FAIL: workspace.${op}.{insert,remove} call found — index mutation must go through ItemStore methods (ADR-007 §S1):"
    echo "$hits" | sed 's/^/  /'
    fail=1
  fi
done

# M4.  ADR-007 cadence table mentions every committed PR.  Mirrors
#      I2 / I6 / K4 / L4.
adr7="docs/adr/ADR-007-item-store.md"
for tag in "| 1 |" "| 2 |" "| 3 |"; do
  if ! grep -qF "$tag" "$adr7"; then
    echo "FAIL: ${adr7} cadence table missing entry matching '${tag}'"
    fail=1
  fi
done

if [[ "$fail" -eq 0 ]]; then
  echo "OK: item-store invariants hold (Workspace encapsulated, mutation through ItemStore methods)"
fi
exit $fail
