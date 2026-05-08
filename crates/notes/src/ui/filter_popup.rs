use std::collections::HashSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
  Frame,
  layout::{Alignment, Constraint, Direction, Layout, Rect},
  style::{Color, Modifier, Style},
  widgets::{
    Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
  },
};
use tui_textarea::{CursorMove, TextArea};

use crate::{
  filter::{
    CriteriaRelation, Filter, FilterCriterion, criterion::TagFilterOption,
  },
  theme,
};

use super::{PopupReturn, ui_functions::centered_rect};

pub type FilterPopupReturn = PopupReturn<Option<Filter>>;

const FOOTER_TEXT: &str = r"Tab: Change focused control | Enter or <Ctrl-m>: Confirm | Esc or <Ctrl-c>: Cancel | <Ctrl-r>: Change Matching Logic | <Space>: Tags Toggle Selected";
const FOOTER_MARGIN: usize = 8;

/// Text to show in tags list indicating that untagged notes are included.
///
/// # Note:
/// This name is used as an identifier for this option; the trailing spaces are intentional
/// to avoid clashing with user tags of the same text.
const NO_TAGS_TEXT: &str = "NO_TAGS (Notes with no tags)  ";

pub struct FilterPopup<'a> {
  active_control: FilterControl,
  tags_state: ListState,
  tags: Vec<String>,
  relation: CriteriaRelation,
  selected_tags: HashSet<String>,
  title_txt: TextArea<'a>,
  content_txt: TextArea<'a>,
}

#[derive(Debug, PartialEq, Eq)]
enum FilterControl {
  TitleTxt,
  ContentTxt,
  TagsList,
}

impl FilterPopup<'_> {
  pub fn new(mut tags: Vec<String>, filter: Option<Filter>) -> Self {
    let filter = filter.unwrap_or_default();
    // Add no-tags option to list of tags when there are some tags
    if !tags.is_empty() {
      tags.push(NO_TAGS_TEXT.into());
    }

    let relation = filter.relation;

    let mut selected_tags = HashSet::new();
    let mut title_text = String::default();
    let mut content_text = String::default();

    filter.criteria.into_iter().for_each(|cr| match cr {
      FilterCriterion::Tag(TagFilterOption::Tag(tag)) => {
        selected_tags.insert(tag);
      }
      FilterCriterion::Tag(TagFilterOption::NoTags) => {
        selected_tags.insert(NO_TAGS_TEXT.into());
      }
      FilterCriterion::Title(title_search) => title_text = title_search,
      FilterCriterion::Content(content_search) => content_text = content_search,
    });

    let mut title_txt = TextArea::new(vec![title_text]);
    title_txt.move_cursor(CursorMove::End);

    let mut content_txt = TextArea::new(vec![content_text]);
    content_txt.move_cursor(CursorMove::End);

    let mut filter_popup = FilterPopup {
      active_control: FilterControl::TitleTxt,
      tags_state: ListState::default(),
      tags,
      relation,
      selected_tags,
      title_txt,
      content_txt,
    };

    filter_popup.cycle_next_tag();

    filter_popup
  }

  pub fn render_widget(&mut self, frame: &mut Frame, area: Rect) {
    let area = centered_rect(70, 80, area);

    let block = Block::default()
      .borders(Borders::ALL)
      .border_style(Style::default().fg(theme::current().border))
      .title("─── Filter ───");
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let footer_wrap_width =
      (area.width as usize).saturating_sub(FOOTER_MARGIN).max(1);
    let footer_height =
      textwrap::fill(FOOTER_TEXT, footer_wrap_width).lines().count();

    let chunks = Layout::default()
      .direction(Direction::Vertical)
      .horizontal_margin(4)
      .vertical_margin(2)
      .constraints(
        [
          Constraint::Length(3),
          Constraint::Length(3),
          Constraint::Length(3),
          Constraint::Min(4),
          Constraint::Length(footer_height.try_into().unwrap_or(u16::MAX)),
        ]
        .as_ref(),
      )
      .split(area);

    self.render_relations(frame, chunks[0]);
    self.render_text_boxes(frame, chunks[1], chunks[2]);

    if self.tags.is_empty() {
      self.render_tags_place_holder(frame, chunks[3]);
    } else {
      self.render_tags_list(frame, chunks[3]);
    }

    self.render_footer(frame, chunks[4]);
  }

  fn render_relations(&mut self, frame: &mut Frame, area: Rect) {
    let relation_text = match self.relation {
      CriteriaRelation::And => "Notes must meet all criteria",
      CriteriaRelation::Or => "Notes must meet any of the criteria",
    };

    let relation =
      Paragraph::new(relation_text).alignment(Alignment::Center).block(
        Block::default()
          .borders(Borders::ALL)
          .border_style(Style::default().fg(theme::current().border))
          .title("Matching Logic"),
      );

    frame.render_widget(relation, area);
  }

  fn render_text_boxes(
    &mut self,
    frame: &mut Frame,
    title_area: Rect,
    content_area: Rect,
  ) {
    let active_cursor_style: Style =
      Style::default().fg(theme::current().accent).bg(theme::current().accent);
    let deactivate_cursor_style = Style::default().bg(Color::Reset);

    let mut title_txt_block =
      Block::default().title("Title").borders(Borders::ALL);
    let mut content_txt_block =
      Block::default().title("Content").borders(Borders::ALL);

    match self.active_control {
      FilterControl::TitleTxt => {
        self.title_txt.set_cursor_style(active_cursor_style);
        self.content_txt.set_cursor_style(deactivate_cursor_style);
        title_txt_block =
          title_txt_block.style(Style::default().fg(theme::current().accent));
      }
      FilterControl::ContentTxt => {
        self.title_txt.set_cursor_style(deactivate_cursor_style);
        self.content_txt.set_cursor_style(active_cursor_style);
        content_txt_block =
          content_txt_block.style(Style::default().fg(theme::current().accent));
      }
      FilterControl::TagsList => {
        self.title_txt.set_cursor_style(deactivate_cursor_style);
        self.content_txt.set_cursor_style(deactivate_cursor_style);
      }
    }

    self.title_txt.set_cursor_line_style(Style::default());
    self.content_txt.set_cursor_line_style(Style::default());

    self.title_txt.set_block(title_txt_block);
    self.content_txt.set_block(content_txt_block);

    frame.render_widget(&self.title_txt, title_area);
    frame.render_widget(&self.content_txt, content_area);
  }

  fn render_tags_list(&mut self, frame: &mut Frame, area: Rect) {
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let items: Vec<ListItem> = self
      .tags
      .iter()
      .map(|tag| {
        let is_selected = self.selected_tags.contains(tag);

        let (tag_text, style) = if is_selected {
          (format!("* {tag}"), selected_style)
        } else {
          (tag.to_owned(), Style::reset())
        };

        ListItem::new(tag_text).style(style)
      })
      .collect();

    let highlight_style = match self.active_control {
      FilterControl::TagsList => {
        Style::default().add_modifier(Modifier::REVERSED)
      }
      _ => Style::default().fg(theme::current().text_dim),
    };

    let list = List::new(items)
      .block(self.get_list_block())
      .highlight_style(highlight_style)
      .highlight_symbol(">> ");

    frame.render_stateful_widget(list, area, &mut self.tags_state);
  }

  fn render_tags_place_holder(&mut self, frame: &mut Frame, area: Rect) {
    let place_holder_text = String::from("\nNo notes with tags provided");

    let place_holder = Paragraph::new(place_holder_text)
      .wrap(Wrap { trim: false })
      .alignment(Alignment::Center)
      .block(self.get_list_block());

    frame.render_widget(place_holder, area);
  }

  fn get_list_block<'b>(&self) -> Block<'b> {
    let style = match self.active_control {
      FilterControl::TagsList => Style::default().fg(theme::current().accent),
      _ => Style::default(),
    };
    Block::default().borders(Borders::ALL).title("Tags").style(style)
  }

  fn render_footer(&mut self, frame: &mut Frame, area: Rect) {
    let footer = Paragraph::new(FOOTER_TEXT)
      .alignment(Alignment::Center)
      .wrap(Wrap { trim: false })
      .block(Block::default().borders(Borders::NONE).style(Style::default()));

    frame.render_widget(footer, area);
  }

  pub fn handle_input(&mut self, key: KeyEvent) -> FilterPopupReturn {
    let has_control = key.modifiers.contains(KeyModifiers::CONTROL);

    if self.active_control != FilterControl::TagsList {
      match key.code {
        KeyCode::Tab => self.cycle_next_control(),
        KeyCode::Esc => FilterPopupReturn::Cancel,
        KeyCode::Char('c') if has_control => FilterPopupReturn::Cancel,
        KeyCode::Enter => self.confirm(),
        KeyCode::Char('m') if has_control => self.confirm(),
        KeyCode::Char('r') if has_control => {
          self.change_relation();
          FilterPopupReturn::KeepPopup
        }
        _ => {
          match self.active_control {
            FilterControl::TitleTxt => _ = self.title_txt.input(key),
            FilterControl::ContentTxt => _ = self.content_txt.input(key),
            FilterControl::TagsList => {
              unreachable!("Tags List is unreachable here")
            }
          };
          FilterPopupReturn::KeepPopup
        }
      }
    } else {
      match key.code {
        KeyCode::Tab => self.cycle_next_control(),
        KeyCode::Char('j') | KeyCode::Down => {
          self.cycle_next_tag();
          FilterPopupReturn::KeepPopup
        }
        KeyCode::Char('k') | KeyCode::Up => {
          self.cycle_prev_tag();
          FilterPopupReturn::KeepPopup
        }
        KeyCode::Char(' ') => {
          self.toggle_selected();
          FilterPopupReturn::KeepPopup
        }
        KeyCode::Char('r') => {
          self.change_relation();
          FilterPopupReturn::KeepPopup
        }
        KeyCode::Esc | KeyCode::Char('q') => FilterPopupReturn::Cancel,
        KeyCode::Char('c') if has_control => FilterPopupReturn::Cancel,
        KeyCode::Enter => self.confirm(),
        KeyCode::Char('m') if has_control => self.confirm(),
        _ => FilterPopupReturn::KeepPopup,
      }
    }
  }

  fn cycle_next_control(&mut self) -> FilterPopupReturn {
    self.active_control = match self.active_control {
      FilterControl::TitleTxt => FilterControl::ContentTxt,
      FilterControl::ContentTxt => FilterControl::TagsList,
      FilterControl::TagsList => FilterControl::TitleTxt,
    };

    FilterPopupReturn::KeepPopup
  }

  fn cycle_next_tag(&mut self) {
    if self.tags.is_empty() {
      return;
    }

    let last_index = self.tags.len() - 1;
    let new_index = self
      .tags_state
      .selected()
      .map(|idx| if idx >= last_index { 0 } else { idx + 1 })
      .unwrap_or(0);

    self.tags_state.select(Some(new_index));
  }

  fn cycle_prev_tag(&mut self) {
    if self.tags.is_empty() {
      return;
    }

    let last_index = self.tags.len() - 1;
    let new_index = self
      .tags_state
      .selected()
      .map(|idx| idx.checked_sub(1).unwrap_or(last_index))
      .unwrap_or(last_index);

    self.tags_state.select(Some(new_index));
  }

  fn change_relation(&mut self) {
    self.relation = match self.relation {
      CriteriaRelation::And => CriteriaRelation::Or,
      CriteriaRelation::Or => CriteriaRelation::And,
    }
  }

  fn toggle_selected(&mut self) {
    if let Some(idx) = self.tags_state.selected() {
      let tag = self
        .tags
        .get(idx)
        .expect("tags has the index of the selected item in list");

      if self.selected_tags.contains(tag) {
        self.selected_tags.remove(tag);
      } else {
        self.selected_tags.insert(tag.to_owned());
      }
    }
  }

  fn confirm(&mut self) -> FilterPopupReturn {
    let mut criteria: Vec<_> = self
      .selected_tags
      .iter()
      .map(|tag| {
        if tag == NO_TAGS_TEXT {
          FilterCriterion::Tag(TagFilterOption::NoTags)
        } else {
          FilterCriterion::Tag(TagFilterOption::Tag(tag.into()))
        }
      })
      .collect();

    let title_filter =
      self.title_txt.lines().first().expect("Title TextBox has one line");

    if !title_filter.is_empty() {
      criteria.push(FilterCriterion::Title(title_filter.to_owned()));
    }

    let content_filter =
      self.content_txt.lines().first().expect("Content TextBox has one line");

    if !content_filter.is_empty() {
      criteria.push(FilterCriterion::Content(content_filter.to_owned()));
    }

    if criteria.is_empty() {
      FilterPopupReturn::Apply(None)
    } else {
      let filter = Filter { relation: self.relation, criteria };

      FilterPopupReturn::Apply(Some(filter))
    }
  }
}
