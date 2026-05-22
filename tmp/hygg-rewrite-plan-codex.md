# Hygg `cli-text-reader` ratatui Rewrite Plan

Historical rewrite plan for the pre-tread reader path. trench now treats tread
as the reader boundary; this file is kept only as older design context.

## Goal

Rewrite `cli-text-reader` from direct `crossterm` drawing into a ratatui-rendered document widget that can either:

- run standalone with its own `Terminal<CrosstermBackend<Stdout>>`, and
- be embedded by a parent ratatui application via:

```rust
pub fn draw(frame: &mut Frame, area: Rect, editor: &mut Editor);
pub fn handle_key(key: KeyEvent, editor: &mut Editor) -> EditorAction;
```

The current code already separates most editing behavior from rendering, but rendering, terminal lifecycle, polling, resize handling, cursor placement, command execution UI, demo timing, and voice redraw policy are still mixed together.

## Current Architecture Summary

- [`cli-text-reader/src/editor/display_loop.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/display_loop.rs) owns the main loop, redraw policy, crossterm polling, resize handling, progress saving, voice polling, demo ticking, and final cursor writes.
- [`cli-text-reader/src/editor/display.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/display.rs) renders the main viewport line-by-line with direct cursor movement and manual clearing.
- [`cli-text-reader/src/editor/display_split.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/display_split.rs) duplicates a large amount of highlight/render logic for split panes.
- [`cli-text-reader/src/editor/highlighting.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/highlighting.rs), [`cli-text-reader/src/editor/highlighting_persistent.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/highlighting_persistent.rs), and [`cli-text-reader/src/editor/highlighting_selection.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/highlighting_selection.rs) encode highlight state as imperative terminal writes instead of composable style spans.
- [`cli-text-reader/src/editor/status_line.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/status_line.rs) and [`cli-text-reader/src/editor/settings.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/settings.rs) are also hand-drawn overlays.
- [`cli-text-reader/src/editor/voice_control.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/voice_control.rs) is mostly UI-agnostic, but the voice status and word-highlighting effects are implemented in the display layer.
- [`cli-text-reader/src/core_state.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/core_state.rs), [`cli-text-reader/src/core_types.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/core_types.rs), and [`cli-text-reader/src/editor/core.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/core.rs) still store terminal width/height, redraw flags, and cursor visibility state inside `Editor`.
- [`cli-text-reader/src/lib.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/lib.rs) only exposes standalone `run_*` entrypoints today.

## Guiding Refactor Rule

Do not port line-by-line terminal writes directly to ratatui widgets. First build a pure render model:

- viewport and pane layout from `Rect`
- a styled line representation from document state
- widget composition from that styled line representation

That is the only way to avoid preserving the current duplication between normal and split rendering.

## Stage 1: Core ratatui scaffold

### Files to modify

- [`cli-text-reader/Cargo.toml`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/Cargo.toml)
- [`cli-text-reader/src/lib.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/lib.rs)
- [`cli-text-reader/src/core_state.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/core_state.rs)
- [`cli-text-reader/src/core_types.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/core_types.rs)
- [`cli-text-reader/src/editor/mod.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/mod.rs)
- [`cli-text-reader/src/editor/display_init.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/display_init.rs)
- [`cli-text-reader/src/editor/display_loop.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/display_loop.rs)
- [`cli-text-reader/src/editor/core.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/core.rs)
- [`cli-text-reader/src/editor/event_handler.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/event_handler.rs)

### Files to create

- `cli-text-reader/src/editor/widget.rs`
- `cli-text-reader/src/editor/runtime.rs`
- `cli-text-reader/src/editor/actions.rs`

### Specific rewrites

- Replace `Editor::run()` terminal ownership in `display_init.rs` with a standalone ratatui runtime:
  - create `Terminal<CrosstermBackend<Stdout>>`
  - keep `EnterAlternateScreen`, `LeaveAlternateScreen`, `enable_raw_mode`, `disable_raw_mode` for standalone mode only
  - move loop timing, poll/read, and `terminal.draw(...)` into `runtime.rs`
- Replace `display_loop.rs::main_loop` with:
  - `Editor::tick()` or equivalent for non-input periodic work
  - `runtime::run(editor: &mut Editor)` for standalone mode
  - `widget::draw(frame, area, editor)` for embeddable mode
- Replace `handle_event(key, stdout)` with `handle_key(key, editor) -> EditorAction`
  - remove `stdout` from the event API
  - return actions like `None`, `Quit`, `NeedsRedraw`, `RunCommandOutput`, `OpenOverlay`, or a minimal equivalent
- Add an `EditorAction` enum in `actions.rs`
- Add an `editor.set_viewport(area: Rect)` or `editor.update_layout(width, height)` helper so all width/height mutation becomes an explicit layout update instead of terminal-global state

### ratatui primitives replacing crossterm

- `Terminal<CrosstermBackend<Stdout>>` replaces manual stdout buffering and `write_all`/`flush`
- `terminal.draw(|frame| ...)` replaces `Clear` + full-frame buffered writes
- `Frame::area()` or passed-in `Rect` replaces `terminal::size()`

### Dependencies

- Add `ratatui = "0.29"` to [`cli-text-reader/Cargo.toml`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/Cargo.toml)
- Keep `crossterm = "0.29"` for backend and input events
- Remove no dependencies yet

### Compilation checkpoint

- Standalone `run_*` APIs still work through ratatui `Terminal`
- No rendering port yet; `widget::draw` may temporarily render a placeholder block with editor mode and cursor info
- `handle_key` exists and is the single input entrypoint used by both standalone and embedded paths

## Stage 2: Viewport and line rendering

### Files to modify

- [`cli-text-reader/src/editor/display.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/display.rs)
- [`cli-text-reader/src/editor/core.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/core.rs)
- [`cli-text-reader/src/core_state.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/core_state.rs)
- `cli-text-reader/src/editor/widget.rs`

### Files to create

- `cli-text-reader/src/editor/render_lines.rs`

### Specific rewrites

- Rewrite `draw_content` and `draw_content_buffered` into pure builders that return ratatui text structures instead of writing bytes.
- Extract a `VisibleLine` or `RenderedLine` model containing:
  - document line index
  - gutter/prefix padding
  - text segments
  - line-level style
  - flags for current line, dimmed line, overscroll blank
- Move `center_offset_string` logic into layout-time indentation, not string concatenation
- Convert the viewport loop into a `Vec<Line>` or `Text<'a>` builder for the current visible region
- Stop manually clearing tail space with `ClearType::UntilNewLine`; instead render a full-width block/paragraph over the viewport area

### ratatui primitives replacing crossterm

- `Paragraph` replaces per-line `MoveTo` and `write!`
- `Text`, `Line`, and `Span` replace `center_offset_string + line` string assembly
- `Block` or base `Style` over the viewport area replaces explicit end-of-line clearing and line background fill

### Dependencies

- No new dependency beyond `ratatui`

### Compilation checkpoint

- Normal single-pane document view renders through ratatui
- Overscroll blanks still appear
- Current-line centering and cursor movement still work
- Search/selection/persistent highlighting can be temporarily disabled or stubbed, but plain text viewport rendering must match current scroll behavior

## Stage 3: Highlights

### Files to modify

- [`cli-text-reader/src/editor/highlighting.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/highlighting.rs)
- [`cli-text-reader/src/editor/highlighting_persistent.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/highlighting_persistent.rs)
- [`cli-text-reader/src/editor/highlighting_selection.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/highlighting_selection.rs)
- [`cli-text-reader/src/editor/display.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/display.rs)
- `cli-text-reader/src/editor/render_lines.rs`

### Files to create

- `cli-text-reader/src/editor/highlight_spans.rs`

### Specific rewrites

- Replace all imperative highlight functions with pure range calculators:
  - current-line background range
  - search match range
  - selection ranges
  - persistent highlight ranges
  - combined precedence rules
- Keep `has_*_on_line` style helpers only if they are still useful for performance; otherwise remove them and compute a single merged style map per line
- Replace the current overlapping-highlight rendering in `render_combined_highlights*` with a deterministic segment builder that emits non-overlapping styled spans in order
- Reuse the same style-merging code for both normal and split rendering

### ratatui primitives replacing crossterm

- `Span::styled(...)` replaces `SetBackgroundColor`, `SetForegroundColor`, and `ResetColor`
- `Style::bg(...)`, `Style::fg(...)`, `Modifier::REVERSED` replace manual style toggles
- `Line::from(Vec<Span>)` replaces interleaved `write!` + style resets

### Dependencies

- No dependency change required

### Compilation checkpoint

- Current line highlight works via style, not by filling raw terminal cells
- Search preview, committed search match, visual char selection, visual line selection, and persistent highlights all render in normal view
- Combined selection + persistent highlight behavior matches current priority rules closely enough to replace the old path

## Stage 4: Vim motions and navigation audit

### Files to modify

- [`cli-text-reader/src/editor/cursor.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/cursor.rs)
- [`cli-text-reader/src/editor/movement.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/movement.rs)
- [`cli-text-reader/src/editor/line_navigation.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/line_navigation.rs)
- [`cli-text-reader/src/editor/page_navigation.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/page_navigation.rs)
- [`cli-text-reader/src/editor/screen_position.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/screen_position.rs)
- [`cli-text-reader/src/editor/navigation.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/navigation.rs)
- [`cli-text-reader/src/editor/normal_navigation.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/normal_navigation.rs)
- [`cli-text-reader/src/editor/normal_navigation_basic.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/normal_navigation_basic.rs)
- [`cli-text-reader/src/editor/normal_navigation_find.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/normal_navigation_find.rs)
- [`cli-text-reader/src/editor/normal_navigation_jumps.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/normal_navigation_jumps.rs)
- [`cli-text-reader/src/editor/visual_mode.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/visual_mode.rs)
- [`cli-text-reader/src/editor/visual_mode_movement.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/visual_mode_movement.rs)
- [`cli-text-reader/src/editor/search_mode.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/search_mode.rs)
- [`cli-text-reader/src/editor/buffer_state.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/buffer_state.rs)

### Specific rewrites

- Audit what carries over unchanged:
  - most text-object, jump, search, yank, and operator-pending logic is state-driven and can remain
  - `handle_normal_mode_event` dispatching can remain
- Rewrite what depends on terminal rows:
  - anything that uses `self.height`, `self.width`, or `viewport_height` must derive from the current ratatui `Rect`
  - split-pane cursor row calculation in `cursor.rs`
  - screen-relative commands and paging
  - centering code that currently assumes one global content area
- Remove direct cursor style/show/hide logic from motion code
- Change resize behavior from `CEvent::Resize` side effects to an explicit `editor.update_layout(rect)` step invoked before `draw`

### ratatui primitives replacing crossterm

- No direct widget replacement here; this stage is about removing hidden terminal assumptions from motion code
- `Rect.height`/`Rect.width` become the authoritative viewport source

### Dependencies

- No dependency change required

### Compilation checkpoint

- All normal, visual, search, and paging motions compile against the new layout model
- Cursor movement, centering, and search-preview navigation behave the same in single-pane mode under ratatui
- No motion code still depends on raw cursor show/hide APIs

## Stage 5: Split pane

### Files to modify

- [`cli-text-reader/src/editor/display_split.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/display_split.rs)
- [`cli-text-reader/src/editor/buffer_split.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/buffer_split.rs)
- [`cli-text-reader/src/editor/cursor.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/cursor.rs)
- [`cli-text-reader/src/editor/core.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/core.rs)
- `cli-text-reader/src/editor/widget.rs`
- `cli-text-reader/src/editor/render_lines.rs`

### Files to create

- `cli-text-reader/src/editor/layout.rs`

### Specific rewrites

- Replace all manual split-row math in `display_split.rs` with a `Layout::vertical(...)` split:
  - top pane
  - separator row or bordered pane split
  - bottom pane
- Delete the duplicate buffered and non-buffered render paths
- Render each pane by calling the same line-builder used by normal view, but with pane-specific buffer state and `Rect`
- Move the tutorial-specific “which buffer belongs to which pane” mapping into one helper in `layout.rs` or `buffer_split.rs`
- Replace `split_ratio` row math with a ratatui layout constraint derived from the same ratio
- Ensure pane-local viewport heights are updated from the actual `Rect` heights, not from terminal-global state

### ratatui primitives replacing crossterm

- `Layout` replaces top/bottom row arithmetic
- `Block` borders or a dedicated 1-row separator `Paragraph` replace the raw `─` separator write
- `Paragraph` reused per pane replaces `draw_pane*` raw writing

### Dependencies

- No dependency change required

### Compilation checkpoint

- Horizontal split renders correctly through ratatui
- Pane switching, split close, and command-output buffer focus still work
- Tutorial mode buffer routing still works in split mode
- Normal and split views share the same styled-line rendering path

## Stage 6: Status line and settings popup

### Files to modify

- [`cli-text-reader/src/editor/status_line.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/status_line.rs)
- [`cli-text-reader/src/editor/settings.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/settings.rs)
- [`cli-text-reader/src/editor/display_loop.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/display_loop.rs)
- `cli-text-reader/src/editor/widget.rs`

### Files to create

- `cli-text-reader/src/editor/widgets/status_bar.rs`
- `cli-text-reader/src/editor/widgets/settings_popup.rs`

### Specific rewrites

- Convert mode line and progress line into ratatui widgets with explicit layout:
  - main content area
  - optional info row
  - status row
- Convert settings popup from manual box-drawing strings into a `Clear` + `Block` + inner `Layout`
- Move status text generation into pure helpers returning `Line`/`Span`
- Keep settings editing logic in `settings.rs`; only port the drawing layer

### ratatui primitives replacing crossterm

- `Layout` replaces bottom-row coordinate math
- `Paragraph` replaces manual prompt printing
- `Block::bordered()` replaces Unicode box-drawing strings
- `Clear` widget replaces background-overwrite assumptions for the popup

### Dependencies

- No dependency change required

### Compilation checkpoint

- Command prompt, search prompt, visual-mode indicator, tutorial indicator, progress text, and reading-mode label all render through ratatui
- Settings popup opens, edits, saves, and closes without raw cursor writes

## Stage 7: Voice integration

### Files to modify

- [`cli-text-reader/src/editor/voice_control.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/voice_control.rs)
- [`cli-text-reader/src/editor/display.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/display.rs)
- [`cli-text-reader/src/editor/status_line.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/status_line.rs)
- `cli-text-reader/src/editor/render_lines.rs`
- `cli-text-reader/src/editor/widget.rs`

### Specific rewrites

- Keep playback control logic mostly unchanged
- Move voice UI effects into the line-style builder:
  - dim non-active paragraph lines with a ratatui `Style`
  - reverse or otherwise style the current spoken word as a span-level modifier
- Move voice spinner/status rendering entirely into the ratatui status widget
- Keep periodic voice polling in runtime/tick logic, not in rendering methods

### ratatui primitives replacing crossterm

- `Style::fg(Color::DarkGray)` or equivalent theme style replaces manual dim writes
- `Modifier::REVERSED` or dedicated word style replaces `SetAttribute(Reverse)`
- Status-line `Span`s replace manual bottom-left voice indicator printing

### Dependencies

- No dependency change required

### Compilation checkpoint

- Voice status, loading spinner, playing/paused indicators, paragraph dimming, and current-word highlight all render correctly in ratatui
- Continuous reading still auto-advances

## Stage 8: Command mode

### Files to modify

- [`cli-text-reader/src/editor/event_handler.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/event_handler.rs)
- [`cli-text-reader/src/editor/command_mode.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/command_mode.rs)
- [`cli-text-reader/src/editor/commands.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/commands.rs)
- [`cli-text-reader/src/editor/command_execution_core.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/command_execution_core.rs)
- [`cli-text-reader/src/editor/buffer_split.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/buffer_split.rs)
- [`cli-text-reader/src/editor/search_mode.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/search_mode.rs)
- [`cli-text-reader/src/editor/status_line.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/status_line.rs)
- [`cli-text-reader/src/lib.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/lib.rs)
- `cli-text-reader/src/editor/actions.rs`

### Specific rewrites

- Remove the last `stdout` dependency from command execution paths
- Refactor `execute_command` so it mutates editor state and returns `EditorAction`
  - `Quit`
  - `Redraw`
  - possibly `CommandOutputOpened`
- Decide how to handle blocking input inside command mode:
  - current `Ctrl+R` path uses direct `event::read()`
  - replace with a short-lived input submode or staged key handling in `EditorAction`
- Keep shell command execution security logic unchanged
- Keep command-output split behavior, but make the creation of command buffers a pure state transition; rendering should no longer know or care that the output came from a shell command

### ratatui primitives replacing crossterm

- No new widget class required beyond the status/command line already built in Stage 6
- The important replacement here is architectural: command execution must stop depending on terminal handles

### Dependencies

- No dependency change required

### Compilation checkpoint

- Embedded callers can drive the editor entirely with `handle_key`
- `:` commands, `/` and `?` search input, shell command output in split view, and `:q` semantics all work without a `Stdout` parameter
- No editor mutation path reads directly from `crossterm::event::read()`

## Stage 9: Tutorial and demo

### Files to modify

- [`cli-text-reader/src/lib.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/lib.rs)
- [`cli-text-reader/src/editor/display_init.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/display_init.rs)
- [`cli-text-reader/src/editor/display_loop.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/display_loop.rs)
- [`cli-text-reader/src/editor/demo_renderer.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/demo_renderer.rs)
- [`cli-text-reader/src/editor/demo_executor.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/demo_executor.rs)
- [`cli-text-reader/src/editor/tutorial_display.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/tutorial_display.rs)
- [`cli-text-reader/src/editor/tutorial_interactive.rs`](/Users/temp/Desktop/projects/pproject-forks/hygg/cli-text-reader/src/editor/tutorial_interactive.rs)
- `cli-text-reader/src/editor/widget.rs`

### Specific rewrites

- Assess features separately:
  - interactive tutorial overlay: likely worth porting because it already behaves like an editor-owned overlay buffer
  - demo hints/marketing demos: portable, but only needed for standalone mode
  - legacy `show_tutorial()` full-screen pager: least valuable for the embedded widget API
- Recommended outcome:
  - keep interactive tutorial overlays
  - keep demo support behind standalone runtime APIs only
  - deprecate or drop the legacy full-screen tutorial pager if it is not used outside standalone mode
- Move any standalone-only tutorial/demo boot behavior out of `Editor::draw` and into `runtime.rs`

### ratatui primitives replacing crossterm

- Reuse `Clear`, `Block`, `Paragraph`, and overlay layout for demo hints/tutorial overlays

### Dependencies

- No dependency change required

### Compilation checkpoint

- Embedded widget API is stable and tutorial-safe
- Standalone runner can still launch tutorial/demo flows if desired
- Legacy tutorial pager is either ported deliberately or removed deliberately, not left half-working

## Risk Assessment

### 1. State duplication between `Editor`, `EditorState`, and `BufferState`

Risk:

- `Editor` still owns `lines`, `offset`, `cursor_x`, `cursor_y`, `width`, `height`, and many view flags directly, while buffer-local copies also exist in `BufferState`.
- `editor_state` also mirrors mode, command buffer, selection, and search state during an unfinished migration.

Why difficult:

- The ratatui port wants a pure `draw(frame, area, editor)` path, but the current code often mutates shared and per-buffer state interchangeably.

Mitigation:

- In Stage 1 and Stage 4, define one authoritative source for layout and one for active-buffer state before deep rendering changes.

### 2. Rendering duplication in split mode

Risk:

- `display_split.rs` reimplements selection, persistent highlights, search highlighting, and line rendering separately from `display.rs`.

Why difficult:

- A direct port would preserve duplication and make future bugs twice as likely.

Mitigation:

- Build one styled-line pipeline in Stage 2/3 and use it for both normal and split panes in Stage 5.

### 3. Byte-indexed text logic versus terminal cell width

Risk:

- Search matches, selections, persistent highlights, and voice word highlighting all use string byte indices.
- ratatui renders cells, not bytes.

Why difficult:

- Unicode graphemes and wide characters can make cursor columns and highlight ranges diverge from what the user sees.

Mitigation:

- Preserve current byte-index behavior first to avoid scope explosion, but isolate range calculations behind render helpers so a later grapheme-width pass is possible.
- Call this out as a known correctness gap if full Unicode support is expected.

### 4. Cursor handling

Risk:

- `cursor.rs` still assumes the widget controls the real terminal cursor, including style changes and split-pane row calculation.

Why difficult:

- Embedded ratatui apps may not want the child widget to own cursor style or visibility globally.

Mitigation:

- Separate logical cursor position from terminal cursor policy.
- Public API should expose logical cursor info or an optional “desired cursor” rather than unconditionally writing to terminal state.

### 5. Blocking input inside command mode

Risk:

- `command_mode.rs` uses `event::read()` directly for `Ctrl+R`.

Why difficult:

- Embedded apps expect all input to enter through the parent event loop.

Mitigation:

- Replace blocking reads with explicit submode state and let the parent keep feeding `KeyEvent`s.

### 6. Command execution and split creation are coupled

Risk:

- `execute_shell_command` currently creates split buffers directly as part of the command path.

Why difficult:

- The new API should treat rendering as a consequence of state, not as something command execution “draws”.

Mitigation:

- Keep command execution mutating buffers, but ensure it only produces editor state changes and `EditorAction`, not terminal writes.

### 7. Voice redraw timing and animation

Risk:

- Spinner animation and word highlighting currently depend on redraw timing inside `display_loop.rs`.

Why difficult:

- Embedded parents may redraw on their own schedule.

Mitigation:

- Add a lightweight `tick()` API or document that the parent should call `draw` on a timer while voice is active.

### 8. Tutorial/demo startup side effects

Risk:

- `display_init.rs` triggers tutorial/demo startup in `run()`.

Why difficult:

- Embedded widgets should not unexpectedly enter alternate tutorial flows on first draw.

Mitigation:

- Make tutorial/demo startup explicit in standalone runtime only, or expose opt-in constructor/config APIs.

## Public API Target

At the end of the rewrite, `lib.rs` should expose:

```rust
pub use crate::editor::Editor;
pub use crate::editor::actions::EditorAction;

pub fn draw(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, editor: &mut Editor);
pub fn handle_key(key: crossterm::event::KeyEvent, editor: &mut Editor) -> EditorAction;
```

Recommended additional exports:

```rust
pub fn tick(editor: &mut Editor) -> EditorAction;
pub fn update_layout(editor: &mut Editor, area: ratatui::layout::Rect);
```

Recommended `EditorAction` shape:

```rust
pub enum EditorAction {
    None,
    Redraw,
    Quit,
}
```

If the parent app needs more structured coordination, use:

```rust
pub enum EditorAction {
    None,
    Redraw,
    Quit,
    CursorChanged,
    OpenedSplit,
    ClosedSplit,
}
```

Do not expose raw terminal lifecycle from the embeddable API. Keep standalone helpers as optional convenience wrappers:

```rust
pub fn run_cli_text_reader(...);
pub fn run_cli_text_reader_with_demo(...);
```

Those wrappers should internally create a ratatui terminal and repeatedly call:

1. `tick(editor)`
2. `terminal.draw(|f| draw(f, f.area(), editor))`
3. `handle_key(key, editor)` on input

## Recommended End State by Module

- `display_loop.rs`: reduced to standalone runtime glue or removed in favor of `runtime.rs`
- `display.rs`: pure viewport renderer to styled lines/widgets
- `display_split.rs`: reduced to pane composition, not custom line rendering
- `highlighting*.rs`: pure range/style composition
- `status_line.rs`: pure status text + ratatui widget composition
- `settings.rs`: state handling stays; drawing becomes ratatui popup widget
- `voice_control.rs`: keep state logic; no rendering side effects
- `core_state.rs` and `core_types.rs`: include explicit API-facing action/layout types
- `lib.rs`: expose embeddable draw/input entrypoints and keep standalone wrappers

## Recommendation on sequencing

Do not start with split mode or popups. The least risky order is:

1. terminal/runtime scaffold
2. single-pane styled viewport
3. all highlight layers
4. navigation/layout audit
5. split panes
6. status and popup overlays
7. voice rendering
8. command-mode event API cleanup
9. tutorial/demo decision and cleanup

That sequence keeps the branch compiling while steadily removing the current raw-terminal assumptions.
