use pulldown_cmark::{
  CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};
use ratatui::{
  style::{Color, Modifier, Style},
  text::{Line, Span},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockKind {
  Paragraph,
  Heading(HeadingLevel),
  ListItem,
}

#[derive(Clone, Debug)]
struct InlineBlock {
  kind: BlockKind,
  runs: Vec<StyledRun>,
  first_prefix: String,
  continuation_prefix: String,
}

#[derive(Clone, Debug)]
struct CodeBlock {
  lines: Vec<String>,
}

#[derive(Clone, Debug)]
enum MarkdownBlock {
  Inline(InlineBlock),
  Code(CodeBlock),
  Rule,
}

#[derive(Clone, Debug)]
struct ListState {
  ordered: bool,
  next_index: usize,
}

#[derive(Clone, Debug)]
struct RenderContext {
  current_inline: Option<InlineBlock>,
  current_code: Option<CodeBlock>,
  blocks: Vec<MarkdownBlock>,
  list_stack: Vec<ListState>,
  blockquote_depth: usize,
  emphasis_depth: usize,
  strong_depth: usize,
  link_depth: usize,
}

impl RenderContext {
  fn new() -> Self {
    Self {
      current_inline: None,
      current_code: None,
      blocks: Vec::new(),
      list_stack: Vec::new(),
      blockquote_depth: 0,
      emphasis_depth: 0,
      strong_depth: 0,
      link_depth: 0,
    }
  }

  fn ensure_inline_block(&mut self, kind: BlockKind) {
    if self.current_inline.is_some() {
      return;
    }

    let indent = "  ".repeat(self.blockquote_depth);
    let (first_prefix, continuation_prefix) = match kind {
      BlockKind::ListItem => {
        let nested_indent =
          "  ".repeat(self.list_stack.len().saturating_sub(1));
        let marker = if let Some(top) = self.list_stack.last_mut() {
          if top.ordered {
            let text = format!("{}. ", top.next_index);
            top.next_index += 1;
            text
          } else {
            "• ".to_string()
          }
        } else {
          "• ".to_string()
        };
        let first = format!("{indent}{nested_indent}{marker}");
        let cont = format!(
          "{indent}{nested_indent}{}",
          " ".repeat(marker.chars().count())
        );
        (first, cont)
      }
      _ => {
        let prefix = indent;
        (prefix.clone(), prefix)
      }
    };

    self.current_inline = Some(InlineBlock {
      kind,
      runs: Vec::new(),
      first_prefix,
      continuation_prefix,
    });
  }

  fn push_text(&mut self, text: &str, style: Style) {
    if text.is_empty() {
      return;
    }

    if let Some(code) = self.current_code.as_mut() {
      append_code_text(code, text);
      return;
    }

    if self.current_inline.is_none() {
      self.ensure_inline_block(BlockKind::Paragraph);
    }

    if let Some(block) = self.current_inline.as_mut() {
      push_styled_text(&mut block.runs, text, style);
    }
  }

  fn flush_inline(&mut self) {
    if let Some(block) = self.current_inline.take() {
      if !block.runs.is_empty() || block.kind == BlockKind::ListItem {
        self.blocks.push(MarkdownBlock::Inline(block));
      }
    }
  }

  fn flush_code(&mut self) {
    if let Some(mut code) = self.current_code.take() {
      if code.lines.last().is_some_and(|line| line.is_empty()) {
        code.lines.pop();
      }
      self.blocks.push(MarkdownBlock::Code(code));
    }
  }
}

#[derive(Clone, Debug)]
pub struct StyledRun {
  pub text: String,
  pub style: Style,
}

#[derive(Clone, Debug)]
pub struct RenderedLine {
  pub runs: Vec<StyledRun>,
  pub pannable: bool,
}

#[derive(Clone, Debug)]
pub struct MarkdownRenderCache {
  pub wrap_width: usize,
  pub lines: Vec<RenderedLine>,
  pub has_pannable_lines: bool,
}

pub fn render_markdown(
  content: &str,
  wrap_width: usize,
) -> MarkdownRenderCache {
  let width = wrap_width.max(20);
  let mut ctx = RenderContext::new();
  let parser = Parser::new_ext(content, Options::all());

  for event in parser {
    match event {
      Event::Start(tag) => match tag {
        Tag::Paragraph => ctx.ensure_inline_block(BlockKind::Paragraph),
        Tag::Heading { level, .. } => {
          ctx.ensure_inline_block(BlockKind::Heading(level))
        }
        Tag::List(start) => ctx.list_stack.push(ListState {
          ordered: start.is_some(),
          next_index: start.unwrap_or(1) as usize,
        }),
        Tag::Item => ctx.ensure_inline_block(BlockKind::ListItem),
        Tag::BlockQuote(_) => ctx.blockquote_depth += 1,
        Tag::CodeBlock(CodeBlockKind::Indented) => {
          ctx.flush_inline();
          ctx.current_code = Some(CodeBlock { lines: vec![String::new()] });
        }
        Tag::CodeBlock(CodeBlockKind::Fenced(_)) => {
          ctx.flush_inline();
          ctx.current_code = Some(CodeBlock { lines: vec![String::new()] });
        }
        Tag::Emphasis => ctx.emphasis_depth += 1,
        Tag::Strong => ctx.strong_depth += 1,
        Tag::Link { .. } => ctx.link_depth += 1,
        _ => {}
      },
      Event::End(tag_end) => match tag_end {
        TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item => {
          ctx.flush_inline()
        }
        TagEnd::List(_) => {
          ctx.flush_inline();
          ctx.list_stack.pop();
        }
        TagEnd::BlockQuote(_) => {
          ctx.flush_inline();
          ctx.blockquote_depth = ctx.blockquote_depth.saturating_sub(1);
        }
        TagEnd::CodeBlock => ctx.flush_code(),
        TagEnd::Emphasis => {
          ctx.emphasis_depth = ctx.emphasis_depth.saturating_sub(1)
        }
        TagEnd::Strong => ctx.strong_depth = ctx.strong_depth.saturating_sub(1),
        TagEnd::Link => ctx.link_depth = ctx.link_depth.saturating_sub(1),
        _ => {}
      },
      Event::Text(text) => ctx.push_text(&text, inline_style(&ctx)),
      Event::Code(text) => {
        ctx.push_text(
          &text,
          Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        );
      }
      Event::SoftBreak => ctx.push_text(" ", inline_style(&ctx)),
      Event::HardBreak => ctx.push_text("\n", inline_style(&ctx)),
      Event::Rule => {
        ctx.flush_inline();
        ctx.flush_code();
        ctx.blocks.push(MarkdownBlock::Rule);
      }
      Event::Html(html) => {
        let rendered = html_fragment_to_text(&html, false);
        ctx.push_text(&rendered, inline_style(&ctx));
      }
      Event::InlineHtml(html) => {
        let rendered = html_fragment_to_text(&html, true);
        ctx.push_text(&rendered, inline_style(&ctx));
      }
      Event::TaskListMarker(checked) => {
        let marker = if checked { "[x] " } else { "[ ] " };
        ctx.push_text(marker, inline_style(&ctx));
      }
      _ => {}
    }
  }

  ctx.flush_inline();
  ctx.flush_code();

  let mut lines = Vec::new();
  let mut has_pannable_lines = false;

  for (idx, block) in ctx.blocks.iter().enumerate() {
    match block {
      MarkdownBlock::Inline(block) => {
        lines.extend(render_inline_block(block, width));
      }
      MarkdownBlock::Code(block) => {
        let block_lines = render_code_block(block);
        has_pannable_lines |= !block_lines.is_empty();
        lines.extend(block_lines);
      }
      MarkdownBlock::Rule => {
        lines.push(RenderedLine {
          runs: vec![StyledRun {
            text: "─".repeat(width),
            style: Style::default().fg(Color::DarkGray),
          }],
          pannable: false,
        });
      }
    }

    if idx + 1 < ctx.blocks.len()
      && !matches!(
        (&ctx.blocks[idx], &ctx.blocks[idx + 1]),
        (
          MarkdownBlock::Inline(InlineBlock { kind: BlockKind::ListItem, .. }),
          MarkdownBlock::Inline(InlineBlock { kind: BlockKind::ListItem, .. })
        )
      )
    {
      lines.push(RenderedLine { runs: Vec::new(), pannable: false });
    }
  }

  MarkdownRenderCache { wrap_width: width, lines, has_pannable_lines }
}

pub fn line_to_ratatui(
  line: &RenderedLine,
  h_offset: usize,
  max_width: usize,
) -> Line<'static> {
  let runs = if line.pannable {
    crop_runs(&line.runs, h_offset, max_width)
  } else {
    line
      .runs
      .iter()
      .map(|run| Span::styled(run.text.clone(), run.style))
      .collect()
  };
  Line::from(runs)
}

fn render_inline_block(block: &InlineBlock, width: usize) -> Vec<RenderedLine> {
  let mut lines = Vec::new();
  let tokens = tokenize_runs(&block.runs);
  let first_prefix = prefix_runs(block);
  let continuation_prefix = continuation_prefix_runs(block);
  let mut current = first_prefix.clone();
  let first_width = runs_width(&first_prefix);
  let cont_width = runs_width(&continuation_prefix);
  let mut current_width = first_width;
  let mut pending_spaces: Vec<StyledRun> = Vec::new();
  let mut pending_space_width = 0usize;
  let mut prefix_width = first_width;
  let available_first = width.max(first_width + 1);
  let available_cont = width.max(cont_width + 1);

  for token in tokens {
    match token.kind {
      TokenKind::Newline => {
        lines.push(RenderedLine { runs: current, pannable: false });
        current = continuation_prefix.clone();
        current_width = cont_width;
        prefix_width = cont_width;
        pending_spaces.clear();
        pending_space_width = 0;
      }
      TokenKind::Space => {
        if current_width > prefix_width {
          pending_space_width += token.width;
          pending_spaces.extend(token.runs);
        }
      }
      TokenKind::Word => {
        let fits_current =
          current_width + pending_space_width + token.width <= width;
        if fits_current {
          if !pending_spaces.is_empty() {
            current.extend(pending_spaces.drain(..));
            current_width += pending_space_width;
            pending_space_width = 0;
          }
          current.extend(token.runs);
          current_width += token.width;
          continue;
        }

        if current_width > prefix_width {
          lines.push(RenderedLine { runs: current, pannable: false });
          current = continuation_prefix.clone();
          current_width = cont_width;
          prefix_width = cont_width;
        }

        pending_spaces.clear();
        pending_space_width = 0;

        let mut remaining = token.runs;
        while !remaining.is_empty() {
          let limit = if lines.is_empty() && current_width == first_width {
            available_first
          } else {
            available_cont
          };
          let room = limit.saturating_sub(current_width).max(1);
          let (head, tail) = split_runs_at_width(remaining, room);
          current.extend(head);
          current_width = runs_width(&current);
          if tail.is_empty() {
            remaining = tail;
          } else {
            lines.push(RenderedLine { runs: current, pannable: false });
            current = continuation_prefix.clone();
            current_width = cont_width;
            prefix_width = cont_width;
            remaining = tail;
          }
        }
      }
    }
  }

  if !current.is_empty() {
    lines.push(RenderedLine { runs: current, pannable: false });
  }

  lines
}

fn render_code_block(block: &CodeBlock) -> Vec<RenderedLine> {
  if block.lines.is_empty() {
    return vec![RenderedLine { runs: Vec::new(), pannable: true }];
  }

  block
    .lines
    .iter()
    .map(|line| RenderedLine {
      runs: vec![StyledRun {
        text: line.clone(),
        style: Style::default().fg(Color::Green),
      }],
      pannable: true,
    })
    .collect()
}

fn prefix_runs(block: &InlineBlock) -> Vec<StyledRun> {
  let mut runs = Vec::new();
  if !block.first_prefix.is_empty() {
    let style = match block.kind {
      BlockKind::ListItem => Style::default().fg(Color::Cyan),
      _ => Style::default(),
    };
    runs.push(StyledRun { text: block.first_prefix.clone(), style });
  }
  runs
}

fn continuation_prefix_runs(block: &InlineBlock) -> Vec<StyledRun> {
  if block.continuation_prefix.is_empty() {
    return Vec::new();
  }
  vec![StyledRun {
    text: block.continuation_prefix.clone(),
    style: Style::default(),
  }]
}

fn inline_style(ctx: &RenderContext) -> Style {
  let mut style = Style::default();
  if ctx.emphasis_depth > 0 {
    style = style.add_modifier(Modifier::ITALIC);
  }
  if ctx.strong_depth > 0 {
    style = style.add_modifier(Modifier::BOLD);
  }
  if ctx.link_depth > 0 {
    style = style.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED);
  }

  if let Some(block) = ctx.current_inline.as_ref() {
    if let BlockKind::Heading(level) = block.kind {
      style = style.add_modifier(Modifier::BOLD).fg(match level {
        HeadingLevel::H1 => Color::Yellow,
        HeadingLevel::H2 => Color::White,
        _ => Color::Cyan,
      });
    }
  }

  style
}

fn push_styled_text(runs: &mut Vec<StyledRun>, text: &str, style: Style) {
  if text.is_empty() {
    return;
  }

  if let Some(last) = runs.last_mut()
    && last.style == style
  {
    last.text.push_str(text);
    return;
  }

  runs.push(StyledRun { text: text.to_string(), style });
}

fn append_code_text(block: &mut CodeBlock, text: &str) {
  if block.lines.is_empty() {
    block.lines.push(String::new());
  }

  for ch in text.chars() {
    if ch == '\n' {
      block.lines.push(String::new());
    } else if let Some(line) = block.lines.last_mut() {
      line.push(ch);
    }
  }
}

fn html_fragment_to_text(fragment: &str, inline: bool) -> String {
  match html2text::from_read(fragment.as_bytes(), 10_000) {
    Ok(text) => {
      if inline {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
      } else {
        text.trim_end_matches('\n').to_string()
      }
    }
    Err(_) => strip_basic_html(fragment),
  }
}

fn strip_basic_html(fragment: &str) -> String {
  let mut out = String::new();
  let mut in_tag = false;
  for ch in fragment.chars() {
    match ch {
      '<' => in_tag = true,
      '>' => in_tag = false,
      _ if !in_tag => out.push(ch),
      _ => {}
    }
  }
  out
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TokenKind {
  Word,
  Space,
  Newline,
}

#[derive(Clone, Debug)]
struct Token {
  kind: TokenKind,
  runs: Vec<StyledRun>,
  width: usize,
}

fn tokenize_runs(runs: &[StyledRun]) -> Vec<Token> {
  let mut tokens = Vec::new();
  let mut current_kind: Option<TokenKind> = None;
  let mut current_runs: Vec<StyledRun> = Vec::new();
  let mut current_width = 0usize;

  let flush = |tokens: &mut Vec<Token>,
               current_kind: &mut Option<TokenKind>,
               current_runs: &mut Vec<StyledRun>,
               current_width: &mut usize| {
    if let Some(kind) = current_kind.take() {
      tokens.push(Token {
        kind,
        runs: std::mem::take(current_runs),
        width: *current_width,
      });
      *current_width = 0;
    }
  };

  for run in runs {
    let mut buffer = String::new();
    let mut buffer_kind: Option<TokenKind> = None;
    for ch in run.text.chars() {
      let next_kind = if ch == '\n' {
        TokenKind::Newline
      } else if ch.is_whitespace() {
        TokenKind::Space
      } else {
        TokenKind::Word
      };

      if buffer_kind.as_ref() != Some(&next_kind) {
        if let Some(kind) = buffer_kind.take() {
          push_token_piece(
            &mut tokens,
            &mut current_kind,
            &mut current_runs,
            &mut current_width,
            kind,
            std::mem::take(&mut buffer),
            run.style,
          );
        }
        buffer_kind = Some(next_kind);
      }
      buffer.push(ch);
    }

    if let Some(kind) = buffer_kind.take() {
      push_token_piece(
        &mut tokens,
        &mut current_kind,
        &mut current_runs,
        &mut current_width,
        kind,
        buffer,
        run.style,
      );
    }
  }

  flush(&mut tokens, &mut current_kind, &mut current_runs, &mut current_width);

  tokens
}

fn push_token_piece(
  tokens: &mut Vec<Token>,
  current_kind: &mut Option<TokenKind>,
  current_runs: &mut Vec<StyledRun>,
  current_width: &mut usize,
  kind: TokenKind,
  text: String,
  style: Style,
) {
  if text.is_empty() {
    return;
  }

  if current_kind.as_ref() != Some(&kind) {
    if let Some(existing_kind) = current_kind.take() {
      tokens.push(Token {
        kind: existing_kind,
        runs: std::mem::take(current_runs),
        width: *current_width,
      });
      *current_width = 0;
    }
    *current_kind = Some(kind.clone());
  }

  *current_width += text.chars().count();
  if let Some(last) = current_runs.last_mut()
    && last.style == style
  {
    last.text.push_str(&text);
  } else {
    current_runs.push(StyledRun { text, style });
  }
}

fn split_runs_at_width(
  runs: Vec<StyledRun>,
  max_width: usize,
) -> (Vec<StyledRun>, Vec<StyledRun>) {
  let mut taken = Vec::new();
  let mut remaining = Vec::new();
  let mut width = 0usize;

  for run in runs {
    if width >= max_width {
      remaining.push(run);
      continue;
    }

    let run_width = run.text.chars().count();
    if width + run_width <= max_width {
      width += run_width;
      taken.push(run);
      continue;
    }

    let keep = max_width.saturating_sub(width);
    let head: String = run.text.chars().take(keep).collect();
    let tail: String = run.text.chars().skip(keep).collect();
    if !head.is_empty() {
      taken.push(StyledRun { text: head, style: run.style });
    }
    if !tail.is_empty() {
      remaining.push(StyledRun { text: tail, style: run.style });
    }
    width = max_width;
  }

  (taken, remaining)
}

fn runs_width(runs: &[StyledRun]) -> usize {
  runs.iter().map(|run| run.text.chars().count()).sum()
}

fn crop_runs(
  runs: &[StyledRun],
  h_offset: usize,
  max_width: usize,
) -> Vec<Span<'static>> {
  if max_width == 0 {
    return Vec::new();
  }

  let visible_start = h_offset;
  let visible_end = h_offset + max_width;
  let mut spans = Vec::new();
  let mut current = 0usize;

  for run in runs {
    let run_width = run.text.chars().count();
    let start = current;
    let end = current + run_width;
    current = end;

    let s = start.max(visible_start);
    let e = end.min(visible_end);
    if s >= e {
      continue;
    }

    let local_start = s - start;
    let local_end = e - start;
    let text: String = run
      .text
      .chars()
      .skip(local_start)
      .take(local_end - local_start)
      .collect();
    if !text.is_empty() {
      spans.push(Span::styled(text, run.style));
    }
  }

  spans
}
