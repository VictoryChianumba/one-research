use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Direction, Layout},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Clear, Paragraph, Wrap},
};

use super::super::widgets::{
  draw_card_footer, popup_inner, settings_card_block, settings_modal_rect,
  truncate,
};
use crate::app::App;

const HELP_SECTIONS: &[(&str, &[(&str, &str)])] = &[
  (
    "Navigation",
    &[
      ("j / k", "Move down / up"),
      ("g / G", "Jump to top / bottom"),
      ("Tab / Shift+Tab", "Cycle tabs forward / backward"),
      ("Enter", "Open paper in reader"),
      ("Space", "Show abstract/details"),
      ("/", "Search — fuzzy + relevance-ranked (see Search)"),
      ("f", "Open filter panel"),
      ("?", "Open help"),
      ("q", "Quit (context-aware confirm)"),
      ("Esc", "Clear/back/cancel"),
      ("Mouse", "Click to focus interactive pane"),
    ],
  ),
  (
    "Search",
    &[
      ("Ranking", "Best match first; title > author > abstract"),
      ("Fuzzy", "Typo-tolerant — subsequence match, not exact"),
      ("ti: / abs:", "Restrict a term to title / abstract"),
      ("au: / author:", "Restrict a term to authors"),
      ("cat: / category:", "arXiv subject — cs.LG (exact) · cs (all cs.*)"),
      ("year:", "2024 · 2020-2024 · >2020 · >=2020 · <2024"),
      ("Quotes", "author:\"Yann LeCun\" groups a value with spaces"),
      ("Multiple terms", "All must match (AND); plain words match anywhere"),
    ],
  ),
  (
    "Leader",
    &[
      ("Ldr = Ctrl+T", ""),
      ("? / Ldr+?", "This help screen"),
      ("Ldr+q", "Quit application"),
      ("Ldr+s", "Open settings"),
      ("Reader", "Ldr+Enter popup · Ldr+f reader feed/drawer · Ldr+Esc back"),
      ("Ldr+n", "Open notes from current context"),
      ("Ldr+c", "Toggle chat panel"),
      ("Ldr+z", "Move chat top / bottom"),
      ("Pane focus", "Ldr+h/j/k/l move by direction"),
      ("Ldr+1 / 2 / 3", "Focus interactive pane by number"),
      ("Tabs", "Ldr+[ prev · Ldr+] next · Ldr+w close"),
    ],
  ),
  (
    "Inbox",
    &[
      ("Scope", "Only items with state == Inbox"),
      ("R", "Refresh all sources"),
      ("o", "Open URL in browser"),
      ("v", "Open repo viewer"),
      ("Workflow", "i inbox · r read · w queue · x archive"),
      ("Filter panel", "f open · Space toggle · c clear · Esc close"),
    ],
  ),
  (
    "Library",
    &[
      ("Scope", "Items where state ≠ Inbox"),
      ("[ / ]", "Cycle workflow filter (All/Queue/Read/Archived)"),
      ("Filter panel", "f opens the Library workflow filter"),
      ("V", "Enter visual selection mode (Shift+v)"),
      ("t", "Open tag picker"),
      ("Workflow keys", "i / r / w / x apply to current row"),
      ("Visual mode", "j/k extend · r/w/x/i bulk apply · t bulk tag · Esc"),
    ],
  ),
  (
    "Discoveries",
    &[
      ("Search bar", "Any printable char focuses · Enter runs"),
      ("/", "Open slash command palette"),
      ("Ctrl+N", "Force new discovery (clear session)"),
      ("Palette", "↑↓ choose · Tab complete · Enter run · Esc cancel"),
      ("Slash cmds", "/discover · /sota · /reading-list · /code"),
      ("", "/compare · /digest · /author · /trending · /watch"),
    ],
  ),
  (
    "History",
    &[
      ("Scope", "Paper opens + discovery queries"),
      ("[ / ]", "Cycle time filter (All/Today/24h/48h/Week/Month)"),
      ("Filter panel", "f opens the History time window filter"),
      ("/", "Search by title (filters within current window)"),
      ("Enter", "Reopen paper · re-run query"),
      ("Ctrl+D", "Delete selected entry"),
      ("/clear history", "Wipe entire history"),
    ],
  ),
  (
    "Tags",
    &[
      ("t (Library)", "Open tag picker for current item"),
      ("t (visual mode)", "Open tag picker for all selected"),
      ("In picker: type", "Add new tag name"),
      ("In picker: ↑↓", "Navigate existing tags"),
      ("In picker: Space", "Toggle highlighted tag"),
      ("In picker: Enter", "Add new tag (or toggle if input empty)"),
      ("In picker: Esc", "Close"),
      ("Filter panel", "Toggle tags via Tags section"),
    ],
  ),
  (
    "Reader",
    &[
      ("vim keys", "Standard vim navigation"),
      ("Tab", "Switch primary / secondary pane"),
      ("Ldr+f", "Cycle reader feed / drawer layout"),
      ("Ldr+n", "Open notes for current paper"),
      ("i", "Toggle figure-preview side pane (60/40 split)"),
      ("]f / [f", "Step next / previous figure in preview"),
      ("]] / [[", "Jump next / previous section header"),
      ("q / Esc", "Close / step back reader state"),
      ("Feed drawer", "j/k move · d details · / search · Enter open"),
      ("", ""),
      ("Tabs", ""),
      ("Ldr+t", "Open in new tab (prompt if dual)"),
      ("Ldr+[", "Previous tab"),
      ("Ldr+]", "Next tab"),
      ("Ldr+w", "Close current tab"),
      ("Voice", "r read · R read from cursor · Ctrl+p continuous"),
      ("Playback", "Space pause/resume · c re-centre · Esc stop"),
    ],
  ),
  (
    "Repo Viewer",
    &[
      ("j / k", "Move in tree or scroll preview"),
      ("Enter", "Open file or folder"),
      ("b / Backspace", "Go to parent directory"),
      ("Tab / Shift+Tab", "Switch tree / preview focus"),
      ("h / l", "Pan preview left / right"),
      ("+ / -", "Adjust markdown wrap width"),
      ("o", "Open current GitHub URL in browser"),
      ("y / u", "Copy path / GitHub URL"),
      ("d", "Download current file"),
      ("Esc", "Back to tree pane, then close viewer"),
      ("q", "Close repo viewer"),
    ],
  ),
  (
    "Notes",
    &[
      ("Ldr+n", "Open notes from current context"),
      ("[ / ]", "Cycle mode: Paper Notes / Library / Capture"),
      ("j / k", "Move note selection"),
      ("g / G", "Jump to first / last note"),
      ("PageUp / PageDown", "Move by page"),
      ("n / Enter", "In Capture, open the prefilled linked-note composer"),
      ("Enter", "In Library/Paper Notes, edit the selected note"),
      ("a / x", "Attach / detach current paper on selected note"),
      ("Ldr+w", "Close the notes pane"),
      ("Esc", "Back out of editor/popups, then close notes pane"),
    ],
  ),
  (
    "Chat",
    &[
      ("Enter", "Send message"),
      ("j / k", "Scroll (normal mode)"),
      ("i / a / Enter", "Insert mode (normal mode)"),
      ("Esc", "Normal mode / back to session list"),
      ("/", "Open slash command palette"),
      ("Tab", "Complete slash command"),
      ("Up / Down", "Navigate slash commands"),
      ("Ctrl+n / Ctrl+p", "Next / previous slash command"),
      ("/clear", "Clear chat · /clear discoveries · /clear history"),
      ("/export-history", "[md|jsonl]"),
      ("/export-library", "[md|jsonl] (respects filters)"),
      ("/add", "/add CATEGORY · /add-feed URL"),
      ("Session list", "n new · d delete · Enter open"),
      ("Ldr+c", "Close chat panel"),
      ("Ldr+z", "Move chat top / bottom"),
    ],
  ),
  (
    "Settings",
    &[
      ("Ldr+s", "Open settings"),
      ("j / k", "Navigate fields"),
      ("Enter", "Edit field or cycle option"),
      ("s / S", "Save all fields"),
      ("p", "Manage sources"),
      ("q / Esc", "Close settings"),
      ("Sources", "Space toggle · Enter or / add URL · d delete"),
      ("Theme picker", "j/k preview · Enter select/create · e edit"),
      ("Theme editor", "Space apply · x hex · n rename · s save"),
    ],
  ),
  (
    "Repo Viewer",
    &[
      ("j / k", "Navigate file tree"),
      ("Enter", "Open file or folder"),
      ("b / Backspace", "Go back"),
      ("Tab", "Switch tree / content pane"),
      ("h / l", "Scroll content left / right"),
      ("+/= / -", "Zoom in / out"),
      ("y", "Copy file path"),
      ("d", "Download file"),
      ("q", "Close viewer"),
    ],
  ),
];

pub const HELP_SECTION_COUNT: usize = HELP_SECTIONS.len();

pub fn draw_help_overlay(frame: &mut Frame, app: &mut App) {
  let t = app.theme();
  let area = frame.area();

  let (section_name, bindings) =
    HELP_SECTIONS[app.help.section.min(HELP_SECTIONS.len() - 1)];
  let popup_rect = settings_modal_rect(area);

  frame.render_widget(Clear, popup_rect);

  let block = settings_card_block(" Help ", &t);
  let block_inner = block.inner(popup_rect);
  let inner = popup_inner(block_inner, 1, 0);
  frame.render_widget(block, popup_rect);

  if inner.width < 44 || inner.height < 14 {
    frame.render_widget(
      Paragraph::new(Span::styled(
        " terminal too small for help ",
        Style::default().fg(t.text_dim).bg(t.bg_panel),
      ))
      .alignment(Alignment::Center),
      inner,
    );
    return;
  }

  let body_footer = Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Min(0), Constraint::Length(2)])
    .split(inner);
  let body = body_footer[0];
  let footer_area = body_footer[1];

  let columns = if body.width >= 86 {
    Layout::default()
      .direction(Direction::Horizontal)
      .constraints([Constraint::Length(25), Constraint::Min(0)])
      .split(body)
  } else {
    Layout::default()
      .direction(Direction::Horizontal)
      .constraints([Constraint::Length(0), Constraint::Min(0)])
      .split(body)
  };

  let key_col_w = bindings
    .iter()
    .filter(|(_, desc)| !desc.is_empty())
    .map(|(key, _)| key.chars().count())
    .max()
    .unwrap_or(10)
    .clamp(10, 16);

  let key_style = Style::default().fg(t.accent);
  let header_style = Style::default().fg(t.header).add_modifier(Modifier::BOLD);
  let desc_style = Style::default().fg(t.text);
  let dim_style = Style::default().fg(t.text_dim);
  let bg_style = Style::default().bg(t.bg_panel);

  if columns[0].width > 0 {
    let rail = columns[0];
    let rail_rule = "─".repeat(rail.width.saturating_sub(4) as usize);
    let mut rail_lines = vec![
      Line::from(""),
      Line::from(Span::styled("  Sections", header_style)),
      Line::from(Span::styled(format!("  {rail_rule}"), dim_style)),
      Line::from(""),
    ];

    for (i, (name, bindings)) in HELP_SECTIONS.iter().enumerate() {
      let selected = i == app.help.section;
      let marker = if selected { ">" } else { " " };
      let style = if selected {
        t.style_selection_text()
      } else {
        Style::default().fg(t.text)
      };
      let count = bindings.iter().filter(|(_, desc)| !desc.is_empty()).count();
      let label_width = rail.width.saturating_sub(10) as usize;
      rail_lines.push(Line::from(vec![
        Span::styled(format!(" {marker} "), style),
        Span::styled(
          format!("{:<label_width$}", truncate(name, label_width)),
          style,
        ),
        Span::styled(format!(" {count:>2}"), dim_style),
      ]));
    }

    frame.render_widget(
      Paragraph::new(rail_lines).wrap(Wrap { trim: false }).style(bg_style),
      rail,
    );
  }

  let content_area = columns[1];
  let content_rows = Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Length(3), Constraint::Min(0)])
    .split(content_area);
  let title_area = content_rows[0];
  let body_area = content_rows[1];

  let title_rule = "─".repeat(title_area.width.saturating_sub(2) as usize);
  let mut title_lines = vec![
    Line::from(vec![
      Span::styled("  ", dim_style),
      Span::styled(section_name, header_style),
      Span::styled(
        format!("  {}/{}", app.help.section + 1, HELP_SECTIONS.len()),
        dim_style,
      ),
    ]),
    Line::from(Span::styled(format!("  {title_rule}"), dim_style)),
  ];
  if columns[0].width == 0 {
    title_lines.push(Line::from(Span::styled(
      "  h/l or Tab changes section",
      dim_style,
    )));
  } else {
    title_lines.push(Line::from(""));
  }
  frame.render_widget(Paragraph::new(title_lines).style(bg_style), title_area);

  let mut body_lines: Vec<Line> = Vec::new();
  for (key, desc) in bindings.iter() {
    if key.is_empty() && desc.is_empty() {
      // blank spacer row
      body_lines.push(Line::from(""));
      continue;
    }
    if !key.is_empty() && desc.is_empty() {
      // section subheading (key text, no description)
      body_lines.push(Line::from(vec![
        Span::styled("  ", dim_style),
        Span::styled(*key, header_style),
      ]));
      continue;
    }
    let key_cell = format!("{:<width$}  ", key, width = key_col_w);
    body_lines.push(Line::from(vec![
      Span::styled("  ", dim_style),
      Span::styled(key_cell, key_style),
      Span::styled(*desc, desc_style),
    ]));
  }

  let total_lines = body_lines.len();
  let max_scroll = total_lines.saturating_sub(body_area.height as usize);
  // SEAM-EXEMPT: help popup's scroll bound is sized against `body_area`
  // — a Rect local to this render fn that no other code path consumes.
  // Lifting into FrameLayout would mean adding a field + helper for
  // one in-popup mutation that has no cross-pane analog (ADR-008 §S2).
  app.help.scroll.set_viewport(body_area.height as usize);
  app.help.scroll.set_max(max_scroll);
  let scroll = app.help.scroll.offset() as u16;

  frame.render_widget(
    Paragraph::new(body_lines)
      .scroll((scroll, 0))
      .style(Style::default().bg(t.bg_panel)),
    body_area,
  );
  draw_card_footer(
    frame,
    footer_area,
    &t,
    "  h/l or Tab section · j/k scroll · q/Esc close",
  );
}
