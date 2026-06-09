//! Terminal-safe text sanitization for any string that originates from a
//! network source and may end up rendered through ratatui to the user's
//! terminal.
//!
//! The threat model is that a hostile feed (RSS post body, a paper title on
//! HuggingFace, an arXiv `<arxiv:comment>` field, an OpenReview summary, or
//! the `content` of a Claude/OpenAI streaming response) embeds raw ANSI escape
//! sequences. Without sanitization, ratatui passes those bytes through to the
//! terminal, which interprets them — letting an attacker move the cursor,
//! clear the screen, hijack OSC 52 to write to the user's clipboard, or open
//! an OSC 8 hyperlink pointing at a phishing URL labelled with friendly text.
//!
//! `sanitize_terminal_text` is the single chokepoint helper. Apply at:
//! - ingestion-time on every `FeedItem` string field (one cost per item)
//! - cache-load on items already in `cache.json` (catches pre-fix data)
//! - chat-render on assistant message content (streaming bypasses FeedItem)
//!
//! Reading the bytes one-at-a-time over `chars()` is correct: ESC and CSI
//! introducers are pure ASCII (single-byte UTF-8), so byte-walking and
//! char-walking diverge only inside escape *payloads*, which we discard.
//! Multi-byte UTF-8 characters in the *visible* text pass through untouched.

/// Truncate `s` to at most `max` *characters* (not bytes), borrowing when
/// no truncation is needed and allocating only when it does.
///
/// `&s[..N]` byte-slicing panics if byte `N` falls in the middle of a
/// multi-byte UTF-8 sequence — which happens routinely on emoji, em-dash,
/// or accented characters in API error bodies. Use this everywhere we
/// slice API/error text.
///
/// Returns `Cow<str>` so the common case (input already short enough) is
/// allocation-free.
pub(crate) fn truncate_chars(s: &str, max: usize) -> std::borrow::Cow<'_, str> {
  if s.chars().count() <= max {
    std::borrow::Cow::Borrowed(s)
  } else {
    std::borrow::Cow::Owned(s.chars().take(max).collect())
  }
}

/// Strip every ANSI escape sequence, control byte, and DEL from `s`.
///
/// Preserves: printable text (including non-ASCII / multi-byte UTF-8),
/// horizontal tab, line feed, carriage return.
///
/// Removes:
/// - CSI: `ESC [ ... <final 0x40-0x7E>`
/// - OSC: `ESC ] ... (BEL | ST)` where ST = `ESC \`
/// - DCS: `ESC P ... ST`
/// - SOS / PM / APC: `ESC X|^|_ ... ST`
/// - Bare two-byte ESC introducers (e.g. `ESC c` reset, `ESC =` keypad mode)
/// - Stray trailing `ESC` with no follow-up byte
/// - Bare control bytes `0x00-0x1F` (other than `\t`, `\n`, `\r`) → replaced with a single space
/// - DEL `0x7F` → dropped
///
/// Idempotent: re-sanitizing already-clean text returns identical output.
pub(crate) fn sanitize_terminal_text(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  let mut chars = s.chars().peekable();
  while let Some(c) = chars.next() {
    match c {
      // Tab / LF / CR — always preserved.
      '\t' | '\n' | '\r' => out.push(c),

      // ESC introducer — discard the entire sequence.
      '\x1b' => {
        let Some(&next) = chars.peek() else {
          // Stray trailing ESC — drop it.
          continue;
        };
        match next {
          '[' => {
            // CSI: consume until a final byte 0x40-0x7E (@A-Z[\]^_`a-z{|}~).
            chars.next();
            for ch in chars.by_ref() {
              let b = ch as u32;
              if (0x40..=0x7E).contains(&b) {
                break;
              }
            }
          }
          ']' => {
            // OSC: consume until BEL or ST.
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
            // DCS / SOS / PM / APC: consume until ST.
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
            // Two-byte ESC sequence (e.g. `ESC c`, `ESC =`, `ESC >`,
            // `ESC \`, `ESC D`, etc.) — drop the introducer byte.
            chars.next();
          }
        }
      }

      // Other C0 control bytes — replace with a single space so word
      // boundaries are preserved without leaking the byte to the terminal.
      c if (c as u32) < 0x20 => out.push(' '),

      // DEL — drop entirely (visually noise, no semantic value in a feed).
      '\x7f' => {}

      // Everything else — printable, including non-ASCII multi-byte UTF-8.
      _ => out.push(c),
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use super::{sanitize_terminal_text, truncate_chars};

  #[test]
  fn truncate_chars_borrows_when_short_enough() {
    let s = "hello";
    let out = truncate_chars(s, 10);
    assert_eq!(out, "hello");
    // Should be a Borrowed variant — no allocation.
    assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
  }

  #[test]
  fn truncate_chars_caps_long_ascii() {
    let s = "abcdefghij";
    let out = truncate_chars(s, 5);
    assert_eq!(out, "abcde");
    assert!(matches!(out, std::borrow::Cow::Owned(_)));
  }

  #[test]
  fn truncate_chars_does_not_panic_on_emoji_boundary() {
    // 🚀 is 4 bytes (U+1F680). Byte-slicing `&s[..3]` would panic mid-emoji.
    let s = "abc🚀def";
    let out = truncate_chars(s, 3);
    assert_eq!(out, "abc");
  }

  #[test]
  fn truncate_chars_does_not_panic_on_emdash() {
    // — is 3 bytes (U+2014). Byte-slicing `&s[..2]` would panic mid-emdash.
    let s = "a\u{2014}b\u{2014}c";
    let out = truncate_chars(s, 2);
    assert_eq!(out, "a\u{2014}");
  }

  #[test]
  fn truncate_chars_handles_zero_max() {
    assert_eq!(truncate_chars("anything", 0), "");
    assert_eq!(truncate_chars("", 0), "");
  }

  #[test]
  fn truncate_chars_empty_input() {
    let out = truncate_chars("", 100);
    assert_eq!(out, "");
    assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
  }

  #[test]
  fn truncate_chars_idempotent() {
    let s = "café — naïve résumé";
    let once = truncate_chars(s, 10).into_owned();
    let twice = truncate_chars(&once, 10).into_owned();
    assert_eq!(once, twice);
  }

  #[test]
  fn passes_through_plain_ascii() {
    let s = "Hello world! 123 -- foo.bar/baz";
    assert_eq!(sanitize_terminal_text(s), s);
  }

  #[test]
  fn preserves_tab_lf_cr() {
    let s = "col1\tcol2\nline2\r\nline3";
    assert_eq!(sanitize_terminal_text(s), s);
  }

  #[test]
  fn preserves_multi_byte_utf8() {
    // Em-dash, smart quotes, accented chars, emoji.
    let s = "résumé — \u{201C}quote\u{201D} — café — 🚀 — Σ Ψ Φ";
    assert_eq!(sanitize_terminal_text(s), s);
  }

  #[test]
  fn strips_csi_clear_screen() {
    let s = "before\x1b[2J\x1b[Hafter";
    assert_eq!(sanitize_terminal_text(s), "beforeafter");
  }

  #[test]
  fn strips_csi_color_codes() {
    let s = "\x1b[31mred\x1b[0m and \x1b[1;32mbold green\x1b[0m";
    assert_eq!(sanitize_terminal_text(s), "red and bold green");
  }

  #[test]
  fn strips_osc_52_clipboard_payload() {
    // `ESC ] 52 ; c ; <base64> BEL` — the "write to user clipboard" OSC.
    let s = "harmless\x1b]52;c;cm0gLXJmIC8=\x07tail";
    assert_eq!(sanitize_terminal_text(s), "harmlesstail");
  }

  #[test]
  fn strips_osc_52_with_st_terminator() {
    // Same payload but terminated with ST (ESC \) instead of BEL.
    let s = "harmless\x1b]52;c;cm0gLXJmIC8=\x1b\\tail";
    assert_eq!(sanitize_terminal_text(s), "harmlesstail");
  }

  #[test]
  fn strips_osc_8_hyperlink_introducers() {
    // `ESC ] 8 ; ; <url> ST <text> ESC ] 8 ; ; ST` — clickable link.
    let s =
      "before\x1b]8;;https://evil.example/\x1b\\trusted\x1b]8;;\x1b\\after";
    // After stripping both OSC 8 sequences, only the visible text + before/after remain.
    assert_eq!(sanitize_terminal_text(s), "beforetrustedafter");
  }

  #[test]
  fn strips_dcs_sequence() {
    // `ESC P <payload> ST` — Device Control String (e.g. sixel).
    let s = "before\x1bP1;2;3pmalicious payload\x1b\\after";
    assert_eq!(sanitize_terminal_text(s), "beforeafter");
  }

  #[test]
  fn strips_apc_pm_sos_sequences() {
    let cases = [
      ("a\x1b_apc body\x1b\\b", "ab"), // APC
      ("a\x1b^pm body\x1b\\b", "ab"),  // PM
      ("a\x1bXsos body\x1b\\b", "ab"), // SOS
    ];
    for (input, expected) in cases {
      assert_eq!(sanitize_terminal_text(input), expected, "input: {input:?}");
    }
  }

  #[test]
  fn strips_two_byte_esc_sequences() {
    // `ESC c` = full reset, `ESC =` = keypad mode, `ESC D` = index, etc.
    let s = "a\x1bcb\x1b=c\x1bDd";
    assert_eq!(sanitize_terminal_text(s), "abcd");
  }

  #[test]
  fn drops_stray_trailing_esc() {
    let s = "trailing escape\x1b";
    assert_eq!(sanitize_terminal_text(s), "trailing escape");
  }

  #[test]
  fn drops_del_byte() {
    let s = "before\x7fafter";
    assert_eq!(sanitize_terminal_text(s), "beforeafter");
  }

  #[test]
  fn replaces_bare_control_bytes_with_space() {
    // 0x01 (SOH), 0x07 (BEL), 0x0C (FF) — all become spaces.
    let s = "a\x01b\x07c\x0cd";
    assert_eq!(sanitize_terminal_text(s), "a b c d");
  }

  #[test]
  fn idempotent() {
    let inputs = [
      "plain text",
      "\x1b[31mred\x1b[0m",
      "résumé — \u{201C}quote\u{201D}",
      "before\x1b]52;c;evil\x07after",
      "with\ttab\nand newline\r\n",
      "",
    ];
    for input in inputs {
      let once = sanitize_terminal_text(input);
      let twice = sanitize_terminal_text(&once);
      assert_eq!(once, twice, "not idempotent for input: {input:?}");
    }
  }

  #[test]
  fn handles_real_world_attack_payloads() {
    // 1. Title hijack via window-title OSC.
    let title_attack = "Paper Title\x1b]0;rm -rf /\x07";
    assert_eq!(sanitize_terminal_text(title_attack), "Paper Title");

    // 2. Hidden cursor (could be used to obscure injected text).
    let cursor_hide = "before\x1b[?25lafter";
    assert_eq!(sanitize_terminal_text(cursor_hide), "beforeafter");

    // 3. 8-bit color set + reset, mixed with prose.
    let color_mix =
      "Authors: \x1b[38;2;255;0;0mAlice\x1b[0m, \x1b[38;2;0;255;0mBob\x1b[0m";
    assert_eq!(sanitize_terminal_text(color_mix), "Authors: Alice, Bob");

    // 4. Concatenated CSI sequences with parameter bytes.
    let many_csi = "\x1b[2J\x1b[H\x1b[1;1H\x1b[?1049h\x1b[?25lhello";
    assert_eq!(sanitize_terminal_text(many_csi), "hello");
  }

  #[test]
  fn empty_input_returns_empty() {
    assert_eq!(sanitize_terminal_text(""), "");
  }

  #[test]
  fn handles_only_escape_sequences() {
    let only_escapes = "\x1b[2J\x1b]52;c;evil\x07\x1bP1pdcs\x1b\\";
    assert_eq!(sanitize_terminal_text(only_escapes), "");
  }

  #[test]
  fn preserves_multi_byte_utf8_around_escapes() {
    // Multi-byte chars adjacent to escape introducers — verify byte
    // boundaries aren't confused and only the escape sequence is stripped.
    let s = "café\x1b[31mrouge\x1b[0m\u{2014}suite";
    assert_eq!(sanitize_terminal_text(s), "caférouge\u{2014}suite");
  }
}
