use std::collections::HashMap;
use std::path::PathBuf;

use ratatui::style::Color;

/// Ordered list of predefined (non-custom) RSS sources.
pub const PREDEFINED_SOURCES: &[&str] = &[
  "huggingface",
  "openai",
  "deepmind",
  "import_ai",
  "bair",
  "mit_news_ai",
  "openreview",
  "core",
];

#[derive(Debug, serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct Config {
  #[serde(default)]
  pub github_token: Option<String>,
  #[serde(default)]
  pub semantic_scholar_key: Option<String>,
  #[serde(default)]
  pub claude_api_key: Option<String>,
  #[serde(default)]
  pub openai_api_key: Option<String>,
  #[serde(default)]
  pub core_api_key: Option<String>,
  #[serde(default)]
  pub perplexity_api_key: Option<String>,
  #[serde(default = "default_chat_provider")]
  pub default_chat_provider: String,
  #[serde(default)]
  pub sources: SourcesConfig,
  #[serde(default)]
  pub theme: ui_theme::ThemeId,
  #[serde(default)]
  pub active_custom_theme_id: Option<String>,
  #[serde(default)]
  pub custom_themes: Vec<CustomThemeConfig>,
}

fn default_chat_provider() -> String {
  "claude".to_string()
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct SourcesConfig {
  pub arxiv_categories: Vec<String>,
  pub enabled_sources: HashMap<String, bool>,
  pub custom_feeds: Vec<CustomFeed>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct CustomFeed {
  pub url: String,
  pub name: String,
  pub feed_type: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct CustomThemeConfig {
  pub id: String,
  pub name: String,
  pub base: ui_theme::ThemeId,
  #[serde(default)]
  pub colors: CustomThemeColors,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct CustomThemeColors {
  pub accent: String,
  pub header: String,
  pub text: String,
  pub text_dim: String,
  pub border: String,
  pub border_active: String,
  pub bg: String,
  pub bg_panel: String,
  pub bg_input: String,
  pub bg_selection: String,
  pub bg_code: String,
  pub bg_chat: String,
  pub bg_user_msg: String,
  pub bg_popup: String,
  pub text_on_accent: String,
  pub success: String,
  pub warning: String,
  pub error: String,
  pub math: String,
  pub mono: String,
  pub rule: String,
  pub toc_dim: String,
  pub bookmark_bg: String,
  pub cursor_bg: String,
  pub cursor_fg: String,
  pub search_match_bg: String,
  pub search_match_fg: String,
}

#[derive(Debug, Clone, Copy)]
pub struct CustomThemeRole {
  pub key: &'static str,
  pub label: &'static str,
}

pub const CUSTOM_THEME_ROLES: &[CustomThemeRole] = &[
  CustomThemeRole { key: "accent", label: "Accent" },
  CustomThemeRole { key: "header", label: "Headers" },
  CustomThemeRole { key: "text", label: "Text" },
  CustomThemeRole { key: "text_dim", label: "Dim text" },
  CustomThemeRole { key: "border", label: "Border" },
  CustomThemeRole { key: "border_active", label: "Active border" },
  CustomThemeRole { key: "bg", label: "Background" },
  CustomThemeRole { key: "bg_panel", label: "Panels" },
  CustomThemeRole { key: "bg_input", label: "Inputs" },
  CustomThemeRole { key: "bg_selection", label: "Selection" },
  CustomThemeRole { key: "bg_popup", label: "Popups" },
  CustomThemeRole { key: "success", label: "Success" },
  CustomThemeRole { key: "warning", label: "Warning" },
  CustomThemeRole { key: "error", label: "Error" },
];

impl CustomThemeConfig {
  pub fn from_theme(
    id: String,
    name: String,
    base: ui_theme::ThemeId,
    theme: ui_theme::Theme,
  ) -> Self {
    Self { id, name, base, colors: CustomThemeColors::from_theme(theme) }
  }

  pub fn to_theme(&self) -> ui_theme::Theme {
    self.colors.to_theme(self.base.theme())
  }
}

impl CustomThemeColors {
  pub fn from_theme(t: ui_theme::Theme) -> Self {
    Self {
      accent: color_to_hex(t.accent),
      header: color_to_hex(t.header),
      text: color_to_hex(t.text),
      text_dim: color_to_hex(t.text_dim),
      border: color_to_hex(t.border),
      border_active: color_to_hex(t.border_active),
      bg: color_to_hex(t.bg),
      bg_panel: color_to_hex(t.bg_panel),
      bg_input: color_to_hex(t.bg_input),
      bg_selection: color_to_hex(t.bg_selection),
      bg_code: color_to_hex(t.bg_code),
      bg_chat: color_to_hex(t.bg_chat),
      bg_user_msg: color_to_hex(t.bg_user_msg),
      bg_popup: color_to_hex(t.bg_popup),
      text_on_accent: color_to_hex(t.text_on_accent),
      success: color_to_hex(t.success),
      warning: color_to_hex(t.warning),
      error: color_to_hex(t.error),
      math: color_to_hex(t.math),
      mono: color_to_hex(t.mono),
      rule: color_to_hex(t.rule),
      toc_dim: color_to_hex(t.toc_dim),
      bookmark_bg: color_to_hex(t.bookmark_bg),
      cursor_bg: color_to_hex(t.cursor_bg),
      cursor_fg: color_to_hex(t.cursor_fg),
      search_match_bg: color_to_hex(t.search_match_bg),
      search_match_fg: color_to_hex(t.search_match_fg),
    }
  }

  pub fn to_theme(&self, fallback: ui_theme::Theme) -> ui_theme::Theme {
    ui_theme::Theme {
      accent: parse_hex_color(&self.accent).unwrap_or(fallback.accent),
      header: parse_hex_color(&self.header).unwrap_or(fallback.header),
      text: parse_hex_color(&self.text).unwrap_or(fallback.text),
      text_dim: parse_hex_color(&self.text_dim).unwrap_or(fallback.text_dim),
      border: parse_hex_color(&self.border).unwrap_or(fallback.border),
      border_active: parse_hex_color(&self.border_active)
        .unwrap_or(fallback.border_active),
      bg: parse_hex_color(&self.bg).unwrap_or(fallback.bg),
      bg_panel: parse_hex_color(&self.bg_panel).unwrap_or(fallback.bg_panel),
      bg_input: parse_hex_color(&self.bg_input).unwrap_or(fallback.bg_input),
      bg_selection: parse_hex_color(&self.bg_selection)
        .unwrap_or(fallback.bg_selection),
      bg_code: parse_hex_color(&self.bg_code).unwrap_or(fallback.bg_code),
      bg_chat: parse_hex_color(&self.bg_chat).unwrap_or(fallback.bg_chat),
      bg_user_msg: parse_hex_color(&self.bg_user_msg)
        .unwrap_or(fallback.bg_user_msg),
      bg_popup: parse_hex_color(&self.bg_popup).unwrap_or(fallback.bg_popup),
      text_on_accent: parse_hex_color(&self.text_on_accent)
        .unwrap_or(fallback.text_on_accent),
      success: parse_hex_color(&self.success).unwrap_or(fallback.success),
      warning: parse_hex_color(&self.warning).unwrap_or(fallback.warning),
      error: parse_hex_color(&self.error).unwrap_or(fallback.error),
      math: parse_hex_color(&self.math).unwrap_or(fallback.math),
      mono: parse_hex_color(&self.mono).unwrap_or(fallback.mono),
      rule: parse_hex_color(&self.rule).unwrap_or(fallback.rule),
      toc_dim: parse_hex_color(&self.toc_dim).unwrap_or(fallback.toc_dim),
      bookmark_bg: parse_hex_color(&self.bookmark_bg)
        .unwrap_or(fallback.bookmark_bg),
      cursor_bg: parse_hex_color(&self.cursor_bg).unwrap_or(fallback.cursor_bg),
      cursor_fg: parse_hex_color(&self.cursor_fg).unwrap_or(fallback.cursor_fg),
      search_match_bg: parse_hex_color(&self.search_match_bg)
        .unwrap_or(fallback.search_match_bg),
      search_match_fg: parse_hex_color(&self.search_match_fg)
        .unwrap_or(fallback.search_match_fg),
    }
  }

  pub fn get_role(&self, key: &str) -> Option<&str> {
    Some(match key {
      "accent" => &self.accent,
      "header" => &self.header,
      "text" => &self.text,
      "text_dim" => &self.text_dim,
      "border" => &self.border,
      "border_active" => &self.border_active,
      "bg" => &self.bg,
      "bg_panel" => &self.bg_panel,
      "bg_input" => &self.bg_input,
      "bg_selection" => &self.bg_selection,
      "bg_popup" => &self.bg_popup,
      "success" => &self.success,
      "warning" => &self.warning,
      "error" => &self.error,
      _ => return None,
    })
  }

  pub fn set_role(&mut self, key: &str, value: String) -> bool {
    let slot = match key {
      "accent" => &mut self.accent,
      "header" => &mut self.header,
      "text" => &mut self.text,
      "text_dim" => &mut self.text_dim,
      "border" => &mut self.border,
      "border_active" => &mut self.border_active,
      "bg" => &mut self.bg,
      "bg_panel" => &mut self.bg_panel,
      "bg_input" => &mut self.bg_input,
      "bg_selection" => &mut self.bg_selection,
      "bg_popup" => &mut self.bg_popup,
      "success" => &mut self.success,
      "warning" => &mut self.warning,
      "error" => &mut self.error,
      _ => return false,
    };
    *slot = value;
    true
  }
}

impl Default for CustomThemeColors {
  fn default() -> Self {
    Self::from_theme(ui_theme::ThemeId::Dark.theme())
  }
}

pub fn color_to_hex(color: Color) -> String {
  match color {
    Color::Rgb(r, g, b) => format!("#{r:02X}{g:02X}{b:02X}"),
    Color::Black => "#000000".to_string(),
    Color::White => "#FFFFFF".to_string(),
    Color::Red => "#FF0000".to_string(),
    Color::Green => "#00FF00".to_string(),
    Color::Blue => "#0000FF".to_string(),
    Color::Yellow => "#FFFF00".to_string(),
    Color::Cyan => "#00FFFF".to_string(),
    Color::Magenta => "#FF00FF".to_string(),
    Color::Gray => "#808080".to_string(),
    Color::DarkGray => "#404040".to_string(),
    Color::LightRed => "#FF6666".to_string(),
    Color::LightGreen => "#66FF66".to_string(),
    Color::LightBlue => "#6666FF".to_string(),
    Color::LightYellow => "#FFFF66".to_string(),
    Color::LightCyan => "#66FFFF".to_string(),
    Color::LightMagenta => "#FF66FF".to_string(),
    _ => "#000000".to_string(),
  }
}

pub fn parse_hex_color(value: &str) -> Option<Color> {
  let hex = value.strip_prefix('#').unwrap_or(value);
  if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
    return None;
  }
  let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
  let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
  let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
  Some(Color::Rgb(r, g, b))
}

impl Default for SourcesConfig {
  fn default() -> Self {
    Self {
      arxiv_categories: vec![
        "cs.LG".to_string(),
        "cs.AI".to_string(),
        "stat.ML".to_string(),
      ],
      enabled_sources: HashMap::from([
        ("huggingface".to_string(), true),
        ("openai".to_string(), true),
        ("deepmind".to_string(), true),
        ("import_ai".to_string(), true),
        ("bair".to_string(), true),
        ("mit_news_ai".to_string(), true),
        ("openreview".to_string(), true),
        ("core".to_string(), false), // requires API key — disabled until configured
      ]),
      custom_feeds: vec![],
    }
  }
}

impl Config {
  pub fn load() -> Self {
    let path = match config_path() {
      Some(p) => p,
      None => return Config::default(),
    };
    let bytes = match std::fs::read(&path) {
      Ok(b) => b,
      Err(_) => return Config::default(),
    };
    match serde_json::from_slice(&bytes) {
      Ok(v) => v,
      Err(e) => {
        crate::store::quarantine_corrupted(&path, "one-research/config", &e);
        Config::default()
      }
    }
  }

  pub fn save(&self) {
    let path = match config_path() {
      Some(p) => p,
      None => return,
    };
    if let Some(parent) = path.parent() {
      let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec(self) {
      if let Err(e) = crate::store::atomic_write(&path, &json) {
        log::error!("config: failed to write {}: {e}", path.display());
      } else {
        crate::store::set_private(&path);
        log::debug!("config: wrote {}", path.display());
      }
    }
  }
}

fn config_path() -> Option<PathBuf> {
  Some(dirs::home_dir()?.join(".config/one-research/config.json"))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_hex_colors_with_or_without_hash() {
    assert_eq!(parse_hex_color("#0A1B2C"), Some(Color::Rgb(10, 27, 44)));
    assert_eq!(parse_hex_color("0a1b2c"), Some(Color::Rgb(10, 27, 44)));
    assert_eq!(parse_hex_color("#XYZ123"), None);
    assert_eq!(parse_hex_color("#123"), None);
  }

  #[test]
  fn serializes_and_loads_custom_theme_fields() {
    let base = ui_theme::ThemeId::Dark;
    let custom = CustomThemeConfig::from_theme(
      "custom-test".to_string(),
      "My Theme".to_string(),
      base,
      base.theme(),
    );
    let cfg = Config {
      theme: base,
      active_custom_theme_id: Some(custom.id.clone()),
      custom_themes: vec![custom],
      ..Config::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let decoded: Config = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.active_custom_theme_id.as_deref(), Some("custom-test"));
    assert_eq!(decoded.custom_themes[0].name, "My Theme");
    assert_eq!(
      decoded.custom_themes[0].colors.accent,
      color_to_hex(base.theme().accent)
    );
  }
}
