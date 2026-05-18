use ratatui::{
  Frame,
  layout::{Constraint, Direction, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::Paragraph,
};

use crate::app::{App, FeedTab, FocusedReader, NotesMode, PaneId};

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
  let t = app.theme();
  let rows = Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Length(1), Constraint::Length(1)])
    .split(area);

  let status_line: Option<Line> = if app.fulltext_loading {
    let spin = SPINNER[app.spinner_frame % SPINNER.len()];
    Some(Line::from(Span::styled(
      format!("{spin} fetching article…"),
      Style::default().fg(t.warning),
    )))
  } else if let Some(msg) = &app.status_message {
    Some(Line::from(Span::styled(msg.clone(), Style::default().fg(t.warning))))
  } else if app.is_loading {
    let spin = SPINNER[app.spinner_frame % SPINNER.len()];
    let sources = app.loading_sources.join(", ");
    let prefix = if app.is_refreshing { "↻ refreshing" } else { "fetching" };
    Some(Line::from(Span::styled(
      format!(
        "{spin} {prefix}: {}  │  {} items",
        sources,
        app.workspace.items.len()
      ),
      Style::default().fg(t.warning),
    )))
  } else {
    None
  };

  let command_line = footer_command_line(app);
  if let Some(line) = status_line {
    frame.render_widget(Paragraph::new(vec![line]), rows[0]);
    frame.render_widget(Paragraph::new(vec![command_line]), rows[1]);
  } else {
    frame.render_widget(Paragraph::new(vec![command_line]), rows[0]);
    frame.render_widget(Paragraph::new(""), rows[1]);
  }
}

fn footer_command_line(app: &App) -> Line<'static> {
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

  if app.leader_active {
    spans.push(Span::styled("leader", accent));
    spans.push(Span::styled(
      ": f feed | t tab | [/] tabs | n notes | c chat | h/j/k/l focus | ? help",
      ordinary,
    ));
    return Line::from(spans);
  }

  if app.reader.dual_active
    && app.reader_bottom_open
    && app.reader_bottom_focused
  {
    let label = if app.reader_bottom_details {
      "feed drawer details"
    } else {
      "feed drawer"
    };
    let keys = if app.reader_bottom_details {
      ": j/k scroll | d back | q/Esc close | ? help"
    } else if app.feed.search_active {
      ": type filter | Enter keep | Esc clear | j/k move | ? help"
    } else {
      ": j/k move | / search | Enter open | d details | q/Esc close | ? help"
    };
    spans.push(Span::styled(label, accent));
    spans.push(Span::styled(keys, ordinary));
    return Line::from(spans);
  }

  if app.feed.search_active {
    spans.push(Span::styled("search", accent));
    spans.push(Span::styled(
      ": type to filter | Enter keep | Esc clear | ? help",
      ordinary,
    ));
    return Line::from(spans);
  }

  if app.feed.filter_focus {
    spans.push(Span::styled("filters", accent));
    spans.push(Span::styled(
      ": j/k move | Space toggle | c clear | f/Tab return | Esc clear",
      ordinary,
    ));
    return Line::from(spans);
  }

  if app.focus.focused_pane == PaneId::Reader
    || app.focus.focused_pane == PaneId::SecondaryReader
  {
    spans.push(Span::styled("reader", accent));
    spans.push(Span::styled(
      ": q/Esc close | Tab switch pane | Ldr+t new tab | Ldr+[ / ] tabs | Ldr+n notes | ? help",
      ordinary,
    ));
    return Line::from(spans);
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
    spans
      .push(Span::styled(app.notes_mode_for_side(side).footer_label(), accent));
    let keys = match app.notes_mode_for_side(side) {
      NotesMode::Capture => {
        ": [ / ] modes | n/Enter create | Ldr+n hide | ? help"
      }
      NotesMode::PaperNotes => {
        ": [ / ] modes | j/k move | a attach | x detach | Enter edit | Ldr+[ / ] tabs | Ldr+w close | Ldr+n hide | ? help"
      }
      NotesMode::Library => {
        ": [ / ] modes | j/k move | Enter edit | a attach | x detach | Ldr+[ / ] tabs | Ldr+w close | Ldr+n hide | ? help"
      }
    };
    spans.push(Span::styled(keys, ordinary));
    return Line::from(spans);
  }

  if app.focus.focused_pane == PaneId::Chat && app.chat.active {
    spans.push(Span::styled("chat", accent));
    spans.push(Span::styled(
      ": Enter send | / commands | Esc sessions | Ldr+c hide | ? help",
      ordinary,
    ));
    return Line::from(spans);
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
    spans.push(Span::styled("discoveries", accent));
    spans.push(Span::styled(
      ": / search | Enter open | Ctrl+N new | Tab history | ? help",
      ordinary,
    ));
  } else if app.feed.feed_tab == FeedTab::Library {
    let label =
      if app.feed.library_visual_mode { "library visual" } else { "library" };
    let keys = if app.feed.library_visual_mode {
      ": j/k select | r read | w queue | x archive | t tag | Esc cancel"
    } else {
      ": [/] state | v select | t tag | Tab discoveries | ? help"
    };
    spans.push(Span::styled(label, accent));
    spans.push(Span::styled(keys, ordinary));
  } else if app.feed.feed_tab == FeedTab::History {
    spans.push(Span::styled("history", accent));
    spans.push(Span::styled(
      ": [/] time | Enter reopen | Ctrl+D delete | / search | Tab inbox | ? help",
      ordinary,
    ));
  } else {
    spans.push(Span::styled("feed", accent));
    spans.push(Span::styled(
      ": j/k move | Enter read | Space details | f filters | Tab library",
      ordinary,
    ));
    spans.push(Span::styled(" | ", ordinary));
    spans.push(Span::styled(
      "i inbox | r read | w queue | x archive | q quit | ? help",
      ordinary,
    ));
  }

  Line::from(spans)
}
