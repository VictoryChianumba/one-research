//! Subject Browser tab renderer (ADR-011).
//!
//! Single right-side rail with drill-replace semantics. The rail shows one
//! taxonomy level at a time:
//!     - Groups (default)
//!     - Archives of a selected Group (after one `Enter` drill)
//!     - Categories of a selected Archive (after two drills)
//!
//! A one-line breadcrumb sits above the rail showing the current path
//! (`Mathematics ›` or `Mathematics › Mathematics ›`). The left-hand
//! pane is the regular feed table — populated by `draw_feed_pane` in
//! `main_row.rs`. There is no longer a separate "details panel" — the
//! ADR-010 `draw_browse_detail_panel` was deleted in PR 1 of ADR-011.
//!
//! Promoted categories (those in `config.sources.arxiv_categories`)
//! render with a leading `★` marker (ADR-010 §D5 stays in force).

use ratatui::{
  Frame,
  layout::Rect,
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::Paragraph,
};

use super::widgets::{pane_inset, truncate_str};
use crate::app::{BrowseFocus, BrowseModel};
use crate::models::arxiv_taxonomy::TAXONOMY;

/// Render the taxonomy rail into the area allocated by `main_row.rs`.
/// The surrounding split border is owned by `main_row.rs`, so this
/// function draws no outer frame.
pub fn draw_browse_tab(
  frame: &mut Frame,
  browse: &BrowseModel,
  promoted: &[String],
  subject_follow: bool,
  search_active: bool,
  theme: &ui_theme::Theme,
  area: Rect,
) {
  if area.height < 3 || area.width < 12 {
    return;
  }

  // Cap the rail to its natural column width so a wide-but-short pane — the
  // narrow-mode bottom strip (ADR-011) — keeps label and count together
  // instead of stretching the count to the far edge, and centre that column
  // in the strip. In wide mode the pane is already <= RIGHT_COL_WIDTH, so the
  // cap is a no-op and the offset is zero.
  let capped_w = area.width.min(super::RIGHT_COL_WIDTH);
  let x_off = area.width.saturating_sub(capped_w) / 2;
  let area = Rect { x: area.x + x_off, width: capped_w, ..area };

  let inner = pane_inset(area);
  if inner.height < 3 {
    return;
  }

  // Layout: breadcrumb (1) + gap (1) + rail body + footer (1).
  // Footer renders the subject-follow indicator so the toggle state
  // is always visible (ADR-011 §E4).
  let breadcrumb_area = Rect { height: 1, ..inner };
  let footer_h = if inner.height >= 5 { 1 } else { 0 };
  let body = Rect {
    y: inner.y.saturating_add(2),
    height: inner.height.saturating_sub(2 + footer_h),
    ..inner
  };
  let footer_area = if footer_h > 0 {
    Some(Rect {
      x: inner.x,
      y: inner.y + inner.height - footer_h,
      width: inner.width,
      height: footer_h,
    })
  } else {
    None
  };

  let rail_focused = browse.focus == BrowseFocus::Rail;

  draw_breadcrumb(frame, browse, theme, breadcrumb_area, rail_focused);
  draw_rail_rows(frame, browse, promoted, theme, body);
  if let Some(fa) = footer_area {
    draw_rail_footer(
      frame,
      subject_follow,
      search_active,
      theme,
      fa,
      rail_focused,
    );
  }
}

/// Subject-follow indicator pinned at the bottom of the rail. Toggled
/// via `x` while the rail is focused. While a search is active the search
/// bar captures `x` (it types into the query), so the footer tells you to
/// exit search first rather than implying `x` works.
fn draw_rail_footer(
  frame: &mut Frame,
  subject_follow: bool,
  search_active: bool,
  theme: &ui_theme::Theme,
  area: Rect,
  focused: bool,
) {
  let (label, mut label_style) = if search_active {
    // `x` is being typed into the search bar right now — Esc or Enter
    // both exit search input, after which `x` toggles follow.
    ("Follow: Esc/Enter, then x", Style::default().fg(theme.accent))
  } else if subject_follow {
    (
      "Follow: x",
      Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
    )
  } else {
    ("Follow: .", Style::default().fg(theme.text_dim))
  };
  if !focused && !search_active {
    label_style = Style::default().fg(theme.border);
  }
  let text = truncate_str(label, area.width.saturating_sub(1) as usize);
  frame.render_widget(
    Paragraph::new(Span::styled(format!(" {text}"), label_style)),
    area,
  );
}

fn draw_breadcrumb(
  frame: &mut Frame,
  browse: &BrowseModel,
  theme: &ui_theme::Theme,
  area: Rect,
  focused: bool,
) {
  let crumbs = breadcrumb_for(browse);
  let crumb = truncate_str(&crumbs, area.width.saturating_sub(1) as usize);
  let style = if focused {
    Style::default().fg(theme.header).add_modifier(Modifier::BOLD)
  } else {
    Style::default().fg(theme.text_dim)
  };
  frame.render_widget(
    Paragraph::new(Span::styled(format!(" {crumb}"), style)),
    Rect { height: 1, ..area },
  );
}

/// Build the breadcrumb string for the current rail path. Used by
/// `draw_breadcrumb` and exposed for tests + the title-bar subtitle.
pub fn breadcrumb_for(browse: &BrowseModel) -> String {
  match browse.rail_path.len() {
    0 => "Browse arXiv".to_string(),
    1 => browse.current_group().map(|g| g.name.to_string()).unwrap_or_default(),
    _ => {
      let g = browse.current_group().map(|g| g.name).unwrap_or("");
      let a = browse.current_archive().map(|a| a.name).unwrap_or("");
      format!("{g} › {a}")
    }
  }
}

fn draw_rail_rows(
  frame: &mut Frame,
  browse: &BrowseModel,
  promoted: &[String],
  theme: &ui_theme::Theme,
  area: Rect,
) {
  let rail_focused = browse.focus == BrowseFocus::Rail;
  let selected = browse.rail_cursor.selected();
  let len = browse.current_level_len();
  let visible = visible_range(selected, len, area.height);
  let max_text = area.width.saturating_sub(2) as usize;

  let lines: Vec<Line> = match browse.rail_path.len() {
    0 => TAXONOMY
      .iter()
      .enumerate()
      .skip(visible.start)
      .take(visible.end.saturating_sub(visible.start))
      .map(|(i, g)| {
        rail_row(
          theme,
          i == selected,
          rail_focused,
          /* marker = */ None,
          g.name,
          Some(g.archives.len()),
          max_text,
        )
      })
      .collect(),
    1 => {
      let archives = browse.current_group().map(|g| g.archives).unwrap_or(&[]);
      archives
        .iter()
        .enumerate()
        .skip(visible.start)
        .take(visible.end.saturating_sub(visible.start))
        .map(|(i, a)| {
          rail_row(
            theme,
            i == selected,
            rail_focused,
            None,
            a.name,
            Some(a.categories.len()),
            max_text,
          )
        })
        .collect()
    }
    _ => {
      let cats = browse.current_archive().map(|a| a.categories).unwrap_or(&[]);
      cats
        .iter()
        .enumerate()
        .skip(visible.start)
        .take(visible.end.saturating_sub(visible.start))
        .map(|(i, c)| {
          let label = if c.code == c.name {
            c.name.to_string()
          } else {
            format!("{} · {}", c.code, c.name)
          };
          let is_promoted = promoted.iter().any(|p| p == c.code);
          let marker = if is_promoted { Some("★") } else { None };
          // ADR-015 §F6: an honest per-category indicator. Blank = never
          // fetched this session, `0` = fetched but empty, `N` = N papers
          // buffered. Replaces the silent empty cell that left a drilled
          // category looking broken when it was simply un-fetched.
          rail_row(
            theme,
            i == selected,
            rail_focused,
            marker,
            &label,
            browse.loaded_count(c.code),
            max_text,
          )
        })
        .collect()
    }
  };

  frame.render_widget(Paragraph::new(lines), area);
}

/// Render one rail row: optional marker glyph (`★` for promoted),
/// label (truncated to the column width minus the marker + count),
/// and an optional right-aligned count.
fn rail_row(
  theme: &ui_theme::Theme,
  selected: bool,
  focused: bool,
  marker: Option<&'static str>,
  label: &str,
  count: Option<usize>,
  width: usize,
) -> Line<'static> {
  let count_w = if count.is_some() { 4 } else { 0 };
  let marker_w = 1;
  let label_w = width.saturating_sub(marker_w + count_w);
  let truncated = truncate_str(label, label_w);
  let text_style = row_text_style(theme, selected, focused);
  let marker_style =
    Style::default().fg(if focused { theme.accent } else { theme.border });
  let count_style = if selected && focused {
    theme.style_selection_dim()
  } else {
    Style::default().fg(theme.text_dim)
  };

  let mut spans = vec![
    Span::styled(marker.unwrap_or(" ").to_string(), marker_style),
    Span::styled(
      format!("{:<label_w$}", truncated, label_w = label_w),
      text_style,
    ),
  ];
  if let Some(n) = count {
    spans.push(Span::styled(format!(" {n:>2} "), count_style));
  }
  Line::from(spans)
}

fn row_text_style(
  theme: &ui_theme::Theme,
  selected: bool,
  focused: bool,
) -> Style {
  if selected && focused {
    theme.style_selection_text()
  } else if selected {
    Style::default().fg(theme.header)
  } else {
    Style::default().fg(theme.text_dim)
  }
}

/// Window of visible row indices around `selected` for a `len`-row
/// column at `height`-row screen real estate. Same shape as the
/// helper that used to live inline in ADR-010 PR 1 / 2 — extracted
/// here so the rail can scroll.
fn visible_range(
  selected: usize,
  len: usize,
  height: u16,
) -> std::ops::Range<usize> {
  if len == 0 || height == 0 {
    return 0..0;
  }
  let visible = height as usize;
  let start = if selected >= visible { selected + 1 - visible } else { 0 };
  let end = (start + visible).min(len);
  start..end
}
