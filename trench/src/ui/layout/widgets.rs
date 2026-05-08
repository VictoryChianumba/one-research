//! Cross-pane drawing utilities — popup/modal geometry, layout split boxes,
//! text truncation, rect margins, and small style helpers used by every pane.

use ratatui::{
  Frame,
  layout::Rect,
  style::{Color, Modifier, Style},
  text::{Line, Span},
  widgets::{Block, Borders, Paragraph},
};

use crate::theme::Theme;

// ── Popup / modal geometry ──────────────────────────────────────────────────

pub(super) fn popup_rect(
  area: Rect,
  width_pct: u16,
  desired_h: u16,
  min_w: u16,
  min_h: u16,
  max_h_pct: u16,
) -> Rect {
  let popup_w = (area.width as u32 * width_pct as u32 / 100) as u16;
  let popup_w = popup_w.max(min_w).min(area.width);
  let max_h = (area.height as u32 * max_h_pct as u32 / 100) as u16;
  let popup_h = desired_h
    .max(min_h)
    .min(max_h.max(min_h).min(area.height))
    .min(area.height);
  let popup_x = area.x + area.width.saturating_sub(popup_w) / 2;
  let popup_y = area.y + area.height.saturating_sub(popup_h) / 2;
  Rect::new(popup_x, popup_y, popup_w, popup_h)
}

pub(super) fn popup_inner(block_inner: Rect, pad_x: u16, pad_y: u16) -> Rect {
  Rect {
    x: block_inner.x.saturating_add(pad_x),
    y: block_inner.y.saturating_add(pad_y),
    width: block_inner.width.saturating_sub(pad_x.saturating_mul(2)),
    height: block_inner.height.saturating_sub(pad_y.saturating_mul(2)),
  }
}

pub(super) fn quiet_popup_block(
  title: &'static str,
  t: &Theme,
) -> Block<'static> {
  Block::default()
    .borders(Borders::ALL)
    .border_style(Style::default().fg(t.border_active))
    .title(Span::styled(
      title,
      Style::default().fg(t.header).add_modifier(Modifier::BOLD),
    ))
}

pub(super) fn settings_modal_rect(area: Rect) -> Rect {
  let popup_w =
    (area.width as u32 * 72 / 100).max(72).min(area.width as u32) as u16;
  let popup_h =
    (area.height as u32 * 74 / 100).max(22).min(area.height as u32) as u16;
  let x = area.x + area.width.saturating_sub(popup_w) / 2;
  let y = area.y + area.height.saturating_sub(popup_h) / 2;
  Rect::new(x, y, popup_w, popup_h)
}

pub(super) fn settings_card_block(
  title: &'static str,
  t: &Theme,
) -> Block<'static> {
  Block::default()
    .borders(Borders::ALL)
    .title(Span::styled(
      title,
      Style::default().fg(t.text).add_modifier(Modifier::BOLD),
    ))
    .border_style(Style::default().fg(t.border))
    .style(Style::default().bg(t.bg_panel))
}

pub(super) fn draw_card_footer(
  frame: &mut Frame,
  area: Rect,
  t: &Theme,
  text: &'static str,
) {
  let footer_rule = "─".repeat(area.width as usize);
  let footer = Paragraph::new(vec![
    Line::from(Span::styled(footer_rule, Style::default().fg(t.border))),
    Line::from(Span::styled(
      text,
      Style::default().fg(t.text_dim).bg(t.bg_panel),
    )),
  ])
  .style(Style::default().bg(t.bg_panel));
  frame.render_widget(footer, area);
}

// ── Color swatch (used by both settings and theme picker) ──────────────────

pub(super) fn swatch(color: Color) -> Span<'static> {
  Span::styled("  ", Style::default().bg(color))
}

// ── Text truncation ─────────────────────────────────────────────────────────

/// Truncate `s` to at most `max_chars` Unicode scalar values.
/// Returns a `&str` slice ending on a char boundary — never panics on multibyte input.
pub(super) fn safe_truncate_chars(s: &str, max_chars: usize) -> &str {
  match s.char_indices().nth(max_chars) {
    Some((byte_idx, _)) => &s[..byte_idx],
    None => s,
  }
}

pub(super) fn truncate_str(s: &str, max: usize) -> String {
  let chars: Vec<char> = s.chars().collect();
  if chars.len() <= max {
    s.to_string()
  } else {
    chars[..max.saturating_sub(1)].iter().collect::<String>() + "…"
  }
}

pub(super) fn truncate(s: &str, max_chars: usize) -> String {
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

// ── Rect helpers ────────────────────────────────────────────────────────────

/// Shrink a rect by `margin` columns on each side (horizontal only).
pub(super) fn h_margin(r: Rect, margin: u16) -> Rect {
  Rect { x: r.x + margin, width: r.width.saturating_sub(margin * 2), ..r }
}

// ── Shared-box layout helpers ───────────────────────────────────────────────

/// Draws one outer DarkGray border enclosing two side-by-side columns.
/// `right_w` is the width of the right column INSIDE the border (no border chars).
/// Draws a `│` divider between columns, `┬`/`┴` connectors at top/bottom border,
/// and title strings (` {title} ` padded with `─`) embedded in the top border row.
/// Returns `(left_inner, right_inner)` — content rects with no own borders.
pub(super) fn draw_horiz_split_box(
  frame: &mut Frame,
  area: Rect,
  right_w: u16,
  left_title: &str,
  right_title: &str,
  t: &Theme,
) -> (Rect, Rect) {
  let s = Style::default().fg(t.border);

  // Outer border (provides ┌┐└┘ and ─/│ edges)
  frame.render_widget(
    Block::default().borders(Borders::ALL).border_style(s),
    area,
  );

  // Inner content rect
  let inner = Rect {
    x: area.x + 1,
    y: area.y + 1,
    width: area.width.saturating_sub(2),
    height: area.height.saturating_sub(2),
  };

  // Clamp right_w so there is always at least 1 column on each side
  let right_w = right_w.min(inner.width.saturating_sub(2));
  let left_w = inner.width.saturating_sub(right_w + 1); // +1 for divider col
  let div_x = inner.x + left_w;

  // Vertical divider body
  if inner.height > 0 {
    let div_lines: Vec<Line> =
      (0..inner.height).map(|_| Line::from(Span::styled("│", s))).collect();
    frame.render_widget(
      Paragraph::new(div_lines),
      Rect { x: div_x, y: inner.y, width: 1, height: inner.height },
    );
  }

  // ┬ / ┴ connectors
  frame.render_widget(
    Paragraph::new(Span::styled("┬", s)),
    Rect { x: div_x, y: area.y, width: 1, height: 1 },
  );
  if area.height > 1 {
    frame.render_widget(
      Paragraph::new(Span::styled("┴", s)),
      Rect { x: div_x, y: area.y + area.height - 1, width: 1, height: 1 },
    );
  }

  // Title overlays on the top border row
  if left_w > 0 {
    let t = format!("{:─^w$}", format!(" {left_title} "), w = left_w as usize);
    frame.render_widget(
      Paragraph::new(Span::styled(t, s)),
      Rect { x: area.x + 1, y: area.y, width: left_w, height: 1 },
    );
  }
  if right_w > 0 {
    let t =
      format!("{:─^w$}", format!(" {right_title} "), w = right_w as usize);
    frame.render_widget(
      Paragraph::new(Span::styled(t, s)),
      Rect { x: div_x + 1, y: area.y, width: right_w, height: 1 },
    );
  }

  let left_rect =
    Rect { x: inner.x, y: inner.y, width: left_w, height: inner.height };
  let right_rect =
    Rect { x: div_x + 1, y: inner.y, width: right_w, height: inner.height };
  (left_rect, right_rect)
}

/// Draws one outer DarkGray border enclosing two vertically stacked rows.
/// The top section title is embedded in the top border; the bottom section
/// title is embedded in a `├─ Title ─┤` divider row between the sections.
/// Returns `(top_inner, bottom_inner)` — content rects with no own borders.
pub(super) fn draw_vert_split_box(
  frame: &mut Frame,
  area: Rect,
  top_title: &str,
  bottom_title: &str,
  t: &Theme,
) -> (Rect, Rect) {
  let s = Style::default().fg(t.border);

  frame.render_widget(
    Block::default().borders(Borders::ALL).border_style(s),
    area,
  );

  let inner = Rect {
    x: area.x + 1,
    y: area.y + 1,
    width: area.width.saturating_sub(2),
    height: area.height.saturating_sub(2),
  };

  // Split evenly; divider row is 1 row
  let top_h = (inner.height / 2).max(3).min(inner.height.saturating_sub(2));
  let div_y = inner.y + top_h;
  let bot_h = inner.height.saturating_sub(top_h + 1);

  // ├─ Bottom title ─┤ divider row
  let div_content =
    format!("{:─^w$}", format!(" {bottom_title} "), w = inner.width as usize);
  let div_line = format!("├{div_content}┤");
  frame.render_widget(
    Paragraph::new(Span::styled(div_line, s)),
    Rect { x: area.x, y: div_y, width: area.width, height: 1 },
  );

  // Top title overlay in top border row
  if inner.width > 0 {
    let t =
      format!("{:─^w$}", format!(" {top_title} "), w = inner.width as usize);
    frame.render_widget(
      Paragraph::new(Span::styled(t, s)),
      Rect { x: area.x + 1, y: area.y, width: inner.width, height: 1 },
    );
  }

  let top_rect =
    Rect { x: inner.x, y: inner.y, width: inner.width, height: top_h };
  let bot_rect =
    Rect { x: inner.x, y: div_y + 1, width: inner.width, height: bot_h };
  (top_rect, bot_rect)
}
