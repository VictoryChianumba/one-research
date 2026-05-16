use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Block, Borders, Clear, Paragraph},
};

use super::widgets::{
  popup_inner, popup_rect, quiet_popup_block, truncate, truncate_str,
};
use crate::app::{App, FeedTab, FocusedReader, ReaderTab};

pub(super) fn chat_context_line(app: &App) -> Option<String> {
  if app.reader.active {
    let title = match app.reader.focused {
      FocusedReader::Secondary if app.reader.dual_active => app
        .reader.secondary.tabs
        .get(app.reader.secondary.active_tab)
        .map(|tab| tab.title.as_str()),
      _ => {
        app.reader.primary.tabs.get(app.reader.primary.active_tab).map(|tab| tab.title.as_str())
      }
    };
    if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
      return Some(format!("active reader · {}", truncate(title, 96)));
    }
  }

  app.selected_item().map(|item| {
    let source = if item.source_name.is_empty() {
      item.source_platform.short_label()
    } else {
      item.source_name.as_str()
    };
    format!("selected item · {} · {}", source, truncate(&item.title, 96))
  })
}

pub(super) fn reader_workspace_split(area: Rect) -> (Rect, Rect) {
  if area.height <= 1 {
    return (area, Rect { x: area.x, y: area.y, width: area.width, height: 0 });
  }
  let rows =
    Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
  (rows[0], rows[1])
}

fn reader_tab_title(tabs: &[ReaderTab], active: usize) -> &str {
  tabs.get(active).map(|tab| tab.title.as_str()).unwrap_or("no paper")
}

pub(super) fn draw_reader_workspace_header(
  frame: &mut Frame,
  app: &App,
  area: Rect,
  label: &str,
) {
  if area.height == 0 || area.width == 0 {
    return;
  }
  if label == "Dual Reader" {
    draw_dual_reader_workspace_header(frame, app, area);
    return;
  }
  let t = app.theme();
  let primary = reader_tab_title(&app.reader.primary.tabs, app.reader.primary.active_tab);
  let context = primary.to_string();
  let label_style =
    Style::default().fg(t.accent).bg(t.bg_panel).add_modifier(Modifier::BOLD);
  let dim_style = Style::default().fg(t.text_dim).bg(t.bg_panel);
  let text_style = Style::default().fg(t.text).bg(t.bg_panel);
  let prefix = format!(" {label} ");
  let sep = "· ";
  let max_context = (area.width as usize)
    .saturating_sub(prefix.chars().count() + sep.chars().count() + 1);
  let context = truncate(&context, max_context);
  let line = Line::from(vec![
    Span::styled(prefix, label_style),
    Span::styled(sep, dim_style),
    Span::styled(context, text_style),
  ]);
  frame.render_widget(
    Paragraph::new(line).style(Style::default().bg(t.bg_panel)),
    area,
  );
}

fn draw_dual_reader_workspace_header(frame: &mut Frame, app: &App, area: Rect) {
  let t = app.theme();
  let label_w = 14.min(area.width);
  let label_rect = Rect { x: area.x, y: area.y, width: label_w, height: 1 };
  let title_area = Rect {
    x: area.x + label_w,
    y: area.y,
    width: area.width.saturating_sub(label_w),
    height: 1,
  };
  let label_style =
    Style::default().fg(t.accent).bg(t.bg_panel).add_modifier(Modifier::BOLD);
  frame.render_widget(
    Paragraph::new(Span::styled(" Dual Reader ", label_style))
      .style(Style::default().bg(t.bg_panel)),
    label_rect,
  );

  let halves = Layout::horizontal([
    Constraint::Percentage(50),
    Constraint::Percentage(50),
  ])
  .split(title_area);
  draw_reader_header_title(
    frame,
    halves[0],
    "primary",
    reader_tab_title(&app.reader.primary.tabs, app.reader.primary.active_tab),
    &t,
  );
  draw_reader_header_title(
    frame,
    halves[1],
    "secondary",
    reader_tab_title(
      &app.reader.secondary.tabs,
      app.reader.secondary.active_tab,
    ),
    &t,
  );
}

fn draw_reader_header_title(
  frame: &mut Frame,
  area: Rect,
  label: &str,
  title: &str,
  t: &crate::theme::Theme,
) {
  if area.width == 0 {
    return;
  }
  let prefix = format!("{label}: ");
  let title_w =
    (area.width as usize).saturating_sub(prefix.chars().count() + 2);
  let line = Line::from(vec![
    Span::styled(" · ", Style::default().fg(t.text_dim).bg(t.bg_panel)),
    Span::styled(prefix, Style::default().fg(t.text_dim).bg(t.bg_panel)),
    Span::styled(
      truncate(title, title_w),
      Style::default().fg(t.text).bg(t.bg_panel),
    ),
  ]);
  frame.render_widget(
    Paragraph::new(line).style(Style::default().bg(t.bg_panel)),
    area,
  );
}

pub(super) fn draw_reader_tab_bar(
  frame: &mut Frame,
  area: Rect,
  tabs: &[ReaderTab],
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
      let sep = Span::raw("  ");
      vec![Span::styled(label, style), sep]
    })
    .collect();
  frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub fn draw_reader_popup(frame: &mut Frame, app: &mut App, area: Rect) {
  let t = app.theme();
  let desired_h = (area.height as u32 * 58 / 100) as u16;
  let popup_rect = popup_rect(area, 70, desired_h, 64, 14, 88);

  frame.render_widget(Clear, popup_rect);

  let block = quiet_popup_block(" Reader · Esc close ", &t);

  let block_inner = block.inner(popup_rect);
  let inner = popup_inner(block_inner, 1, 1);
  frame.render_widget(block, popup_rect);

  let tread_theme = app.theme_for_tread();
  let kitty = app.kitty_supported;
  // Layout-derived editor resize now lives in `ReaderPopupModel::pre_draw`
  // (ADR-002 §D3). One mutable borrow on `app.reader_popup` per frame.
  app
    .reader_popup
    .pre_draw(crate::ui::Viewport::new(inner.width, inner.height));
  let popup = &mut app.reader_popup;
  if let Some(editor) = popup.editor.as_mut() {
    tread::draw(frame, inner, editor, &tread_theme);
    tread::after_draw_guarded(
      editor,
      &mut popup.image_state,
      inner,
      kitty,
      popup.burst.in_burst(),
    );
  }
}

pub(super) fn draw_narrow_feed_details_popup(
  frame: &mut Frame,
  app: &App,
  area: Rect,
) {
  let t = app.theme();
  let popup_h = (area.height * 40 / 100).max(6).min(area.height);
  let popup_w = area.width.saturating_sub(4).max(20);
  let popup_x = area.x + (area.width.saturating_sub(popup_w)) / 2;
  let popup_y = area.y + area.height.saturating_sub(popup_h);
  let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);

  frame.render_widget(Clear, popup_rect);

  let block = Block::default()
    .borders(Borders::ALL)
    .border_style(Style::default().fg(t.border_active))
    .title(Span::styled(
      " Details · j/k: scroll  d/Esc: close ",
      Style::default().fg(t.accent),
    ));

  let inner = block.inner(popup_rect);
  frame.render_widget(block, popup_rect);

  if inner.height == 0 {
    return;
  }

  // details_scroll.max is set in App::pre_draw_update to MAX when
  // the narrow popup is open. We just read offset() here.
  let scroll = app.details_scroll.offset();
  let items = app.items_for_tab();
  let sel = app.active_selected_index();
  let Some(item) = items.get(sel) else { return };

  let text = format!("{}\n\n{}", item.title, item.summary_short);
  let para = Paragraph::new(text)
    .wrap(ratatui::widgets::Wrap { trim: false })
    .scroll((scroll as u16, 0))
    .style(Style::default().fg(t.text));
  frame.render_widget(para, inner);
}

pub(super) fn draw_reader_bottom_pane(
  frame: &mut Frame,
  app: &mut App,
  area: Rect,
) {
  let t = app.theme();
  const POPUP_H: u16 = 11; // border(2) + hint row(1) + sep(1) + content(7)
  let popup_w = (area.width as u32 * 60 / 100) as u16;
  let popup_x = area.x + (area.width.saturating_sub(popup_w)) / 2;
  let popup_y = area.y + area.height.saturating_sub(POPUP_H);
  let popup_rect = Rect::new(popup_x, popup_y, popup_w, POPUP_H);

  frame.render_widget(Clear, popup_rect);

  let focused = app.reader_bottom_focused;
  let border_color = if focused { t.border_active } else { t.border };

  let title_str = if app.reader_bottom_details {
    " Feed Drawer Details · d: back  j/k: scroll  Esc: back "
  } else if app.feed.search_active || !app.feed.search_query.is_empty() {
    " Feed Drawer · / search active  Enter: open  Esc: clear  q: close "
  } else {
    " Feed Drawer · j/k: navigate  /: search  Enter: open  d: details  q: close "
  };
  let block = Block::default()
    .borders(Borders::ALL)
    .border_style(Style::default().fg(border_color))
    .title(Span::styled(title_str, Style::default().fg(t.accent)));

  let inner = block.inner(popup_rect);
  frame.render_widget(block, popup_rect);

  if inner.height == 0 {
    return;
  }

  if app.reader_bottom_details {
    draw_bottom_pane_details(frame, app, inner);
  } else {
    draw_bottom_pane_feed(frame, app, inner);
  }
}

fn draw_bottom_pane_details(frame: &mut Frame, app: &App, area: Rect) {
  // reader_bottom_scroll.max is set in App::pre_draw_update to MAX
  // when details mode is open. We just read offset() here.
  let scroll = app.reader_bottom_scroll.offset();

  let t = app.theme();
  let sel = app.reader_feed_popup_selected;
  if app.feed.feed_tab == FeedTab::History {
    let Some(entry) = app.history_get(sel) else { return };
    let rows =
      Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);
    let title = Line::from(Span::styled(
      entry.title.clone(),
      Style::default().fg(t.text).add_modifier(Modifier::BOLD),
    ));
    let meta = Line::from(vec![
      Span::styled(
        super::feed::history_source_label(entry),
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
      ),
      Span::styled("  ", Style::default().fg(t.text_dim)),
      Span::styled(
        match entry.kind {
          crate::history::HistoryKind::Paper => "paper",
          crate::history::HistoryKind::Query => "query",
        },
        Style::default().fg(t.text_dim),
      ),
      Span::styled("  ", Style::default().fg(t.text_dim)),
      Span::styled(
        crate::history::format_ago(entry.opened_at, chrono::Utc::now()),
        Style::default().fg(t.text_dim),
      ),
    ]);
    frame.render_widget(
      Paragraph::new(vec![title, Line::from(""), meta])
        .wrap(ratatui::widgets::Wrap { trim: false }),
      rows[0],
    );

    let body = if let Some(item) = app.history_item(entry) {
      if item.summary_short.trim().is_empty() {
        "No summary available.".to_string()
      } else {
        item.summary_short
      }
    } else {
      match entry.kind {
        crate::history::HistoryKind::Paper => {
          "This history entry is not backed by a cached paper body.".to_string()
        }
        crate::history::HistoryKind::Query => {
          format!("Discovery query:\n\n{}", entry.key)
        }
      }
    };
    let para = Paragraph::new(body)
      .wrap(ratatui::widgets::Wrap { trim: false })
      .scroll((scroll as u16, 0))
      .style(Style::default().fg(t.text));
    frame.render_widget(para, rows[1]);
    return;
  }

  let Some(item) = app.visible_get(sel) else { return };

  let rows =
    Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);
  let title = Line::from(Span::styled(
    item.title.clone(),
    Style::default().fg(t.text).add_modifier(Modifier::BOLD),
  ));
  let meta = Line::from(vec![
    Span::styled(
      super::feed::feed_source_label(item),
      Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    ),
    Span::styled("  ", Style::default().fg(t.text_dim)),
    Span::styled(
      item.content_type.short_label(),
      Style::default().fg(t.text_dim),
    ),
    Span::styled("  ", Style::default().fg(t.text_dim)),
    Span::styled(item.published_at.clone(), Style::default().fg(t.text_dim)),
  ]);
  frame.render_widget(
    Paragraph::new(vec![title, Line::from(""), meta])
      .wrap(ratatui::widgets::Wrap { trim: false }),
    rows[0],
  );

  let para = Paragraph::new(item.summary_short.clone())
    .wrap(ratatui::widgets::Wrap { trim: false })
    .scroll((scroll as u16, 0))
    .style(Style::default().fg(t.text));
  frame.render_widget(para, rows[1]);
}

fn draw_bottom_pane_feed(frame: &mut Frame, app: &mut App, area: Rect) {
  let t = app.theme();
  let sel = app.reader_feed_popup_selected;
  let rows =
    Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
  let header_area = rows[0];
  let list_area = rows[1];
  let viewport_rows = list_area.height as usize;
  if viewport_rows == 0 {
    return;
  }

  // Intentional render-time mutation. Same pattern as draw_item_table /
  // draw_history_tab: this auto-scroll needs viewport_rows, which is
  // layout-derived. The B2b hoist (reader-bottom variant) wasn't
  // attempted after B2a's regressions; stays here until refactor B's
  // deferred layout-metrics extraction lands.
  let total = if app.feed.feed_tab == FeedTab::History {
    app.history_count()
  } else {
    app.visible_count()
  };
  app.reader_bottom_scroll.set_max(total.saturating_sub(1));
  let mut offset = app.reader_bottom_scroll.offset();
  if sel < offset {
    offset = sel;
  } else if sel >= offset.saturating_add(viewport_rows) {
    offset = sel + 1 - viewport_rows;
  }
  app.reader_bottom_scroll.set_offset(offset);

  frame.render_widget(
    Paragraph::new(drawer_feed_header_line(list_area.width as usize, &t)),
    header_area,
  );

  if total == 0 {
    let empty =
      if app.feed.search_query.is_empty() { "No items" } else { "No matches" };
    frame.render_widget(
      Paragraph::new(empty)
        .alignment(Alignment::Center)
        .style(Style::default().fg(t.text_dim)),
      list_area,
    );
    return;
  }

  if app.feed.feed_tab == FeedTab::History {
    let window =
      app.history_window(offset, offset.saturating_add(viewport_rows));
    for (rel_i, entry) in window.iter().enumerate() {
      let i = offset + rel_i;
      let is_selected = i == sel;
      let row_y = list_area.y + rel_i as u16;
      let row_rect = Rect::new(list_area.x, row_y, list_area.width, 1);
      let row = drawer_history_row_line(
        entry,
        list_area.width as usize,
        is_selected,
        &t,
      );
      if is_selected {
        frame.render_widget(
          Paragraph::new(row).style(t.style_selection()),
          row_rect,
        );
      } else {
        frame.render_widget(Paragraph::new(row), row_rect);
      }
    }
    return;
  }

  let window = app.visible_window(offset, offset.saturating_add(viewport_rows));
  for (rel_i, item) in window.iter().enumerate() {
    let i = offset + rel_i;
    let is_selected = i == sel;
    let row_y = list_area.y + rel_i as u16;
    let row_rect = Rect::new(list_area.x, row_y, list_area.width, 1);
    let row =
      drawer_feed_row_line(item, list_area.width as usize, is_selected, &t);
    if is_selected {
      frame.render_widget(
        Paragraph::new(row).style(t.style_selection()),
        row_rect,
      );
    } else {
      frame.render_widget(Paragraph::new(row), row_rect);
    }
  }
}

fn drawer_history_row_line(
  entry: &crate::history::HistoryEntry,
  width: usize,
  selected: bool,
  t: &crate::theme::Theme,
) -> Line<'static> {
  let source_w = 7usize;
  let kind_w = 6usize;
  let date_w = 10usize;
  let title_w = width.saturating_sub(source_w + kind_w + date_w + 3).max(8);
  let source =
    truncate_str(&super::feed::history_source_label(entry), source_w);
  let kind = match entry.kind {
    crate::history::HistoryKind::Paper => "paper",
    crate::history::HistoryKind::Query => "query",
  };
  let date = entry
    .paper_meta
    .as_ref()
    .map(|m| truncate_str(&m.published_at, date_w))
    .unwrap_or_default();
  let title = truncate_str(&entry.title, title_w);
  let row = format!(
    "{source:<source_w$} {kind:<kind_w$} {title:<title_w$} {date:<date_w$}"
  );
  if selected {
    Line::from(Span::styled(row, t.style_selection_text()))
  } else {
    Line::from(row)
  }
}

pub(super) fn drawer_feed_header_line(
  width: usize,
  t: &crate::theme::Theme,
) -> Line<'static> {
  if width < 34 {
    return Line::from(Span::styled(
      "Title",
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    ));
  }

  let source_w = 7usize;
  let kind_w = 6usize;
  let date_w = 10usize;
  let gap_w = 3usize;
  let title_w = width.saturating_sub(source_w + kind_w + date_w + gap_w).max(8);

  Line::from(vec![
    Span::styled(
      format!("{:<source_w$}", "Src"),
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    ),
    Span::raw(" "),
    Span::styled(
      format!("{:<kind_w$}", "Kind"),
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    ),
    Span::raw(" "),
    Span::styled(
      format!("{:<title_w$}", "Title"),
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    ),
    Span::raw(" "),
    Span::styled(
      format!("{:<date_w$}", "Date"),
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    ),
  ])
}

pub(super) fn drawer_feed_row_line(
  item: &crate::models::FeedItem,
  width: usize,
  selected: bool,
  t: &crate::theme::Theme,
) -> Line<'static> {
  if width < 34 {
    let title = truncate_str(&item.title, width);
    let style = if selected {
      t.style_selection_text()
    } else {
      Style::default().fg(t.text)
    };
    return Line::from(Span::styled(title, style));
  }

  let source_w = 7usize;
  let kind_w = 6usize;
  let date_w = 10usize;
  let gap_w = 3usize;
  let title_w = width.saturating_sub(source_w + kind_w + date_w + gap_w).max(8);

  let source = truncate_str(&super::feed::feed_source_label(item), source_w);
  let kind = truncate_str(item.content_type.short_label(), kind_w);
  let title = truncate_str(&item.title, title_w);
  let date = truncate_str(&item.published_at, date_w);

  if selected {
    let row = format!(
      "{source:<source_w$} {kind:<kind_w$} {title:<title_w$} {date:<date_w$}"
    );
    return Line::from(Span::styled(row, t.style_selection_text()));
  }

  Line::from(vec![
    Span::styled(format!("{source:<source_w$}"), Style::default().fg(t.accent)),
    Span::raw(" "),
    Span::styled(format!("{kind:<kind_w$}"), Style::default().fg(t.text_dim)),
    Span::raw(" "),
    Span::styled(format!("{title:<title_w$}"), Style::default().fg(t.text)),
    Span::raw(" "),
    Span::styled(format!("{date:<date_w$}"), Style::default().fg(t.text_dim)),
  ])
}
