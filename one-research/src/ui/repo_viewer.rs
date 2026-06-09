use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Direction, Layout, Rect},
  style::{Color, Modifier, Style},
  text::{Line, Span},
  widgets::{Block, Borders, Paragraph},
};

use crate::app::{App, RepoFileKind, RepoPane};
use crate::github::NodeType;
use crate::ui::repo_markdown;
use ui_theme::Theme;

pub fn draw_repo_viewer(frame: &mut Frame, app: &mut App) {
  let area = frame.area();
  let rows = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Length(1),
      Constraint::Min(0),
      Constraint::Length(1),
    ])
    .split(area);

  let t = app.theme();
  draw_workspace_rule(frame, rows[0], "Repository", &t);
  draw_summary_row(frame, app, rows[1], &t);
  draw_context_row(frame, app, rows[2], &t);
  draw_main(frame, app, rows[3], &t);
  draw_help(frame, app, rows[4], &t);
}

fn draw_workspace_rule(frame: &mut Frame, area: Rect, title: &str, t: &Theme) {
  let width = area.width as usize;
  let style = t.style_border();
  let title_style = t.style_header().add_modifier(Modifier::BOLD);
  let left = "── ";
  let right = " ";
  let fill =
    "─".repeat(width.saturating_sub(left.len() + title.len() + right.len()));
  let line = Line::from(vec![
    Span::styled(left, style),
    Span::styled(title.to_string(), title_style),
    Span::styled(format!("{right}{fill}"), style),
  ]);
  frame.render_widget(Paragraph::new(line), area);
}

fn draw_summary_row(frame: &mut Frame, app: &App, area: Rect, t: &Theme) {
  let Some(ctx) = app.repo_context.as_ref() else {
    return;
  };

  let repo = format!("github.com/{}/{}", ctx.owner, ctx.repo_name);
  let branch = if ctx.default_branch.is_empty() {
    "unknown".to_string()
  } else {
    ctx.default_branch.clone()
  };
  let status = app.repo_status_label().unwrap_or_else(|| "ready".to_string());

  let line = Line::from(vec![
    Span::styled("  ", t.style_dim()),
    Span::styled(repo, t.style_accent().add_modifier(Modifier::BOLD)),
    Span::styled("  ·  ", t.style_dim()),
    Span::styled(format!("branch {branch}"), t.style_dim()),
    Span::styled("  ·  ", t.style_dim()),
    Span::styled(status.clone(), repo_status_style(&status, t)),
  ]);
  frame.render_widget(Paragraph::new(line), area);
}

fn draw_context_row(frame: &mut Frame, app: &App, area: Rect, t: &Theme) {
  let Some(ctx) = app.repo_context.as_ref() else {
    return;
  };

  let line = Line::from(Span::styled(
    format!("  {}", repo_context_description(ctx)),
    t.style_dim(),
  ));
  frame.render_widget(Paragraph::new(line), area);
}

fn draw_main(frame: &mut Frame, app: &mut App, area: Rect, t: &Theme) {
  let ctx = match app.repo_context.as_mut() {
    Some(c) => c,
    None => return,
  };

  if ctx.no_token {
    draw_repo_shell_box(frame, area, "Access", false, t);
    let inner = inset(area);
    render_center_state(
      frame,
      inner,
      "GitHub token required",
      &[
        "Set github_token in ~/.config/one-research/config.json",
        "o opens the repository in the browser",
      ],
      t,
    );
    return;
  }

  let tree_w = (area.width / 3).max(26).min(52);
  let tree_title = if ctx.tree_path.is_empty() {
    "Tree · /".to_string()
  } else {
    format!("Tree · /{}", ctx.tree_path)
  };
  let preview_title = if let Some(name) = ctx.file_name.as_deref() {
    format!("Preview · {} · {}", name, repo_kind_label(ctx.file_kind))
  } else {
    "Preview".to_string()
  };

  let (tree_rect, file_rect) = draw_repo_split_box(
    frame,
    area,
    tree_w,
    &tree_title,
    ctx.pane_focus == RepoPane::Tree,
    &preview_title,
    ctx.pane_focus == RepoPane::File,
    t,
  );

  draw_tree_pane(frame, ctx, tree_rect, t);
  draw_file_pane(frame, ctx, file_rect, t);
}

fn draw_tree_pane(
  frame: &mut Frame,
  ctx: &crate::app::RepoContext,
  area: Rect,
  t: &Theme,
) {
  if area.width == 0 || area.height == 0 {
    return;
  }

  if ctx.tree_nodes.is_empty() {
    let status = ctx.status_message.as_deref().unwrap_or("Repository is empty");
    let title = if status.starts_with("Error:") {
      "Could not load repository tree"
    } else if status.contains("Loading") {
      "Loading repository tree"
    } else {
      "No tree entries"
    };
    render_center_state(frame, area, title, &[status], t);
    return;
  }

  let visible_h = area.height as usize;
  let scroll = if ctx.tree_cursor >= visible_h.saturating_sub(2) {
    ctx.tree_cursor + 2 - visible_h
  } else {
    0
  };
  let max_name = area.width.saturating_sub(4) as usize;

  let mut y = area.y;
  for (i, node) in
    ctx.tree_nodes.iter().enumerate().skip(scroll).take(visible_h)
  {
    let is_selected = i == ctx.tree_cursor;
    let (icon, icon_style, name_style) = match node.node_type {
      NodeType::Dir => (
        "▸",
        if is_selected {
          t.style_selection_text()
        } else {
          Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        },
        if is_selected {
          t.style_selection_text().add_modifier(Modifier::BOLD)
        } else {
          Style::default().fg(t.header).add_modifier(Modifier::BOLD)
        },
      ),
      NodeType::File => (
        "·",
        if is_selected {
          t.style_selection_text()
        } else {
          Style::default().fg(t.text_dim)
        },
        if is_selected {
          t.style_selection_text()
        } else {
          Style::default().fg(t.text)
        },
      ),
    };
    let name = truncate(&node.name, max_name.saturating_sub(1));
    let line = Line::from(vec![
      Span::styled(
        if is_selected { "› " } else { "  " },
        if is_selected { t.style_selection_text() } else { t.style_dim() },
      ),
      Span::styled(format!("{icon} "), icon_style),
      Span::styled(name, name_style),
    ]);
    let row_rect = Rect::new(area.x, y, area.width, 1);
    if is_selected {
      frame.render_widget(
        Paragraph::new(line).style(t.style_selection()),
        row_rect,
      );
    } else {
      frame.render_widget(Paragraph::new(line), row_rect);
    }
    y += 1;
    if y >= area.y + area.height {
      break;
    }
  }
}

fn draw_file_pane(
  frame: &mut Frame,
  ctx: &mut crate::app::RepoContext,
  area: Rect,
  t: &Theme,
) {
  if area.width == 0 || area.height == 0 {
    return;
  }

  if ctx.file_lines.is_empty() {
    if let Some(status) = ctx.status_message.as_deref() {
      if status.starts_with("Error:") || status.contains("Loading") {
        let title = if status.starts_with("Error:") {
          "Could not load file"
        } else {
          "Loading file"
        };
        render_center_state(frame, area, title, &[status], t);
        return;
      }
    }

    render_center_state(
      frame,
      area,
      "No file open",
      &["Press Enter to open the selected file."],
      t,
    );
    return;
  }

  let visible_h = area.height as usize;
  let pane_w = area.width as usize;
  let render_w =
    if ctx.wrap_width > 0 { ctx.wrap_width.min(pane_w) } else { pane_w };
  let h_off = ctx.h_offset;
  let show_pan_indicator = h_off > 0;

  if ctx.file_kind == RepoFileKind::Markdown {
    prepare_markdown_cache(ctx, render_w);
  }

  if ctx.file_kind == RepoFileKind::Markdown {
    let cache = ctx
      .markdown_cache
      .as_ref()
      .expect("markdown cache should be prepared before drawing");
    let lines: Vec<Line> = cache
      .lines
      .iter()
      .skip(ctx.file_scroll)
      .take(visible_h)
      .map(|line| repo_markdown::line_to_ratatui(line, h_off, render_w))
      .collect();
    frame.render_widget(Paragraph::new(lines), area);
  } else if !ctx.file_highlighted.is_empty() {
    let total_lines = ctx.file_lines.len();
    let line_num_w = format!("{total_lines}").len();

    let lines: Vec<Line> = ctx
      .file_highlighted
      .iter()
      .enumerate()
      .skip(ctx.file_scroll)
      .take(visible_h)
      .map(|(i, spans)| {
        let mut line_spans =
          vec![Span::styled(format!("{:>line_num_w$} ", i + 1), t.style_dim())];
        let content: String =
          spans.iter().map(|(_, _, _, text)| text.as_str()).collect();
        let content_sliced = apply_h_offset(&content, h_off, render_w);
        let mut col_offset = 0usize;
        for (r, g, b, text) in spans {
          let start = col_offset;
          let end = col_offset + text.chars().count();
          col_offset = end;
          let sliced = slice_char_range(
            &content_sliced,
            &content,
            start,
            end,
            h_off,
            render_w,
          );
          if !sliced.is_empty() {
            line_spans.push(Span::styled(
              sliced,
              Style::default().fg(Color::Rgb(*r, *g, *b)),
            ));
          }
        }
        Line::from(line_spans)
      })
      .collect();

    frame.render_widget(Paragraph::new(lines), area);
  } else {
    let total_lines = ctx.file_lines.len();
    let line_num_w = format!("{total_lines}").len();

    let lines: Vec<Line> = ctx
      .file_lines
      .iter()
      .enumerate()
      .skip(ctx.file_scroll)
      .take(visible_h)
      .map(|(i, line)| {
        let sliced = apply_h_offset(line, h_off, render_w);
        Line::from(vec![
          Span::styled(format!("{:>line_num_w$} ", i + 1), t.style_dim()),
          Span::raw(sliced),
        ])
      })
      .collect();

    frame.render_widget(Paragraph::new(lines), area);
  }

  if show_pan_indicator
    && area.width >= 4
    && (ctx.file_kind != RepoFileKind::Markdown
      || ctx.markdown_has_pannable_lines)
  {
    let indicator = format!("◀+{h_off}");
    let x = area.x + area.width.saturating_sub(indicator.len() as u16 + 1);
    let indicator_area =
      Rect { x, y: area.y, width: indicator.len() as u16 + 1, height: 1 };
    let p =
      Paragraph::new(Span::styled(indicator, Style::default().fg(t.warning)));
    frame.render_widget(p, indicator_area);
  }
}

fn draw_repo_split_box(
  frame: &mut Frame,
  area: Rect,
  tree_w: u16,
  tree_title: &str,
  tree_focused: bool,
  file_title: &str,
  file_focused: bool,
  t: &Theme,
) -> (Rect, Rect) {
  draw_repo_shell_box(frame, area, "", false, t);

  let inner = inset(area);
  let tree_w = tree_w.min(inner.width.saturating_sub(2));
  let file_w = inner.width.saturating_sub(tree_w + 1);
  let div_x = inner.x + tree_w;

  if inner.height > 0 {
    let divider: Vec<Line> = (0..inner.height)
      .map(|_| Line::from(Span::styled("│", t.style_border())))
      .collect();
    frame.render_widget(
      Paragraph::new(divider),
      Rect { x: div_x, y: inner.y, width: 1, height: inner.height },
    );
  }

  draw_pane_rule(
    frame,
    Rect::new(area.x + 1, area.y, tree_w, 1),
    tree_title,
    tree_focused,
    t,
  );
  draw_pane_rule(
    frame,
    Rect::new(div_x + 1, area.y, file_w, 1),
    file_title,
    file_focused,
    t,
  );

  let tree_rect =
    Rect { x: inner.x, y: inner.y, width: tree_w, height: inner.height };
  let file_rect =
    Rect { x: div_x + 1, y: inner.y, width: file_w, height: inner.height };

  if tree_focused {
    draw_active_pane_outline(frame, area, tree_rect, t);
  }
  if file_focused {
    draw_active_pane_outline(frame, area, file_rect, t);
  }

  (tree_rect, file_rect)
}

fn draw_repo_shell_box(
  frame: &mut Frame,
  area: Rect,
  title: &str,
  focused: bool,
  t: &Theme,
) {
  let border_style =
    if focused { t.style_border_active() } else { t.style_border() };

  let block = if title.is_empty() {
    Block::default().borders(Borders::ALL).border_style(border_style)
  } else {
    Block::default()
      .borders(Borders::ALL)
      .border_style(border_style)
      .title(title)
  };
  frame.render_widget(block, area);
}

fn inset(area: Rect) -> Rect {
  Rect {
    x: area.x.saturating_add(1),
    y: area.y.saturating_add(1),
    width: area.width.saturating_sub(2),
    height: area.height.saturating_sub(2),
  }
}

fn draw_pane_rule(
  frame: &mut Frame,
  area: Rect,
  title: &str,
  focused: bool,
  t: &Theme,
) {
  if area.width == 0 {
    return;
  }
  let style = if focused { t.style_border_active() } else { t.style_border() };
  let title_style = if focused {
    t.style_accent().add_modifier(Modifier::BOLD)
  } else {
    t.style_header().add_modifier(Modifier::BOLD)
  };
  let width = area.width as usize;
  let label = truncate(title, width.saturating_sub(4));
  let left = " ";
  let right = " ";
  let fill =
    "─".repeat(width.saturating_sub(left.len() + label.len() + right.len()));
  let line = Line::from(vec![
    Span::styled(left, style),
    Span::styled(label, title_style),
    Span::styled(format!("{right}{fill}"), style),
  ]);
  frame.render_widget(Paragraph::new(line), area);
}

fn draw_active_pane_outline(
  frame: &mut Frame,
  outer_area: Rect,
  pane_rect: Rect,
  t: &Theme,
) {
  if pane_rect.width == 0 || pane_rect.height == 0 {
    return;
  }

  let outline = Rect {
    x: pane_rect.x.saturating_sub(1),
    y: pane_rect.y.saturating_sub(1),
    width: pane_rect
      .width
      .saturating_add(2)
      .min(outer_area.x + outer_area.width - pane_rect.x.saturating_sub(1)),
    height: pane_rect
      .height
      .saturating_add(2)
      .min(outer_area.y + outer_area.height - pane_rect.y.saturating_sub(1)),
  };

  frame.render_widget(
    Block::default()
      .borders(Borders::ALL)
      .border_style(t.style_border_active()),
    outline,
  );
}

fn render_center_state(
  frame: &mut Frame,
  area: Rect,
  title: &str,
  lines: &[&str],
  t: &Theme,
) {
  let mut rendered = Vec::with_capacity(lines.len() + 2);
  rendered.push(Line::from(""));
  rendered.push(Line::from(Span::styled(
    title.to_string(),
    Style::default().fg(t.text).add_modifier(Modifier::BOLD),
  )));
  rendered.push(Line::from(""));
  for line in lines {
    rendered.push(Line::from(Span::styled(
      (*line).to_string(),
      Style::default().fg(t.text_dim),
    )));
  }
  frame
    .render_widget(Paragraph::new(rendered).alignment(Alignment::Center), area);
}

fn repo_context_path(ctx: &crate::app::RepoContext) -> String {
  match ctx.pane_focus {
    RepoPane::File => ctx
      .file_path
      .clone()
      .or_else(|| ctx.file_name.clone())
      .unwrap_or_else(|| "/".to_string()),
    RepoPane::Tree => ctx
      .tree_nodes
      .get(ctx.tree_cursor)
      .map(|node| node.path.clone())
      .or_else(|| {
        if ctx.tree_path.is_empty() {
          None
        } else {
          Some(ctx.tree_path.clone())
        }
      })
      .unwrap_or_else(|| "/".to_string()),
  }
}

fn repo_context_description(ctx: &crate::app::RepoContext) -> String {
  match ctx.pane_focus {
    RepoPane::Tree => {
      let selected = ctx
        .tree_nodes
        .get(ctx.tree_cursor)
        .map(|node| node.path.clone())
        .unwrap_or_else(|| repo_context_path(ctx));
      "Selected path ".to_string() + &selected + " · tree view"
    }
    RepoPane::File => {
      let selected = ctx
        .file_path
        .clone()
        .or_else(|| ctx.file_name.clone())
        .unwrap_or_else(|| repo_context_path(ctx));
      format!(
        "Selected path {} · {} preview",
        selected,
        repo_kind_label(ctx.file_kind)
      )
    }
  }
}

fn repo_kind_label(kind: RepoFileKind) -> &'static str {
  match kind {
    RepoFileKind::Markdown => "markdown",
    RepoFileKind::Code => "code",
    RepoFileKind::PlainText => "text",
  }
}

fn repo_status_style(status: &str, t: &Theme) -> Style {
  if status.starts_with("Error:") || status.contains("rejected") {
    Style::default().fg(t.warning).add_modifier(Modifier::BOLD)
  } else if status.contains("Loading") {
    Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
  } else if status == "ready" {
    t.style_dim()
  } else {
    Style::default().fg(t.text_dim)
  }
}

fn prepare_markdown_cache(ctx: &mut crate::app::RepoContext, render_w: usize) {
  let needs_refresh = ctx
    .markdown_cache
    .as_ref()
    .is_none_or(|cache| cache.wrap_width != render_w);

  if needs_refresh {
    let cache = repo_markdown::render_markdown(&ctx.raw_file_content, render_w);
    ctx.rendered_line_count = cache.lines.len();
    ctx.markdown_has_pannable_lines = cache.has_pannable_lines;
    if !ctx.markdown_has_pannable_lines {
      ctx.h_offset = 0;
    }
    ctx.markdown_cache = Some(cache);
  } else if let Some(cache) = &ctx.markdown_cache {
    ctx.rendered_line_count = cache.lines.len();
    ctx.markdown_has_pannable_lines = cache.has_pannable_lines;
  }

  let max_scroll = ctx.rendered_line_count.saturating_sub(1);
  if ctx.file_scroll > max_scroll {
    ctx.file_scroll = max_scroll;
  }
}

fn apply_h_offset(s: &str, h_off: usize, max_w: usize) -> String {
  if max_w == 0 {
    return String::new();
  }
  s.chars().skip(h_off).take(max_w).collect()
}

fn slice_char_range(
  _content_sliced: &str,
  full: &str,
  start: usize,
  end: usize,
  h_off: usize,
  render_w: usize,
) -> String {
  let vis_start = h_off;
  let vis_end = h_off + render_w;
  let s = start.max(vis_start);
  let e = end.min(vis_end);
  if s >= e {
    return String::new();
  }
  full.chars().skip(s).take(e - s).collect()
}

fn draw_help(frame: &mut Frame, app: &App, area: Rect, t: &Theme) {
  let Some(ctx) = app.repo_context.as_ref() else {
    return;
  };

  let help = if ctx.no_token {
    "repo: o open repo · q/Esc back"
  } else {
    match ctx.pane_focus {
      RepoPane::Tree => {
        "repo: j/k move · Enter open · Tab preview · b up · o open · y path · u url · q back"
      }
      RepoPane::File => {
        "repo: j/k scroll · h/l pan · +/- wrap · Tab tree · o open · y path · u url · d download · Esc tree · q back"
      }
    }
  };

  frame.render_widget(Paragraph::new(help).style(t.style_dim()), area);
}

fn truncate(s: &str, max_chars: usize) -> String {
  if max_chars == 0 {
    return String::new();
  }
  let mut chars = s.chars();
  let mut out = String::new();
  let mut count = 0;
  for c in &mut chars {
    if count >= max_chars {
      if chars.next().is_some() {
        out.push('…');
      }
      break;
    }
    out.push(c);
    count += 1;
  }
  out
}
