use std::mem;
use std::sync::mpsc;

use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
  layout::{Alignment, Constraint, Direction, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span, Text},
  widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
  Frame,
};
use ui_theme::Theme;

use crate::{
  provider::ProviderResponse,
  provider_registry::{parse_provider_prefix, ProviderRegistry},
  storage::{
    create_session, delete_session, load_index, load_session, save_index,
    save_session,
  },
  ChatIndex, ChatMessage, ChatSession, ChatSessionMeta, Role,
};

mod render;
use render::{
  append_stream_chunk, backspace_at_cursor, centered_rect, compute_cost_and_ctx,
  fmt_tokens, message_gap_needed, parse_api_error, render_assistant_message,
  render_user_message, sanitize_content, split_stream_chunks, step_cursor_back,
  step_cursor_forward, truncate_for_width,
};

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatUiState {
  SessionList,
  Chat,
  NewSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatAction {
  None,
  Quit,
  Sending,
  SlashCommand(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSlashCommandSpec {
  pub command: String,
  pub completion: String,
  pub description: String,
  /// Short category label shown in the palette, e.g. "disc", "src".
  pub badge: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatInputMode {
  Insert,
  Normal,
}

pub struct ChatUi {
  pub state: ChatUiState,
  pub sessions: Vec<ChatSessionMeta>,
  pub session_list_state: ListState,
  pub active_session: Option<ChatSession>,
  pub input: String,
  pub input_cursor: usize,
  pub scroll_offset: usize,
  pub follow_tail: bool,
  pub provider_registry: ProviderRegistry,
  pub default_provider: String,
  pub new_session_input: String,
  pub viewport_height: usize,
  pub pending_response:
    Option<mpsc::Receiver<Result<ProviderResponse, String>>>,
  pub is_loading: bool,
  pub frame_count: u64,
  /// Text chunks remaining to reveal during streaming simulation.
  /// `VecDeque` so the per-tick word reveal pops from the front in O(1)
  /// instead of shifting the entire vector on every tick (the prior
  /// `Vec::remove(0)` was O(N) at ~62Hz × N words remaining).
  pub streaming_words: std::collections::VecDeque<String>,
  /// True while word-by-word reveal is in progress.
  pub is_streaming: bool,

  /// Cached wrapped + styled lines per message. Key:
  /// `(msg_idx, has_streaming_cursor)` — one entry per message. Cache hit
  /// compares `content_len` against current to detect streaming-append
  /// staleness; mismatch falls through to re-render and overwrites the
  /// same key. Cleared on resize and on session switch.
  line_cache: std::collections::HashMap<(usize, bool), CachedRender>,
  /// Width the cache was built for. Diverges from current width on resize;
  /// triggers a full cache clear.
  line_cache_width: usize,
  /// Session id the cache was built for. Diverges on session switch;
  /// triggers a full cache clear.
  line_cache_session_id: Option<String>,
  /// Vim-style input mode for the chat pane.
  pub input_mode: ChatInputMode,
  /// Selected row in the slash-command suggestion palette.
  pub slash_selected: usize,
  /// Top visible row in the slash-command suggestion palette.
  pub slash_scroll: usize,
  pub slash_commands: Vec<ChatSlashCommandSpec>,
}

/// Single entry in `ChatUi.line_cache`. Stores the rendered lines for a
/// message at a particular `content_len`; on cache lookup the caller
/// compares its current content length against `content_len` to decide
/// whether the cache is fresh.
struct CachedRender {
  content_len: usize,
  lines: Vec<ratatui::text::Line<'static>>,
}

// ── Construction ─────────────────────────────────────────────────────────────

impl ChatUi {
  pub fn new(
    registry: ProviderRegistry,
    default_provider: String,
    slash_commands: Vec<ChatSlashCommandSpec>,
  ) -> Self {
    let index = load_index();
    let sessions = index.sessions;
    let mut session_list_state = ListState::default();
    if !sessions.is_empty() {
      session_list_state.select(Some(0));
    }
    Self {
      state: ChatUiState::SessionList,
      sessions,
      session_list_state,
      active_session: None,
      input: String::new(),
      input_cursor: 0,
      scroll_offset: 0,
      follow_tail: true,
      provider_registry: registry,
      default_provider,
      new_session_input: String::new(),
      viewport_height: 20,
      pending_response: None,
      is_loading: false,
      frame_count: 0,
      streaming_words: std::collections::VecDeque::new(),
      is_streaming: false,
      line_cache: std::collections::HashMap::new(),
      line_cache_width: 0,
      line_cache_session_id: None,
      input_mode: ChatInputMode::Insert,
      slash_selected: 0,
      slash_scroll: 0,
      slash_commands,
    }
  }

  /// Returns (session_title, provider_name, model_name) all owned.
  /// Owned strings are required because `draw_chat_view` holds them
  /// across mid-function `&mut self` mutations (viewport_height,
  /// scroll_offset, clamp_slash_scroll); the mutations depend on layout
  /// values computed mid-function so they can't be hoisted.
  pub fn workspace_summary(&self) -> (String, String, String) {
    let provider_name = self
      .active_session
      .as_ref()
      .and_then(|s| s.provider.as_deref().map(|p| p.to_string()))
      .unwrap_or_else(|| self.default_provider.clone());
    let model_name = self
      .provider_registry
      .get(&provider_name)
      .map(|p| p.model().to_string())
      .unwrap_or_else(|| "unknown".to_string());
    let session_title = self
      .active_session
      .as_ref()
      .map(|s| s.title.clone())
      .unwrap_or_else(|| "chat".to_string());
    (session_title, provider_name, model_name)
  }
}

// ── Tick (called each frame by host) ─────────────────────────────────────────

impl ChatUi {
  pub fn tick(&mut self) {
    self.frame_count = self.frame_count.wrapping_add(1);

    // Word-by-word streaming reveal: one word per tick (~16ms each).
    if self.is_streaming {
      if self.streaming_words.is_empty() {
        self.is_streaming = false;
        if let Some(session) = self.active_session.as_ref() {
          let _ = save_session(session);
          let meta = crate::storage::session_to_meta(session);
          let id = meta.id.clone();
          if let Some(pos) = self.sessions.iter().position(|s| s.id == id) {
            self.sessions[pos] = meta;
          }
          self.sync_index();
        }
      } else if let Some(word) = self.streaming_words.pop_front() {
        if let Some(session) = self.active_session.as_mut() {
          if let Some(last_msg) = session.messages.last_mut() {
            append_stream_chunk(&mut last_msg.content, &word);
          }
        }
        if self.follow_tail {
          self.scroll_offset = usize::MAX;
        }
      }
      return;
    }

    let Some(rx) = self.pending_response.as_ref() else {
      return;
    };
    let result = match rx.try_recv() {
      Ok(r) => Some(Ok(r)),
      Err(mpsc::TryRecvError::Empty) => None,
      Err(mpsc::TryRecvError::Disconnected) => {
        Some(Err("thread disconnected".to_string()))
      }
    };

    match result {
      None => {}
      Some(inner) => {
        self.pending_response = None;
        self.is_loading = false;

        let response = match inner {
          Ok(Ok(resp)) => ProviderResponse {
            content: sanitize_content(&resp.content),
            ..resp
          },
          Ok(Err(e)) => ProviderResponse {
            content: parse_api_error(&e),
            input_tokens: 0,
            output_tokens: 0,
          },
          Err(e) => ProviderResponse {
            content: format!("thread error — {e}"),
            input_tokens: 0,
            output_tokens: 0,
          },
        };

        log::debug!(
          "chat: response received ({} chars, {}↑ {}↓ tokens)",
          response.content.len(),
          response.input_tokens,
          response.output_tokens
        );

        if let Some(session) = self.active_session.as_mut() {
          session.total_input_tokens += response.input_tokens;
          session.total_output_tokens += response.output_tokens;
        }

        // Split into whitespace-preserving chunks for streaming reveal.
        let words: std::collections::VecDeque<String> =
          split_stream_chunks(&response.content);

        // Push a placeholder assistant message (content will fill as we stream).
        if let Some(session) = self.active_session.as_mut() {
          session.messages.push(ChatMessage {
            role: Role::Assistant,
            content: String::new(),
            timestamp: Utc::now(),
          });
          session.updated_at = Utc::now();
        }

        if words.is_empty() {
          if let Some(session) = self.active_session.as_ref() {
            let _ = save_session(session);
          }
        } else {
          self.streaming_words = words;
          self.is_streaming = true;
        }

        self.follow_tail = true;
        self.scroll_offset = usize::MAX;
      }
    }
  }
}

// ── Top-level draw / handle_key ───────────────────────────────────────────────

impl ChatUi {
  pub fn draw(&mut self, frame: &mut Frame, area: Rect, t: &Theme) {
    self.draw_with_context(frame, area, t, None);
  }

  pub fn draw_with_context(
    &mut self,
    frame: &mut Frame,
    area: Rect,
    t: &Theme,
    context: Option<&str>,
  ) {
    match self.state {
      // Session list and new-session overlay both draw on top of the
      // chat background — always render the chat background first.
      ChatUiState::SessionList => {
        self.draw_chat_background(frame, area, t);
        self.draw_session_list(frame, area, t);
      }
      ChatUiState::NewSession => {
        self.draw_chat_background(frame, area, t);
        self.draw_session_list(frame, area, t);
        self.draw_new_session_overlay(frame, area, t);
      }
      ChatUiState::Chat => self.draw_chat(frame, area, t, context),
    }
  }

  /// Returns true only when the chat conversation pane is open and needs a
  /// dedicated row of screen space.  False when showing the session list or
  /// new-session overlay (those float over the main layout).
  pub fn needs_panel(&self) -> bool {
    self.state == ChatUiState::Chat
  }

  /// Render the session-list (or new-session) as a floating popup over the
  /// given area (normally the full terminal rect).  No background panel.
  pub fn draw_overlay(&mut self, frame: &mut Frame, area: Rect, t: &Theme) {
    match self.state {
      ChatUiState::SessionList => self.draw_session_list(frame, area, t),
      ChatUiState::NewSession => {
        self.draw_session_list(frame, area, t);
        self.draw_new_session_overlay(frame, area, t);
      }
      _ => {}
    }
  }

  pub fn handle_key(&mut self, key: KeyEvent) -> ChatAction {
    match self.state {
      ChatUiState::SessionList => self.handle_session_list_key(key),
      ChatUiState::NewSession => self.handle_new_session_key(key),
      ChatUiState::Chat => self.handle_chat_key(key),
    }
  }
}

// ── Background ────────────────────────────────────────────────────────────────

impl ChatUi {
  fn draw_chat_background(&self, frame: &mut Frame, area: Rect, t: &Theme) {
    let bg = Block::default().style(Style::default().bg(t.bg_chat));
    frame.render_widget(bg, area);
    // Top separator line.
    frame.render_widget(
      Paragraph::new("─".repeat(area.width as usize))
        .style(Style::default().fg(t.border).bg(t.bg_chat)),
      Rect { x: area.x, y: area.y, width: area.width, height: 1 },
    );
  }
}

// ── Session list ──────────────────────────────────────────────────────────────

impl ChatUi {
  fn draw_session_list(&mut self, frame: &mut Frame, area: Rect, t: &Theme) {
    let popup_w = (area.width as u32 * 60 / 100).max(30) as u16;
    // spec: min(session_count + 4, 12)
    let popup_h = ((self.sessions.len() as u16 + 4).min(12)).max(3);
    // Centered horizontally; bottom-anchored above footer (3 rows) with 2
    // rows of clearance: y = terminal_height - popup_height - footer_height - 2
    let footer_h: u16 = 3;
    let x = area.x + area.width.saturating_sub(popup_w) / 2;
    let y =
      area.y.saturating_add(area.height.saturating_sub(popup_h + footer_h + 2));
    let popup_rect = Rect::new(
      x,
      y,
      popup_w.min(area.width),
      popup_h.min(area.height.saturating_sub(footer_h + 2)),
    );

    frame.render_widget(Clear, popup_rect);

    let items: Vec<ListItem> = self
      .sessions
      .iter()
      .map(|s| {
        let date = s.updated_at.format("%Y-%m-%d").to_string();
        let provider = s.provider.as_deref().unwrap_or("default");
        let line = Line::from(vec![
          Span::styled(
            s.title.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
          ),
          Span::styled(
            format!("  {}  [{}]", date, provider),
            Style::default().fg(t.text_dim),
          ),
        ]);
        ListItem::new(line)
      })
      .collect();

    let list = List::new(items)
      .block(
        Block::default()
          .borders(Borders::ALL)
          .border_style(Style::default().fg(t.border))
          .style(Style::default().bg(t.bg_panel))
          .title(Span::styled(
            " ── sessions ── ",
            Style::default().fg(t.header),
          ))
          .title_alignment(Alignment::Center),
      )
      .highlight_style(t.style_selection_text())
      .highlight_symbol("  ");

    frame.render_stateful_widget(
      list,
      popup_rect,
      &mut self.session_list_state,
    );
  }

  fn handle_session_list_key(&mut self, key: KeyEvent) -> ChatAction {
    match key.code {
      KeyCode::Esc => ChatAction::Quit,

      KeyCode::Char('n') => {
        self.new_session_input.clear();
        self.state = ChatUiState::NewSession;
        ChatAction::None
      }

      KeyCode::Enter => {
        if let Some(idx) = self.session_list_state.selected() {
          if let Some(meta) = self.sessions.get(idx) {
            let id = meta.id.clone();
            if let Some(session) = load_session(&id) {
              self.active_session = Some(session);
              self.follow_tail = true;
              self.scroll_offset = usize::MAX;
              self.input_mode = ChatInputMode::Insert;
              self.state = ChatUiState::Chat;
            }
          }
        }
        ChatAction::None
      }

      KeyCode::Char('d') => {
        if let Some(idx) = self.session_list_state.selected() {
          if idx < self.sessions.len() {
            let id = self.sessions[idx].id.clone();
            let _ = delete_session(&id);
            self.sessions.remove(idx);
            self.sync_index();
            let new_sel = if self.sessions.is_empty() {
              None
            } else {
              Some(idx.min(self.sessions.len() - 1))
            };
            self.session_list_state.select(new_sel);
          }
        }
        ChatAction::None
      }

      KeyCode::Char('j') | KeyCode::Down => {
        let len = self.sessions.len();
        if len > 0 {
          let next = self
            .session_list_state
            .selected()
            .map(|i| (i + 1).min(len - 1))
            .unwrap_or(0);
          self.session_list_state.select(Some(next));
        }
        ChatAction::None
      }

      KeyCode::Char('k') | KeyCode::Up => {
        if !self.sessions.is_empty() {
          let prev = self
            .session_list_state
            .selected()
            .map(|i| i.saturating_sub(1))
            .unwrap_or(0);
          self.session_list_state.select(Some(prev));
        }
        ChatAction::None
      }

      _ => ChatAction::None,
    }
  }
}

// ── New session overlay ───────────────────────────────────────────────────────

impl ChatUi {
  fn draw_new_session_overlay(&self, frame: &mut Frame, area: Rect, t: &Theme) {
    let overlay = centered_rect(50, 3, area);
    frame.render_widget(Clear, overlay);

    let input_display = format!("{}_", self.new_session_input);
    let para = Paragraph::new(input_display)
      .block(
        Block::default()
          .borders(Borders::ALL)
          .border_style(Style::default().fg(t.border))
          .style(Style::default().bg(t.bg_panel))
          .title(" new session (enter: confirm  esc: cancel) "),
      )
      .style(Style::default().fg(t.text));
    frame.render_widget(para, overlay);
  }

  fn handle_new_session_key(&mut self, key: KeyEvent) -> ChatAction {
    match key.code {
      KeyCode::Esc => {
        self.state = ChatUiState::SessionList;
        ChatAction::None
      }

      KeyCode::Enter => {
        let title = if self.new_session_input.trim().is_empty() {
          "New conversation".to_string()
        } else {
          mem::take(&mut self.new_session_input)
        };
        let session = create_session(title, None);
        let meta = crate::storage::session_to_meta(&session);
        let _ = save_session(&session);
        self.sessions.push(meta);
        self.sync_index();
        let new_idx = self.sessions.len() - 1;
        self.session_list_state.select(Some(new_idx));
        self.active_session = Some(session);
        self.input.clear();
        self.input_cursor = 0;
        self.follow_tail = true;
        self.scroll_offset = 0;
        self.input_mode = ChatInputMode::Insert;
        self.state = ChatUiState::Chat;
        ChatAction::None
      }

      KeyCode::Backspace => {
        self.new_session_input.pop();
        ChatAction::None
      }

      KeyCode::Char(c) => {
        self.new_session_input.push(c);
        ChatAction::None
      }

      _ => ChatAction::None,
    }
  }
}

// ── Chat view ─────────────────────────────────────────────────────────────────

impl ChatUi {
  fn draw_chat(
    &mut self,
    frame: &mut Frame,
    area: Rect,
    t: &Theme,
    context: Option<&str>,
  ) {
    let (session_title, provider_name, model_name) = self.workspace_summary();

    // Full background fill.
    frame.render_widget(
      Block::default().style(Style::default().bg(t.bg_chat)),
      area,
    );

    // Layout: separator(1) | header(1) | context(0/1) | messages(fill) | input(3) | status(1)
    let context_h = if context.is_some() { 1 } else { 0 };
    let chunks = Layout::default()
      .direction(Direction::Vertical)
      .constraints([
        Constraint::Length(1),         // top separator
        Constraint::Length(1),         // header
        Constraint::Length(context_h), // paper context strip
        Constraint::Min(0),            // message viewport
        Constraint::Length(3),         // input block
        Constraint::Length(1),         // status bar
      ])
      .split(area);

    let sep_area = chunks[0];
    let header_area = chunks[1];
    let context_area = chunks[2];
    let messages_area = chunks[3];
    let input_area = chunks[4];
    let status_area = chunks[5];

    // ── Top separator ──────────────────────────────────────────────
    frame.render_widget(
      Paragraph::new("─".repeat(area.width as usize))
        .style(Style::default().fg(t.border).bg(t.bg_chat)),
      sep_area,
    );

    // ── Header: "── session title ── model · provider ──"
    let model_provider = format!("{model_name} · {provider_name}");
    // Char count, not byte count: paper titles often contain multi-byte
    // characters (em-dash, accents, smart quotes) and `String::len()` would
    // under-count display width, producing visible header-fill misalignment.
    let used = 4
      + session_title.chars().count()
      + 4
      + model_provider.chars().count()
      + 2;
    let fill = (area.width as usize).saturating_sub(used);

    let header_line = Line::from(vec![
      Span::styled("── ", Style::default().fg(t.border)),
      Span::styled(
        session_title,
        Style::default().fg(t.text).add_modifier(Modifier::BOLD),
      ),
      Span::styled(" ── ", Style::default().fg(t.border)),
      Span::styled(model_provider, Style::default().fg(t.text_dim)),
      Span::styled("─".repeat(fill), Style::default().fg(t.border)),
    ]);
    frame.render_widget(
      Paragraph::new(header_line).style(Style::default().bg(t.bg_chat)),
      header_area,
    );

    if let Some(context) = context {
      let max = context_area.width as usize;
      let text = truncate_for_width(context, max.saturating_sub(2));
      let line = Line::from(vec![
        Span::styled(
          "  Discussing: ",
          Style::default().fg(t.text_dim).bg(t.bg_chat),
        ),
        Span::styled(text, Style::default().fg(t.accent).bg(t.bg_chat)),
      ]);
      frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(t.bg_chat)),
        context_area,
      );
    }

    // ── Messages ──────────────────────────────────────────────────
    let messages_content_area = messages_area;
    let width = messages_content_area.width as usize;
    let msg_lines = self.build_message_lines(width, t);
    let total_lines = msg_lines.len();
    let viewport_height = messages_content_area.height as usize;
    self.viewport_height = viewport_height;

    let max_scroll = total_lines.saturating_sub(viewport_height);
    if self.follow_tail || self.scroll_offset > max_scroll {
      self.scroll_offset = max_scroll;
    }
    if self.scroll_offset >= max_scroll {
      self.follow_tail = true;
    }

    frame.render_widget(
      Paragraph::new(Text::from(msg_lines))
        .style(Style::default().bg(t.bg_chat))
        .scroll((self.scroll_offset as u16, 0)),
      messages_content_area,
    );

    self.draw_slash_palette(frame, messages_content_area, t);

    // Scroll indicator — top-right corner when not at bottom.
    if self.scroll_offset < max_scroll && messages_content_area.height > 0 {
      let label = " ↑ more ";
      let lw = label.len() as u16;
      let x = messages_content_area.x
        + messages_content_area.width.saturating_sub(lw);
      frame.render_widget(
        Paragraph::new(label)
          .style(Style::default().fg(t.text_dim).bg(t.bg_chat)),
        Rect { x, y: messages_content_area.y, width: lw, height: 1 },
      );
    }

    // ── Input bar ─────────────────────────────────────────────────
    let input_bg = t.bg_input;
    // Stripe color signals mode: accent = insert (ready to type), dim = normal/loading.
    let stripe_color =
      if self.is_loading || self.input_mode == ChatInputMode::Normal {
        t.text_dim
      } else {
        t.accent
      };
    let stripe =
      Span::styled("│ ", Style::default().fg(stripe_color).bg(input_bg));

    let input_line = if self.is_loading {
      let dots_idx = ((self.frame_count / 8) as usize) % 4;
      let dots = ["·", "··", "···", "··"][dots_idx];
      Line::from(vec![
        stripe,
        Span::styled(
          dots.to_string(),
          Style::default().fg(t.text_dim).bg(input_bg),
        ),
      ])
    } else if self.input_mode == ChatInputMode::Normal {
      let text = if self.input.is_empty() {
        "press i to type  ·  j/k to scroll".to_string()
      } else {
        self.input.clone()
      };
      Line::from(vec![
        stripe,
        Span::styled(text, Style::default().fg(t.text_dim).bg(input_bg)),
      ])
    } else if self.input.is_empty() {
      Line::from(vec![
        stripe,
        Span::styled(
          "Type your message or / for commands",
          Style::default().fg(t.text_dim).bg(input_bg),
        ),
      ])
    } else {
      // Render cursor at input_cursor position so Left/Right/Home/End
      // are visually reflected. split_at on a char boundary is safe
      // because every cursor mutation steps by char.
      let cursor = self.input_cursor.min(self.input.len());
      let (before, after) = self.input.split_at(cursor);
      Line::from(vec![
        stripe,
        Span::styled(
          format!("{before}█{after}"),
          Style::default().fg(t.text).bg(input_bg),
        ),
      ])
    };
    let empty_input_line = Line::from(Span::styled(
      " ".repeat(input_area.width as usize),
      Style::default().bg(input_bg),
    ));
    frame.render_widget(
      Paragraph::new(vec![
        empty_input_line.clone(),
        input_line,
        empty_input_line,
      ])
      .style(Style::default().bg(input_bg)),
      input_area,
    );

    // ── Status bar ────────────────────────────────────────────────
    let status_line = self.build_status_line(
      &provider_name,
      &model_name,
      area.width as usize,
      t,
    );
    frame.render_widget(
      Paragraph::new(status_line).style(Style::default().bg(t.bg_chat)),
      status_area,
    );
  }

  fn build_status_line(
    &self,
    provider_name: &str,
    model_name: &str,
    width: usize,
    t: &Theme,
  ) -> Line<'static> {
    let (in_tok, out_tok) = self
      .active_session
      .as_ref()
      .map(|s| (s.total_input_tokens, s.total_output_tokens))
      .unwrap_or((0, 0));

    if in_tok == 0 && out_tok == 0 {
      return Line::from(vec![]);
    }

    let (cost, ctx_pct, ctx_k) =
      compute_cost_and_ctx(provider_name, model_name, in_tok, out_tok);

    let s = format!(
      "↑{}  ↓{}  ${:.3}  {:.1}%/{}k (auto)  {} · {}",
      fmt_tokens(in_tok),
      fmt_tokens(out_tok),
      cost,
      ctx_pct,
      ctx_k,
      model_name,
      provider_name,
    );
    let s = if s.len() > width { s[..width].to_string() } else { s };

    Line::from(Span::styled(s, Style::default().fg(t.text_dim)))
  }

  fn handle_chat_key(&mut self, key: KeyEvent) -> ChatAction {
    log::debug!(
      "chat: key event {:?} (is_loading={}, mode={:?})",
      key.code,
      self.is_loading,
      self.input_mode
    );

    // Only block keys during the actual API call, not during streaming reveal.
    if self.is_loading {
      if key.code == KeyCode::Esc {
        log::debug!("chat: request cancelled by user");
        self.pending_response = None;
        self.is_loading = false;
      }
      return ChatAction::None;
    }

    match self.input_mode {
      ChatInputMode::Insert => self.handle_chat_key_insert(key),
      ChatInputMode::Normal => self.handle_chat_key_normal(key),
    }
  }

  fn handle_chat_key_insert(&mut self, key: KeyEvent) -> ChatAction {
    match key.code {
      KeyCode::Esc => {
        self.input_mode = ChatInputMode::Normal;
        ChatAction::None
      }

      KeyCode::Enter => {
        if self.complete_slash_on_enter() {
          return ChatAction::None;
        }
        if !self.input.trim().is_empty() {
          self.send_message()
        } else {
          ChatAction::None
        }
      }

      KeyCode::Tab => {
        if self.complete_selected_slash_command() {
          return ChatAction::None;
        }
        ChatAction::None
      }

      KeyCode::Down => {
        if self.move_slash_selection(1) {
          return ChatAction::None;
        }
        ChatAction::None
      }

      KeyCode::Up => {
        if self.move_slash_selection(-1) {
          return ChatAction::None;
        }
        ChatAction::None
      }

      KeyCode::Char('n') if key.modifiers == KeyModifiers::CONTROL => {
        if self.move_slash_selection(1) {
          return ChatAction::None;
        }
        ChatAction::None
      }

      KeyCode::Char('p') if key.modifiers == KeyModifiers::CONTROL => {
        if self.move_slash_selection(-1) {
          return ChatAction::None;
        }
        ChatAction::None
      }

      KeyCode::Left => {
        self.input_cursor = step_cursor_back(&self.input, self.input_cursor);
        ChatAction::None
      }

      KeyCode::Right => {
        self.input_cursor = step_cursor_forward(&self.input, self.input_cursor);
        ChatAction::None
      }

      KeyCode::Home => {
        self.input_cursor = 0;
        ChatAction::None
      }

      KeyCode::End => {
        self.input_cursor = self.input.len();
        ChatAction::None
      }

      KeyCode::Backspace => {
        // Delete the char immediately before the cursor and step the cursor
        // back.
        self.input_cursor =
          backspace_at_cursor(&mut self.input, self.input_cursor);
        self.clamp_slash_selection();
        ChatAction::None
      }

      KeyCode::Char(c) => {
        // Insert at cursor, advance cursor by the char's UTF-8 byte width.
        self.input.insert(self.input_cursor, c);
        self.input_cursor += c.len_utf8();
        self.clamp_slash_selection();
        ChatAction::None
      }

      _ => ChatAction::None,
    }
  }

  fn handle_chat_key_normal(&mut self, key: KeyEvent) -> ChatAction {
    match key.code {
      KeyCode::Esc => {
        self.state = ChatUiState::SessionList;
        self.input.clear();
        self.input_cursor = 0;
        self.input_mode = ChatInputMode::Insert;
        ChatAction::None
      }

      KeyCode::Char('i') | KeyCode::Char('a') => {
        self.input_mode = ChatInputMode::Insert;
        ChatAction::None
      }

      KeyCode::Enter => {
        self.input_mode = ChatInputMode::Insert;
        ChatAction::None
      }

      KeyCode::Char('j') | KeyCode::Down => {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
        ChatAction::None
      }

      KeyCode::Char('k') | KeyCode::Up => {
        self.follow_tail = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
        ChatAction::None
      }

      KeyCode::PageDown => {
        let step = (self.viewport_height / 2).max(1);
        self.scroll_offset = self.scroll_offset.saturating_add(step);
        ChatAction::None
      }

      KeyCode::PageUp => {
        self.follow_tail = false;
        let step = (self.viewport_height / 2).max(1);
        self.scroll_offset = self.scroll_offset.saturating_sub(step);
        ChatAction::None
      }

      _ => ChatAction::None,
    }
  }

  /// Returns borrowed references into `self.slash_commands`. Hot path —
  /// `clamp_slash_selection` invokes this on every keystroke; cloning the
  /// matching specs would mean a fresh Vec per call. Callers only read
  /// `.command`, `.completion`, `.description`, `.badge`.
  fn slash_suggestions(&self) -> Vec<&ChatSlashCommandSpec> {
    let input = self.input.trim_start();
    if !input.starts_with('/') {
      return Vec::new();
    }

    let query = input.to_lowercase();
    self
      .slash_commands
      .iter()
      .filter(|spec| {
        spec.command.starts_with(&query)
          || spec.completion.trim_end().starts_with(&query)
          || query.starts_with(&spec.command)
          || spec.command.contains(query.trim_start_matches('/'))
      })
      .collect()
  }

  fn clamp_slash_selection(&mut self) {
    let len = self.slash_suggestions().len();
    if len == 0 {
      self.slash_selected = 0;
      self.slash_scroll = 0;
    } else if self.slash_selected >= len {
      self.slash_selected = len - 1;
    }
    self.clamp_slash_scroll(len);
  }

  fn move_slash_selection(&mut self, delta: isize) -> bool {
    let len = self.slash_suggestions().len();
    if len == 0 {
      return false;
    }

    let current = self.slash_selected.min(len - 1) as isize;
    self.slash_selected = (current + delta).clamp(0, len as isize - 1) as usize;
    self.clamp_slash_scroll(len);
    true
  }

  fn clamp_slash_scroll(&mut self, len: usize) {
    if len == 0 {
      self.slash_scroll = 0;
      return;
    }

    let viewport = len.min(6);
    if self.slash_selected < self.slash_scroll {
      self.slash_scroll = self.slash_selected;
    } else if self.slash_selected >= self.slash_scroll + viewport {
      self.slash_scroll = self.slash_selected + 1 - viewport;
    }

    let max_scroll = len.saturating_sub(viewport);
    if self.slash_scroll > max_scroll {
      self.slash_scroll = max_scroll;
    }
  }

  fn complete_selected_slash_command(&mut self) -> bool {
    let suggestions = self.slash_suggestions();
    let Some(spec) = suggestions
      .get(self.slash_selected.min(suggestions.len().saturating_sub(1)))
    else {
      return false;
    };
    self.input = spec.completion.clone();
    self.input_cursor = self.input.len();
    true
  }

  fn complete_slash_on_enter(&mut self) -> bool {
    let input = self.input.trim();
    if !input.starts_with('/') || input.contains(' ') {
      return false;
    }

    self.complete_selected_slash_command()
  }

  fn draw_slash_palette(
    &mut self,
    frame: &mut Frame,
    messages_area: Rect,
    t: &Theme,
  ) {
    if self.input_mode != ChatInputMode::Insert || self.is_loading {
      return;
    }

    // Capture the suggestion count first so the borrow on
    // self.slash_commands drops before the &mut self mutations below.
    let suggestions_len = self.slash_suggestions().len();
    if suggestions_len == 0 || messages_area.height == 0 {
      return;
    }

    self.slash_selected = self.slash_selected.min(suggestions_len - 1);

    let visible = suggestions_len.min(6);
    self.clamp_slash_scroll(suggestions_len);

    // Re-fetch the suggestion borrow after the &mut self mutations
    // above. Cheap: the inner Vec just holds &Spec pointers, no clones.
    let suggestions = self.slash_suggestions();

    // 1 separator + visible rows + 1 count line
    let height = (visible as u16 + 2).min(messages_area.height);
    let area = Rect {
      x: messages_area.x,
      y: messages_area.y + messages_area.height.saturating_sub(height),
      width: messages_area.width,
      height,
    };

    frame.render_widget(Clear, area);

    let w = area.width as usize;
    let name_col = 18usize;
    let badge_col = 8usize;
    let desc_col = w.saturating_sub(name_col + badge_col + 4);

    let start = self.slash_scroll;
    let end = (start + visible).min(suggestions_len);

    // Separator line
    let sep_fill = "─".repeat(w.saturating_sub(16));
    let sep_line = Line::from(Span::styled(
      format!("─── Commands ──{sep_fill}"),
      Style::default().fg(t.border),
    ));

    let mut lines: Vec<Line> = vec![sep_line];

    for (i, spec) in
      suggestions.iter().skip(start).take(end - start).enumerate()
    {
      let selected = start + i == self.slash_selected;

      let (arrow, name_style, desc_style) = if selected {
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
      let badge_style = Style::default().fg(t.text_dim);

      let desc: String = spec.description.chars().take(desc_col).collect();

      lines.push(Line::from(vec![
        Span::styled(arrow, Style::default().fg(t.accent)),
        Span::styled(name_padded, name_style),
        Span::styled(badge_padded, badge_style),
        Span::styled(desc, desc_style),
      ]));
    }

    // Count line — right-aligned
    let count_str =
      format!("({}/{})", self.slash_selected + 1, suggestions_len);
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

  fn send_message(&mut self) -> ChatAction {
    // If streaming is in progress, flush all remaining words immediately
    // so the previous message is complete before we send the next one.
    if self.is_streaming {
      let remaining = mem::take(&mut self.streaming_words);
      if let Some(session) = self.active_session.as_mut() {
        if let Some(last_msg) = session.messages.last_mut() {
          for word in &remaining {
            append_stream_chunk(&mut last_msg.content, word);
          }
        }
        let _ = save_session(session);
      }
      self.is_streaming = false;
    }

    let raw_input = mem::take(&mut self.input);
    self.input_cursor = 0;

    if raw_input.starts_with('/') {
      if let Some(session) = self.active_session.as_mut() {
        session.messages.push(ChatMessage {
          role: Role::User,
          content: raw_input.clone(),
          timestamp: Utc::now(),
        });
        session.updated_at = Utc::now();
        let _ = save_session(session);
        let meta = crate::storage::session_to_meta(session);
        let id = meta.id.clone();
        if let Some(pos) = self.sessions.iter().position(|s| s.id == id) {
          self.sessions[pos] = meta;
        }
        self.sync_index();
      }
      self.follow_tail = true;
      self.scroll_offset = usize::MAX;
      return ChatAction::SlashCommand(raw_input);
    }

    let (prefix, content) = parse_provider_prefix(&raw_input);
    let provider_name = prefix.unwrap_or_else(|| self.default_provider.clone());

    if let Some(session) = self.active_session.as_mut() {
      session.messages.push(ChatMessage {
        role: Role::User,
        content: content.clone(),
        timestamp: Utc::now(),
      });
      session.updated_at = Utc::now();
    }

    let messages = self
      .active_session
      .as_ref()
      .map(|s| s.messages.clone())
      .unwrap_or_default();

    let provider = match self.provider_registry.get(&provider_name) {
      Some(p) => p,
      None => {
        let err = format!("provider '{}' not registered", provider_name);
        log::debug!("chat: {err}");
        if let Some(session) = self.active_session.as_mut() {
          session.messages.push(ChatMessage {
            role: Role::Assistant,
            content: err,
            timestamp: Utc::now(),
          });
          session.updated_at = Utc::now();
          let _ = save_session(session);
          let meta = crate::storage::session_to_meta(session);
          let id = meta.id.clone();
          if let Some(pos) = self.sessions.iter().position(|s| s.id == id) {
            self.sessions[pos] = meta;
          }
          self.sync_index();
        }
        self.follow_tail = true;
        self.scroll_offset = usize::MAX;
        return ChatAction::None;
      }
    };

    log::debug!(
      "chat: spawning background thread for provider '{provider_name}'"
    );

    let (tx, rx) = mpsc::channel::<Result<ProviderResponse, String>>();
    self.pending_response = Some(rx);
    self.is_loading = true;
    self.follow_tail = true;
    self.scroll_offset = usize::MAX;

    std::thread::spawn(move || {
      let tx_panic = tx.clone();
      let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
          let result = provider.send(&messages).map_err(|e| e.to_string());
          let _ = tx.send(result);
        }));
      if let Err(payload) = outcome {
        let msg = if let Some(s) = payload.downcast_ref::<&'static str>() {
          (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
          s.clone()
        } else {
          "thread panicked (non-string payload)".to_string()
        };
        log::error!("chat provider thread panicked — {msg}");
        let _ =
          tx_panic.send(Err(format!("chat provider thread panicked: {msg}")));
      }
    });

    ChatAction::None
  }

  /// Build pre-wrapped message lines (Feynman style).
  ///
  /// User messages: full-width background highlight, white text.
  /// Assistant messages: no background, gray text, markdown bold handled.
  /// Single blank line between each pair.
  ///
  /// Per-message lines are cached on `self.line_cache` so streaming reveals
  /// only re-wrap the streaming message instead of the entire history (see
  /// the field doc for the cache invalidation strategy).
  fn build_message_lines(
    &mut self,
    width: usize,
    t: &Theme,
  ) -> Vec<Line<'static>> {
    // Invalidate the entire cache on width change (resize) or session
    // switch — every cached entry's wrap was relative to the old width or
    // the old conversation.
    let session_id = self.active_session.as_ref().map(|s| s.id.clone());
    if width != self.line_cache_width
      || session_id != self.line_cache_session_id
    {
      self.line_cache.clear();
      self.line_cache_width = width;
      self.line_cache_session_id = session_id;
    }

    if self.active_session.is_none() {
      return vec![];
    }

    let wrap_width = width.max(1);

    // Lightweight metadata pass: capture (orig_idx, role, content_len) for
    // every non-System message without cloning content. Cloning every
    // body up front to release the borrow before mutating line_cache
    // costs ~100KB per frame on a 50-message session even when every
    // message is a cache hit. Now we only clone content on cache miss.
    let metas: Vec<MsgMeta> = self
      .active_session
      .as_ref()
      .map(|s| {
        s.messages
          .iter()
          .enumerate()
          .filter(|(_, m)| !matches!(m.role, Role::System))
          .map(|(orig_idx, m)| MsgMeta {
            orig_idx,
            role: m.role,
            content_len: m.content.len(),
          })
          .collect()
      })
      .unwrap_or_default();

    let total = metas.len();
    let mut lines: Vec<Line<'static>> = Vec::new();

    for filt_i in 0..total {
      let meta = &metas[filt_i];
      let is_last = filt_i + 1 == total;
      let has_streaming_cursor =
        self.is_streaming && is_last && matches!(meta.role, Role::Assistant);
      let key = (filt_i, has_streaming_cursor);

      // Cache hit fast path. For non-streaming messages we require an
      // exact content_len match. For the streaming-tail message we
      // Bucket the comparison: as long as the cached render is within
      // STREAM_BUCKET bytes of current content, treat as a hit. Drops
      // per-word streaming re-render from one full markdown+wrap pass
      // per word to one per ~STREAM_BUCKET chars (~10 words at typical
      // token sizes); total work over an N-word stream goes O(N²) →
      // ~O(N²/STREAM_BUCKET). User-visible effect: streaming reveals
      // in chunks instead of word-by-word; on stream end the cache key
      // changes and a final exact render fires.
      const STREAM_BUCKET: usize = 64;
      if let Some(cached) = self.line_cache.get(&key) {
        let same_enough = if has_streaming_cursor {
          cached.content_len / STREAM_BUCKET == meta.content_len / STREAM_BUCKET
            && cached.content_len <= meta.content_len
        } else {
          cached.content_len == meta.content_len
        };
        if same_enough {
          lines.extend(cached.lines.iter().cloned());
          let next_role = metas.get(filt_i + 1).map(|m| m.role);
          if message_gap_needed(meta.role, next_role) {
            lines.push(Line::from(""));
          }
          continue;
        }
      }

      // Cache miss or stale entry: clone the message content in a scoped
      // borrow so it drops before we touch line_cache mutably.
      let content_owned = self
        .active_session
        .as_ref()
        .map(|s| s.messages[meta.orig_idx].content.clone())
        .unwrap_or_default();
      let msg_lines = match meta.role {
        Role::System => continue,
        Role::User => render_user_message(&content_owned, wrap_width, t),
        Role::Assistant => render_assistant_message(
          &content_owned,
          wrap_width,
          has_streaming_cursor,
          t,
        ),
      };

      // Overwrite the (msg_idx, has_streaming_cursor) entry with the new
      // rendered output. The HashMap stays bounded — single entry per
      // message regardless of how many streaming ticks have happened.
      self.line_cache.insert(
        key,
        CachedRender {
          content_len: meta.content_len,
          lines: msg_lines.clone(),
        },
      );
      lines.extend(msg_lines);

      // Single blank line between messages, not after the last.
      let next_role = metas.get(filt_i + 1).map(|m| m.role);
      if message_gap_needed(meta.role, next_role) {
        lines.push(Line::from(""));
      }
    }

    lines
  }
}

/// Lightweight per-message metadata used by `build_message_lines` so the
/// cache-hit path doesn't have to clone any message content.
struct MsgMeta {
  orig_idx: usize,
  role: Role,
  content_len: usize,
}

// ── Shared helpers ────────────────────────────────────────────────────────────

impl ChatUi {
  fn sync_index(&self) {
    let index = ChatIndex {
      sessions: self.sessions.clone(),
      default_provider: self.default_provider.clone(),
    };
    let _ = save_index(&index);
  }
}

