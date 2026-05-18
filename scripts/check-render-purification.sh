#!/usr/bin/env bash
#
# Render-purification invariants for the per-pane composition-root
# refactor: ADR-001 (feed, slice 1), ADR-002 (reader, slice 2),
# ADR-003 (notes, slice 3), ADR-005 (discovery, slice 5).
#
# Fails CI if any of the locked-down properties regress.  Each slice
# extends this script with its own checks when it lands.
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


# ── Slice 3 (notes pane) ────────────────────────────────────────────────

# I8.  The 11 bifurcated notes_* / secondary_notes_* fields migrated by
#      Slice 3 PR 2 must stay off App's top level.  If a future PR
#      re-introduces any of them — say, as a "temporary" shortcut beside
#      app.notes — the bifurcation is back and the slice is undone.
#      Scoped to the App struct body via awk because `notes_context` is
#      also a legitimate field on `PendingTreadFetch` (a transient
#      pending-fetch struct, unrelated to slice 3).
app_struct_body=$(awk '/^pub struct App \{/,/^\}/' "$app_mod")
for field in \
  "pub notes_app:" \
  "pub notes_active:" \
  "pub notes_tabs:" \
  "pub notes_active_tab:" \
  "pub notes_mode:" \
  "pub notes_context:" \
  "pub secondary_notes_active:" \
  "pub secondary_notes_tabs:" \
  "pub secondary_notes_active_tab:" \
  "pub secondary_notes_mode:" \
  "pub secondary_notes_context:" \
; do
  if echo "$app_struct_body" | grep -qF "$field"; then
    echo "FAIL: ${app_mod} App still declares '${field}' — Slice 3 PR 2 migrated this into App.notes (ADR-003)"
    fail=1
  fi
done

# I9.  NotesPaneModel must be wired on App as the single composition root
#      for notes state.
if ! grep -qF "pub notes: NotesPaneModel" "$app_mod"; then
  echo "FAIL: ${app_mod} missing slice-3 model field 'pub notes: NotesPaneModel' (ADR-003 §D1)"
  fail=1
fi

# I10. NotesInstanceModel stays pure content — no visibility field.
#      Visibility is a pane-level concern (lives on NotesPaneModel as
#      primary_visible / secondary_visible) per ADR-003 §S3.  If an
#      instance grows a `visible: bool`, that decision must be revisited
#      in a new ADR, not slipped in.
notes_state="trench/src/app/state/notes.rs"
inst_body=$(awk '/^pub struct NotesInstanceModel/,/^\}/' "$notes_state")
if echo "$inst_body" | grep -qE '\bvisible\b'; then
  echo "FAIL: ${notes_state} — NotesInstanceModel grew a visibility field (ADR-003 §S3)"
  fail=1
fi

# I11. Render paths reach notes state via app.notes.* only.  The 11
#      migrated field names must not reappear as App-level shortcuts.
#      App-level method wrappers like app.notes_mode_for_side(...) are
#      explicitly allowed (ADR-003 §S4) — the word-boundary regex on the
#      11 exact field names excludes method calls.
for fld in notes_app notes_active notes_tabs notes_active_tab notes_mode notes_context \
           secondary_notes_active secondary_notes_tabs secondary_notes_active_tab \
           secondary_notes_mode secondary_notes_context; do
  hits=$(grep -rnE "\\bapp\\.${fld}\\b" trench/src/ 2>/dev/null | grep -v ':[[:space:]]*//' || true)
  if [[ -n "$hits" ]]; then
    echo "FAIL: render path reaches around App.notes via 'app.${fld}' (ADR-003 §I11):"
    echo "$hits" | sed 's/^/  /'
    fail=1
  fi
done

# ── Slice 5 (discovery pane) ────────────────────────────────────────────

# K1.  FeedModel must NOT carry a `discovery:` field.  Pre-C7 it nested
#      `DiscoveryModel` inside `FeedModel.discovery`; PR 2 promoted the
#      field to `App.discovery` (ADR-005 §S2).  A re-nesting would
#      collapse discovery back into feed.
feed_mod="trench/src/feed/mod.rs"
feed_struct_body=$(awk '/^pub struct FeedModel \{/,/^\}/' "$feed_mod")
if echo "$feed_struct_body" | grep -qE '\bpub\s+discovery\s*:'; then
  echo "FAIL: ${feed_mod} FeedModel still declares 'pub discovery:' — Slice 5 PR 2 promoted discovery to App.discovery (ADR-005 §S2)"
  fail=1
fi

# K2.  App must declare `pub discovery: DiscoveryModel` as a sibling to
#      feed/reader/notes — the slice-5 composition root.
if ! grep -qE 'pub discovery: DiscoveryModel' "$app_mod"; then
  echo "FAIL: ${app_mod} missing slice-5 model field 'pub discovery: DiscoveryModel' (ADR-005 §S2)"
  fail=1
fi

# K3.  No `app.feed.discovery` field-access path anywhere in trench/src.
#      Every render path goes through `app.discovery.*`.  Doc comments
#      referencing the old path (history notes in CONTEXT.md or ADR-005)
#      stay legal because they're outside trench/src.
hits=$(grep -rnE '\bapp\.feed\.discovery\b|\bself\.feed\.discovery\b' trench/src/ 2>/dev/null | grep -v ':[[:space:]]*//' || true)
if [[ -n "$hits" ]]; then
  echo "FAIL: render path reaches discovery via the old nested 'feed.discovery' route (ADR-005 §S2):"
  echo "$hits" | sed 's/^/  /'
  fail=1
fi

# K4.  ADR-005 cadence table mentions every slice-5 PR.  Mirrors I2 / I6
#      so the table can't silently drop a PR row.
adr5="docs/adr/ADR-005-discovery-slice.md"
for tag in "| 1 |" "| 2 |" "| 3 |" "| 4 |"; do
  if ! grep -qF "$tag" "$adr5"; then
    echo "FAIL: ${adr5} cadence table missing entry matching '${tag}'"
    fail=1
  fi
done

if [[ "$fail" -eq 0 ]]; then
  echo "OK: render-purification invariants hold (feed + reader + notes + discovery)"
fi
exit $fail
