use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Paragraph, Wrap},
};

use super::widgets::truncate;
use crate::app::{App, FocusedReader, NotesMode, PaneId};

pub(super) fn note_pane_for_side(side: FocusedReader) -> PaneId {
  match side {
    FocusedReader::Primary => PaneId::Notes,
    FocusedReader::Secondary => PaneId::SecondaryNotes,
  }
}

/// Shrink a rect by per-edge padding (CSS order: top, right, bottom, left),
/// saturating so an over-tight rect collapses to zero rather than underflowing.
fn pad(area: Rect, top: u16, right: u16, bottom: u16, left: u16) -> Rect {
  Rect {
    x: area.x.saturating_add(left),
    y: area.y.saturating_add(top),
    width: area.width.saturating_sub(left.saturating_add(right)),
    height: area.height.saturating_sub(top.saturating_add(bottom)),
  }
}

fn notes_browser_visible<'a>(
  app: &'a App,
  side: FocusedReader,
) -> Vec<&'a notes::Note> {
  let Some(notes_app) = app.notes.app.as_ref() else {
    return Vec::new();
  };
  match app.notes_mode_for_side(side) {
    NotesMode::Capture => Vec::new(),
    NotesMode::Library => notes_app.get_active_notes().collect(),
    NotesMode::PaperNotes => {
      let Some(context) = app.notes_context_for_side(side) else {
        return Vec::new();
      };
      notes_app
        .get_active_notes()
        .filter(|note| {
          note.linked_papers.iter().any(|paper| paper.id == context.paper.id)
        })
        .collect()
    }
  }
}

fn notes_browser_selected_index(
  app: &App,
  side: FocusedReader,
  visible: &[&notes::Note],
) -> Option<usize> {
  // Selection identity is owned by the model (ADR-016). The orchestrator
  // keeps `selected` in sync with the backend after each key dispatch;
  // render reads it here, never `current_note_id`.
  let selected = app.notes.instance(side)?.selected_note_id()?;
  visible.iter().position(|note| note.note_id == selected)
}

fn notes_browser_selected_note<'a>(
  app: &App,
  side: FocusedReader,
  visible: &[&'a notes::Note],
) -> Option<&'a notes::Note> {
  let idx = notes_browser_selected_index(app, side, visible)?;
  visible.get(idx).copied()
}

fn draw_notes_mode_switcher(
  frame: &mut Frame,
  area: Rect,
  mode: NotesMode,
  focused: bool,
  paper_title: Option<&str>,
  t: &crate::theme::Theme,
) {
  // Bold carries the hierarchy: the whole switcher reads as the pane header by
  // weight, so no divider rule is needed to set it apart from the content below.
  let separator = Style::default().fg(t.text_dim);
  let inactive = Style::default().fg(t.text_dim).add_modifier(Modifier::BOLD);
  let active = if focused {
    Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
  } else {
    Style::default().fg(t.header).add_modifier(Modifier::BOLD)
  };
  let mut spans = Vec::new();
  for (idx, candidate) in
    [NotesMode::PaperNotes, NotesMode::Library, NotesMode::Capture]
      .into_iter()
      .enumerate()
  {
    if idx > 0 {
      spans.push(Span::styled("  ·  ", separator));
    }
    let label = match candidate {
      NotesMode::PaperNotes => "Paper Notes",
      NotesMode::Library => "Library",
      NotesMode::Capture => "Capture",
    };
    spans.push(Span::styled(
      label,
      if candidate == mode { active } else { inactive },
    ));
  }
  // The paper in context rides at the right edge as a dim breadcrumb. Dim
  // weight plus far-right placement keep it from reading as a fourth mode; it
  // is dropped whole when the pane is too narrow to seat it without crowding.
  if let Some(title) = paper_title {
    let switcher_w: usize =
      spans.iter().map(|s| s.content.chars().count()).sum();
    let total = area.width as usize;
    const GAP: usize = 3;
    const MIN_CRUMB: usize = 12;
    const RIGHT_MARGIN: usize = 1;
    let avail = total.saturating_sub(switcher_w + GAP + RIGHT_MARGIN);
    if avail >= MIN_CRUMB {
      let crumb = truncate(title, avail);
      let filler =
        total.saturating_sub(switcher_w + crumb.chars().count() + RIGHT_MARGIN);
      spans.push(Span::raw(" ".repeat(filler)));
      spans.push(Span::styled(crumb, Style::default().fg(t.text_dim)));
    }
  }
  frame.render_widget(
    Paragraph::new(Line::from(spans)).style(Style::default().fg(t.text_dim)),
    area,
  );
}

fn draw_notes_empty_state(
  frame: &mut Frame,
  area: Rect,
  mode: NotesMode,
  paper_title: Option<&str>,
  t: &crate::theme::Theme,
) {
  let lines = match mode {
    NotesMode::PaperNotes => vec![
      Line::from(Span::styled(
        "No notes linked to this paper.",
        Style::default().fg(t.text).add_modifier(Modifier::BOLD),
      )),
      Line::from(""),
      Line::from(Span::styled(
        "Press ] to switch to Capture, then n or Enter to create one.",
        Style::default().fg(t.text_dim),
      )),
    ],
    NotesMode::Library => vec![
      Line::from(Span::styled(
        "No notes yet.",
        Style::default().fg(t.text).add_modifier(Modifier::BOLD),
      )),
      Line::from(""),
      Line::from(Span::styled(
        "Press n to create a note, or open notes from a paper with Ldr+n.",
        Style::default().fg(t.text_dim),
      )),
    ],
    NotesMode::Capture => vec![
      Line::from(Span::styled(
        // Only claim "this paper" when a paper is actually in context; the
        // breadcrumb names it, so the prompt stays generic otherwise.
        if paper_title.is_some() {
          "Capture a note linked to this paper."
        } else {
          "Capture a linked note."
        },
        Style::default().fg(t.text).add_modifier(Modifier::BOLD),
      )),
      Line::from(""),
      Line::from(Span::styled(
        "Press n or Enter to begin.",
        Style::default().fg(t.text_dim),
      )),
    ],
  };
  let chunks = Layout::vertical([
    Constraint::Percentage(24),
    Constraint::Length((lines.len() as u16).saturating_add(1)),
    Constraint::Min(0),
  ])
  .split(area);
  frame.render_widget(
    Paragraph::new(lines)
      .alignment(Alignment::Center)
      .wrap(Wrap { trim: false })
      .style(Style::default().fg(t.text_dim)),
    chunks[1],
  );
}

fn note_preview_meta_summary(note: &notes::Note) -> String {
  let mut parts = Vec::new();
  let noun = if note.linked_papers.len() == 1 { "paper" } else { "papers" };
  parts.push(format!("{} {noun}", note.linked_papers.len()));
  parts.push(note.updated_at.format("%Y-%m-%d").to_string());
  if !note.tags.is_empty() {
    parts
      .push(note.tags.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
  }
  parts.join("  ·  ")
}

fn draw_notes_browser_list(
  frame: &mut Frame,
  area: Rect,
  notes: &[&notes::Note],
  selected_idx: Option<usize>,
  focused: bool,
  t: &crate::theme::Theme,
) {
  if area.height == 0 || area.width == 0 {
    return;
  }
  if notes.is_empty() {
    draw_notes_empty_state(frame, area, NotesMode::Library, None, t);
    return;
  }

  let slots = (area.height as usize).max(1);
  let selected = selected_idx.unwrap_or(0).min(notes.len().saturating_sub(1));
  let start =
    selected.saturating_sub(slots / 2).min(notes.len().saturating_sub(slots));
  let end = (start + slots).min(notes.len());
  let selection_style = if focused {
    Style::default().bg(t.bg_selection).fg(t.text)
  } else {
    Style::default().bg(t.bg_panel).fg(t.text)
  };

  let mut y = area.y;
  for (idx, note) in notes[start..end].iter().enumerate() {
    let note_index = start + idx;
    let row_area = Rect { x: area.x, y, width: area.width, height: 1 };
    let is_selected = note_index == selected;
    let title = truncate(&note.title, area.width.saturating_sub(3) as usize);
    let line = Line::from(vec![
      Span::styled(
        if is_selected { "› " } else { "  " },
        if is_selected {
          selection_style
        } else {
          Style::default().fg(t.text_dim)
        },
      ),
      Span::styled(
        title,
        if is_selected {
          selection_style.add_modifier(Modifier::BOLD)
        } else {
          Style::default().fg(t.text)
        },
      ),
    ]);
    frame.render_widget(
      Paragraph::new(line)
        .style(if is_selected { selection_style } else { Style::default() })
        .wrap(Wrap { trim: false }),
      row_area,
    );
    y = y.saturating_add(1);
    if y >= area.y + area.height {
      break;
    }
  }
}

fn draw_notes_browser_preview(
  frame: &mut Frame,
  area: Rect,
  note: Option<&notes::Note>,
  t: &crate::theme::Theme,
) {
  let Some(note) = note else {
    frame.render_widget(
      Paragraph::new("No note selected")
        .style(Style::default().fg(t.text_dim))
        .alignment(Alignment::Center),
      area,
    );
    return;
  };
  let mut lines = vec![
    Line::from(""),
    Line::from(Span::styled(
      truncate(&note.title, area.width as usize),
      Style::default().fg(t.text).add_modifier(Modifier::BOLD),
    )),
  ];
  lines.push(Line::from(Span::styled(
    truncate(&note_preview_meta_summary(note), area.width as usize),
    Style::default().fg(t.text_dim),
  )));
  lines.push(Line::from(""));
  if note.content.trim().is_empty() {
    lines.push(Line::from(Span::styled(
      "Empty note",
      Style::default().fg(t.text_dim),
    )));
  } else {
    for line in textwrap::wrap(&note.content, area.width as usize).into_iter() {
      lines.push(Line::from(Span::styled(
        line.into_owned(),
        Style::default().fg(t.text),
      )));
    }
  }
  frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

pub fn draw_notes_surface(
  frame: &mut Frame,
  app: &mut App,
  area: Rect,
  side: FocusedReader,
  preview_when_unfocused: bool,
  theme: &crate::theme::Theme,
) {
  if area.height == 0 || area.width == 0 {
    return;
  }
  let is_focused = app.focus.focused_pane == note_pane_for_side(side);
  // Secondary may not exist yet (Option<NotesInstanceModel>); bail —
  // layout should already have 0-sized the rect via the visibility gate
  // upstream, but this is defense-in-depth.
  if app.notes.instance(side).is_none() {
    return;
  }
  // Every mode shares one header row; the paper in context (Paper Notes and
  // Capture only — Library is the unfiltered library) rides it as a right-
  // aligned breadcrumb, so all three modes start their content at the same row.
  let mode = app.notes_mode_for_side(side);
  let paper_title = match mode {
    NotesMode::PaperNotes | NotesMode::Capture => {
      app.notes_context_for_side(side).map(|ctx| ctx.paper.title.clone())
    }
    NotesMode::Library => None,
  };
  let rows =
    Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
  draw_notes_mode_switcher(
    frame,
    rows[0],
    mode,
    is_focused,
    paper_title.as_deref(),
    theme,
  );
  let content_area = rows[1];

  let editor_active = app.notes.app.as_ref().is_some_and(|notes_app| {
    notes_app.notes_state == notes::app::NotesState::Editor
  });
  let popup_active = app
    .notes
    .app
    .as_ref()
    .is_some_and(|notes_app| !notes_app.active_popup.is_none());

  if editor_active {
    if let Some(notes_app) = app.notes.app.as_mut() {
      notes_app.draw_editor_surface(frame, content_area);
      if popup_active {
        notes_app.draw_popup_overlay(frame, content_area);
      }
    }
    return;
  }

  if preview_when_unfocused && !is_focused {
    draw_note_preview(frame, app, content_area, side, theme);
    if popup_active {
      if let Some(notes_app) = app.notes.app.as_mut() {
        notes_app.draw_popup_overlay(frame, content_area);
      }
    }
    return;
  }

  match app.notes_mode_for_side(side) {
    NotesMode::Capture => {
      draw_notes_empty_state(
        frame,
        content_area,
        NotesMode::Capture,
        paper_title.as_deref(),
        theme,
      );
    }
    NotesMode::PaperNotes | NotesMode::Library => {
      let visible = notes_browser_visible(app, side);
      if visible.is_empty() {
        draw_notes_empty_state(
          frame,
          content_area,
          app.notes_mode_for_side(side),
          paper_title.as_deref(),
          theme,
        );
      } else if content_area.width >= 72 {
        let chunks = Layout::horizontal([
          Constraint::Percentage(44),
          Constraint::Length(1),
          Constraint::Percentage(56),
        ])
        .split(content_area);
        draw_notes_browser_list(
          frame,
          pad(chunks[0], 1, 1, 0, 1),
          &visible,
          notes_browser_selected_index(app, side, &visible),
          is_focused,
          theme,
        );
        frame.render_widget(
          Paragraph::new("│").style(Style::default().fg(theme.border)),
          chunks[1],
        );
        draw_notes_browser_preview(
          frame,
          pad(chunks[2], 0, 1, 0, 2),
          notes_browser_selected_note(app, side, &visible),
          theme,
        );
      } else {
        let chunks = Layout::vertical([
          Constraint::Percentage(48),
          Constraint::Length(1),
          Constraint::Percentage(52),
        ])
        .split(content_area);
        draw_notes_browser_list(
          frame,
          pad(chunks[0], 1, 1, 0, 1),
          &visible,
          notes_browser_selected_index(app, side, &visible),
          is_focused,
          theme,
        );
        frame.render_widget(
          Paragraph::new("─".repeat(chunks[1].width as usize))
            .style(Style::default().fg(theme.border)),
          chunks[1],
        );
        draw_notes_browser_preview(
          frame,
          pad(chunks[2], 0, 1, 0, 1),
          notes_browser_selected_note(app, side, &visible),
          theme,
        );
      }
    }
  }

  if popup_active {
    if let Some(notes_app) = app.notes.app.as_mut() {
      notes_app.draw_popup_overlay(frame, content_area);
    }
  }
}

pub fn draw_note_dock(
  frame: &mut Frame,
  app: &mut App,
  area: Rect,
  side: FocusedReader,
  theme: &crate::theme::Theme,
) {
  draw_notes_surface(frame, app, area, side, true, theme);
}

fn draw_note_preview(
  frame: &mut Frame,
  app: &App,
  area: Rect,
  side: FocusedReader,
  t: &crate::theme::Theme,
) {
  let selected = app
    .notes
    .instance(side)
    .and_then(|inst| inst.selected.as_deref())
    .and_then(|id| app.notes.app.as_ref().and_then(|na| na.get_note(id)));
  draw_notes_browser_preview(frame, pad(area, 0, 1, 0, 1), selected, t);
}
