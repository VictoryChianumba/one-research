# Reference — Hotkeys

[Back to docs](../README.md)

The exhaustive list of every binding shipped in one-research. The in-app help overlay
(`?`) is the terse view; this is the complete one.

> Contributors: when adding or changing a binding, update this file alongside
> `HELP_SECTIONS` in `one-research/src/ui/layout/popups/help.rs` so the two stay in
> sync.

---

## Global

| Key | Action |
|---|---|
| `Ctrl+T` | Leader key — arms a 1-second window for `Ldr+<key>` bindings |
| `?` | Open help overlay |
| `q` | Quit (context-aware popup: clean / discovery in progress / unsent chat / leave reader) |
| `Esc` | Clear / back / cancel (context-dependent) |
| Mouse click | Focus interactive pane |

### Quit popup
| Key | Action |
|---|---|
| `q` or `Enter` | Confirm |
| `Esc` | Cancel |

---

## Feed view (all tabs share these unless otherwise noted)

| Key | Action |
|---|---|
| `Tab` | Cycle tabs forward: Inbox → Browse → Library → Discoveries → History → Inbox |
| `Shift+Tab` | Cycle tabs backward: Inbox → History → Discoveries → Library → Browse → Inbox |
| `j` / `k` or `↓`/`↑` | Move down / up |
| `g` / `G` | Jump to top / bottom |
| `Enter` | Open paper in reader (Inbox/Library/Discoveries) |
| `Space` | Abstract popup |
| `/` | Open search bar (filters items by title/author) |
| `f` | Open filter panel |
| `R` | Refresh all sources |
| `o` | Open selected URL in browser |
| Workflow state keys (apply to selected item) |
| `i` | Mark Inbox |
| `r` | Mark DeepRead |
| `w` | Mark Queued |
| `x` | Archive |
| `v` | Open repo viewer when the selected item has a linked repo. On Library, `v` enters visual mode instead. |

### Inbox tab
- Shows only items in `WorkflowState::Inbox` state.
- All generic feed keys above apply.

### Library tab
- Shows items where state != Inbox, narrowed by the Library workflow filter.
| Key | Action |
|---|---|
| `[` / `]` | Cycle workflow filter backward / forward (All / Queue / Read / Archived) |
| `f` | Open filter panel, including the Library workflow section |
| `v` | Enter visual selection mode |
| `t` | Open tag picker for current item |

### Library visual mode (after `v`)
| Key | Action |
|---|---|
| `j` / `k` | Extend / contract selection from anchor |
| `r` | Mark all selected as DeepRead |
| `w` | Mark all selected as Queued |
| `x` | Archive all selected |
| `i` | Move all selected back to Inbox |
| `t` | Open tag picker for all selected items |
| `Esc` | Exit visual mode without applying |

### Discoveries tab
| Key | Action |
|---|---|
| Any printable char | Focus the persistent search bar at bottom |
| `Enter` | Run search or dispatch slash command |
| `Ctrl+N` | Force new discovery session (reset prior context) |
| `/` | Open slash command palette in search bar |
| In palette: `↑`/`↓` | Navigate suggestions |
| In palette: `Tab` | Complete selected command |
| In palette: `Enter` | Run selected command |
| `Esc` | Cancel / unfocus search bar |

### Browse tab
The feed stays on the left. The right companion pane shows the subject rail,
or the filter panel while filter focus is active.

| Key | Action |
|---|---|
| `l` / `Right` from feed | Focus the right-side subject rail |
| `h` / `Left` at rail root | Return focus to the feed |
| `j` / `k` | Move the rail cursor |
| `l` / `Right` in rail | Drill into Group / Archive; from Category return to feed |
| `Enter` | Drill into Group / Archive, or load recent papers at Category |
| `h` / `Left` / `Backspace` / `Esc` | Drill back one rail level |
| `p` | Promote / un-promote the selected Category |
| `x` / `F` | Toggle subject-follow scope |

Categories page in from arXiv. With subject-follow on, resting the rail cursor
on a Category auto-loads its first page after a brief settle (no `Enter`);
scrolling toward the tail fetches further pages until the archive runs out.
`Enter` loads immediately and is the way to load with follow off.

### History tab
| Key | Action |
|---|---|
| `[` / `]` | Cycle time filter backward / forward (All / Today / 24h / 48h / Week / Month) |
| `f` | Open filter panel, including the History time-window section |
| `j` / `k` / `g` / `G` | Navigate (own list, not generic feed) |
| `Enter` | Reopen paper, or re-run query (clears session for fresh result) |
| `Ctrl+D` | Delete selected entry |

---

## Search bar
| Key | Action |
|---|---|
| `/` | Open (clears query) |
| Type | Append to query |
| `Backspace` | Pop char |
| `Enter` | Apply / unfocus |
| `Esc` | Cancel and clear |

---

## Filter panel (after pressing `f`)

| Key | Action |
|---|---|
| `j` / `k` | Navigate filter rows |
| `Space` | Toggle selected filter |
| `c` | Clear all filters |
| `f` / `Tab` | Close panel without clearing filters |
| `Esc` | Close panel without clearing filters |

Sections: Sources · Signal · Content · Tags · Sort · Browse · Clear filters
(Tags section appears only when at least one tag exists)

---

## Tag picker popup (after pressing `t`)

| Key | Action |
|---|---|
| Type | Append to input field (used for adding new tags) |
| `↑` / `↓` | Navigate tag list |
| `Space` | Toggle highlighted tag on target(s) |
| `Enter` | If input non-empty: add new tag and apply. Else toggle highlighted. |
| `Backspace` | Pop char from input |
| `Esc` | Close picker |

Behaviour: if any target lacks the tag, applying *adds* to all targets. If
every target already has the tag, applying *removes* from all (idempotent
multi-toggle).

---

## Reader (tread inside One-Research)

| Key | Action |
|---|---|
| Vim navigation | `h/j/k/l`, `0/$`, `gg/G`, `^d/^u`, `^f/^b`, etc. |
| `q` / `Esc` | Close reader or step back reader/feed state |
| `Tab` | Switch primary / secondary reader pane |
| `Ldr+f` | Cycle reader feed / feed drawer layout |
| `Ldr+n` | Open notes for the current reader paper |
| `Ldr+t` | Open in new tab (prompts target pane if dual reader active) |
| `Ldr+[` / `Ldr+]` | Previous / next reader tab |
| `Ldr+w` | Close current reader tab |
| Voice | `r` read aloud · `R` from cursor · `Ctrl+P` continuous |
| Playback | `Space` pause/resume · `c` re-centre · `Esc` stop |
| Feed drawer | `j/k` move · `d` details · `/` search · `Enter` open |

---

## Chat

| Key | Action |
|---|---|
| `Ldr+c` | Toggle chat panel |
| `Ldr+z` | Move chat top / bottom |
| `Enter` | Send message |
| `Esc` | Switch to normal mode |
| Normal mode: `i` / `a` / `Enter` | Back to insert mode |
| Normal mode: `j` / `k` | Scroll chat history |
| Normal mode: `PageDown` / `PageUp` | Half-page scroll |
| `/` | Open slash command palette |
| In palette: `↑` / `↓` or `Ctrl+P` / `Ctrl+N` | Navigate |
| In palette: `Tab` | Complete selected command |
| In palette: `Enter` | Run selected command |
| Session list: `n` | New session |
| Session list: `d` | Delete session |
| Session list: `Enter` | Open session |

---

## Notes

| Key | Action |
|---|---|
| `Ldr+n` | Open notes from current context, or hide the currently focused notes pane |
| `[` / `]` | Cycle notes mode backward / forward (`Paper Notes` / `Library` / `Capture`) |
| `j` / `k` or `↓`/`↑` | Move note selection in the One Research notes browser |
| `g` / `G` | Jump to first / last note |
| `PageUp` / `PageDown` | Move note selection by page |
| `n` / `Enter` | In `Capture`, open the prefilled linked-note composer |
| `Enter` | In `Library` / `Paper Notes`, edit the selected note |
| `a` | Attach the current paper context to the active note |
| `x` | Detach the current paper context from the active note |
| `Ldr+[` / `Ldr+]` | Previous / next note tab |
| `Ldr+w` | Close active note tab |
| `Esc` | Back out of preview/editor/popups, then close the notes pane |

Notes browser rendering is One Research-owned in v1; `crates/notes` still owns the
editor and note popups.

---

## Settings (Ldr+s)

| Key | Action |
|---|---|
| `Ldr+s` | Open settings |
| `j` / `k` | Navigate fields |
| `Enter` | Edit field or cycle option |
| `s` / `S` | Save all fields |
| `p` | Manage sources |
| `q` / `Esc` | Close settings |

### Sources picker
- `Space` toggle · `Enter` or `/` add URL · `d` delete

### Theme picker
- `j` / `k` preview · `Enter` select / create · `e` edit existing

### Theme editor
- `Space` apply · `x` enter hex · `n` rename · `s` save

---

## Repo viewer (`v` on a feed item with a linked repository)

| Key | Action |
|---|---|
| `j` / `k` | Move in the tree or scroll the preview |
| `Enter` | Open file or folder |
| `b` / `Backspace` | Go back |
| `Tab` / `Shift+Tab` | Switch tree / content pane |
| `h` / `l` | Pan preview left / right |
| `+` / `=` / `-` | Zoom in / out |
| `o` | Open the current GitHub repo / path / file URL in the browser |
| `y` | Copy the current repository path |
| `u` | Copy the current GitHub URL |
| `d` | Download file |
| `Esc` | Step back to the tree pane, then close the viewer |
| `q` | Close viewer |

---

## Leader bindings (after Ctrl+T)

| Key | Action |
|---|---|
| `?` | Help overlay |
| `q` | Quit application |
| `s` | Open settings |
| `c` | Toggle chat panel |
| `z` | Move chat to top / bottom |
| `n` | Toggle reader notes dock |
| `f` | Cycle reader feed / feed drawer layout |
| `Enter` | Open floating reader popup (Ldr+Esc to dismiss) |
| `t` | Open paper in new reader tab (prompts pane if dual) |
| `[` / `]` | Previous / next reader or notes tab |
| `w` | Close current tab |
| `h` / `j` / `k` / `l` | Pane focus by direction |
| `1` / `2` / `3` | Focus interactive pane by number |

---

## Slash commands

### Discovery palette (only show in discovery bar)
- `/discover TOPIC` — generic discovery (auto-classified intent)
- `/sota TOPIC` — state-of-the-art / benchmark comparison
- `/reading-list TOPIC` — ordered learning path
- `/code TOPIC` — implementation search
- `/compare TOPIC` — side-by-side approach comparison
- `/digest` — what happened in AI/ML this week (no topic)
- `/author NAME` — find papers by a researcher
- `/trending TOPIC` — recency-weighted trending papers
- `/watch TOPIC` — coming soon: monitor a topic over time

### Chat-only slash commands (not in discovery palette)
- `/clear` — clear current chat session
- `/clear discoveries` — clear discovery results + session history
- `/clear history` — wipe activity history
- `/add CATEGORY` — add an arXiv category permanently
- `/add-feed URL` — add an RSS/Atom feed permanently
- `/export-history [md|jsonl]` — export current history view to `~/.config/one-research/exports/`
- `/export-library [md|jsonl]` — export current library view (respects active filters)

---

## Modal overlays (intercept all keys)

| Overlay | Trigger | Dismiss |
|---|---|---|
| Quit popup | `q` from feed | `q`/`Enter` confirm · `Esc` cancel |
| Tag picker | `t` on Library item | `Esc` |
| Help overlay | `?` or `Ldr+?` | `q` / `Esc` |
| Theme picker | settings → theme field | `q` / `Esc` |
| Abstract popup | `Space` on feed item | `Space` / `Enter` / `Esc` |
| Reader popup (Ldr+Enter) | leader+Enter on feed item | `Esc` |
| Sources popup | `p` in settings | `q` / `Esc` |
| Tab window prompt | `Ldr+t` while dual reader active | `1` / `2` choose · `Esc` cancel |

---

## Quick reference — what's NEW this session

These are bindings that didn't exist before the session that shipped the
History/Library/tag work. Sanity-check: if any of these are unfamiliar to a
user, they belong in onboarding/help update notes.

- `Tab` cycles 4 tabs (was 2)
- `[` / `]` cycle workflow filters on Library
- `[` / `]` cycle time filters on History
- `v` enters visual mode (Library only)
- `t` opens tag picker (Library only)
- `Ctrl+D` deletes selected History entry
- `Ctrl+N` forces new discovery search
- `/clear history` slash command
- `/export-history` and `/export-library` slash commands
- `/sota`, `/reading-list`, `/code`, `/compare`, `/digest`, `/author`, `/trending`, `/watch` slash commands
- Quit popup intercepts `q` from feed
