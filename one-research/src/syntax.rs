use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

static SS: OnceLock<SyntaxSet> = OnceLock::new();
static TS: OnceLock<ThemeSet> = OnceLock::new();

fn ss() -> &'static SyntaxSet {
  SS.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn ts() -> &'static ThemeSet {
  TS.get_or_init(ThemeSet::load_defaults)
}

/// Highlight `content` using the file extension from `filename`.
/// Returns a Vec of lines, each line a Vec of (r, g, b, text) spans.
/// Returns `None` for plain-text / unknown file types.
pub fn highlight_file(
  content: &str,
  filename: &str,
) -> Option<Vec<Vec<(u8, u8, u8, String)>>> {
  let ss = ss();
  let ts = ts();

  let ext = std::path::Path::new(filename)
    .extension()
    .and_then(|e| e.to_str())
    .unwrap_or("");

  let syntax = ss
    .find_syntax_by_extension(ext)
    .unwrap_or_else(|| ss.find_syntax_plain_text());

  if syntax.name == "Plain Text" {
    return None;
  }

  let theme = match ts.themes.get("base16-ocean.dark") {
    Some(t) => t,
    None => {
      return Some(
        content
          .lines()
          .map(|l| vec![(204u8, 204u8, 204u8, l.to_string())])
          .collect(),
      );
    }
  };

  let mut h = HighlightLines::new(syntax, theme);

  Some(
    content
      .lines()
      .map(|line| {
        let ranges = h.highlight_line(line, ss).unwrap_or_default();
        ranges
          .into_iter()
          .map(|(style, text)| {
            let fg = style.foreground;
            (fg.r, fg.g, fg.b, text.to_string())
          })
          .collect()
      })
      .collect(),
  )
}
