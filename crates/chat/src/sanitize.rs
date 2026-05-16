//! Terminal-safe text sanitization for streamed chat content.
//!
//! Mirrors `trench/src/sanitize.rs` exactly. We keep a local copy here rather
//! than introducing a cross-crate dependency on trench internals — the helper
//! is a few dozen lines and changes rarely.
//!
//! Streamed assistant responses (Claude / OpenAI / Perplexity) bypass the
//! `FeedItem` ingestion path, so the chat render must sanitize independently.
//! User-typed messages are also sanitized — pasting hostile text into the
//! prompt could otherwise hijack the user's own terminal on render.

/// True if `s` is safe to use as a filename component on disk. Rejects
/// path separators, parent-directory traversal, leading dots, and length
/// outliers. Accepts UUIDs, nanosecond timestamps, and any future
/// alphanumeric-with-hyphens identifier without requiring a migration of
/// existing notes / chat sessions.
///
/// This is the gate at every `note_path` / `session_path` entry point —
/// without it, a malicious imported JSON with `note_id = "../../etc/foo"`
/// would resolve to a path outside the notes/chat directory (Sec MED
/// #20-21 from the code review).
pub(crate) fn is_safe_id(s: &str) -> bool {
  if s.is_empty() || s.len() > 64 {
    return false;
  }
  if s == "." || s == ".." || s.starts_with('.') {
    return false;
  }
  s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Truncate `s` to at most `max` *characters* (not bytes), borrowing when
/// no truncation is needed. Mirrors `trench::sanitize::truncate_chars` —
/// see that doc for the threat model. Local copy here to avoid a chat →
/// trench dependency edge.
pub(crate) fn truncate_chars(s: &str, max: usize) -> std::borrow::Cow<'_, str> {
  if s.chars().count() <= max {
    std::borrow::Cow::Borrowed(s)
  } else {
    std::borrow::Cow::Owned(s.chars().take(max).collect())
  }
}

/// Strip every ANSI escape sequence, control byte, and DEL from `s`.
/// See `trench::sanitize::sanitize_terminal_text` for the full specification.
pub(crate) fn sanitize_terminal_text(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  let mut chars = s.chars().peekable();
  while let Some(c) = chars.next() {
    match c {
      '\t' | '\n' | '\r' => out.push(c),
      '\x1b' => {
        let Some(&next) = chars.peek() else {
          continue;
        };
        match next {
          '[' => {
            chars.next();
            for ch in chars.by_ref() {
              let b = ch as u32;
              if (0x40..=0x7E).contains(&b) {
                break;
              }
            }
          }
          ']' => {
            chars.next();
            let mut prev = '\0';
            for ch in chars.by_ref() {
              if ch == '\x07' {
                break;
              }
              if prev == '\x1b' && ch == '\\' {
                break;
              }
              prev = ch;
            }
          }
          'P' | 'X' | '^' | '_' => {
            chars.next();
            let mut prev = '\0';
            for ch in chars.by_ref() {
              if prev == '\x1b' && ch == '\\' {
                break;
              }
              prev = ch;
            }
          }
          _ => {
            chars.next();
          }
        }
      }
      c if (c as u32) < 0x20 => out.push(' '),
      '\x7f' => {}
      _ => out.push(c),
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use super::{is_safe_id, sanitize_terminal_text, truncate_chars};

  #[test]
  fn is_safe_id_accepts_uuid() {
    assert!(is_safe_id("550e8400-e29b-41d4-a716-446655440000"));
  }

  #[test]
  fn is_safe_id_accepts_alphanumerics_and_hyphens_underscores() {
    assert!(is_safe_id("abc123"));
    assert!(is_safe_id("session_42-test"));
    assert!(is_safe_id("UPPER-and-lower_123"));
  }

  #[test]
  fn is_safe_id_rejects_traversal() {
    assert!(!is_safe_id(".."));
    assert!(!is_safe_id("../etc"));
    assert!(!is_safe_id("foo/bar"));
    assert!(!is_safe_id("foo\\bar"));
  }

  #[test]
  fn is_safe_id_rejects_leading_dot_and_dot() {
    assert!(!is_safe_id("."));
    assert!(!is_safe_id(".hidden"));
  }

  #[test]
  fn is_safe_id_rejects_empty_and_oversized() {
    assert!(!is_safe_id(""));
    assert!(!is_safe_id(&"a".repeat(65)));
  }

  #[test]
  fn is_safe_id_rejects_special_chars() {
    assert!(!is_safe_id("foo bar"));
    assert!(!is_safe_id("foo:bar"));
    assert!(!is_safe_id("foo|bar"));
    assert!(!is_safe_id("foo$bar"));
    assert!(!is_safe_id("foo\nbar"));
  }

  #[test]
  fn truncate_chars_borrows_short() {
    let out = truncate_chars("hi", 10);
    assert_eq!(out, "hi");
    assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
  }

  #[test]
  fn truncate_chars_handles_emoji() {
    // Byte-slicing `&s[..3]` would panic mid-emoji; char-based truncation
    // is safe.
    let out = truncate_chars("abc🚀def", 3);
    assert_eq!(out, "abc");
  }

  #[test]
  fn truncate_chars_handles_emdash() {
    let out = truncate_chars("a\u{2014}b", 2);
    assert_eq!(out, "a\u{2014}");
  }

  #[test]
  fn truncate_chars_zero() {
    assert_eq!(truncate_chars("hello", 0), "");
  }

  #[test]
  fn strips_csi_and_osc_in_assistant_response() {
    let s = "Sure, here's the answer:\x1b[2J\x1b]52;c;evil\x07 done";
    assert_eq!(sanitize_terminal_text(s), "Sure, here's the answer: done");
  }

  #[test]
  fn preserves_markdown_and_unicode() {
    let s = "## Heading\n- item with `code` and *emphasis*\n— em-dash 🚀";
    assert_eq!(sanitize_terminal_text(s), s);
  }

  #[test]
  fn idempotent() {
    let s = "before\x1b[31mred\x1b[0mafter";
    let once = sanitize_terminal_text(s);
    let twice = sanitize_terminal_text(&once);
    assert_eq!(once, twice);
  }
}
