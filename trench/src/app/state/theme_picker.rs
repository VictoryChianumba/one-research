use crate::config::CustomThemeConfig;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CustomThemeEditorMode {
  Palette,
  Name,
  Hex,
  DeleteConfirm,
}

#[derive(Clone)]
pub struct CustomThemeEditorState {
  pub theme: CustomThemeConfig,
  pub is_new: bool,
  pub mode: CustomThemeEditorMode,
  pub role_cursor: usize,
  pub hue_cursor: usize,
  pub shade_cursor: usize,
  pub edit_buf: String,
}

/// Theme picker popup state. Grouped from `theme_picker_*` and
/// `custom_theme_editor` (the editor is logically a sub-state of the picker).
#[derive(Default)]
pub struct ThemePickerState {
  pub active: bool,
  pub cursor: usize,
  pub scroll: usize,
  /// Original theme before picker opened — restored on Esc cancel.
  pub original: Option<(ui_theme::ThemeId, Option<String>)>,
  pub custom_editor: Option<CustomThemeEditorState>,
}
