use ratatui::{
  Frame,
  layout::{Constraint, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span, Text},
  widgets::{
    Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Table,
  },
};

use super::reader::{drawer_feed_header_line, drawer_feed_row_line};
use super::widgets::{pane_inset, safe_truncate_chars, truncate, truncate_str};
use crate::app::FeedTab;
use crate::models::SourcePlatform;

pub fn draw_feed_pane(
  frame: &mut Frame,
  model: &mut crate::feed::FeedModel,
  discovery: &mut crate::app::DiscoveryModel,
  ctx: &crate::feed::FeedContext,
  area: Rect,
) {
  if area.height == 0 {
    return;
  }
  let content_area = area;

  // Discoveries tab: paper list always shown; persistent search bar pinned at bottom.
  if model.feed_tab == FeedTab::Discoveries {
    draw_discoveries_with_searchbar(frame, model, discovery, ctx, content_area);
    return;
  }

  // History tab: filter chips + activity log.
  if model.feed_tab == FeedTab::History {
    draw_history_tab(frame, model, discovery, ctx, content_area);
    return;
  }

  // Library tab: workflow-state filter chips + filtered item list.
  if model.feed_tab == FeedTab::Library {
    draw_library_tab(frame, model, discovery, ctx, content_area);
    return;
  }

  // Narrow pane: switch to title-only list to avoid squished columns.
  if area.width < 70 {
    draw_narrow_feed(frame, model, discovery, ctx, content_area);
  } else {
    draw_item_table(frame, model, discovery, ctx, content_area);
  }
}

/// Discoveries tab: paper list above, persistent search bar below.
fn draw_discoveries_with_searchbar(
  frame: &mut Frame,
  model: &mut crate::feed::FeedModel,
  discovery: &mut crate::app::DiscoveryModel,
  ctx: &crate::feed::FeedContext,
  area: Rect,
) {
  const FOOTER_H: u16 = 3; // separator + input + hint
  if area.height <= FOOTER_H {
    draw_discovery_searchbar(frame, model, discovery, ctx, area);
    return;
  }

  let list_h = area.height - FOOTER_H;
  let list_area =
    Rect { x: area.x, y: area.y, width: area.width, height: list_h };
  let bar_area =
    Rect { x: area.x, y: area.y + list_h, width: area.width, height: FOOTER_H };

  // Paper list
  if area.width < 70 {
    draw_narrow_feed(frame, model, discovery, ctx, list_area);
  } else {
    draw_item_table(frame, model, discovery, ctx, list_area);
  }

  draw_discovery_searchbar(frame, model, discovery, ctx, bar_area);
  draw_discovery_palette(frame, model, discovery, ctx, list_area);
}

fn draw_discovery_searchbar(
  frame: &mut Frame,
  _model: &crate::feed::FeedModel,
  discovery: &crate::app::DiscoveryModel,
  ctx: &crate::feed::FeedContext,
  area: Rect,
) {
  let t = ctx.theme;
  let w = area.width as usize;
  let has_session = !discovery.session.is_empty();
  let intent_label = discovery.intent.label();

  // Separator line — title shows current status inline rather than a separate row.
  let intent_badge = if intent_label != "papers" {
    format!(" [{}]", intent_label)
  } else {
    String::new()
  };
  let (title_text, title_style) = if discovery.loading {
    let short = discovery.status.trim_end_matches('…').trim_end_matches("...");
    (
      format!("{}…{}", short, intent_badge),
      Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    )
  } else if has_session {
    (
      format!("Discovery ●{}", intent_badge),
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    )
  } else {
    (format!("Discovery{}", intent_badge), Style::default().fg(t.border))
  };
  let sep_fill = "─".repeat(w.saturating_sub(title_text.len() + 8));
  let sep_line = Line::from(vec![
    Span::styled("─── ", Style::default().fg(t.border)),
    Span::styled(title_text, title_style),
    Span::styled(format!(" {sep_fill}"), Style::default().fg(t.border)),
  ]);

  // Input line — prompt only when focused, query dim when unfocused.
  let cursor = if discovery.search_focused { "█" } else { "" };
  let (prompt, query_style) = if discovery.search_focused {
    (
      Span::styled("  ", Style::default().fg(t.accent)),
      Style::default().fg(t.text),
    )
  } else {
    (
      Span::styled("  ", Style::default().fg(t.text_dim)),
      Style::default().fg(t.text_dim),
    )
  };
  let input_line = Line::from(vec![
    prompt,
    Span::styled(format!("{}{}", discovery.query, cursor), query_style),
  ]);

  // Hint line — contextual, always rendered to avoid height jitter.
  let hint_text = if discovery.search_focused {
    if discovery.query.starts_with('/') {
      "Tab: complete  ↑↓: navigate  Enter: run  Esc: cancel"
    } else if has_session {
      "Enter: refine  Ctrl+N: new search  Esc: unfocus"
    } else {
      "Enter: search  /: commands  Esc: unfocus"
    }
  } else if has_session {
    "Any key to refine  ·  Ctrl+N: new search  ·  / for commands"
  } else {
    "Any key to focus  ·  / for commands"
  };
  let hint_line =
    Line::from(Span::styled(hint_text, Style::default().fg(t.text_dim)));

  frame
    .render_widget(Paragraph::new(vec![sep_line, input_line, hint_line]), area);
}

fn draw_discovery_palette(
  frame: &mut Frame,
  _model: &crate::feed::FeedModel,
  discovery: &crate::app::DiscoveryModel,
  ctx: &crate::feed::FeedContext,
  list_area: Rect,
) {
  if !discovery.search_focused || !discovery.query.starts_with('/') {
    return;
  }

  let all_specs = crate::commands::registry::discovery_slash_specs();
  let query_lower = discovery.query_lower.as_str();
  let suggestions: Vec<_> = all_specs
    .iter()
    .filter(|s| query_lower == "/" || s.command.starts_with(query_lower))
    .collect();

  if suggestions.is_empty() || list_area.height == 0 {
    return;
  }

  let t = ctx.theme;
  let w = list_area.width as usize;
  let visible = suggestions.len().min(8);
  let selected = discovery.palette.selected().min(suggestions.len() - 1);
  let scroll = discovery.palette.offset();
  let start = scroll;
  let end = (start + visible).min(suggestions.len());

  // separator + rows + count
  let height = (visible as u16 + 2).min(list_area.height);
  let area = Rect {
    x: list_area.x,
    y: list_area.y + list_area.height.saturating_sub(height),
    width: list_area.width,
    height,
  };

  frame.render_widget(Clear, area);

  let name_col = 16usize;
  let badge_col = 7usize;
  let desc_col = w.saturating_sub(name_col + badge_col + 4);

  let sep_fill = "─".repeat(w.saturating_sub(16));
  let mut lines: Vec<Line> = vec![Line::from(Span::styled(
    format!("─── Commands ──{sep_fill}"),
    Style::default().fg(t.border),
  ))];

  for (i, spec) in suggestions.iter().skip(start).take(end - start).enumerate()
  {
    let is_selected = start + i == selected;
    let (arrow, name_style, desc_style) = if is_selected {
      (
        "→ ",
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        Style::default().fg(t.text),
      )
    } else {
      ("  ", Style::default().fg(t.text), Style::default().fg(t.text_dim))
    };

    let name = spec.command.trim_start_matches('/');
    let name_padded = format!("{:<width$}", name, width = name_col);
    let badge = if spec.badge.is_empty() {
      String::new()
    } else {
      format!("[{}]", spec.badge)
    };
    let badge_padded = format!("{:<width$}", badge, width = badge_col);
    let desc: String = spec.description.chars().take(desc_col).collect();

    lines.push(Line::from(vec![
      Span::styled(arrow, Style::default().fg(t.accent)),
      Span::styled(name_padded, name_style),
      Span::styled(badge_padded, Style::default().fg(t.text_dim)),
      Span::styled(desc, desc_style),
    ]));
  }

  let count_str = format!("({}/{})", selected + 1, suggestions.len());
  let padding = w.saturating_sub(count_str.len());
  lines.push(Line::from(Span::styled(
    format!("{}{}", " ".repeat(padding), count_str),
    Style::default().fg(t.text_dim),
  )));

  frame.render_widget(
    Paragraph::new(lines).style(Style::default().bg(t.bg_chat)),
    area,
  );
}

pub fn draw_library_tab(
  frame: &mut Frame,
  model: &mut crate::feed::FeedModel,
  discovery: &mut crate::app::DiscoveryModel,
  ctx: &crate::feed::FeedContext,
  area: Rect,
) {
  let t = ctx.theme;
  if area.height == 0 {
    return;
  }

  // ── Filter chip row ───────────────────────────────────────────────────
  let chips_area = Rect { height: 1, ..area };
  let chips_sep_area = Rect { y: area.y + 1, height: 1, ..area };

  // Per-chip count from the pre-computed aggregate carried in FeedContext.
  let count_queued = ctx.item_counts.queued;
  let count_deep_read = ctx.item_counts.deep_read;
  let count_archived = ctx.item_counts.archived;
  let chip_count = |filter: crate::library::LibraryFilter| -> usize {
    match filter {
      crate::library::LibraryFilter::All => count_queued + count_deep_read,
      crate::library::LibraryFilter::Queue => count_queued,
      crate::library::LibraryFilter::Read => count_deep_read,
      crate::library::LibraryFilter::Archived => count_archived,
    }
  };

  let mut chip_spans: Vec<Span> = vec![Span::raw("  ")];
  let mut chip_width: usize = 2;
  for (i, filter) in crate::library::LibraryFilter::ORDER.iter().enumerate() {
    let active = *filter == model.library_filter;
    let style = if active {
      Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
    } else {
      Style::default().fg(t.text_dim)
    };
    let text = format!("[{} {}]", filter.label(), chip_count(*filter));
    chip_width += text.chars().count();
    chip_spans.push(Span::styled(text, style));
    if i + 1 < crate::library::LibraryFilter::ORDER.len() {
      chip_spans.push(Span::raw("  "));
      chip_width += 2;
    }
  }
  let hint = if model.library_visual_mode {
    let n = model.library_selected_urls.len();
    format!("VISUAL · {n} selected · r read · w queue · x archive · Esc cancel")
  } else {
    "[ ] cycle  ·  v select  ·  f filter  ·  / search".to_string()
  };
  let hint_style = if model.library_visual_mode {
    Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
  } else {
    Style::default().fg(t.text_dim)
  };
  let total = area.width as usize;
  if total > chip_width + hint.chars().count() + 4 {
    let pad = total - chip_width - hint.chars().count() - 2;
    chip_spans.push(Span::raw(" ".repeat(pad)));
    chip_spans.push(Span::styled(hint, hint_style));
  }
  frame.render_widget(Paragraph::new(Line::from(chip_spans)), chips_area);

  frame.render_widget(
    Paragraph::new("─".repeat(area.width as usize))
      .style(Style::default().fg(t.border)),
    chips_sep_area,
  );

  // ── Item list (reuse the table renderer) ─────────────────────────────
  let list_area = Rect {
    x: area.x,
    y: area.y + 2,
    width: area.width,
    height: area.height.saturating_sub(2),
  };
  if list_area.height == 0 {
    return;
  }

  if ctx.visible_indices.is_empty() {
    let msg = if ctx.workspace.items.is_empty() {
      "No items yet — fetch a feed first."
    } else {
      "No items match this filter."
    };
    frame.render_widget(
      Paragraph::new(Line::from(Span::styled(
        format!("  {msg}"),
        Style::default().fg(t.text_dim),
      ))),
      list_area,
    );
    return;
  }

  if list_area.width < 70 {
    draw_narrow_feed(frame, model, discovery, ctx, list_area);
  } else {
    draw_item_table(frame, model, discovery, ctx, list_area);
  }
}

fn draw_history_tab(
  frame: &mut Frame,
  model: &mut crate::feed::FeedModel,
  discovery: &mut crate::app::DiscoveryModel,
  ctx: &crate::feed::FeedContext,
  area: Rect,
) {
  let t = ctx.theme;
  if area.height == 0 {
    return;
  }

  // ── Filter chips row ────────────────────────────────────────────────
  let chips_area = Rect { height: 1, ..area };
  let chips_sep_area = Rect { y: area.y + 1, height: 1, ..area };
  let mut chip_spans: Vec<Span> = vec![Span::styled("  ", Style::default())];
  let mut chip_width: usize = 2;
  for (i, filter) in crate::history::HistoryFilter::ORDER.iter().enumerate() {
    let active = *filter == model.history_filter;
    let style = if active {
      Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
    } else {
      Style::default().fg(t.text_dim)
    };
    let text = format!("[{}]", filter.label());
    chip_width += text.chars().count();
    chip_spans.push(Span::styled(text, style));
    if i + 1 < crate::history::HistoryFilter::ORDER.len() {
      chip_spans.push(Span::raw("  "));
      chip_width += 2;
    }
  }
  let hint = "[ ] cycle  ·  f filter  ·  / search";
  let total = area.width as usize;
  if total > chip_width + hint.chars().count() + 4 {
    let pad = total - chip_width - hint.chars().count() - 2;
    chip_spans.push(Span::raw(" ".repeat(pad)));
    chip_spans.push(Span::styled(hint, Style::default().fg(t.text_dim)));
  }
  frame.render_widget(Paragraph::new(Line::from(chip_spans)), chips_area);
  frame.render_widget(
    Paragraph::new("─".repeat(area.width as usize))
      .style(Style::default().fg(t.border)),
    chips_sep_area,
  );

  // ── Activity list ──────────────────────────────────────────────────
  let list_area = Rect {
    x: area.x,
    y: area.y + 2,
    width: area.width,
    height: area.height.saturating_sub(2),
  };
  if list_area.height == 0 {
    return;
  }

  let total = ctx.filtered_history.len();
  if total == 0 {
    let msg = if ctx.workspace.history.is_empty() {
      "No history yet — open a paper or run a search."
    } else {
      "No entries in this time window."
    };
    frame.render_widget(
      Paragraph::new(Line::from(Span::styled(
        format!("  {msg}"),
        Style::default().fg(t.text_dim),
      ))),
      list_area,
    );
    return;
  }

  let header_style = Style::default().fg(t.header).add_modifier(Modifier::BOLD);
  let header = Row::new(vec![
    feed_header_cell("Src", header_style),
    feed_header_cell("Kind", header_style),
    feed_header_cell("Title", header_style),
    feed_header_cell("Date", header_style),
    feed_header_cell("Viewed", header_style),
  ])
  .height(2);

  let pane = pane_inset(list_area);
  if pane.height == 0 {
    return;
  }
  let inner = Rect {
    y: pane.y,
    height: pane.height,
    width: pane.width.saturating_sub(2),
    ..pane
  };
  let scrollbar_rect = Rect {
    x: inner.x + inner.width + 1,
    y: inner.y,
    width: 1,
    height: inner.height,
  };
  if inner.height == 0 {
    return;
  }

  let now = chrono::Utc::now();
  let title_w = (inner.width.saturating_sub(7 + 7 + 10 + 10 + 4)) as usize;
  let title_wrap_w = title_w.max(10);
  let viewport_rows = inner.height.saturating_sub(2) as usize;
  if viewport_rows == 0 {
    let table = Table::new(
      Vec::<Row>::new(),
      [
        Constraint::Length(5),
        Constraint::Min(0),
        Constraint::Length(14),
        Constraint::Length(10),
      ],
    )
    .header(header)
    .column_spacing(1)
    .row_highlight_style(Style::default());
    frame.render_widget(table, inner);
    return;
  }
  // Auto-scroll the active list to keep the cursor visible. Lives in
  // `FeedModel::pre_draw` post-PR-4 — history's items are fixed-height,
  // so `items_fitting` is just `viewport_rows`. See ADR-001 D3.
  model.pre_draw(
    discovery,
    crate::ui::Viewport::new(inner.width, viewport_rows as u16),
    total,
    viewport_rows,
  );
  let selected = model.history_list.selected().min(total.saturating_sub(1));
  let offset = model.history_list.offset();

  let end = (offset + viewport_rows + 2).min(total);
  let window = &ctx.filtered_history[offset..end];
  // Store raw title strings (not Vec<Line>) — see draw_item_table's
  // window_data shape for the same rationale.
  let window_data: Vec<(u16, Vec<String>)> = window
    .iter()
    .map(|entry| {
      let mut raw_lines = textwrap::wrap(&entry.title, title_wrap_w);
      let row_height = raw_lines.len().min(2).max(1) as u16;
      if raw_lines.len() > 2 {
        raw_lines.truncate(2);
        if let Some(last) = raw_lines.last_mut() {
          let s = last.clone().into_owned();
          let trimmed = safe_truncate_chars(&s, title_wrap_w.saturating_sub(1));
          *last = std::borrow::Cow::Owned(format!("{trimmed}…"));
        }
      }
      let title_lines: Vec<String> =
        raw_lines.into_iter().map(|l| l.into_owned()).collect();
      (row_height, title_lines)
    })
    .collect();

  let rows: Vec<Row> = window
    .iter()
    .enumerate()
    .map(|(i, entry)| {
      let item_idx = offset + i;
      let is_selected = item_idx == selected;
      let (content_height, title_lines) = &window_data[i];
      // O(1) hashmap lookup. Was a per-row O(items + discovery_items)
      // chain+find scan against ~3K items.
      let cached_item = ctx
        .workspace
        .url_index
        .get(&entry.key)
        .map(|&idx| &ctx.workspace.items[idx])
        .or_else(|| {
          discovery
            .url_index
            .get(&entry.key)
            .map(|&idx| &discovery.items[idx])
        });
      let row_style =
        if is_selected { t.style_selection() } else { Style::default() };
      let selected_text_style = t.style_selection_text();
      let selected_dim_style = t.style_selection_dim();
      let dim_style = if is_selected {
        selected_dim_style
      } else {
        Style::default().fg(t.text_dim)
      };
      let source = cached_item
        .map(feed_source_label)
        .unwrap_or_else(|| history_source_label(entry));
      let kind = match (entry.kind, cached_item) {
        (crate::history::HistoryKind::Paper, Some(item)) => {
          item.content_type.short_label().to_string()
        }
        (crate::history::HistoryKind::Paper, None) => "paper".to_string(),
        (crate::history::HistoryKind::Query, _) => "query".to_string(),
      };
      let date = cached_item
        .map(|item| item.published_at.as_str())
        .or_else(|| {
          entry.paper_meta.as_ref().map(|meta| meta.published_at.as_str())
        })
        .unwrap_or("");
      let source_style = if is_selected {
        selected_text_style
      } else if entry.kind == crate::history::HistoryKind::Query {
        Style::default().fg(t.text_dim)
      } else {
        Style::default().fg(t.text_dim)
      };
      Row::new(vec![
        feed_cell(&source, source_style),
        feed_cell(&kind, dim_style),
        Cell::from(Text::from({
          let title_style =
            if is_selected { selected_text_style } else { Style::default() };
          let lines: Vec<Line<'static>> = title_lines
            .iter()
            .map(|s| Line::from(Span::styled(s.clone(), title_style)))
            .chain(std::iter::once(Line::from("")))
            .collect();
          lines
        })),
        feed_cell(date, dim_style),
        feed_cell(&crate::history::format_ago(entry.opened_at, now), dim_style),
      ])
      .style(row_style)
      .height((content_height + 1).max(3))
    })
    .collect();

  let table = Table::new(
    rows,
    [
      Constraint::Length(7),
      Constraint::Length(7),
      Constraint::Min(0),
      Constraint::Length(10),
      Constraint::Length(10),
    ],
  )
  .header(header)
  .column_spacing(1)
  .row_highlight_style(Style::default());
  frame.render_widget(table, inner);

  if total > 0 {
    let mut scrollbar_state = ScrollbarState::new(total)
      .position(offset)
      .viewport_content_length(viewport_rows);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
      .begin_symbol(None)
      .end_symbol(None);
    frame.render_stateful_widget(
      scrollbar,
      scrollbar_rect,
      &mut scrollbar_state,
    );
  }
}

pub(super) fn history_source_label(
  entry: &crate::history::HistoryEntry,
) -> String {
  match entry.paper_meta.as_ref().map(|meta| &meta.source_platform) {
    Some(SourcePlatform::HuggingFace) => "hf".to_string(),
    Some(SourcePlatform::ArXiv) => "arxiv".to_string(),
    Some(SourcePlatform::Rss) if !entry.source.is_empty() => {
      truncate(&entry.source, 7)
    }
    Some(platform) => platform.short_label().to_string(),
    None => truncate(&entry.source, 7),
  }
}

pub fn draw_narrow_feed(
  frame: &mut Frame,
  model: &mut crate::feed::FeedModel,
  discovery: &mut crate::app::DiscoveryModel,
  ctx: &crate::feed::FeedContext,
  area: Rect,
) {
  let t = ctx.theme;
  let rows =
    Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
  let header_area = rows[0];
  let list_area = rows[1];
  if list_area.height == 0 {
    return;
  }
  let viewport_rows = list_area.height as usize;
  let title_w = reader_feed_title_width(list_area.width as usize);

  // Layout owns the textwrap (width-dependent); FeedModel owns the
  // offset arithmetic. Heights for items [0, selected] feed the
  // variable-row reverse-walk inside `pre_draw_narrow_feed`.
  let total = ctx.visible_indices.len();
  let selected = model.active_list(discovery).selected();
  // Phase 1: compute row heights with immutable borrow of `model`,
  // ending the borrow before `model.pre_draw_narrow_feed`.
  let row_heights: Vec<usize> = if total > 0 {
    let upper = selected.saturating_add(1).min(total);
    let items = crate::feed::items_for_tab(ctx.workspace, &*model, discovery);
    ctx.visible_indices[..upper]
      .iter()
      .map(|&i| reader_feed_row_height(&items[i], title_w))
      .collect()
  } else {
    Vec::new()
  };
  model.pre_draw_narrow_feed(
    discovery,
    crate::ui::Viewport::new(list_area.width, list_area.height),
    total,
    &row_heights,
  );
  let offset = model.active_list(discovery).offset();

  frame.render_widget(
    Paragraph::new(drawer_feed_header_line(list_area.width as usize, &t)),
    header_area,
  );

  // Each visible row consumes at least 1 terminal row, so capping the
  // window at viewport_rows is a safe upper bound for what gets drawn.
  let window_end =
    offset.saturating_add(viewport_rows).min(ctx.visible_indices.len());
  let items = crate::feed::items_for_tab(ctx.workspace, &*model, discovery);
  let visible: Vec<&crate::models::FeedItem> = ctx.visible_indices
    [offset..window_end]
    .iter()
    .map(|&i| &items[i])
    .collect();
  // Pre-wrap titles once per item; `reader_feed_row_lines` previously
  // re-ran textwrap on each call, doubling work against the same
  // textwrap done for row-height counting.
  let pre_wrapped: Vec<Vec<String>> = visible
    .iter()
    .map(|item| reader_feed_title_lines(&item.title, title_w))
    .collect();
  let mut y = list_area.y;
  for (rel_i, item) in visible.iter().enumerate() {
    if y >= list_area.y + list_area.height {
      break;
    }
    let abs_i = offset + rel_i;
    let is_selected = abs_i == selected;
    let row_lines = reader_feed_row_lines_with_wrapped(
      item,
      &pre_wrapped[rel_i],
      list_area.width as usize,
      is_selected,
      &t,
    );
    for line in row_lines {
      if y >= list_area.y + list_area.height {
        break;
      }
      let row_rect =
        Rect { x: list_area.x, y, width: list_area.width, height: 1 };
      if is_selected {
        frame.render_widget(
          Paragraph::new(line).style(t.style_selection()),
          row_rect,
        );
      } else {
        frame.render_widget(Paragraph::new(line), row_rect);
      }
      y += 1;
    }
    if y < list_area.y + list_area.height {
      let spacer_rect =
        Rect { x: list_area.x, y, width: list_area.width, height: 1 };
      if is_selected {
        frame.render_widget(
          Paragraph::new(Line::from(" ")).style(t.style_selection()),
          spacer_rect,
        );
      }
      y += 1;
    }
  }
}

fn reader_feed_title_width(width: usize) -> usize {
  if width < 34 {
    width.max(8)
  } else {
    let source_w = 7usize;
    let kind_w = 6usize;
    let date_w = 10usize;
    let gap_w = 3usize;
    width.saturating_sub(source_w + kind_w + date_w + gap_w).max(8)
  }
}

fn reader_feed_row_height(
  item: &crate::models::FeedItem,
  title_w: usize,
) -> usize {
  reader_feed_title_lines(&item.title, title_w).len() + 1
}

fn reader_feed_title_lines(title: &str, title_w: usize) -> Vec<String> {
  let mut raw_lines =
    textwrap::wrap(title, title_w).into_iter().collect::<Vec<_>>();
  if raw_lines.is_empty() {
    return vec![String::new()];
  }
  if raw_lines.len() > 2 {
    raw_lines.truncate(2);
    if let Some(last) = raw_lines.last_mut() {
      let s = last.clone().into_owned();
      let trimmed = safe_truncate_chars(&s, title_w.saturating_sub(1));
      *last = std::borrow::Cow::Owned(format!("{trimmed}…"));
    }
  }
  raw_lines.into_iter().map(|line| line.into_owned()).collect()
}

/// Build the per-row Line<'static>s for a single visible item in the
/// narrow-feed table. Takes pre-wrapped title lines so the caller can
/// share the textwrap output with height-counting.
fn reader_feed_row_lines_with_wrapped(
  item: &crate::models::FeedItem,
  title_lines: &[String],
  width: usize,
  selected: bool,
  t: &crate::theme::Theme,
) -> Vec<Line<'static>> {
  if width < 34 {
    return vec![drawer_feed_row_line(item, width, selected, t)];
  }

  let source_w = 7usize;
  let kind_w = 6usize;
  let date_w = 10usize;
  let title_w = reader_feed_title_width(width);
  let source = truncate_str(&feed_source_label(item), source_w);
  let kind = truncate_str(item.content_type.short_label(), kind_w);
  let date = truncate_str(&item.published_at, date_w);

  title_lines
    .iter()
    .enumerate()
    .map(|(idx, title)| {
      let source_text = if idx == 0 {
        format!("{source:<source_w$}")
      } else {
        " ".repeat(source_w)
      };
      let kind_text =
        if idx == 0 { format!("{kind:<kind_w$}") } else { " ".repeat(kind_w) };
      let date_text =
        if idx == 0 { format!("{date:<date_w$}") } else { " ".repeat(date_w) };

      if selected {
        let row =
          format!("{source_text} {kind_text} {title:<title_w$} {date_text}");
        return Line::from(Span::styled(row, t.style_selection_text()));
      }

      Line::from(vec![
        Span::styled(source_text, Style::default().fg(t.text_dim)),
        Span::raw(" "),
        Span::styled(kind_text, Style::default().fg(t.text_dim)),
        Span::raw(" "),
        Span::styled(format!("{title:<title_w$}"), Style::default().fg(t.text)),
        Span::raw(" "),
        Span::styled(date_text, Style::default().fg(t.text_dim)),
      ])
    })
    .collect()
}

pub fn draw_item_table(
  frame: &mut Frame,
  model: &mut crate::feed::FeedModel,
  discovery: &mut crate::app::DiscoveryModel,
  ctx: &crate::feed::FeedContext,
  area: Rect,
) {
  let t = ctx.theme;
  let t_item_table = std::time::Instant::now();
  // Header: color carries the emphasis; no bold (single-channel emphasis).
  let header_style = Style::default().fg(t.header);

  let header = Row::new(vec![
    feed_header_cell(" ", header_style),
    feed_header_cell("Title", header_style),
    feed_header_cell("Date", header_style),
  ])
  .height(2);

  // Standard pane padding: 2 cols left/right, 1 row top/bottom.
  let pane = pane_inset(area);
  if pane.height == 0 {
    return;
  }
  // Reserve a 2-col strip on the right: one blank gutter between the date
  // column and the scrollbar, then one scrollbar column. This keeps the
  // right-side spacing visually symmetric with the pane border inset.
  let inner = Rect { width: pane.width.saturating_sub(2), ..pane };
  let scrollbar_rect = Rect {
    x: inner.x + inner.width + 1,
    y: inner.y,
    width: 1,
    height: inner.height,
  };

  // Available width for title column: total inner width minus fixed cols.
  // sig(1) + date(10) + 2 column gaps of 2 each = 15
  let title_col_w = (inner.width.saturating_sub(1 + 10 + 4)) as usize;
  let title_wrap_w = title_col_w.max(10);

  // Viewport height in rows (inner height minus 2 header rows).
  let viewport_rows = inner.height.saturating_sub(2) as usize;

  // Auto-scroll: reconcile the active list against the current viewport
  // and apply the 2-item bottom buffer. The math moved into
  // FeedModel::pre_draw in PR 4; the caller computes the layout-derived
  // `items_fitting` (item-height-aware textwrap). See ADR-001 D3.
  //
  // Phase 1: read everything needed for pre_draw with an immutable
  // borrow of `model`, ending the borrow before `model.pre_draw`.
  let total_items = ctx.visible_indices.len();
  let items_fitting = {
    let items = crate::feed::items_for_tab(ctx.workspace, &*model, discovery);
    count_items_fitting_from_indices(
      &ctx.visible_indices,
      items,
      model.active_list(discovery).offset(),
      viewport_rows,
      title_wrap_w,
    )
  };
  model.pre_draw(
    discovery,
    crate::ui::Viewport::new(inner.width, viewport_rows as u16),
    total_items,
    items_fitting,
  );

  // ── Slice to visible window — trust list offset as first visible item ─
  // Take viewport_rows + 2 extra so the last row is never clipped even when
  // an item spans 2 rows.
  let start =
    model.active_list(discovery).offset().min(total_items.saturating_sub(1));
  let end = (start + viewport_rows + 2).min(total_items);
  let items = crate::feed::items_for_tab(ctx.workspace, &*model, discovery);
  let window: Vec<&crate::models::FeedItem> =
    ctx.visible_indices[start..end].iter().map(|&i| &items[i]).collect();

  // ── Single textwrap pass over visible window only ─────────────────────────
  // Produces (row_height, title_lines) together — no second wrap call needed.
  let t_heights = std::time::Instant::now();
  // Store raw title strings (not pre-wrapped Lines). Building the styled
  // Lines directly at row time saves a Vec<Line>::clone + Span content
  // into_owned re-conversion per row.
  let window_data: Vec<(u16, Vec<String>)> = window
    .iter()
    .map(|item| {
      let mut raw_lines = textwrap::wrap(&item.title, title_wrap_w);
      let row_height = raw_lines.len().min(2).max(1) as u16;
      if raw_lines.len() > 2 {
        raw_lines.truncate(2);
        if let Some(last) = raw_lines.last_mut() {
          let s = last.clone().into_owned();
          let max_chars = title_wrap_w.saturating_sub(1);
          let trimmed = safe_truncate_chars(&s, max_chars);
          *last = std::borrow::Cow::Owned(format!("{trimmed}…"));
        }
      }
      let title_lines: Vec<String> =
        raw_lines.into_iter().map(|l| l.into_owned()).collect();
      (row_height, title_lines)
    })
    .collect();
  log::debug!(
    "window textwrap ({} items): {}ms",
    window.len(),
    t_heights.elapsed().as_millis()
  );

  // ── Build rows for visible window only ────────────────────────────────────
  let t_rows = std::time::Instant::now();
  let visual_mode =
    model.feed_tab == FeedTab::Library && model.library_visual_mode;
  let selected_idx = model.active_list(discovery).selected();
  let rows: Vec<Row> = window
    .iter()
    .enumerate()
    .map(|(i, item)| {
      let item_idx = start + i;
      let is_cursor = item_idx == selected_idx;
      let in_visual =
        visual_mode && model.library_selected_urls.contains(&item.url);
      let is_selected = is_cursor || in_visual;
      let (content_height, title_lines) = &window_data[i];

      let vm = crate::view_models::FeedRowVm::from_item(item);

      let signal_style = match vm.signal {
        crate::models::SignalLevel::Primary => Style::default().fg(t.accent),
        crate::models::SignalLevel::Secondary => {
          Style::default().fg(t.text_dim)
        }
        crate::models::SignalLevel::Tertiary => Style::default().fg(t.border),
      };

      let row_style =
        if is_selected { t.style_selection() } else { Style::default() };
      let selected_text_style = t.style_selection_text();
      let selected_dim_style = t.style_selection_dim();

      let row_height = content_height + 1;

      // Suppress unused-variable warnings for view-model fields no longer
      // shown in the table (still rendered in the details pane).
      let _ = (&vm.source_label, vm.content_type_short, &vm.author);
      let _ = selected_dim_style;

      Row::new(vec![
        feed_cell(
          vm.signal_indicator,
          if is_selected { selected_text_style } else { signal_style },
        ),
        Cell::from(Text::from({
          let title_style =
            if is_selected { selected_text_style } else { Style::default() };
          let lines: Vec<Line<'static>> = title_lines
            .iter()
            .map(|s| Line::from(Span::styled(s.clone(), title_style)))
            .chain(std::iter::once(Line::from("")))
            .collect();
          lines
        })),
        feed_cell(
          item.published_at.as_str(),
          if is_selected {
            selected_text_style
          } else {
            Style::default().fg(t.text_dim)
          },
        ),
      ])
      .style(row_style)
      .height(row_height)
    })
    .collect();
  log::debug!(
    "rows build ({} window items): {}ms",
    window.len(),
    t_rows.elapsed().as_millis()
  );

  let table = Table::new(
    rows,
    [Constraint::Length(1), Constraint::Min(0), Constraint::Length(10)],
  )
  .header(header)
  .column_spacing(2)
  .row_highlight_style(Style::default());

  let t_render = std::time::Instant::now();
  frame.render_widget(table, inner);
  log::debug!(
    "frame.render_widget(table): {}ms",
    t_render.elapsed().as_millis()
  );

  // Scrollbar uses item indices for proportions — no full-list row count needed.
  if total_items > 0 {
    let mut scrollbar_state = ScrollbarState::new(total_items)
      .position(start)
      .viewport_content_length(viewport_rows);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
      .begin_symbol(None)
      .end_symbol(None);
    frame.render_stateful_widget(
      scrollbar,
      scrollbar_rect,
      &mut scrollbar_state,
    );
  }
  log::debug!(
    "draw_item_table total: {}ms ({} total items, {} in window)",
    t_item_table.elapsed().as_millis(),
    total_items,
    window.len()
  );
}

// feed_source_label moved to view_models/feed_row.rs as part of Phase 4
// view-model consolidation. Re-export the VM helper for callers in this
// crate that still construct labels inline.
pub(super) use crate::view_models::feed_source_label;

fn feed_header_cell(label: &'static str, style: Style) -> Cell<'static> {
  Cell::from(Text::from(vec![
    Line::from(Span::styled(label, style)),
    Line::from(""),
  ]))
}

fn feed_cell(value: &str, style: Style) -> Cell<'static> {
  Cell::from(Text::from(vec![
    Line::from(Span::styled(value.to_string(), style)),
    Line::from(""),
  ]))
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Count how many items (starting from `list_offset`) fit in `viewport_rows`
/// screen rows, including one spacer row between feed items. Indexed
/// variant — paired with `items_for_tab` to read titles.
fn count_items_fitting_from_indices(
  visible_indices: &[usize],
  items: &[crate::models::FeedItem],
  list_offset: usize,
  viewport_rows: usize,
  title_wrap_w: usize,
) -> usize {
  let mut rows_used = 0usize;
  let mut count = 0usize;
  for &idx in visible_indices.iter().skip(list_offset) {
    let Some(item) = items.get(idx) else { break };
    let item_height = if item.title.len() > title_wrap_w { 3 } else { 2 };
    if rows_used + item_height > viewport_rows {
      break;
    }
    rows_used += item_height;
    count += 1;
  }
  count.max(1)
}
