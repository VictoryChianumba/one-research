# ADR-008 — `FrameLayout` carries layout-derived inputs into a layout-aware `apply_frame_layout` hook

- **Status:** Accepted (2026-05-18). All 3 PRs landed: PR 1 = ADR + `FrameLayout` + empty `apply_frame_layout` + 3 smoke tests + CONTEXT.md vocabulary, PR 2 = wired the hook + migrated the marker site (5 files, +112 / −30), PR 3 = N1-N3 tripwires in `scripts/check-frame-layout.sh` + ci.sh wired + this status flip.
- **Date:** 2026-05-18
- **Owner:** Victory Chianumba
- **Supersedes:** none
- **Extends:** [ADR-001](ADR-001-render-purification.md) §D3 (`pre_draw` owns layout-derived mutation).

## Goal

Close the remaining `// Intentional render-time mutation` comment in the codebase (`one-research/src/ui/layout/reader.rs:424-441`) by giving it a typed home: a `FrameLayout` struct that carries the post-layout `Rect`s the mutation needs, plus an `App::apply_frame_layout(&FrameLayout)` hook invoked between the layout pass and the render pass.

After this slice, **renders become pure functions of `&App + &Layout`** — every state mutation lives in either `pre_draw_update` (no layout knowledge) or `apply_frame_layout` (post-layout). The comment marker that ADR-001 D3 named as the "regression marker" disappears.

## Context

ADR-001 §D3 said:

> Any mutation that needs layout-derived values (viewport size, auto-scroll, width-aware wrapping) lives in `Model::pre_draw(viewport)`, which runs once per frame before render. The `// intentional render-time mutation` comments are a regression marker; their disappearance is how we know a slice is done.

Today only one such marker survives, at `one-research/src/ui/layout/reader.rs:424-428`:

```rust
// Intentional render-time mutation. Same pattern as draw_item_table /
// draw_history_tab: this auto-scroll needs viewport_rows, which is
// layout-derived. The B2b hoist (reader-bottom variant) wasn't
// attempted after B2a's regressions; stays here until refactor B's
// deferred layout-metrics extraction lands.
app.reader_bottom_scroll.set_max(total.saturating_sub(1));
```

The pre-existing `pre_draw_update` (in `one-research/src/app/methods/history.rs:48-83`) handles the *details* side of `reader_bottom_scroll.max` (the `usize::MAX` case for paragraph clipping). It cannot handle the *feed* side because `viewport_rows` is a function of the bottom-drawer list area — only known after the layout pass runs.

The CLAUDE.md TODO list captured the deferred shape verbatim:

> *Deferred architecture: proper layout-metrics extraction for render purification — compute geometry once into an explicit layout/metrics struct, use it in both `update_for_draw(...)` and render, then revisit the remaining viewport-dependent render mutations when there is a concrete forcing function.*

The forcing function is the marker block itself. ADR-008 is the layout-metrics extraction CLAUDE.md anticipated.

## Decision

### The struct (`one-research/src/ui/layout/mod.rs` or sibling)

```rust
/// Layout-derived inputs that `apply_frame_layout` needs to size scroll
/// bounds, viewport caps, and other layout-shaped state.
///
/// Each `Option<Rect>` is `Some` only when the corresponding pane is
/// open *and* its area is non-degenerate. `apply_frame_layout` treats
/// `None` as "leave the bound at its `pre_draw_update` default."
#[derive(Default, Debug, Clone, Copy)]
pub struct FrameLayout {
    /// The reader bottom-drawer *list area* when the drawer is in
    /// feed mode (i.e. `reader_bottom_open && !reader_bottom_details`).
    /// `apply_frame_layout` uses `.height` to set
    /// `reader_bottom_scroll.max` for feed-mode auto-scroll.
    pub reader_bottom_feed_list: Option<Rect>,
}
```

PR 1 lands the struct + a one-field constructor + smoke tests. The migration of `reader.rs:434` happens in PR 2.

### The hook (`one-research/src/app/mod.rs` or `methods/history.rs`)

```rust
impl App {
    /// Per-frame hook invoked between the layout pass and the render
    /// pass. Reads `&self` and the post-layout `FrameLayout`; writes
    /// the layout-derived scroll bounds that pre_draw_update can't
    /// resolve without layout knowledge.
    ///
    /// Called from the top-level draw site after `compute_layout` and
    /// before any `draw_*` function.
    pub fn apply_frame_layout(&mut self, layout: &FrameLayout) {
        // PR 2 wires the reader-bottom feed-mode auto-scroll here.
        // PR 1's body is empty (no callers yet).
    }
}
```

### Slice 8-specific decisions

#### S1. One struct, growing field-by-field

`FrameLayout` is not "the geometry of every pane." It's "the layout values that downstream hooks need." Today that's one field. When the next forcing function arrives — say, a future `:goto N` command needs a viewport-clamped jump — the struct grows by one field, and `apply_frame_layout` grows by one corresponding line. The audit's wording ("compute geometry once into an explicit layout/metrics struct") is interpreted narrowly: capture what's *consumed*, not what's *available*.

This departs from a "store every Rect ever computed" design and matches CLAUDE.md's "when there is a concrete forcing function" clause.

#### S2. Two-pass draw, not one-pass

PR 2 splits today's `draw_main_row` (which interleaves layout + render) into:

1. `compute_frame_layout(app, area) -> (FrameLayout, MainRowRects)` — pure: returns Rects.
2. `app.apply_frame_layout(&FrameLayout)` — mutates `app` based on layout.
3. `render_main_row(frame, app, rects, &t)` — renders, no mutation.

The pattern matches ADR-001 §D3 inside a layout-aware envelope. Renders become `&App` (or `&Pane`) consumers — no `&mut`.

#### S3. `pre_draw_update` stays

`pre_draw_update` keeps its current role: cache invalidation, selection-change resets, fixed-bound scroll caps. It runs *before* the layout pass, so it stays layout-blind. `apply_frame_layout` is the post-layout sibling.

#### S4. 3-PR cadence

| # | PR | Behaviour change |
|---|---|---|
| 1 | ADR-008 + `FrameLayout` struct + `apply_frame_layout` empty hook + 3 smoke tests + CONTEXT.md vocabulary. | None — the hook is unused. |
| 2 | Wire the hook into the draw entry point.  Move `reader.rs:434` set_max + offset clamp into `apply_frame_layout`. Delete the "Intentional render-time mutation" comment. | None — invariant: same viewport, same scroll caps, same observable behaviour. |
| 3 | `scripts/check-frame-layout.sh` with N1-N3 tripwires; ci.sh wired; ADR-008 → Accepted. | None |

### Invariants for PR 3 tripwire

- **N1** No `// Intentional render-time mutation` comments remain anywhere in `one-research/src/`. The marker class is gone.
- **N2** No `set_max(` / `set_offset(` calls inside `one-research/src/ui/layout/` outside of explicitly exempt sites (the help popup keeps inline scroll math — too small to warrant `FrameLayout` fields today; gets `// SEAM-EXEMPT:` annotations).
- **N3** ADR-008 cadence table lists every committed PR (mirrors I2 / I6 / K4 / L4 / M4).

## Consequences

### Positive

- The last `// Intentional render-time mutation` marker disappears. ADR-001 §D3's "regression marker" returns to its dormant state.
- Renders take `&App` rather than `&mut App` for everything in `ui/layout/`. Reader-bottom render becomes pure.
- A new forcing function (next viewport-dependent mutation site) gets a typed extension point instead of growing a fresh marker.
- The structural shape — "compute layout, mutate from layout, render from immutable refs" — is encoded in *function signatures*, not in author discipline.

### Negative

- Two-pass draw is slightly more verbose than the current interleaved version. The added clarity outweighs ~10 lines of plumbing.
- `FrameLayout` is unused at PR 1 — adds 1 file + 1 hook + ~30 LOC of doc/tests before the migration sees any benefit. Matches every prior slice's PR-1 ceremony.

### Trade-offs explicitly accepted

- **Help popup's inline `set_max` stays** as `// SEAM-EXEMPT: in-pane scroll math local to one popup, sized against `body_area.height` directly`. Not every render-side mutation is worth a `FrameLayout` field — the help popup's scroll bound is sized against a local Rect, never used outside the popup, and has no analog elsewhere. Lifting it would be ceremony without payoff.
- **Two-pass draw rather than one struct for everything.** The "every geometry value in one place" version would balloon `FrameLayout` to dozens of Rects most of which are read once locally. Audit's framing accepted via the narrower "compute what's consumed" reading.
- **`pre_draw_update` is NOT folded into `apply_frame_layout`.** The two have different inputs (no layout / post-layout). Keeping them separate is honest about what each can know.

## Risks

1. **Two-pass refactor could regress something subtle.** Mitigation: PR 2's migration is one site (reader.rs:434). The same scroll-bound it sets today gets set in the same shape by `apply_frame_layout`. Diff is small.
2. **`FrameLayout` could become a junk drawer.** Mitigation: N1 / N2 tripwires don't constrain the struct's growth, but the "concrete forcing function" rule from CLAUDE.md applies: a new field requires a marker block it's replacing.
3. **Borrow-checker friction at the draw entry point.** `compute_frame_layout(app, ...)` takes `&App`; `apply_frame_layout` takes `&mut App`. The seam between them is sequential, so no overlapping borrows.

## Related

- [ADR-001](ADR-001-render-purification.md) — parent per-pane refactor; §D3 names the regression marker C6 closes.
- `docs/audits/2026-05-18-architectural-audit.md` — candidate **C6** (LayoutMetrics).
- `CLAUDE.md` — "deferred architecture: proper layout-metrics extraction…" bullet.
- `docs/CONTEXT.md` — vocabulary updated in PR 1.
- `scripts/check-frame-layout.sh` — created in PR 3.
