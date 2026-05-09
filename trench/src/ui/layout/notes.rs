use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Paragraph, Wrap},
};

use super::widgets::truncate;
use crate::app::{App, FocusedReader, NotesMode, NotesTab, PaneId};

pub(super) fn note_pane_for_side(side: FocusedReader) -> PaneId {
  match side {
    FocusedReader::Primary => PaneId::Notes,
    FocusedReader::Secondary => PaneId::SecondaryNotes,
  }
}

fn notes_browser_visible<'a>(
  app: &'a App,
  side: FocusedReader,
) -> Vec<&'a notes::Note> {
  let Some(notes_app) = app.notes_app.as_ref() else {
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
  visible: &[&notes::Note],
) -> Option<usize> {
  let current_id = app.notes_app.as_ref()?.current_note_id.as_ref()?;
  visible.iter().position(|note| &note.note_id == current_id)
}

fn notes_browser_selected_note<'a>(
  app: &App,
  visible: &[&'a notes::Note],
) -> Option<&'a notes::Note> {
  let idx = notes_browser_selected_index(app, visible)?;
  visible.get(idx).copied()
}

fn draw_notes_mode_switcher(
  frame: &mut Frame,
  area: Rect,
  mode: NotesMode,
  focused: bool,
  t: &crate::theme::Theme,
) {
  let ordinary = Style::default().fg(t.text_dim);
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
      spans.push(Span::styled("  ·  ", ordinary));
    }
    let label = match candidate {
      NotesMode::PaperNotes => "Paper Notes",
      NotesMode::Library => "Library",
      NotesMode::Capture => "Capture",
    };
    spans.push(Span::styled(
      label,
      if candidate == mode { active } else { ordinary },
    ));
  }
  frame.render_widget(
    Paragraph::new(Line::from(spans)).style(Style::default().fg(t.text_dim)),
    area,
  );
}

fn build_notes_summary_line(
  app: &App,
  side: FocusedReader,
  width: u16,
  t: &crate::theme::Theme,
) -> Line<'static> {
  let mode = app.notes_mode_for_side(side);
  let context = app.notes_context_for_side(side);
  // Compute visible once; previously notes_browser_visible ran twice here.
  let visible = notes_browser_visible(app, side);
  let visible_count = visible.len();
  let selected_note = notes_browser_selected_note(app, &visible);
  let summary = match mode {
    NotesMode::Library => {
      if let Some(note) = selected_note {
        format!(
          "{} notes  ·  selected {}",
          visible_count,
          truncate(&note.title, width.saturating_sub(24) as usize)
        )
      } else {
        format!("{visible_count} notes in library")
      }
    }
    NotesMode::PaperNotes => {
      if let Some(ctx) = context {
        format!(
          "{}  ·  {}  ·  {} linked",
          truncate(&ctx.paper.title, width.saturating_sub(28) as usize),
          ctx.source_label,
          visible_count
        )
      } else {
        "No paper context".to_string()
      }
    }
    NotesMode::Capture => {
      if let Some(ctx) = context {
        format!(
          "{}  ·  {}  ·  ready to capture",
          truncate(&ctx.paper.title, width.saturating_sub(36) as usize),
          ctx.source_label
        )
      } else {
        "No paper context".to_string()
      }
    }
  };
  Line::from(Span::styled(
    truncate(&summary, width as usize),
    Style::default().fg(t.text_dim),
  ))
}

fn draw_notes_empty_state(
  frame: &mut Frame,
  area: Rect,
  mode: NotesMode,
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
        "Capture a linked note.",
        Style::default().fg(t.text).add_modifier(Modifier::BOLD),
      )),
      Line::from(""),
      Line::from(Span::styled(
        "Press n or Enter to open the prefilled composer.",
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

fn note_list_meta_summary(note: &notes::Note) -> String {
  let mut parts = Vec::new();
  if !note.tags.is_empty() {
    let noun = if note.tags.len() == 1 { "tag" } else { "tags" };
    parts.push(format!("{} {noun}", note.tags.len()));
  }
  let noun = if note.linked_papers.len() == 1 { "paper" } else { "papers" };
  parts.push(format!("{} {noun}", note.linked_papers.len()));
  parts.push(note.updated_at.format("%Y-%m-%d").to_string());
  parts.join("  ·  ")
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
    draw_notes_empty_state(frame, area, NotesMode::Library, t);
    return;
  }

  let slots = (area.height as usize / 2).max(1);
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
    let row_area = Rect {
      x: area.x,
      y,
      width: area.width,
      height: 2.min(area.y + area.height - y),
    };
    let is_selected = note_index == selected;
    let title = truncate(&note.title, area.width.saturating_sub(3) as usize);
    let meta = truncate(
      &note_list_meta_summary(note),
      area.width.saturating_sub(3) as usize,
    );
    let lines = vec![
      Line::from(vec![
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
      ]),
      Line::from(vec![
        Span::styled(
          "  ",
          if is_selected { selection_style } else { Style::default() },
        ),
        Span::styled(
          meta,
          if is_selected {
            selection_style.remove_modifier(Modifier::BOLD)
          } else {
            Style::default().fg(t.text_dim)
          },
        ),
      ]),
    ];
    frame.render_widget(
      Paragraph::new(lines)
        .style(if is_selected { selection_style } else { Style::default() })
        .wrap(Wrap { trim: false }),
      row_area,
    );
    y = y.saturating_add(2);
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
  let inner = Rect {
    x: area.x.saturating_add(1),
    y: area.y,
    width: area.width.saturating_sub(1),
    height: area.height,
  };
  let Some(note) = note else {
    frame.render_widget(
      Paragraph::new("No note selected")
        .style(Style::default().fg(t.text_dim))
        .alignment(Alignment::Center),
      inner,
    );
    return;
  };
  let mut lines = vec![
    Line::from(""),
    Line::from(Span::styled(
      truncate(&note.title, inner.width as usize),
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    )),
  ];
  lines.push(Line::from(Span::styled(
    truncate(&note_preview_meta_summary(note), inner.width as usize),
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
  frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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
  // Inline the field access so Rust's split-borrow rules can keep the
  // `tabs` borrow disjoint from the later `app.notes_app.as_mut()` —
  // saves a per-draw `Vec<NotesTab>` clone.
  let (tabs, active) = match side {
    FocusedReader::Primary => (&app.notes_tabs, app.notes_active_tab),
    FocusedReader::Secondary => {
      (&app.secondary_notes_tabs, app.secondary_notes_active_tab)
    }
  };
  let show_tabs = tabs.len() > 1;
  let rows = Layout::vertical(if show_tabs {
    vec![
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Min(0),
    ]
  } else {
    vec![
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Min(0),
    ]
  })
  .split(area);
  draw_note_dock_rule(
    frame,
    rows[0],
    app.notes_mode_for_side(side).title(),
    is_focused,
    theme,
  );
  draw_notes_mode_switcher(
    frame,
    rows[1],
    app.notes_mode_for_side(side),
    is_focused,
    theme,
  );
  frame.render_widget(
    Paragraph::new(build_notes_summary_line(app, side, rows[2].width, theme))
      .wrap(Wrap { trim: false }),
    rows[2],
  );
  let content_row = if show_tabs {
    draw_notes_tab_bar(frame, rows[3], &tabs, active, is_focused, theme);
    4
  } else {
    3
  };
  let content_area = rows[content_row];

  let editor_active = app.notes_app.as_ref().is_some_and(|notes_app| {
    notes_app.notes_state == notes::app::NotesState::Editor
  });
  let popup_active = app
    .notes_app
    .as_ref()
    .is_some_and(|notes_app| !notes_app.active_popup.is_none());

  if editor_active {
    if let Some(notes_app) = app.notes_app.as_mut() {
      notes_app.draw_editor_surface(frame, content_area);
      if popup_active {
        notes_app.draw_popup_overlay(frame, content_area);
      }
    }
    return;
  }

  if preview_when_unfocused && !is_focused {
    draw_note_preview(frame, app, content_area, &tabs, active, theme);
    if popup_active {
      if let Some(notes_app) = app.notes_app.as_mut() {
        notes_app.draw_popup_overlay(frame, content_area);
      }
    }
    return;
  }

  match app.notes_mode_for_side(side) {
    NotesMode::Capture => {
      draw_notes_empty_state(frame, content_area, NotesMode::Capture, theme);
    }
    NotesMode::PaperNotes | NotesMode::Library => {
      let visible = notes_browser_visible(app, side);
      if visible.is_empty() {
        draw_notes_empty_state(
          frame,
          content_area,
          app.notes_mode_for_side(side),
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
          chunks[0],
          &visible,
          notes_browser_selected_index(app, &visible),
          is_focused,
          theme,
        );
        frame.render_widget(
          Paragraph::new("│").style(Style::default().fg(theme.border)),
          chunks[1],
        );
        draw_notes_browser_preview(
          frame,
          chunks[2],
          notes_browser_selected_note(app, &visible),
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
          chunks[0],
          &visible,
          notes_browser_selected_index(app, &visible),
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
          chunks[2],
          notes_browser_selected_note(app, &visible),
          theme,
        );
      }
    }
  }

  if popup_active {
    if let Some(notes_app) = app.notes_app.as_mut() {
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

pub(super) fn draw_note_dock_rule(
  frame: &mut Frame,
  area: Rect,
  title: &str,
  focused: bool,
  t: &crate::theme::Theme,
) {
  let w = area.width as usize;
  let style = if focused {
    Style::default().fg(t.border_active)
  } else {
    Style::default().fg(t.border)
  };
  let title_style = if focused {
    Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
  } else {
    Style::default().fg(t.header).add_modifier(Modifier::BOLD)
  };
  let left = "── ";
  let right = " ";
  let fill =
    "─".repeat(w.saturating_sub(left.len() + title.len() + right.len()));
  let line = Line::from(vec![
    Span::styled(left, style),
    Span::styled(title.to_string(), title_style),
    Span::styled(format!("{right}{fill}"), style),
  ]);
  frame.render_widget(Paragraph::new(line), area);
}

fn draw_note_preview(
  frame: &mut Frame,
  app: &App,
  area: Rect,
  tabs: &[NotesTab],
  active: usize,
  t: &crate::theme::Theme,
) {
  let selected = app
    .notes_app
    .as_ref()
    .and_then(|notes_app| notes_app.get_current_note())
    .or_else(|| {
      tabs.get(active).and_then(|tab| {
        app.notes_app.as_ref().and_then(|na| na.get_note(&tab.note_id))
      })
    });
  draw_notes_browser_preview(frame, area, selected, t);
}

fn draw_notes_tab_bar(
  frame: &mut Frame,
  area: Rect,
  tabs: &[NotesTab],
  active: usize,
  focused: bool,
  t: &crate::theme::Theme,
) {
  if tabs.is_empty() {
    return;
  }
  let max_title =
    (area.width as usize).saturating_sub(4).max(8) / tabs.len().max(1);
  let spans: Vec<Span> = tabs
    .iter()
    .enumerate()
    .flat_map(|(i, tab)| {
      let title: String =
        tab.title.chars().take(max_title.saturating_sub(5)).collect();
      let label = format!("[{}: {}]", i + 1, title);
      let style = if i == active {
        if focused {
          Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
          Style::default().fg(t.text).add_modifier(Modifier::BOLD)
        }
      } else {
        Style::default().fg(t.text_dim)
      };
      vec![Span::styled(label, style), Span::raw("  ")]
    })
    .collect();
  frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
