use ratatui::{
  Frame,
  layout::{Constraint, Direction, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::Paragraph,
};

use crate::app::{App, FeedTab, FocusedReader, NotesMode, PaneId};
use std::borrow::Cow;

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
  let t = app.theme();
  let rows = Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Length(1), Constraint::Length(1)])
    .split(area);

  let status_line: Option<Line> = if app.async_jobs.fulltext_loading {
    let spin = SPINNER[app.async_jobs.spinner_frame % SPINNER.len()];
    Some(Line::from(Span::styled(
      format!("{spin} fetching article…"),
      Style::default().fg(t.warning),
    )))
  } else if let Some(msg) = &app.status_message {
    Some(Line::from(Span::styled(msg.clone(), Style::default().fg(t.warning))))
  } else if app.async_jobs.is_loading {
    let spin = SPINNER[app.async_jobs.spinner_frame % SPINNER.len()];
    let sources = app.async_jobs.loading_sources.join(", ");
    let prefix = if app.is_refreshing { "↻ refreshing" } else { "fetching" };
    Some(Line::from(Span::styled(
      format!(
        "{spin} {prefix}: {}  │  {} items",
        sources,
        app.workspace.items_store.len()
      ),
      Style::default().fg(t.warning),
    )))
  } else {
    None
  };

  let command_line = footer_command_line(app, area.width);
  if let Some(line) = status_line {
    frame.render_widget(Paragraph::new(vec![line]), rows[0]);
    frame.render_widget(Paragraph::new(vec![command_line]), rows[1]);
  } else {
    frame.render_widget(Paragraph::new(vec![command_line]), rows[0]);
    frame.render_widget(Paragraph::new(""), rows[1]);
  }
}

fn footer_command_line(app: &App, width: u16) -> Line<'static> {
  let t = app.theme();
  let ordinary = Style::default().fg(t.text_dim);
  let accent = Style::default().fg(t.accent).add_modifier(Modifier::BOLD);
  let repo_style = Style::default().fg(t.success);
  let visible = app.visible_count();
  let total = app.items_for_tab().len();
  let filtered =
    !app.feed.search_query.is_empty() || !app.feed.active_filters.is_empty();
  let repo_available = !app.reader.active
    && !app.chat.fullscreen
    && app.focus.focused_pane == PaneId::Feed
    && app.selected_item().is_some_and(|item| {
      item.github_owner.is_some() && item.github_repo_name.is_some()
    });

  let mut spans = Vec::new();

  if app.leader.is_active() {
    const KEYS: &[FooterKey] = &[
      FooterKey { key: "f", label: "feed" },
      FooterKey { key: "t", label: "tab" },
      FooterKey { key: "[/]", label: "tabs" },
      FooterKey { key: "n", label: "notes" },
      FooterKey { key: "c", label: "chat" },
      FooterKey { key: "h/j/k/l", label: "focus" },
    ];
    return responsive_footer(
      Vec::new(),
      "leader",
      KEYS,
      true,
      width,
      ordinary,
      accent,
    );
  }

  if app.reader.dual_active
    && app.reader_bottom.open
    && app.reader_bottom.focused
  {
    let label = if app.reader_bottom.details {
      "feed drawer details"
    } else {
      "feed drawer"
    };
    const DETAILS: &[FooterKey] = &[
      FooterKey { key: "j/k", label: "scroll" },
      FooterKey { key: "d", label: "back" },
      FooterKey { key: "q/Esc", label: "close" },
    ];
    const SEARCH: &[FooterKey] = &[
      FooterKey { key: "type filter", label: "" },
      FooterKey { key: "Enter", label: "keep" },
      FooterKey { key: "Esc", label: "clear" },
      FooterKey { key: "j/k", label: "move" },
    ];
    const BROWSE: &[FooterKey] = &[
      FooterKey { key: "j/k", label: "move" },
      FooterKey { key: "Enter", label: "open" },
      FooterKey { key: "Space", label: "abstract" },
      FooterKey { key: "d", label: "details" },
      FooterKey { key: "/", label: "search" },
      FooterKey { key: "q/Esc", label: "close" },
    ];
    let items = if app.reader_bottom.details {
      DETAILS
    } else if app.feed.search_active {
      SEARCH
    } else {
      BROWSE
    };
    return responsive_footer(
      Vec::new(),
      label,
      items,
      true,
      width,
      ordinary,
      accent,
    );
  }

  if app.feed.search_active {
    const KEYS: &[FooterKey] = &[
      FooterKey { key: "type to filter", label: "" },
      FooterKey { key: "Enter", label: "keep" },
      FooterKey { key: "Esc", label: "clear" },
    ];
    return responsive_footer(
      Vec::new(),
      "search",
      KEYS,
      true,
      width,
      ordinary,
      accent,
    );
  }

  if app.feed.filter_focus {
    const KEYS: &[FooterKey] = &[
      FooterKey { key: "j/k", label: "move" },
      FooterKey { key: "Space", label: "toggle" },
      FooterKey { key: "c", label: "clear" },
      FooterKey { key: "f/Tab/Esc", label: "close" },
    ];
    return responsive_footer(
      Vec::new(),
      "filters",
      KEYS,
      false,
      width,
      ordinary,
      accent,
    );
  }

  if app.focus.focused_pane == PaneId::Reader
    || app.focus.focused_pane == PaneId::SecondaryReader
  {
    const KEYS: &[FooterKey] = &[
      FooterKey { key: "q/Esc", label: "close" },
      FooterKey { key: "Tab", label: "switch pane" },
      FooterKey { key: "Ldr+n", label: "notes" },
    ];
    return responsive_footer(
      Vec::new(),
      "reader",
      KEYS,
      true,
      width,
      ordinary,
      accent,
    );
  }

  if (app.focus.focused_pane == PaneId::Notes && app.notes.primary_visible)
    || (app.focus.focused_pane == PaneId::SecondaryNotes
      && app.notes.secondary_visible)
  {
    let side = if app.focus.focused_pane == PaneId::SecondaryNotes {
      FocusedReader::Secondary
    } else {
      FocusedReader::Primary
    };
    let mode = app.notes_mode_for_side(side);
    const CAPTURE: &[FooterKey] = &[
      FooterKey { key: "[ / ]", label: "modes" },
      FooterKey { key: "n/Enter", label: "create" },
      FooterKey { key: "Ldr+n", label: "hide" },
    ];
    const PAPER: &[FooterKey] = &[
      FooterKey { key: "[ / ]", label: "modes" },
      FooterKey { key: "j/k", label: "move" },
      FooterKey { key: "a", label: "attach" },
      FooterKey { key: "x", label: "detach" },
      FooterKey { key: "Enter", label: "edit" },
      FooterKey { key: "Ldr+w", label: "close" },
      FooterKey { key: "Ldr+n", label: "hide" },
    ];
    const LIBRARY: &[FooterKey] = &[
      FooterKey { key: "[ / ]", label: "modes" },
      FooterKey { key: "j/k", label: "move" },
      FooterKey { key: "Enter", label: "edit" },
      FooterKey { key: "a", label: "attach" },
      FooterKey { key: "x", label: "detach" },
      FooterKey { key: "Ldr+w", label: "close" },
      FooterKey { key: "Ldr+n", label: "hide" },
    ];
    let items = match mode {
      NotesMode::Capture => CAPTURE,
      NotesMode::PaperNotes => PAPER,
      NotesMode::Library => LIBRARY,
    };
    return responsive_footer(
      Vec::new(),
      mode.footer_label(),
      items,
      true,
      width,
      ordinary,
      accent,
    );
  }

  if app.focus.focused_pane == PaneId::Chat && app.chat.active {
    const KEYS: &[FooterKey] = &[
      FooterKey { key: "Enter", label: "send" },
      FooterKey { key: "/", label: "commands" },
      FooterKey { key: "Esc", label: "sessions" },
      FooterKey { key: "Ldr+c", label: "hide" },
    ];
    return responsive_footer(
      Vec::new(),
      "chat",
      KEYS,
      true,
      width,
      ordinary,
      accent,
    );
  }

  if filtered {
    spans.push(Span::styled(format!("{visible}/{total} filtered"), ordinary));
    spans.push(Span::styled(" | ", ordinary));
  }
  if repo_available {
    spans.push(Span::styled("v repo", repo_style));
    spans.push(Span::styled(" | ", ordinary));
  }

  if app.feed.feed_tab == FeedTab::Discoveries {
    const KEYS: &[FooterKey] = &[
      FooterKey { key: "/", label: "search" },
      FooterKey { key: "Enter", label: "open" },
      FooterKey { key: "Ctrl+N", label: "new" },
      FooterKey { key: "Tab", label: "history" },
    ];
    return responsive_footer(
      spans,
      "discoveries",
      KEYS,
      true,
      width,
      ordinary,
      accent,
    );
  } else if app.feed.feed_tab == FeedTab::Library {
    if app.feed.library_visual_mode {
      const KEYS: &[FooterKey] = &[
        FooterKey { key: "j/k", label: "select" },
        FooterKey { key: "r", label: "read" },
        FooterKey { key: "w", label: "queue" },
        FooterKey { key: "x", label: "archive" },
        FooterKey { key: "t", label: "tag" },
        FooterKey { key: "Esc", label: "cancel" },
      ];
      return responsive_footer(
        spans,
        "library visual",
        KEYS,
        false,
        width,
        ordinary,
        accent,
      );
    } else {
      const KEYS: &[FooterKey] = &[
        FooterKey { key: "[/]", label: "state" },
        FooterKey { key: "v", label: "select" },
        FooterKey { key: "t", label: "tag" },
        FooterKey { key: "Tab", label: "discoveries" },
      ];
      let mode = format!("library · {}", app.feed.library_filter.label());
      return responsive_footer(
        spans, mode, KEYS, true, width, ordinary, accent,
      );
    }
  } else if app.feed.feed_tab == FeedTab::History {
    const KEYS: &[FooterKey] = &[
      FooterKey { key: "[/]", label: "time" },
      FooterKey { key: "Enter", label: "reopen" },
      FooterKey { key: "Ctrl+D", label: "delete" },
      FooterKey { key: "/", label: "search" },
      FooterKey { key: "Tab", label: "inbox" },
    ];
    let mode = format!("history · {}", app.feed.history_filter.label());
    return responsive_footer(spans, mode, KEYS, true, width, ordinary, accent);
  } else if app.feed.feed_tab == FeedTab::Browse {
    if app.browse.focus == crate::app::BrowseFocus::Feed {
      const KEYS: &[FooterKey] = &[
        FooterKey { key: "j/k", label: "move" },
        FooterKey { key: "Enter", label: "read" },
        FooterKey { key: "Space", label: "details" },
        FooterKey { key: "l", label: "subjects" },
        FooterKey { key: "x", label: "archive" },
        FooterKey { key: "Tab", label: "library" },
      ];
      return responsive_footer(
        spans,
        "browse feed",
        KEYS,
        true,
        width,
        ordinary,
        accent,
      );
    } else {
      const KEYS: &[FooterKey] = &[
        FooterKey { key: "h", label: "back/feed" },
        FooterKey { key: "l", label: "drill" },
        FooterKey { key: "j/k", label: "move" },
        FooterKey { key: "Enter", label: "load" },
        FooterKey { key: "p", label: "promote" },
        FooterKey { key: "x", label: "follow" },
        FooterKey { key: "Tab", label: "library" },
      ];
      return responsive_footer(
        spans, "browse", KEYS, false, width, ordinary, accent,
      );
    }
  } else {
    const FEED_KEYS: &[FooterKey] = &[
      FooterKey { key: "j/k", label: "move" },
      FooterKey { key: "Enter", label: "read" },
      FooterKey { key: "Space", label: "details" },
      FooterKey { key: "f", label: "filters" },
      FooterKey { key: "Tab", label: "browse" },
      FooterKey { key: "i", label: "inbox" },
      FooterKey { key: "r", label: "read" },
      FooterKey { key: "w", label: "queue" },
      FooterKey { key: "x", label: "archive" },
      FooterKey { key: "q", label: "quit" },
    ];
    return responsive_footer(
      spans, "feed", FEED_KEYS, true, width, ordinary, accent,
    );
  }
}

/// One footer hotkey: the key glyph(s) and the action it performs. A blank
/// `label` marks an instruction phrase (e.g. `type to filter`) that has no
/// distinct key — it renders verbatim and is left intact when collapsed.
struct FooterKey {
  key: &'static str,
  label: &'static str,
}

/// Build a footer command line that collapses to keys-only when the fully
/// labelled form would overflow `width`. `leading` carries any prefix spans
/// already accumulated (e.g. the `N/M filtered` or `v repo` hints); the `mode`
/// label is always shown, and `? help` is pinned when `help` is set — only the
/// inner key *labels* are dropped on collapse, so no hotkey ever disappears.
fn responsive_footer(
  leading: Vec<Span<'static>>,
  mode: impl Into<Cow<'static, str>>,
  items: &[FooterKey],
  help: bool,
  width: u16,
  ordinary: Style,
  accent: Style,
) -> Line<'static> {
  let mode = mode.into();
  let full = build_footer_line(
    &leading,
    mode.clone(),
    items,
    help,
    false,
    ordinary,
    accent,
  );
  if full.width() as u16 <= width {
    full
  } else {
    build_footer_line(&leading, mode, items, help, true, ordinary, accent)
  }
}

/// Assemble the footer line in one of two tiers. Full tier renders
/// `mode: key label | …`; keys-only renders `mode: key · …`. The hotkey run is
/// a single dim span (matching the existing footer styling); `? help` is
/// appended (kept labelled) only when `help` is set.
fn build_footer_line(
  leading: &[Span<'static>],
  mode: Cow<'static, str>,
  items: &[FooterKey],
  help: bool,
  keys_only: bool,
  ordinary: Style,
  accent: Style,
) -> Line<'static> {
  let sep = if keys_only { " · " } else { " | " };
  let mut s = String::from(": ");
  let mut first = true;
  for it in items {
    if !first {
      s.push_str(sep);
    }
    first = false;
    s.push_str(it.key);
    if !keys_only && !it.label.is_empty() {
      s.push(' ');
      s.push_str(it.label);
    }
  }
  if help {
    if !first {
      s.push_str(sep);
    }
    s.push_str("? help");
  }

  let mut spans = leading.to_vec();
  spans.push(Span::styled(mode, accent));
  spans.push(Span::styled(s, ordinary));
  Line::from(spans)
}
