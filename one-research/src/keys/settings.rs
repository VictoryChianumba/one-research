use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, AppView, CustomThemeEditorMode, CustomThemeEditorState};
use crate::config::{self, CUSTOM_THEME_ROLES, CustomThemeConfig};
use ui_theme::ThemeId;

pub(super) fn handle_settings_view(key: KeyEvent, app: &mut App) -> bool {
  if app.view != AppView::Settings {
    return false;
  }
  if app.theme_picker.active {
    handle_theme_picker(key, app);
    return true;
  }
  if app.settings.editing {
    match key.code {
      KeyCode::Enter => {
        match app.settings.field {
          0 => app.settings.github_token = app.settings.edit_buf.clone(),
          1 => app.settings.s2_key = app.settings.edit_buf.clone(),
          2 => app.settings.claude_key = app.settings.edit_buf.clone(),
          3 => app.settings.openai_key = app.settings.edit_buf.clone(),
          _ => {}
        }
        app.settings.editing = false;
      }
      KeyCode::Esc => {
        app.settings.editing = false;
        app.settings.edit_buf.clear();
      }
      KeyCode::Backspace => {
        app.settings.edit_buf.pop();
      }
      KeyCode::Char(c) => {
        app.settings.edit_buf.push(c);
      }
      _ => {}
    }
  } else {
    match key.code {
      KeyCode::Char('q') | KeyCode::Esc => {
        app.view = AppView::Feed;
      }
      KeyCode::Char('j') | KeyCode::Down => {
        app.settings.field = (app.settings.field + 1).min(5);
      }
      KeyCode::Char('k') | KeyCode::Up => {
        app.settings.field = app.settings.field.saturating_sub(1);
      }
      KeyCode::Enter => {
        if app.settings.field == 4 {
          app.settings.default_chat_provider =
            if app.settings.default_chat_provider == "claude" {
              "openai".to_string()
            } else {
              "claude".to_string()
            };
        } else if app.settings.field == 5 {
          open_theme_picker(app);
        } else {
          app.settings.edit_buf = match app.settings.field {
            0 => app.settings.github_token.clone(),
            1 => app.settings.s2_key.clone(),
            2 => app.settings.claude_key.clone(),
            3 => app.settings.openai_key.clone(),
            _ => String::new(),
          };
          app.settings.editing = true;
        }
      }
      KeyCode::Char('s') | KeyCode::Char('S') => {
        app.config.github_token = if app.settings.github_token.is_empty() {
          None
        } else {
          Some(app.settings.github_token.clone())
        };
        app.config.semantic_scholar_key = if app.settings.s2_key.is_empty() {
          None
        } else {
          Some(app.settings.s2_key.clone())
        };
        app.config.claude_api_key = if app.settings.claude_key.is_empty() {
          None
        } else {
          Some(app.settings.claude_key.clone())
        };
        app.config.openai_api_key = if app.settings.openai_key.is_empty() {
          None
        } else {
          Some(app.settings.openai_key.clone())
        };
        app.config.default_chat_provider =
          app.settings.default_chat_provider.clone();
        app.config.theme = app.active_theme;
        app.config.active_custom_theme_id = app.active_custom_theme_id.clone();
        // Keep github_token field in sync for repo viewer.
        app.github_token = app.config.github_token.clone();
        // Rebuild chat_ui with updated keys on next open.
        app.chat.ui = None;
        app.config.save();
        app.settings.save_time = Some(std::time::Instant::now());
      }
      KeyCode::Char('p') => {
        app.sources_popup.cursor = 0;
        app.sources_popup.input.clear();
        app.sources_popup.input.blur();
        app.sources_popup.detect.reset();
        app.view = AppView::Sources;
        app.modals.push(crate::surfaces::overlays::ActiveModal::Sources);
        log::debug!(
          "sources_popup: opened — current arxiv categories: [{}]",
          app.config.sources.arxiv_categories.join(", ")
        );
      }
      _ => {}
    }
  }
  true
}

fn open_theme_picker(app: &mut App) {
  app.theme_picker.active = true;
  app.theme_picker.custom_editor = None;
  app.theme_picker.original =
    Some((app.active_theme, app.active_custom_theme_id.clone()));
  app.theme_picker.cursor = theme_picker_active_row(app);
  app.theme_picker.scroll = app.theme_picker.cursor.saturating_sub(4);
}

fn handle_theme_picker(key: KeyEvent, app: &mut App) {
  if app.theme_picker.custom_editor.is_some() {
    handle_custom_theme_editor(key, app);
    return;
  }

  let total = theme_picker_row_count(app);
  if total == 0 {
    app.theme_picker.active = false;
    return;
  }

  match key.code {
    KeyCode::Char('q') | KeyCode::Esc => {
      if let Some((theme, custom_id)) = app.theme_picker.original.take() {
        app.active_theme = theme;
        app.active_custom_theme_id = custom_id;
      }
      app.theme_picker.active = false;
    }
    KeyCode::Enter => {
      activate_theme_picker_row(app, true);
    }
    KeyCode::Char('j') | KeyCode::Down => {
      app.theme_picker.cursor = (app.theme_picker.cursor + 1).min(total - 1);
      activate_theme_picker_row(app, false);
      clamp_theme_picker_scroll(app);
    }
    KeyCode::Char('k') | KeyCode::Up => {
      app.theme_picker.cursor = app.theme_picker.cursor.saturating_sub(1);
      activate_theme_picker_row(app, false);
      clamp_theme_picker_scroll(app);
    }
    KeyCode::Char('e') => {
      if let Some(custom) = selected_custom_theme(app).cloned() {
        app.theme_picker.custom_editor = Some(CustomThemeEditorState {
          theme: custom.clone(),
          is_new: false,
          mode: CustomThemeEditorMode::Palette,
          role_cursor: 0,
          hue_cursor: 6,
          shade_cursor: 3,
          edit_buf: String::new(),
        });
      }
    }
    KeyCode::Char('d') => {
      if let Some(custom) = selected_custom_theme(app).cloned() {
        app.theme_picker.custom_editor = Some(CustomThemeEditorState {
          theme: custom.clone(),
          is_new: false,
          mode: CustomThemeEditorMode::DeleteConfirm,
          role_cursor: 0,
          hue_cursor: 6,
          shade_cursor: 3,
          edit_buf: String::new(),
        });
      }
    }
    _ => {}
  }
}

fn clamp_theme_picker_scroll(app: &mut App) {
  const VISIBLE_ROWS: usize = 10;
  if app.theme_picker.cursor < app.theme_picker.scroll {
    app.theme_picker.scroll = app.theme_picker.cursor;
  } else if app.theme_picker.cursor >= app.theme_picker.scroll + VISIBLE_ROWS {
    app.theme_picker.scroll =
      app.theme_picker.cursor.saturating_sub(VISIBLE_ROWS - 1);
  }
}

fn theme_picker_row_count(app: &App) -> usize {
  ThemeId::all().len() + app.config.custom_themes.len() + 1
}

fn theme_picker_active_row(app: &App) -> usize {
  let presets = ThemeId::all();
  if let Some(id) = &app.active_custom_theme_id {
    if let Some(idx) = app.config.custom_themes.iter().position(|t| &t.id == id)
    {
      return presets.len() + idx;
    }
  }
  presets.iter().position(|id| *id == app.active_theme).unwrap_or(0)
}

fn selected_custom_theme(app: &App) -> Option<&CustomThemeConfig> {
  let presets = ThemeId::all().len();
  let idx = app.theme_picker.cursor.checked_sub(presets)?;
  app.config.custom_themes.get(idx)
}

fn activate_theme_picker_row(app: &mut App, commit: bool) {
  let presets = ThemeId::all();
  let preset_count = presets.len();
  let custom_count = app.config.custom_themes.len();
  let row =
    app.theme_picker.cursor.min(theme_picker_row_count(app).saturating_sub(1));

  if row < preset_count {
    app.active_theme = presets[row];
    app.active_custom_theme_id = None;
    app.config.theme = app.active_theme;
    app.config.active_custom_theme_id = None;
    if commit {
      app.theme_picker.original = None;
      app.theme_picker.active = false;
    }
  } else if row < preset_count + custom_count {
    let custom = &app.config.custom_themes[row - preset_count];
    app.active_theme = custom.base;
    app.active_custom_theme_id = Some(custom.id.clone());
    app.config.theme = app.active_theme;
    app.config.active_custom_theme_id = app.active_custom_theme_id.clone();
    if commit {
      app.theme_picker.original = None;
      app.theme_picker.active = false;
    }
  } else if commit {
    open_new_custom_theme_editor(app);
  }
}

fn open_new_custom_theme_editor(app: &mut App) {
  let base =
    app.active_custom_theme().map(|t| t.base).unwrap_or(app.active_theme);
  let name = next_custom_theme_name(app);
  let theme = CustomThemeConfig::from_theme(
    next_custom_theme_id(app),
    name.clone(),
    base,
    app.theme(),
  );
  app.theme_picker.custom_editor = Some(CustomThemeEditorState {
    theme,
    is_new: true,
    mode: CustomThemeEditorMode::Name,
    role_cursor: 0,
    hue_cursor: 6,
    shade_cursor: 3,
    edit_buf: name,
  });
}

fn next_custom_theme_id(app: &App) -> String {
  for n in 1..=1000 {
    let id = format!("custom-{n}");
    if !app.config.custom_themes.iter().any(|theme| theme.id == id) {
      return id;
    }
  }
  panic!("custom theme ID space exhausted — 1000 candidates checked")
}

fn next_custom_theme_name(app: &App) -> String {
  for n in 1..=1000 {
    let name = format!("Custom {n}");
    if !app.config.custom_themes.iter().any(|theme| theme.name == name) {
      return name;
    }
  }
  panic!("custom theme name space exhausted — 1000 candidates checked")
}

fn handle_custom_theme_editor(key: KeyEvent, app: &mut App) {
  let Some(mode) =
    app.theme_picker.custom_editor.as_ref().map(|editor| editor.mode)
  else {
    return;
  };

  match mode {
    CustomThemeEditorMode::Name => handle_custom_theme_name_editor(key, app),
    CustomThemeEditorMode::Hex => handle_custom_theme_hex_editor(key, app),
    CustomThemeEditorMode::DeleteConfirm => {
      handle_custom_theme_delete_confirm(key, app)
    }
    CustomThemeEditorMode::Palette => {
      handle_custom_theme_palette_editor(key, app)
    }
  }
}

fn handle_custom_theme_name_editor(key: KeyEvent, app: &mut App) {
  match key.code {
    KeyCode::Enter => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        let name = editor.edit_buf.trim();
        if !name.is_empty() {
          editor.theme.name = name.to_string();
        }
        editor.mode = CustomThemeEditorMode::Palette;
        editor.edit_buf.clear();
      }
    }
    KeyCode::Esc => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        if editor.is_new {
          app.theme_picker.custom_editor = None;
        } else {
          editor.mode = CustomThemeEditorMode::Palette;
          editor.edit_buf.clear();
        }
      }
    }
    KeyCode::Backspace => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        editor.edit_buf.pop();
      }
    }
    KeyCode::Char(c) => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        editor.edit_buf.push(c);
      }
    }
    _ => {}
  }
}

fn handle_custom_theme_hex_editor(key: KeyEvent, app: &mut App) {
  match key.code {
    KeyCode::Enter => {
      let Some(editor) = app.theme_picker.custom_editor.as_mut() else {
        return;
      };
      let value = editor.edit_buf.trim();
      if config::parse_hex_color(value).is_some() {
        let key = CUSTOM_THEME_ROLES
          [editor.role_cursor.min(CUSTOM_THEME_ROLES.len() - 1)]
        .key;
        editor.theme.colors.set_role(key, normalize_hex(value));
        editor.mode = CustomThemeEditorMode::Palette;
        editor.edit_buf.clear();
      } else {
        app.set_notification(
          "Use a 6-digit hex color, e.g. #67D7F5.".to_string(),
        );
      }
    }
    KeyCode::Esc => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        editor.mode = CustomThemeEditorMode::Palette;
        editor.edit_buf.clear();
      }
    }
    KeyCode::Backspace => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        editor.edit_buf.pop();
      }
    }
    KeyCode::Char(c) => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        if c == '#' && editor.edit_buf.is_empty() || c.is_ascii_hexdigit() {
          editor.edit_buf.push(c);
        }
      }
    }
    _ => {}
  }
}

fn handle_custom_theme_delete_confirm(key: KeyEvent, app: &mut App) {
  match key.code {
    KeyCode::Char('y') | KeyCode::Char('Y') => {
      let Some(editor) = app.theme_picker.custom_editor.take() else {
        return;
      };
      let delete_id = editor.theme.id;
      let delete_was_active =
        app.active_custom_theme_id.as_deref() == Some(&delete_id);
      app.config.custom_themes.retain(|theme| theme.id != delete_id);
      if delete_was_active {
        app.active_custom_theme_id = None;
        app.config.active_custom_theme_id = None;
        app.active_theme = editor.theme.base;
        app.config.theme = app.active_theme;
      }
      app.config.save();
      app.settings.save_time = Some(std::time::Instant::now());
      app.theme_picker.cursor =
        app.theme_picker.cursor.min(theme_picker_row_count(app) - 1);
      clamp_theme_picker_scroll(app);
    }
    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        editor.mode = CustomThemeEditorMode::Palette;
      }
    }
    _ => {}
  }
}

fn handle_custom_theme_palette_editor(key: KeyEvent, app: &mut App) {
  match key.code {
    KeyCode::Char('q') | KeyCode::Esc => {
      app.theme_picker.custom_editor = None;
    }
    KeyCode::Char('s') | KeyCode::Enter => {
      save_custom_theme_editor(app);
    }
    KeyCode::Char('n') => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        editor.mode = CustomThemeEditorMode::Name;
        editor.edit_buf = editor.theme.name.clone();
      }
    }
    KeyCode::Char('x') => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        let key = CUSTOM_THEME_ROLES
          [editor.role_cursor.min(CUSTOM_THEME_ROLES.len() - 1)]
        .key;
        editor.edit_buf =
          editor.theme.colors.get_role(key).unwrap_or("#000000").to_string();
        editor.mode = CustomThemeEditorMode::Hex;
      }
    }
    KeyCode::Char('d') => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        if !editor.is_new {
          editor.mode = CustomThemeEditorMode::DeleteConfirm;
        }
      }
    }
    KeyCode::Char('r') => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        editor.theme.colors =
          config::CustomThemeColors::from_theme(editor.theme.base.theme());
      }
    }
    KeyCode::Char('j') | KeyCode::Down => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        editor.role_cursor =
          (editor.role_cursor + 1).min(CUSTOM_THEME_ROLES.len() - 1);
      }
    }
    KeyCode::Char('k') | KeyCode::Up => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        editor.role_cursor = editor.role_cursor.saturating_sub(1);
      }
    }
    KeyCode::Char('h') | KeyCode::Left => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        editor.hue_cursor = editor.hue_cursor.saturating_sub(1);
      }
    }
    KeyCode::Char('l') | KeyCode::Right => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        editor.hue_cursor =
          (editor.hue_cursor + 1).min(THEME_PALETTE[0].len() - 1);
      }
    }
    KeyCode::Char('[') | KeyCode::Char('-') => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        editor.shade_cursor = editor.shade_cursor.saturating_sub(1);
      }
    }
    KeyCode::Char(']') | KeyCode::Char('+') | KeyCode::Char('=') => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        editor.shade_cursor =
          (editor.shade_cursor + 1).min(THEME_PALETTE.len() - 1);
      }
    }
    KeyCode::Char(' ') => {
      if let Some(editor) = app.theme_picker.custom_editor.as_mut() {
        let role = CUSTOM_THEME_ROLES
          [editor.role_cursor.min(CUSTOM_THEME_ROLES.len() - 1)]
        .key;
        editor
          .theme
          .colors
          .set_role(role, selected_palette_hex(editor).to_string());
      }
    }
    _ => {}
  }
}

fn save_custom_theme_editor(app: &mut App) {
  let Some(editor) = app.theme_picker.custom_editor.take() else {
    return;
  };
  let theme = editor.theme;
  if let Some(existing) =
    app.config.custom_themes.iter_mut().find(|t| t.id == theme.id)
  {
    *existing = theme.clone();
  } else {
    app.config.custom_themes.push(theme.clone());
  }
  app.active_theme = theme.base;
  app.active_custom_theme_id = Some(theme.id.clone());
  app.config.theme = app.active_theme;
  app.config.active_custom_theme_id = app.active_custom_theme_id.clone();
  app.theme_picker.cursor = theme_picker_active_row(app);
  app.theme_picker.original = None;
  app.theme_picker.active = false;
  app.config.save();
  app.settings.save_time = Some(std::time::Instant::now());
  clamp_theme_picker_scroll(app);
}

const THEME_PALETTE: &[&[&str]] = &[
  &[
    "#F8FAFC", "#F7FEE7", "#FEFCE8", "#FFFBEB", "#FFF7ED", "#FFF1F2",
    "#FEF2F2", "#FDF2F8", "#FDF4FF", "#FAF5FF", "#F5F3FF", "#EEF2FF",
    "#EFF6FF", "#F0F9FF", "#ECFEFF", "#F0FDFA",
  ],
  &[
    "#E2E8F0", "#ECFCCB", "#FEF9C3", "#FEF3C7", "#FFEDD5", "#FFE4E6",
    "#FEE2E2", "#FCE7F3", "#FAE8FF", "#F3E8FF", "#EDE9FE", "#E0E7FF",
    "#DBEAFE", "#E0F2FE", "#CFFAFE", "#CCFBF1",
  ],
  &[
    "#CBD5E1", "#D9F99D", "#FEF08A", "#FDE68A", "#FED7AA", "#FECDD3",
    "#FECACA", "#FBCFE8", "#F5D0FE", "#E9D5FF", "#DDD6FE", "#C7D2FE",
    "#BFDBFE", "#BAE6FD", "#A5F3FC", "#99F6E4",
  ],
  &[
    "#94A3B8", "#BEF264", "#FDE047", "#FCD34D", "#FDBA74", "#FDA4AF",
    "#FCA5A5", "#F9A8D4", "#F0ABFC", "#D8B4FE", "#C4B5FD", "#A5B4FC",
    "#93C5FD", "#7DD3FC", "#67E8F9", "#5EEAD4",
  ],
  &[
    "#64748B", "#A3E635", "#FACC15", "#F59E0B", "#FB923C", "#FB7185",
    "#F87171", "#F472B6", "#E879F9", "#C084FC", "#A78BFA", "#818CF8",
    "#60A5FA", "#38BDF8", "#22D3EE", "#2DD4BF",
  ],
  &[
    "#475569", "#84CC16", "#EAB308", "#D97706", "#F97316", "#F43F5E",
    "#EF4444", "#EC4899", "#D946EF", "#A855F7", "#8B5CF6", "#6366F1",
    "#3B82F6", "#0EA5E9", "#06B6D4", "#14B8A6",
  ],
  &[
    "#334155", "#65A30D", "#CA8A04", "#B45309", "#EA580C", "#E11D48",
    "#DC2626", "#DB2777", "#C026D3", "#9333EA", "#7C3AED", "#4F46E5",
    "#2563EB", "#0284C7", "#0891B2", "#0D9488",
  ],
  &[
    "#1E293B", "#4D7C0F", "#A16207", "#92400E", "#C2410C", "#BE123C",
    "#B91C1C", "#BE185D", "#A21CAF", "#7E22CE", "#6D28D9", "#4338CA",
    "#1D4ED8", "#0369A1", "#0E7490", "#0F766E",
  ],
  &[
    "#0F172A", "#3F6212", "#854D0E", "#78350F", "#9A3412", "#9F1239",
    "#991B1B", "#9D174D", "#86198F", "#6B21A8", "#5B21B6", "#3730A3",
    "#1E40AF", "#075985", "#155E75", "#115E59",
  ],
  &[
    "#020617", "#365314", "#713F12", "#451A03", "#7C2D12", "#4C0519",
    "#7F1D1D", "#831843", "#701A75", "#581C87", "#4C1D95", "#312E81",
    "#1E3A8A", "#0C4A6E", "#164E63", "#134E4A",
  ],
];

fn selected_palette_hex(editor: &CustomThemeEditorState) -> &'static str {
  THEME_PALETTE[editor.shade_cursor.min(THEME_PALETTE.len() - 1)]
    [editor.hue_cursor.min(THEME_PALETTE[0].len() - 1)]
}

fn normalize_hex(value: &str) -> String {
  let hex = value.strip_prefix('#').unwrap_or(value).to_ascii_uppercase();
  format!("#{hex}")
}
