pub mod arxiv;
pub mod core;
pub mod crossref;
pub mod fulltext;
pub mod huggingface;
pub mod message;
pub mod openreview;
pub mod pipeline;
pub mod registry;
pub mod rss;
pub mod semantic_scholar;

pub(super) fn collapse_whitespace(s: &str) -> String {
  s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Reassembles one XML text node from quick-xml ≥ 0.38 events.
///
/// Since 0.38, quick-xml delivers `a &amp; b` as `Text("a ")`,
/// `GeneralRef("amp")`, `Text(" b")`. The parsers here were written against
/// 0.36, where that was a single `Text` event that callers `unescape()`d
/// and the reader trimmed. Feeding the fragments through this buffer and
/// taking the node at the next non-text event reproduces the 0.36
/// semantics exactly: predefined entities and character references
/// resolved, leading/trailing whitespace trimmed, whitespace-only nodes
/// dropped. Readers must use `trim_text(false)` — reader-side trimming
/// would trim each fragment and eat the spaces around entities.
///
/// One 0.36 quirk is preserved deliberately: an entity outside the XML
/// predefined set (e.g. `&nbsp;`) made `unescape()` fail and callers'
/// `unwrap_or_default()` blanked the whole node, so it blanks here too.
#[derive(Default)]
pub(super) struct XmlTextNode {
  buf: String,
  unresolvable: bool,
}

impl XmlTextNode {
  pub(super) fn push_text(&mut self, e: &quick_xml::events::BytesText) {
    self.buf.push_str(e);
  }

  pub(super) fn push_ref(&mut self, e: &quick_xml::events::BytesRef) {
    let resolved = match e.resolve_char_ref() {
      Ok(Some(c)) => Some(c),
      Ok(None) => match &**e {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "apos" => Some('\''),
        "quot" => Some('"'),
        _ => None,
      },
      Err(_) => None,
    };
    match resolved {
      Some(c) => self.buf.push(c),
      None => self.unresolvable = true,
    }
  }

  /// Takes the completed node, leaving the buffer empty. `None` when the
  /// node is empty after trimming (the 0.36 reader never emitted those).
  pub(super) fn take(&mut self) -> Option<String> {
    let unresolvable = std::mem::take(&mut self.unresolvable);
    let buf = std::mem::take(&mut self.buf);
    let text = buf.trim();
    if text.is_empty() {
      return None;
    }
    Some(if unresolvable { String::new() } else { text.to_string() })
  }
}
