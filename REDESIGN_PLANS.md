# Trench Redesign Plans

Historical planning record. Current behavior is documented in `README.md`,
`docs/CONTEXT.md`, the ADRs, and the in-app help.

This file records the local TUI redesign plans discussed with Codex so future
work can continue without relying on chat history.

## Current Status

Plan 1, the feed/history/details visual system, is partially implemented:

- History selection drives the Details pane.
- History paper entries resolve to cached `FeedItem` data when available, with
  `HistoryPaperMeta` fallback.
- History query entries render query details.
- History no longer shows visit counts.
- History uses feed-like table rows with History-specific columns.
- Details shows URL and action rows separately, including `o open URL`.

Stats and Activity-dashboard content changes are intentionally deferred.

Plan 2, the chat pane research console, is the active v1 pass:

- Chat context is derived without new persistence:
  active reader tab, then selected feed item, then none.
- Compact chat (`Ldr+c`) keeps the half-screen slab and adds a one-line
  `Discussing:` context strip when context exists.
- Expanded chat has been moved to v2. The v1 release should not expose a
  half-finished chat workspace.
- Assistant content should render as terminal-native markdown prose on the
  normal chat background. User prompts keep a subtle colored block.
- Manual context pinning remains deferred.

## Plan 1: Feed, History, And Details Visual System

- Preserve the normal feed row rhythm: wrapped titles, two-line behavior, and
  spacing for Inbox, Library, and Discoveries.
- Make History visually consistent with the feed while keeping History-specific
  metadata:
  - kind: paper/query
  - title
  - source
  - time since last view/run
  - no visible visit count
- Details should render from a current subject:
  - selected feed item
  - selected History paper, resolved to cached item when possible
  - selected History paper metadata fallback
  - selected History query
- Keep the current Details/Activity split layout for now.
- Keep tags from existing `domain_tags` and user tags only.
- Do not add tag harvesting or storage schema changes.

## Plan 2: Chat Pane Research Console

- Redesign chat as a research console rather than a generic message box.
- Add a stronger session/workspace model while preserving current storage.
- Compact chat remains available through `Ldr+c`.
- Expanded chat workspace is deferred to v2. Session selection remains an
  overlay.
- Include conversation area, paper context, and clearer command footer/help text.
- Do not change provider configuration or prompt storage in the first pass.
- Context source order: active reader tab, selected feed item, none. Add a
  manual pin override later without persistence in the first pass.
- Assistant responses must never render as raw dumps. Treat them as markdown
  blocks first, then wrap/render headings, paragraphs, lists, quotes, and code.

## Plan 3: Reader Shell Visual Redesign

- Treat reader states as one named reader workspace:
  - `Reader`
  - `Reader + Feed`
  - `Dual Reader`
  - `Reader + Notes`
- Keep the main `ONE RESEARCH` app header visible.
- Keep existing hotkeys and behavior where possible.
- Add a small reader workspace label above reader content so every reader state
  shares the same shell language.
- Treat bottom feed/details as a temporary command drawer.
- Unify floating reader/details/abstract popup styling.
- Keep full notes redesign separate.

### Plan 3A: Reader Shell

- Add consistent reader workspace labels:
  - `Reader`
  - `Reader + Feed`
  - `Dual Reader`
  - `Reader + Notes`
- Keep split panes inside shared frame containers.
- Rename the compare-style reader state to `Dual Reader` in visible UI.
- Do not change persistence, notes behavior, or reader hotkeys.

### Plan 3B: Reader-Attached Notes

- Status: implemented as the first docked-notes pass.
- Notes dock to the bottom of each reader pane.
- `Dual Reader` supports separate primary/secondary notes tab state, with one
  note dock under each reader.
- The focused notes dock uses the existing notes app; inactive dual-reader
  docks render as quiet previews.
- Note storage and note identity remain reusable from `crates/notes`.
- Follow-up: reader tabs still only store title + editor, so notes still open
  from the selected feed item rather than a persisted reader-tab paper id/url.

### Plan 3C: Reader Interaction Polish

- Status: implemented as the first interaction-polish pass.
- Notes docks open independently per side in `Dual Reader`; opening primary
  notes no longer opens an empty secondary notes dock, and vice versa.
- Directional leader focus covers reader panes, notes docks, and the mini feed
  drawer.
- `Dual Reader` header splits remaining title space evenly between primary and
  secondary active tab titles, truncating each side independently.
- Reader footers expose `Ldr+t` new-tab and tab-switching affordances.
- Mini feed drawer supports `/` search with filtered results.
- Follow-up: decide whether the larger `Reader + Feed` mode should remain
  alongside the mini feed drawer.

### Plan 3D: Reader Feed / Feed Drawer Polish

- Status: implemented as the first visual-unification pass.
- Visible naming now separates the larger `Reader Feed` layout from the
  smaller `Feed Drawer`.
- Feed Drawer rows use the same compact source / kind / title / date language
  as the main feed and history surfaces.
- Feed Drawer footer/help wording uses `Feed drawer` instead of `Bottom feed`
  or `reader feed`.
- Follow-up: decide whether the larger `Reader Feed` layout should be kept,
  replaced by the drawer, or redesigned as its own full browsing mode.

## Plan 4: Notes Integration Redesign

- Make notes feel native to Trench while keeping `crates/notes` reusable.
- Organize notes around:
  - `Paper Notes`
  - `Library`
  - `Capture`
- Use a right-side notes pane as the default integration shape.
- `Ldr+n` from a paper without notes should open a capture composer prefilled
  with paper context.
- Keep `Note { title, content, tags, linked_papers }` unchanged.
- Clean up visible notes footer/help grouping without a full hotkey remap.

## Plan 5: Repo Viewer Redesign

- Keep the two-pane repo browser core:
  - repository tree left
  - file/docs/code preview right
- Add stronger repo header context: owner/repo, branch, path, file kind, and
  loading/error status.
- Restyle tree/file panes to match Trench visual language.
- Improve empty/loading/error states.
- Keep markdown/code/plain-text preview behavior.
- Plan context actions:
  - open GitHub URL
  - copy path/URL
  - send selected repo/file context to chat
- Keep repo search/fuzzy-open as a later extension.

## v1 Release Checklist

Final shipping steps for v1.0.0, in order. Each item depends on the previous.

1. **Port in the new reader.** Block-reader (from the `tread` sister repo) is
   the v1 user-facing reader for arXiv papers. This is the precondition for
   everything below — until block-reader is in, the keybinding set and
   reader-pane shell are still in flux.

2. **(Optional) Trim hygg-reader and its sibling crates.** hygg-reader is the
   legacy standalone CLI inherited from the kruseio/hygg fork. trench doesn't
   import it; it just rides along in the workspace. The trim chain:
   - `hygg-reader/` (the binary)
   - `cli-pdf-to-text/` (used only by hygg-reader)
   - `cli-epub-to-text/` (used only by hygg-reader)
   - `redirect-stderr/` (used only by hygg-reader and cli-pdf-to-text)
   - `cli-justify/` stays — also used by `cli-text-reader`.

   Decide between keeping (for ad-hoc external PDF reading) or removing (the
   final "trench is its own thing" signal). If removing: delete the four
   directories, prune `[workspace] members` in `Cargo.toml`, run `cargo
   build` to confirm clean.

3. **Version bump to v1.0.0** — the final v1 commit. Touches 11 files:
   - `Cargo.toml` `[workspace.package].version`: `0.1.19` → `1.0.0`
   - 4 sibling crate Cargo.tomls with 9 path-dep `version = "0.1"` → `"1.0"`
     declarations (cli-pdf-to-text, cli-epub-to-text, hygg-reader,
     cli-text-reader). If step 2 ran first, only cli-text-reader and its
     dependents remain.
   - `trench/src/ui/layout.rs:23` `const VERSION: &str = "v0.1.0";` → `"v1.0.0"`
   - `Cargo.lock` updates automatically on `cargo build`.

   Then `git tag -a v1.0.0 -m "Release v1.0.0"`, push commit + tag.

   Going to v1.0.0 commits trench to keybinding / config-format / state-schema
   stability per semver §5. This is why it comes last — the keybinding set
   needs to be finalized (after step 1) and the workspace needs to be settled
   (optionally after step 2) before declaring stability.

4. **Optional follow-up:** consider switching the `VERSION` constant in
   `trench/src/ui/layout.rs` to `concat!("v", env!("CARGO_PKG_VERSION"))` so
   future bumps only need to touch `Cargo.toml`. Eliminates the failure mode
   that bit us this session (commit message said "v0.1.0" while Cargo said
   "0.1.19"). Trivial when convenient.
