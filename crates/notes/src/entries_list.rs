use std::collections::HashSet;

use chrono::Datelike;
use ratatui::{
  Frame,
  layout::{Alignment, Rect},
  prelude::Margin,
  style::{Modifier, Style},
  symbols,
  text::{Line, Span},
  widgets::{
    List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
  },
};

use crate::{Note, colored_tags::ColoredTagsManager, theme};

const LIST_INNER_MARGIN: usize = 5;

#[derive(Debug)]
pub struct EntriesList {
  pub state: ListState,
  is_active: bool,
  pub multi_select_mode: bool,
  /// IDs (`note_id`) of notes currently selected in multi-select mode.
  pub selected_notes: HashSet<String>,
}

impl EntriesList {
  pub fn new() -> Self {
    Self {
      state: ListState::default(),
      is_active: false,
      multi_select_mode: false,
      selected_notes: HashSet::new(),
    }
  }

  fn render_list(
    &mut self,
    frame: &mut Frame,
    notes: &[Note],
    colored_tags: &ColoredTagsManager,
    _has_filter: bool,
    area: Rect,
  ) {
    let mut lines_count = 0;

    let items: Vec<ListItem> = notes
      .iter()
      .map(|note| {
        let highlight_selected = self.multi_select_mode
          && self.selected_notes.contains(&note.note_id);

        // *** Title ***
        let mut title = note.title.to_string();

        if highlight_selected {
          title.insert_str(0, "* ");
        }

        // Text wrapping
        let title_lines = textwrap::wrap(
          &title,
          (area.width as usize).saturating_sub(LIST_INNER_MARGIN),
        );

        // title lines
        lines_count += title_lines.len();

        let title_style = match (self.is_active, highlight_selected) {
          (_, true) => {
            Style::default().fg(theme::current().accent).add_modifier(Modifier::BOLD)
          }
          (true, _) => Style::default().add_modifier(Modifier::BOLD),
          (false, _) => Style::reset(),
        };

        let mut spans: Vec<Line> = title_lines
          .iter()
          .map(|line| Line::from(Span::styled(line.to_string(), title_style)))
          .collect();

        // *** Updated-at date ***
        let date_line = format!(
          "{},{},{}",
          note.updated_at.day(),
          note.updated_at.month(),
          note.updated_at.year()
        );
        let date_style = Style::default().fg(theme::current().text_dim);
        spans.push(Line::from(Span::styled(date_line, date_style)));
        lines_count += 1;

        // *** Tags ***
        if !note.tags.is_empty() {
          const TAGS_SEPARATOR: &str = " | ";
          let tags_default_style = Style::reset();

          let mut added_lines = 1;
          spans.push(Line::default());

          for tag in note.tags.iter() {
            let mut last_line = spans.last_mut().unwrap();
            let allowed_width =
              (area.width as usize).saturating_sub(LIST_INNER_MARGIN);
            if !last_line.spans.is_empty() {
              if last_line.width() + TAGS_SEPARATOR.len() > allowed_width {
                added_lines += 1;
                spans.push(Line::default());
                last_line = spans.last_mut().unwrap();
              }
              last_line
                .push_span(Span::styled(TAGS_SEPARATOR, tags_default_style));
            }

            let style = colored_tags
              .get_tag_color(tag)
              .map(|c| Style::default().bg(c.background).fg(c.foreground))
              .unwrap_or(tags_default_style);
            let span_to_add = Span::styled(tag.to_owned(), style);

            if last_line.width() + tag.len() < allowed_width {
              last_line.push_span(span_to_add);
            } else {
              added_lines += 1;
              let line = Line::from(span_to_add);
              spans.push(line);
            }
          }

          lines_count += added_lines;
        }

        ListItem::new(spans)
      })
      .collect();

    let items_count = items.len();

    let highlight_style = if self.is_active {
      Style::default().add_modifier(Modifier::REVERSED)
    } else {
      Style::default().add_modifier(Modifier::REVERSED).fg(theme::current().text_dim)
    };

    let list =
      List::new(items).highlight_style(highlight_style).highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut self.state);

    if lines_count > area.height as usize {
      let avg_item_height = lines_count / items_count;

      self.render_scrollbar(
        frame,
        area,
        self.state.selected().unwrap_or(0),
        items_count,
        avg_item_height,
      );
    }
  }

  fn render_scrollbar(
    &mut self,
    frame: &mut Frame,
    area: Rect,
    pos: usize,
    items_count: usize,
    avg_item_height: usize,
  ) {
    const VIEWPORT_ADJUST: u16 = 4;
    let viewport_len =
      (area.height / avg_item_height as u16).saturating_sub(VIEWPORT_ADJUST);

    let mut state = ScrollbarState::default()
      .content_length(items_count)
      .viewport_content_length(viewport_len as usize)
      .position(pos);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
      .begin_symbol(Some("▲"))
      .end_symbol(Some("▼"))
      .track_symbol(Some(symbols::line::VERTICAL))
      .thumb_symbol(symbols::block::FULL);

    let scroll_area = area.inner(Margin { horizontal: 0, vertical: 1 });

    frame.render_stateful_widget(scrollbar, scroll_area, &mut state);
  }

  fn render_place_holder(
    &mut self,
    frame: &mut Frame,
    area: Rect,
    _has_filter: bool,
  ) {
    let place_holder_text = if self.multi_select_mode {
      String::from("\nNo notes to select")
    } else {
      String::from("\n No notes")
    };

    let place_holder = Paragraph::new(place_holder_text)
      .wrap(Wrap { trim: false })
      .alignment(Alignment::Center);

    frame.render_widget(place_holder, area);
  }

  pub fn render_widget(
    &mut self,
    frame: &mut Frame,
    area: Rect,
    notes: &[Note],
    colored_tags: &ColoredTagsManager,
    has_filter: bool,
  ) {
    if notes.is_empty() {
      self.render_place_holder(frame, area, has_filter);
    } else {
      self.render_list(frame, notes, colored_tags, has_filter, area);
    }
  }

  pub fn set_active(&mut self, active: bool) {
    self.is_active = active;
  }
}
