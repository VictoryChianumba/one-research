use ratatui::{
  Frame,
  layout::Rect,
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::Paragraph,
};

use super::widgets::{pane_inset, safe_truncate_chars, truncate};
use crate::app::{App, FeedTab};
use crate::models::{FeedItem, SourcePlatform};

pub fn draw_details_panel(frame: &mut Frame, app: &mut App, area: Rect) {
  let t = app.theme();
  let t_details = std::time::Instant::now();

  let bottom_h = area.height * 38 / 100;
  let top_h = area.height.saturating_sub(bottom_h + 1);
  let div_y = area.y + top_h;

  let top_area = Rect { height: top_h, ..area };
  let bottom_area = Rect { y: div_y + 1, height: bottom_h, ..area };

  // Divider between selected-paper detail and the activity summary.
  let sb = Style::default().fg(t.border);
  let activity_title = " Activity ";
  let activity_w = activity_title.chars().count();
  let left_rule_w = (area.width as usize).saturating_sub(activity_w) / 2;
  let right_rule_w =
    (area.width as usize).saturating_sub(activity_w + left_rule_w);
  frame.render_widget(
    Paragraph::new(Line::from(vec![
      Span::styled("─".repeat(left_rule_w), sb),
      Span::styled(activity_title, Style::default().fg(t.header)),
      Span::styled("─".repeat(right_rule_w), sb),
    ])),
    Rect { x: area.x, y: div_y, width: area.width, height: 1 },
  );
  frame.render_widget(
    Paragraph::new(Span::styled("├", sb)),
    Rect { x: area.x.saturating_sub(1), y: div_y, width: 1, height: 1 },
  );
  frame.render_widget(
    Paragraph::new(Span::styled("┤", sb)),
    Rect { x: area.x + area.width, y: div_y, width: 1, height: 1 },
  );

  // ── Dashboard (bottom pane) ───────────────────────────────────────────────
  {
    let dash_inner = Rect {
      x: bottom_area.x + 1,
      width: bottom_area.width.saturating_sub(2),
      ..bottom_area
    };
    let w = dash_inner.width as usize;

    // Single memoized read replaces 4 workflow-state scans, the queue-titles
    // scan, and the recent-48h fused pass that previously ran on every draw.
    let counts = app.item_counts();
    let queued = counts.queued;
    let read = counts.deep_read;
    let archived = counts.archived;
    let total = counts.total;
    let recent_count = counts.recent_total;
    let today_count = counts.recent_today;
    let recent_hf = counts.recent_hf;
    let recent_arxiv = counts.recent_arxiv;
    let recent_other = counts.recent_other;

    // Single-channel emphasis: dim color is enough; no bold.
    let activity_label_style = Style::default().fg(t.text_dim);
    let label_style = Style::default().fg(t.text_dim);
    let val_style = Style::default().fg(t.text);

    // Continue Reading
    let continue_title =
      app.last_read.as_deref().unwrap_or("─ nothing opened yet ─");
    let continue_source = app.last_read_source.as_deref().unwrap_or("");

    let label_w = 11;
    let value_w = w.saturating_sub(label_w).max(1);
    let queue_summary =
      format!("{queued} queued item{}", if queued == 1 { "" } else { "s" });
    let fresh_summary = format!("{today_count} today   {recent_count} in 48h");
    let source_summary =
      format!("HF {recent_hf}   arXiv {recent_arxiv}   Other {recent_other}");

    let mut lines: Vec<Line> = vec![Line::from("")];
    push_activity_wrapped(
      &mut lines,
      "Last read",
      continue_title,
      activity_label_style,
      val_style,
      value_w,
      2,
    );
    if !continue_source.is_empty() {
      push_activity_continuation(
        &mut lines,
        continue_source,
        label_style,
        value_w,
        1,
      );
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
      Span::styled("Queue      ", activity_label_style),
      Span::styled(queue_summary, val_style),
    ]));
    if counts.queue_preview.is_empty() {
      lines.push(Line::from(vec![
        Span::styled("           ", label_style),
        Span::styled("─ empty ─", label_style),
      ]));
    } else {
      for title in counts.queue_preview.iter() {
        push_activity_continuation(&mut lines, title, val_style, value_w, 2);
      }
    }
    lines.extend([
      Line::from(""),
      Line::from(vec![
        Span::styled("Fresh      ", activity_label_style),
        Span::styled(truncate(&fresh_summary, value_w), val_style),
      ]),
      Line::from(vec![
        Span::styled("           ", label_style),
        Span::styled(truncate(&source_summary, value_w), label_style),
      ]),
      Line::from(""),
      Line::from(vec![
        Span::styled("Library    ", activity_label_style),
        Span::styled(
          truncate(&format!("Queue {queued}   Read {read}"), value_w),
          val_style,
        ),
      ]),
      Line::from(vec![
        Span::styled("           ", label_style),
        Span::styled(
          truncate(&format!("Archived {archived}   Total {total}"), value_w),
          val_style,
        ),
      ]),
    ]);

    frame.render_widget(Paragraph::new(lines), dash_inner);
  }

  // Standard pane padding — same helper as the feed pane uses.
  let inner = pane_inset(top_area);

  // The selection-change reset for details_scroll lives in
  // App::pre_draw_update (Phase 4 hoist). By the time draw runs,
  // details_scroll is already correct for the current selection.
  let history = app.filtered_history();
  if let Some(subject) = details_subject(app, &history) {
    let title_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
    let meta_style = Style::default().fg(t.text_dim);
    // Single-channel emphasis: dim color carries the "label" signal; no bold.
    let label_style = Style::default().fg(t.text_dim);
    let dim_style = Style::default().fg(t.text_dim);
    let value_style = Style::default().fg(t.text);
    let accent_style = Style::default().fg(t.accent);
    let detail_w = inner.width.max(1) as usize;
    let mut lines = render_details_subject(
      subject,
      app,
      DetailStyles {
        title_style,
        header_style: Style::default()
          .fg(t.header)
          .add_modifier(Modifier::BOLD),
        meta_style,
        label_style,
        dim_style,
        value_style,
        accent_style,
        success_style: Style::default().fg(t.success),
      },
      detail_w,
      inner.width as usize,
      inner.height as usize,
    );

    if lines.len() > inner.height as usize {
      lines.truncate(inner.height as usize);
    }

    let para = Paragraph::new(lines);
    let t_para = std::time::Instant::now();
    frame.render_widget(para, inner);
    // details_scroll.max is set in App::pre_draw_update — render
    // doesn't mutate it here anymore.
    log::debug!("details Paragraph render: {}ms", t_para.elapsed().as_millis());
  } else {
    let hint = Paragraph::new("Select an item from the feed or history")
      .style(Style::default().fg(t.text_dim));
    frame.render_widget(hint, inner);
  }

  log::debug!(
    "draw_details_panel total: {}ms",
    t_details.elapsed().as_millis()
  );
}

enum DetailsSubject<'a> {
  FeedItem(&'a FeedItem),
  HistoryPaper {
    entry: &'a crate::history::HistoryEntry,
    item: Option<&'a FeedItem>,
    meta: Option<&'a crate::history::HistoryPaperMeta>,
  },
  HistoryQuery(&'a crate::history::HistoryEntry),
}

#[derive(Clone, Copy)]
struct DetailStyles {
  title_style: Style,
  header_style: Style,
  meta_style: Style,
  label_style: Style,
  dim_style: Style,
  value_style: Style,
  accent_style: Style,
  success_style: Style,
}

fn details_subject<'a>(
  app: &'a App,
  history: &[&'a crate::history::HistoryEntry],
) -> Option<DetailsSubject<'a>> {
  if app.feed.feed_tab == FeedTab::History {
    let entry = *history.get(app.feed.history_list.selected())?;
    return match entry.kind {
      crate::history::HistoryKind::Paper => {
        // O(1) hashmap lookup vs the prior O(N) chain+find scan that
        // ran per row × per frame on the History tab.
        let item = app
          .workspace
          .url_index
          .get(&entry.key)
          .map(|&i| &app.workspace.items[i])
          .or_else(|| {
            app
              .feed
              .discovery
              .url_index
              .get(&entry.key)
              .map(|&i| &app.feed.discovery.items[i])
          });
        Some(DetailsSubject::HistoryPaper {
          entry,
          item,
          meta: entry.paper_meta.as_ref(),
        })
      }
      crate::history::HistoryKind::Query => {
        Some(DetailsSubject::HistoryQuery(entry))
      }
    };
  }
  app.selected_item().map(DetailsSubject::FeedItem)
}

fn render_details_subject<'a>(
  subject: DetailsSubject<'a>,
  app: &'a App,
  s: DetailStyles,
  detail_w: usize,
  inner_w: usize,
  visible_height: usize,
) -> Vec<Line<'a>> {
  match subject {
    DetailsSubject::FeedItem(item) => {
      render_feed_item_details(item, app, s, detail_w, inner_w, visible_height)
    }
    DetailsSubject::HistoryPaper { entry, item: Some(item), .. } => {
      let mut lines = render_feed_item_details(
        item,
        app,
        s,
        detail_w,
        inner_w,
        visible_height,
      );
      lines.insert(
        2.min(lines.len()),
        Line::from(Span::styled(
          format!(
            "Viewed {}",
            crate::history::format_ago(entry.opened_at, chrono::Utc::now())
          ),
          s.meta_style,
        )),
      );
      lines
    }
    DetailsSubject::HistoryPaper { entry, item: None, meta } => {
      render_history_paper_details(
        entry,
        meta,
        app,
        s,
        detail_w,
        visible_height,
      )
    }
    DetailsSubject::HistoryQuery(entry) => {
      render_history_query_details(entry, s, detail_w)
    }
  }
}

fn render_feed_item_details<'a>(
  item: &'a FeedItem,
  app: &'a App,
  s: DetailStyles,
  detail_w: usize,
  inner_w: usize,
  visible_height: usize,
) -> Vec<Line<'a>> {
  let tags = item.domain_tags.join(", ");
  let authors = item.authors.join(", ");
  let source_label = if item.source_name.is_empty() {
    item.source_platform.short_label().to_string()
  } else {
    item.source_name.clone()
  };
  let mut lines = detail_title_lines(&item.title, detail_w, s.title_style);
  let meta_parts = [
    source_label.as_str(),
    item.content_type.short_label(),
    item.published_at.as_str(),
    item.workflow_state.short_label(),
  ];
  lines.push(Line::from(Span::styled(
    truncate(&meta_parts.join(" · "), detail_w),
    s.meta_style,
  )));
  lines.push(Line::from(""));
  push_detail_field(
    &mut lines,
    "Authors",
    &authors,
    s.label_style,
    s.value_style,
    detail_w,
    3,
  );
  lines.push(Line::from(""));
  let mut source_spans = vec![
    Span::styled("Source   ", s.label_style),
    Span::styled(truncate(&source_label, 16), s.accent_style),
    Span::styled("  ", s.dim_style),
    Span::styled(item.content_type.short_label(), s.accent_style),
  ];
  if item.source_platform == SourcePlatform::HuggingFace
    && item.upvote_count > 0
  {
    source_spans.extend([
      Span::styled("  votes ", s.dim_style),
      Span::styled(item.upvote_count.to_string(), s.value_style),
    ]);
  }
  lines.push(Line::from(source_spans));
  if let Some(ref repo) = item.github_repo {
    let display = repo.strip_prefix("https://").unwrap_or(repo.as_str());
    push_detail_field(
      &mut lines,
      "Repo",
      display,
      s.label_style,
      s.accent_style,
      detail_w,
      1,
    );
  }
  if !tags.is_empty() {
    push_detail_field(
      &mut lines,
      "Topics",
      &tags,
      s.label_style,
      s.value_style,
      detail_w,
      2,
    );
  }
  let user_tags = crate::tags::for_url(&app.workspace.item_tags, &item.url);
  if !user_tags.is_empty() {
    let formatted =
      user_tags.iter().map(|t| format!("[{t}]")).collect::<Vec<_>>().join("  ");
    push_detail_field(
      &mut lines,
      "Tags",
      &formatted,
      s.label_style,
      s.accent_style,
      detail_w,
      2,
    );
  }
  if !item.benchmark_results.is_empty() {
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
      "Benchmarks",
      s.header_style.add_modifier(Modifier::UNDERLINED),
    )));
    for b in item.benchmark_results.iter().take(3) {
      lines.push(Line::from(Span::styled(
        truncate(
          &format!("  {}/{}: {} ({})", b.task, b.dataset, b.score, b.metric),
          inner_w,
        ),
        s.dim_style,
      )));
    }
  }
  let notif =
    app.notification.message.as_deref().filter(|_| {
      app.notification.item_id.as_deref() == Some(item.url.as_str())
    });
  let footer_lines =
    3 + usize::from(
      item.github_owner.is_some() && item.github_repo_name.is_some(),
    ) + notif.map_or(0, |_| 2);
  let summary_lines = visible_height
    .saturating_sub(lines.len().saturating_add(footer_lines + 2))
    .min(7);
  push_summary_and_actions(
    &mut lines,
    &item.summary_short,
    &item.url,
    item.github_owner.is_some() && item.github_repo_name.is_some(),
    notif,
    summary_lines,
    s,
    detail_w,
  );
  lines
}

fn render_history_paper_details<'a>(
  entry: &'a crate::history::HistoryEntry,
  meta: Option<&'a crate::history::HistoryPaperMeta>,
  app: &'a App,
  s: DetailStyles,
  detail_w: usize,
  visible_height: usize,
) -> Vec<Line<'a>> {
  let mut lines = detail_title_lines(&entry.title, detail_w, s.title_style);
  let now = chrono::Utc::now();
  let viewed = crate::history::format_ago(entry.opened_at, now);
  let published = meta.map(|m| m.published_at.as_str()).unwrap_or("");
  let source = meta
    .map(|m| m.source_platform.short_label())
    .unwrap_or(entry.source.as_str());
  let meta_line = if published.is_empty() {
    format!("History · {source} · viewed {viewed}")
  } else {
    format!("History · {source} · {published} · viewed {viewed}")
  };
  lines.push(Line::from(Span::styled(
    truncate(&meta_line, detail_w),
    s.meta_style,
  )));
  lines.push(Line::from(""));
  if let Some(meta) = meta {
    let authors = meta.authors.join(", ");
    push_detail_field(
      &mut lines,
      "Authors",
      &authors,
      s.label_style,
      s.value_style,
      detail_w,
      3,
    );
  }
  lines.push(Line::from(""));
  lines.push(Line::from(vec![
    Span::styled("Source   ", s.label_style),
    Span::styled(truncate(&entry.source, 16), s.accent_style),
  ]));
  let summary = meta.map(|m| m.summary_short.as_str()).unwrap_or("");
  let notif = app.notification.message.as_deref().filter(|_| {
    app.notification.item_id.as_deref() == Some(entry.key.as_str())
  });
  let footer_lines = 3 + notif.map_or(0, |_| 2);
  let summary_lines = visible_height
    .saturating_sub(lines.len().saturating_add(footer_lines + 2))
    .min(7);
  push_summary_and_actions(
    &mut lines,
    summary,
    &entry.key,
    false,
    notif,
    summary_lines,
    s,
    detail_w,
  );
  lines
}

fn render_history_query_details<'a>(
  entry: &'a crate::history::HistoryEntry,
  s: DetailStyles,
  detail_w: usize,
) -> Vec<Line<'a>> {
  let now = chrono::Utc::now();
  let mut lines = detail_title_lines(&entry.title, detail_w, s.title_style);
  lines.push(Line::from(Span::styled(
    truncate(
      &format!(
        "History · query · {}",
        crate::history::format_ago(entry.opened_at, now)
      ),
      detail_w,
    ),
    s.meta_style,
  )));
  lines.push(Line::from(""));
  push_detail_field(
    &mut lines,
    "Intent",
    &entry.source,
    s.label_style,
    s.value_style,
    detail_w,
    1,
  );
  push_detail_field(
    &mut lines,
    "Query",
    &entry.key,
    s.label_style,
    s.value_style,
    detail_w,
    4,
  );
  lines.push(Line::from(""));
  lines.push(Line::from(vec![
    Span::styled("Action   ", s.label_style),
    Span::styled("Enter", s.accent_style),
    Span::styled(" rerun discovery", s.dim_style),
  ]));
  lines
}

fn detail_title_lines<'a>(
  title: &str,
  detail_w: usize,
  title_style: Style,
) -> Vec<Line<'a>> {
  textwrap::wrap(title, detail_w)
    .into_iter()
    .take(2)
    .map(|line| Line::from(Span::styled(line.into_owned(), title_style)))
    .collect()
}

fn push_summary_and_actions<'a>(
  lines: &mut Vec<Line<'a>>,
  summary: &str,
  url: &str,
  has_repo: bool,
  notification: Option<&str>,
  summary_lines: usize,
  s: DetailStyles,
  detail_w: usize,
) {
  if !summary.is_empty() && summary_lines > 0 {
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Summary", s.header_style)));
    lines.extend(wrap_limited_ellipsis(
      summary,
      detail_w,
      summary_lines,
      s.value_style,
    ));
  }
  let url_w = detail_w.saturating_sub(9);
  lines.push(Line::from(""));
  lines.push(Line::from(vec![
    Span::styled("URL      ", s.label_style),
    Span::styled(truncate(url, url_w), s.dim_style),
  ]));
  lines.push(Line::from(vec![
    Span::styled("Action   ", s.label_style),
    Span::styled("o", s.accent_style),
    Span::styled(" open URL", s.dim_style),
  ]));
  if has_repo {
    lines.push(Line::from(vec![
      Span::styled("         ", s.label_style),
      Span::styled("v", s.success_style),
      Span::styled(" view repo", s.dim_style),
    ]));
  }
  if let Some(notification) = notification {
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(notification.to_string(), s.dim_style)));
  }
}

fn push_activity_wrapped<'a>(
  lines: &mut Vec<Line<'a>>,
  label: &'static str,
  value: &str,
  label_style: Style,
  value_style: Style,
  value_width: usize,
  max_lines: usize,
) {
  let mut wrapped: Vec<String> = textwrap::wrap(value, value_width)
    .into_iter()
    .take(max_lines)
    .map(|line| line.into_owned())
    .collect();
  if wrapped.is_empty() {
    wrapped.push(String::new());
  }
  let label_text = format!("{label:<11}");
  for (idx, value_line) in wrapped.into_iter().enumerate() {
    let prefix = if idx == 0 { label_text.as_str() } else { "           " };
    lines.push(Line::from(vec![
      Span::styled(prefix.to_string(), label_style),
      Span::styled(value_line, value_style),
    ]));
  }
}

fn push_activity_continuation<'a>(
  lines: &mut Vec<Line<'a>>,
  value: &str,
  value_style: Style,
  value_width: usize,
  max_lines: usize,
) {
  for value_line in
    textwrap::wrap(value, value_width).into_iter().take(max_lines)
  {
    lines.push(Line::from(vec![
      Span::raw("           "),
      Span::styled(value_line.into_owned(), value_style),
    ]));
  }
}

fn push_detail_field<'a>(
  lines: &mut Vec<Line<'a>>,
  label: &'static str,
  value: &str,
  label_style: Style,
  value_style: Style,
  total_width: usize,
  max_lines: usize,
) {
  let label_w = 9;
  let value_w = total_width.saturating_sub(label_w).max(1);
  let label_text = format!("{label:<9}");
  let wrapped: Vec<String> = textwrap::wrap(value, value_w)
    .into_iter()
    .take(max_lines)
    .map(|line| line.into_owned())
    .collect();
  if wrapped.is_empty() {
    lines.push(Line::from(vec![
      Span::styled(label_text, label_style),
      Span::raw(""),
    ]));
    return;
  }
  for (idx, value_line) in wrapped.into_iter().enumerate() {
    let prefix = if idx == 0 { label_text.as_str() } else { "         " };
    lines.push(Line::from(vec![
      Span::styled(prefix.to_string(), label_style),
      Span::styled(value_line, value_style),
    ]));
  }
}

fn wrap_limited_ellipsis<'a>(
  value: &str,
  width: usize,
  max_lines: usize,
  style: Style,
) -> Vec<Line<'a>> {
  if max_lines == 0 {
    return Vec::new();
  }
  let width = width.max(1);
  let wrapped: Vec<String> = textwrap::wrap(value, width)
    .into_iter()
    .map(|line| line.into_owned())
    .collect();
  if wrapped.is_empty() {
    return vec![Line::from(Span::styled("", style))];
  }
  let overflowed = wrapped.len() > max_lines;
  let mut lines: Vec<String> = wrapped.into_iter().take(max_lines).collect();
  if overflowed {
    if let Some(last) = lines.last_mut() {
      let keep = width.saturating_sub(1);
      let trimmed = safe_truncate_chars(last.trim_end(), keep);
      *last = format!("{trimmed}…");
    }
  }
  lines.into_iter().map(|line| Line::from(Span::styled(line, style))).collect()
}
