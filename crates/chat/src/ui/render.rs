use ratatui::{
  layout::Rect,
  style::{Modifier, Style},
  text::{Line, Span},
};
use ui_theme::Theme;

use crate::Role;

/// Detect `N. rest` numbered list items. Returns `(num, rest)` or `None`.
fn parse_numbered_item(line: &str) -> Option<(u32, &str)> {
  let dot = line.find(". ")?;
  let num: u32 = line[..dot].trim().parse().ok()?;
  Some((num, &line[dot + 2..]))
}

pub(super) fn render_user_message(
  content: &str,
  wrap_width: usize,
  t: &Theme,
) -> Vec<Line<'static>> {
  let text_style = Style::default().fg(t.text).bg(t.bg_user_msg);
  let stripe_style = Style::default().fg(t.accent).bg(t.bg_user_msg);
  let indent_style = Style::default().fg(t.text_dim).bg(t.bg_user_msg);
  let display_content = if content.is_empty() {
    " ".to_string()
  } else {
    crate::sanitize::sanitize_terminal_text(content)
  };
  let inner_width = wrap_width.saturating_sub(2).max(1);
  let mut lines = Vec::new();
  let mut first_line = true;

  lines.push(user_block_empty_line(wrap_width, t));

  for source_line in display_content.lines() {
    let wrapped_lines: Vec<String> = if source_line.is_empty() {
      vec![" ".repeat(inner_width)]
    } else {
      textwrap::wrap(source_line, inner_width)
        .into_iter()
        .map(|line| line.to_string())
        .collect()
    };

    for wrapped in wrapped_lines {
      let marker = if first_line { "▌ " } else { "  " };
      let marker_style = if first_line { stripe_style } else { indent_style };
      let fill = inner_width.saturating_sub(wrapped.chars().count());
      lines.push(Line::from(vec![
        Span::styled(marker, marker_style),
        Span::styled(wrapped, text_style),
        Span::styled(" ".repeat(fill), text_style),
      ]));
      first_line = false;
    }
  }

  if lines.len() == 1 {
    lines.push(Line::from(vec![
      Span::styled("▌ ", stripe_style),
      Span::styled(" ".repeat(inner_width), text_style),
    ]));
  }

  lines.push(user_block_empty_line(wrap_width, t));
  lines
}

fn user_block_empty_line(width: usize, t: &Theme) -> Line<'static> {
  Line::from(Span::styled(
    " ".repeat(width),
    Style::default().fg(t.text_dim).bg(t.bg_user_msg),
  ))
}

pub(super) fn message_gap_needed(_current: Role, _next: Option<Role>) -> bool {
  false
}

pub(super) fn split_stream_chunks(
  content: &str,
) -> std::collections::VecDeque<String> {
  let mut chunks = std::collections::VecDeque::new();
  let mut current = String::new();
  let mut current_is_whitespace: Option<bool> = None;

  for ch in content.chars() {
    let is_whitespace = ch.is_whitespace();
    if current_is_whitespace.is_some_and(|value| value != is_whitespace) {
      chunks.push_back(std::mem::take(&mut current));
    }
    current_is_whitespace = Some(is_whitespace);
    current.push(ch);
  }

  if !current.is_empty() {
    chunks.push_back(current);
  }

  chunks
}

pub(super) fn append_stream_chunk(target: &mut String, chunk: &str) {
  if chunk.is_empty() {
    return;
  }

  if target.is_empty() || chunk.chars().next().is_some_and(char::is_whitespace)
  {
    target.push_str(chunk);
    return;
  }

  if target.chars().last().is_some_and(char::is_whitespace) {
    target.push_str(chunk);
  } else {
    target.push(' ');
    target.push_str(chunk);
  }
}

pub(super) fn render_assistant_message(
  content: &str,
  wrap_width: usize,
  has_streaming_cursor: bool,
  t: &Theme,
) -> Vec<Line<'static>> {
  let base_style = Style::default().fg(t.text);
  let safe_content = crate::sanitize::sanitize_terminal_text(content);
  let display_content = if has_streaming_cursor {
    format!("{safe_content}█")
  } else {
    safe_content
  };
  let display_content = if display_content.is_empty() {
    " ".to_string()
  } else {
    prepare_assistant_markdown(&display_content)
  };

  let mut lines = vec![Line::from("")];
  lines.extend(render_assistant_blocks(
    &display_content,
    wrap_width,
    t,
    base_style,
  ));
  lines.push(Line::from(""));
  lines
}

fn render_assistant_blocks(
  display_content: &str,
  wrap_width: usize,
  t: &Theme,
  base_style: Style,
) -> Vec<Line<'static>> {
  let mut lines = Vec::new();
  let mut in_code_block = false;

  for source_line in display_content.lines() {
    if source_line.trim_start().starts_with("```") {
      in_code_block = !in_code_block;
      if !in_code_block {
        lines.push(Line::from(""));
      }
      continue;
    }

    if in_code_block {
      render_code_line(source_line, wrap_width, t, &mut lines);
      continue;
    }

    if source_line.trim().is_empty() {
      lines.push(Line::from(""));
    } else if let Some(rest) = source_line.strip_prefix("## ") {
      if has_nonblank_line(&lines) {
        lines.push(Line::from(""));
      }
      let style = Style::default().fg(t.accent).add_modifier(Modifier::BOLD);
      push_wrapped_inline(rest, wrap_width, style, &mut lines);
    } else if let Some(rest) = source_line.strip_prefix("### ") {
      let style = base_style.add_modifier(Modifier::BOLD);
      push_wrapped_inline(rest, wrap_width, style, &mut lines);
    } else if let Some(rest) = source_line.strip_prefix("> ") {
      render_quote(rest, wrap_width, t, base_style, &mut lines);
    } else if let Some(rest) =
      source_line.strip_prefix("- ").or_else(|| source_line.strip_prefix("* "))
    {
      render_bullet(rest, wrap_width, t, base_style, &mut lines);
    } else if let Some((num, rest)) = parse_numbered_item(source_line) {
      render_numbered_item(num, rest, wrap_width, t, base_style, &mut lines);
    } else {
      push_wrapped_inline(source_line, wrap_width, base_style, &mut lines);
    }
  }

  lines
}

fn prepare_assistant_markdown(content: &str) -> String {
  let normalized = normalize_markdown(content);
  let mut out = String::with_capacity(normalized.len() + 64);
  let mut in_code_block = false;

  for (idx, line) in normalized.lines().enumerate() {
    if idx > 0 {
      out.push('\n');
    }

    if line.trim_start().starts_with("```") {
      in_code_block = !in_code_block;
      out.push_str(line);
      continue;
    }

    if in_code_block {
      out.push_str(line);
    } else {
      out.push_str(&break_inline_markdown_markers(line));
    }
  }

  out
}

fn break_inline_markdown_markers(line: &str) -> String {
  let mut out = String::with_capacity(line.len() + 32);
  let mut iter = line.char_indices().peekable();

  while let Some((i, ch)) = iter.next() {
    let rest = &line[i..];

    if ch == ' ' || ch == '\t' {
      if let Some(marker_len) = inline_marker_len(rest) {
        if has_visible_text(&out) {
          out.push('\n');
          out.push_str(rest[..marker_len].trim_start());
          for _ in 0..marker_len.saturating_sub(1) {
            iter.next();
          }
          continue;
        }
      }
    }

    out.push(ch);
  }

  out
}

fn inline_marker_len(rest: &str) -> Option<usize> {
  let marker = rest.trim_start();
  let trimmed = rest.len().saturating_sub(marker.len());

  if let Some(after) = marker.strip_prefix("- ") {
    if starts_structural_text(after) {
      return Some(trimmed + 2);
    }
  }

  if let Some(len) = numbered_marker_len(marker) {
    let after = &marker[len..];
    if starts_structural_text(after) {
      return Some(trimmed + len);
    }
  }

  None
}

fn numbered_marker_len(text: &str) -> Option<usize> {
  let bytes = text.as_bytes();
  let mut idx = 0;
  while idx < bytes.len() && bytes[idx].is_ascii_digit() {
    idx += 1;
  }
  if idx == 0 || idx > 3 {
    return None;
  }
  if bytes.get(idx) == Some(&b'.') && bytes.get(idx + 1) == Some(&b' ') {
    Some(idx + 2)
  } else {
    None
  }
}

fn starts_structural_text(text: &str) -> bool {
  text
    .chars()
    .next()
    .is_some_and(|c| c.is_alphanumeric() || c == '*' || c == '`')
}

fn has_visible_text(text: &str) -> bool {
  text.chars().any(|c| !c.is_whitespace())
}

fn has_nonblank_line(lines: &[Line<'static>]) -> bool {
  lines.last().is_some_and(|line| {
    line.spans.iter().any(|span| !span.content.trim().is_empty())
  })
}

fn push_wrapped_inline(
  text: &str,
  width: usize,
  style: Style,
  lines: &mut Vec<Line<'static>>,
) {
  for wrapped in textwrap::wrap(text, width.max(1)) {
    lines.push(parse_markdown_inline(&wrapped, style));
  }
}

fn render_bullet(
  text: &str,
  wrap_width: usize,
  t: &Theme,
  base_style: Style,
  lines: &mut Vec<Line<'static>>,
) {
  let bullet_width = wrap_width.saturating_sub(4).max(1);
  let marker_style = Style::default().fg(t.text_dim);
  let mut first = true;
  for wrapped in textwrap::wrap(text, bullet_width) {
    let mut spans = if first {
      first = false;
      vec![Span::styled("  • ".to_string(), marker_style)]
    } else {
      vec![Span::styled("    ".to_string(), base_style)]
    };
    spans.extend(parse_markdown_inline(&wrapped, base_style).spans);
    lines.push(Line::from(spans));
  }
}

fn render_numbered_item(
  num: u32,
  text: &str,
  wrap_width: usize,
  t: &Theme,
  base_style: Style,
  lines: &mut Vec<Line<'static>>,
) {
  let prefix = format!("{num}. ");
  let first_prefix = format!("  {prefix}");
  let follow_prefix = " ".repeat(first_prefix.chars().count());
  let item_width =
    wrap_width.saturating_sub(first_prefix.chars().count()).max(1);
  let num_style = Style::default().fg(t.text_dim);
  let mut first = true;

  for wrapped in textwrap::wrap(text, item_width) {
    let mut spans = if first {
      first = false;
      vec![Span::styled(first_prefix.clone(), num_style)]
    } else {
      vec![Span::styled(follow_prefix.clone(), base_style)]
    };
    spans.extend(parse_markdown_inline(&wrapped, base_style).spans);
    lines.push(Line::from(spans));
  }
}

fn render_quote(
  text: &str,
  wrap_width: usize,
  t: &Theme,
  base_style: Style,
  lines: &mut Vec<Line<'static>>,
) {
  let quote_width = wrap_width.saturating_sub(3).max(1);
  let marker_style = Style::default().fg(t.accent);
  let quote_style = base_style.fg(t.text_dim);
  for wrapped in textwrap::wrap(text, quote_width) {
    let mut spans = vec![Span::styled("│ ".to_string(), marker_style)];
    spans.extend(parse_markdown_inline(&wrapped, quote_style).spans);
    lines.push(Line::from(spans));
  }
}

fn render_code_line(
  text: &str,
  wrap_width: usize,
  t: &Theme,
  lines: &mut Vec<Line<'static>>,
) {
  let code_width = wrap_width.saturating_sub(2).max(1);
  let style = Style::default().fg(t.mono).bg(t.bg_code);
  let chunks: Vec<String> = if text.is_empty() {
    vec![String::new()]
  } else {
    textwrap::wrap(text, code_width)
      .into_iter()
      .map(|line| line.to_string())
      .collect()
  };
  for chunk in chunks {
    let fill = code_width.saturating_sub(chunk.chars().count());
    lines.push(Line::from(vec![
      Span::styled(" ", style),
      Span::styled(chunk, style),
      Span::styled(" ".repeat(fill + 1), style),
    ]));
  }
}

/// Parse inline markdown (`**bold**`, `*italic*`) into styled spans.
/// Uses a char-indexed walk to correctly distinguish `**` from `*`.
fn parse_markdown_inline(text: &str, base_style: Style) -> Line<'static> {
  let bold_style = base_style.add_modifier(Modifier::BOLD);
  let italic_style = base_style.add_modifier(Modifier::ITALIC);

  let chars: Vec<char> = text.chars().collect();
  let mut spans: Vec<Span<'static>> = Vec::new();
  let mut i = 0;
  let mut current = String::new();
  let current_style = base_style;

  while i < chars.len() {
    if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
      let start = i + 2;
      let mut j = start;
      while j + 1 < chars.len() && !(chars[j] == '*' && chars[j + 1] == '*') {
        j += 1;
      }
      if j + 1 < chars.len() {
        if !current.is_empty() {
          spans.push(Span::styled(current.clone(), current_style));
          current.clear();
        }
        let inner: String = chars[start..j].iter().collect();
        spans.push(Span::styled(inner, bold_style));
        i = j + 2;
      } else {
        current.push(chars[i]);
        i += 1;
      }
    } else if chars[i] == '*' {
      let start = i + 1;
      let mut j = start;
      while j < chars.len() && chars[j] != '*' {
        j += 1;
      }
      if j < chars.len() {
        if !current.is_empty() {
          spans.push(Span::styled(current.clone(), current_style));
          current.clear();
        }
        let inner: String = chars[start..j].iter().collect();
        spans.push(Span::styled(inner, italic_style));
        i = j + 1;
      } else {
        current.push(chars[i]);
        i += 1;
      }
    } else if chars[i] == '`' {
      let start = i + 1;
      let mut j = start;
      while j < chars.len() && chars[j] != '`' {
        j += 1;
      }
      if j < chars.len() {
        if !current.is_empty() {
          spans.push(Span::styled(current.clone(), current_style));
          current.clear();
        }
        let inner: String = chars[start..j].iter().collect();
        spans.push(Span::styled(
          inner,
          base_style.add_modifier(Modifier::REVERSED),
        ));
        i = j + 1;
      } else {
        current.push(chars[i]);
        i += 1;
      }
    } else {
      current.push(chars[i]);
      i += 1;
    }
  }

  if !current.is_empty() {
    spans.push(Span::styled(current, current_style));
  }

  if spans.is_empty() {
    Line::from(Span::styled(String::new(), base_style))
  } else {
    Line::from(spans)
  }
}

/// Sanitize a successful response body — if the content looks like a JSON
/// error (e.g. the provider returned a 200 with an error payload), forward it
/// through `parse_api_error` so the user sees a clean message.
pub(super) fn sanitize_content(content: &str) -> String {
  let lower = content.to_lowercase();
  let looks_like_error = lower.contains("insufficient_quota")
    || lower.contains("invalid_api_key")
    || lower.contains("rate_limit")
    || (content.trim_start().starts_with('{') && lower.contains("\"error\""));
  if looks_like_error {
    parse_api_error(content)
  } else {
    content.to_string()
  }
}

/// Inject newlines before structural markdown markers that the model often
/// emits inline (because most chat completions return markdown as one or two
/// long paragraphs with embedded headings and lists). Without this pass, the
/// chat renderer's line-prefix parser can't recognise `### 6. Foo` or
/// `2. **Bar**:` as headings/list items because they don't sit at the start
/// of a line — and the user sees a wall of running prose.
fn normalize_markdown(content: &str) -> String {
  let mut out = String::with_capacity(content.len() + 64);
  let mut iter = content.char_indices().peekable();
  while let Some((i, ch)) = iter.next() {
    let rest = &content[i..];
    let prev_is_space =
      out.chars().last().map_or(false, |c| c == ' ' || c == '\t');

    if prev_is_space {
      if rest.starts_with("### ") && starts_numbered_heading(&rest[4..]) {
        if out.ends_with(' ') {
          out.pop();
        }
        out.push_str("\n\n### ");
        for _ in 0..3 {
          iter.next();
        }
        continue;
      }
      if let Some(after) = rest.strip_prefix("## ") {
        if after
          .chars()
          .next()
          .map_or(false, |c| c.is_alphanumeric() || c == '*')
        {
          if out.ends_with(' ') {
            out.pop();
          }
          out.push_str("\n\n## ");
          for _ in 0..2 {
            iter.next();
          }
          continue;
        }
      }
      if let Some(consumed) = match_numbered_bold(rest) {
        if out.ends_with(' ') {
          out.pop();
        }
        out.push('\n');
        out.push_str(&rest[..consumed]);
        for _ in 0..(consumed - 1) {
          iter.next();
        }
        continue;
      }
    }

    out.push(ch);
  }
  out
}

fn starts_numbered_heading(s: &str) -> bool {
  let bytes = s.as_bytes();
  let mut j = 0;
  while j < bytes.len() && bytes[j].is_ascii_digit() {
    j += 1;
  }
  j > 0 && j + 1 < bytes.len() && bytes[j] == b'.' && bytes[j + 1] == b' '
}

fn match_numbered_bold(s: &str) -> Option<usize> {
  let bytes = s.as_bytes();
  let mut j = 0;
  while j < bytes.len() && bytes[j].is_ascii_digit() {
    j += 1;
  }
  if j == 0 {
    return None;
  }
  if bytes.get(j) != Some(&b'.') {
    return None;
  }
  if bytes.get(j + 1) != Some(&b' ') {
    return None;
  }
  if bytes.get(j + 2) != Some(&b'*') || bytes.get(j + 3) != Some(&b'*') {
    return None;
  }
  Some(j + 4)
}

/// Step a cursor byte offset back by one char in `s`. Returns the new offset.
/// Returns 0 if the cursor is already at the start. Char-aware so multi-byte
/// codepoints are stepped over as a unit, not byte-by-byte.
pub(super) fn step_cursor_back(s: &str, cursor: usize) -> usize {
  if cursor == 0 {
    return 0;
  }
  s[..cursor]
    .char_indices()
    .next_back()
    .map(|(i, _)| i)
    .unwrap_or(0)
}

/// Step a cursor byte offset forward by one char in `s`. Returns the new
/// offset. Returns `s.len()` if the cursor is already at the end.
pub(super) fn step_cursor_forward(s: &str, cursor: usize) -> usize {
  if cursor >= s.len() {
    return s.len();
  }
  s[cursor..]
    .char_indices()
    .nth(1)
    .map(|(i, _)| cursor + i)
    .unwrap_or(s.len())
}

/// Delete the char immediately before `cursor` from `s` and return the new
/// cursor position. No-op when cursor is at 0.
pub(super) fn backspace_at_cursor(s: &mut String, cursor: usize) -> usize {
  if cursor == 0 {
    return 0;
  }
  let new_cursor = step_cursor_back(s, cursor);
  s.replace_range(new_cursor..cursor, "");
  new_cursor
}

/// Map a raw API error string to a friendly one-line message.
pub(super) fn parse_api_error(err: &str) -> String {
  let lower = err.to_lowercase();
  if lower.contains("authentication")
    || lower.contains("invalid api key")
    || lower.contains("unauthorized")
    || lower.contains("invalid_api_key")
  {
    return "invalid API key — check settings".to_string();
  }
  if lower.contains("rate limit")
    || lower.contains("rate_limit")
    || lower.contains("429")
  {
    return "rate limit exceeded — try again shortly".to_string();
  }
  if lower.contains("quota")
    || lower.contains("insufficient_quota")
    || lower.contains("billing")
  {
    return "quota exceeded — check billing".to_string();
  }
  let short = crate::sanitize::truncate_chars(err, 80);
  let short = crate::sanitize::sanitize_terminal_text(&short);
  format!("API error — {short}")
}

/// Format a token count as a compact string ("1.2k", "45.3k", "1.2M").
pub(super) fn fmt_tokens(n: u64) -> String {
  if n < 1_000 {
    n.to_string()
  } else if n < 1_000_000 {
    format!("{:.1}k", n as f64 / 1_000.0)
  } else {
    format!("{:.1}M", n as f64 / 1_000_000.0)
  }
}

/// Returns `(cost_usd, ctx_pct, ctx_k)` for the status bar.
pub(super) fn compute_cost_and_ctx(
  provider_name: &str,
  model_name: &str,
  input_tokens: u64,
  output_tokens: u64,
) -> (f64, f64, u64) {
  let (in_rate, out_rate, ctx_window) = model_rates(provider_name, model_name);
  let cost = (input_tokens as f64 * in_rate + output_tokens as f64 * out_rate)
    / 1_000_000.0;
  let total = input_tokens + output_tokens;
  let ctx_pct = if ctx_window > 0 {
    (total as f64 / ctx_window as f64) * 100.0
  } else {
    0.0
  };
  (cost, ctx_pct, ctx_window / 1_000)
}

/// Returns `(input_$/1M, output_$/1M, context_window)` for a model.
fn model_rates(provider: &str, model: &str) -> (f64, f64, u64) {
  match provider {
    "claude" => {
      if model.contains("opus") {
        (15.00, 75.00, 200_000)
      } else if model.contains("haiku") {
        (0.80, 4.00, 200_000)
      } else {
        (3.00, 15.00, 200_000)
      }
    }
    "openai" => {
      if model.contains("gpt-4o") {
        (2.50, 10.00, 128_000)
      } else if model.contains("gpt-4") {
        (30.00, 60.00, 128_000)
      } else if model.contains("gpt-3.5") {
        (0.50, 1.50, 16_385)
      } else {
        (2.50, 10.00, 128_000)
      }
    }
    _ => (3.00, 15.00, 200_000),
  }
}

pub(super) fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
  let x = area.x + area.width.saturating_sub(width) / 2;
  let y = area.y + area.height.saturating_sub(height) / 2;
  Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}

pub(super) fn truncate_for_width(s: &str, max_chars: usize) -> String {
  if max_chars == 0 {
    return String::new();
  }
  let mut out = String::new();
  let mut chars = s.chars();
  for _ in 0..max_chars {
    let Some(c) = chars.next() else { return out };
    out.push(c);
  }
  if chars.next().is_some() {
    out.push('…');
  }
  out
}

#[cfg(test)]
mod tests {
  use super::{
    append_stream_chunk, backspace_at_cursor, normalize_markdown,
    prepare_assistant_markdown, render_assistant_message, render_user_message,
    split_stream_chunks, step_cursor_back, step_cursor_forward,
  };
  use ui_theme::Theme;

  #[test]
  fn step_cursor_back_handles_ascii_and_emoji() {
    assert_eq!(step_cursor_back("hello", 5), 4);
    assert_eq!(step_cursor_back("hello", 1), 0);
    assert_eq!(step_cursor_back("hello", 0), 0);
    assert_eq!(step_cursor_back("😀", 4), 0);
    assert_eq!(step_cursor_back("a😀b", 5), 1);
  }

  #[test]
  fn step_cursor_forward_handles_ascii_and_emoji() {
    assert_eq!(step_cursor_forward("hello", 0), 1);
    assert_eq!(step_cursor_forward("hello", 4), 5);
    assert_eq!(step_cursor_forward("hello", 5), 5);
    assert_eq!(step_cursor_forward("😀", 0), 4);
    assert_eq!(step_cursor_forward("a😀b", 1), 5);
  }

  #[test]
  fn backspace_at_cursor_removes_char_before_cursor() {
    let mut s = String::from("hello");
    let new_cursor = backspace_at_cursor(&mut s, 3);
    assert_eq!(s, "helo");
    assert_eq!(new_cursor, 2);
  }

  #[test]
  fn backspace_at_cursor_at_start_is_noop() {
    let mut s = String::from("hello");
    let new_cursor = backspace_at_cursor(&mut s, 0);
    assert_eq!(s, "hello");
    assert_eq!(new_cursor, 0);
  }

  #[test]
  fn backspace_at_cursor_handles_multibyte() {
    let mut s = String::from("a😀b");
    let new_cursor = backspace_at_cursor(&mut s, 5);
    assert_eq!(s, "ab");
    assert_eq!(new_cursor, 1);
  }

  #[test]
  fn injects_newline_before_inline_h3_with_number() {
    let got = normalize_markdown("foo bar ### 6. Heading more text");
    assert!(got.contains("\n\n### 6. Heading"), "got: {got:?}");
  }

  #[test]
  fn injects_newline_before_inline_h2() {
    let got = normalize_markdown("intro ## Section follows");
    assert!(got.contains("\n\n## Section"), "got: {got:?}");
  }

  #[test]
  fn injects_newline_before_numbered_bold_item() {
    let got =
      normalize_markdown("intro 1. **First**: thing 2. **Second**: thing");
    assert!(got.contains("\n1. **First**"), "got: {got:?}");
    assert!(got.contains("\n2. **Second**"), "got: {got:?}");
  }

  #[test]
  fn leaves_already_well_formatted_markdown_alone() {
    let src = "## Heading\n\n- bullet\n- bullet\n\n1. **First**: foo\n2. **Second**: bar";
    let got = normalize_markdown(src);
    assert!(!got.contains("\n\n\n"), "got: {got:?}");
  }

  #[test]
  fn does_not_split_mid_word() {
    let got = normalize_markdown("abc1. not a list");
    assert_eq!(got, "abc1. not a list");
  }

  #[test]
  fn breaks_inline_bullets_into_renderable_blocks() {
    let got = prepare_assistant_markdown(
      "focus on these topics: - Scalars - Vectors - Matrix operations",
    );
    assert!(
      got.contains("topics:\n- Scalars\n- Vectors\n- Matrix operations"),
      "got: {got:?}"
    );
  }

  #[test]
  fn breaks_inline_numbered_items_into_renderable_blocks() {
    let got =
      prepare_assistant_markdown("start 1. First point 2. Second point");
    assert!(
      got.contains("start\n1. First point\n2. Second point"),
      "got: {got:?}"
    );
  }

  #[test]
  fn leaves_code_block_markers_inside_code_alone() {
    let got = prepare_assistant_markdown("```\na - b 1. not list\n```");
    assert_eq!(got, "```\na - b 1. not list\n```");
  }

  #[test]
  fn stream_chunks_preserve_newlines() {
    let chunks = split_stream_chunks("one\n\n- two");
    let mut out = String::new();
    for chunk in chunks {
      append_stream_chunk(&mut out, &chunk);
    }
    assert_eq!(out, "one\n\n- two");
  }

  #[test]
  fn user_block_owns_vertical_spacing_and_full_width() {
    let theme = Theme::dark();
    let lines = render_user_message("hello world", 20, &theme);
    assert_eq!(rendered_width(&lines[0]), 20);
    assert_eq!(rendered_width(lines.last().unwrap()), 20);
    assert_eq!(rendered_width(&lines[1]), 20);
    assert!(lines[0].spans.iter().all(|span| span.style.bg.is_some()));
    assert!(lines
      .last()
      .unwrap()
      .spans
      .iter()
      .all(|span| span.style.bg.is_some()));
  }

  #[test]
  fn assistant_message_owns_unhighlighted_vertical_spacing() {
    let theme = Theme::dark();
    let lines = render_assistant_message("hello world", 20, false, &theme);
    assert_eq!(lines.first().unwrap().spans.len(), 0);
    assert_eq!(lines.last().unwrap().spans.len(), 0);
    assert!(lines[1].spans.iter().all(|span| span.style.bg.is_none()));
  }

  fn rendered_width(line: &ratatui::text::Line<'static>) -> usize {
    line.spans.iter().map(|span| span.content.chars().count()).sum()
  }
}
