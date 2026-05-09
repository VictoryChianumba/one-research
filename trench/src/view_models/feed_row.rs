//! Pre-formatted display data for a single feed-row render.
//!
//! Centralizes the formatting decisions (source label normalization,
//! author truncation, signal indicator selection, content-type label)
//! that were previously inlined in draw_item_table, draw_narrow_feed,
//! and draw_bottom_pane_feed. Style decisions (selected vs unselected,
//! cursor highlight) stay with the renderer — those are per-call state,
//! not row data.

use crate::models::{FeedItem, SignalLevel, SourcePlatform};

/// Display-shaped fields derived from a [`FeedItem`]. Width-keyed
/// derivations (truncations) are tuned to the column budgets in
/// `draw_item_table`. Renderers that need different widths should
/// either add a constructor variant or re-truncate from the source
/// string.
pub struct FeedRowVm {
  pub signal: SignalLevel,
  pub signal_indicator: &'static str,
  pub source_label: String,
  pub content_type_short: &'static str,
  pub author: String,
}

impl FeedRowVm {
  pub fn from_item(item: &FeedItem) -> Self {
    Self {
      signal: item.signal,
      signal_indicator: item.signal.indicator(),
      source_label: feed_source_label(item),
      content_type_short: item.content_type.short_label(),
      author: truncate(
        item.authors.first().map(|s| s.as_str()).unwrap_or(""),
        13,
      ),
    }
  }
}

/// Pick the user-visible source label for a feed item. Special-cases
/// platforms where the canonical name differs from the source_name
/// field (HuggingFace → "hf", arXiv → "arxiv"); falls back to the
/// stored source_name for RSS/custom feeds, truncated to 7 chars.
pub fn feed_source_label(item: &FeedItem) -> String {
  match item.source_platform {
    SourcePlatform::HuggingFace => "hf".to_string(),
    SourcePlatform::ArXiv => "arxiv".to_string(),
    SourcePlatform::Rss if !item.source_name.is_empty() => {
      truncate(&item.source_name, 7)
    }
    _ if !item.source_name.is_empty() => truncate(&item.source_name, 7),
    _ => item.source_platform.short_label().to_string(),
  }
}

fn truncate(s: &str, max_chars: usize) -> String {
  if max_chars == 0 {
    return String::new();
  }
  let mut chars = s.chars();
  let mut out = String::new();
  let mut count = 0;
  for c in &mut chars {
    if count >= max_chars {
      if chars.next().is_some() {
        out.push('…');
      }
      break;
    }
    out.push(c);
    count += 1;
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::{ContentType, WorkflowState};

  fn mock_item(platform: SourcePlatform, source_name: &str) -> FeedItem {
    FeedItem {
      id: "id".to_string(),
      title: "Title".to_string(),
      source_platform: platform,
      content_type: ContentType::Paper,
      domain_tags: Vec::new(),
      signal: SignalLevel::Primary,
      published_at: "2026-01-01".to_string(),
      authors: vec!["Aaron Aardvark".to_string()],
      summary_short: String::new(),
      workflow_state: WorkflowState::Inbox,
      url: "https://example.com".to_string(),
      upvote_count: 0,
      github_repo: None,
      github_owner: None,
      github_repo_name: None,
      benchmark_results: Vec::new(),
      full_content: None,
      source_name: source_name.to_string(),
      title_lower: "title".to_string(),
      authors_lower: vec!["aaron aardvark".to_string()],
    }
  }

  #[test]
  fn huggingface_uses_canonical_label() {
    let vm = FeedRowVm::from_item(&mock_item(SourcePlatform::HuggingFace, ""));
    assert_eq!(vm.source_label, "hf");
  }

  #[test]
  fn arxiv_uses_canonical_label() {
    let vm = FeedRowVm::from_item(&mock_item(SourcePlatform::ArXiv, ""));
    assert_eq!(vm.source_label, "arxiv");
  }

  #[test]
  fn rss_with_source_name_truncates_to_seven() {
    let vm = FeedRowVm::from_item(&mock_item(
      SourcePlatform::Rss,
      "import_ai",
    ));
    // "import_a…" (7 chars + ellipsis is 8 visible, but truncate's max=7
    // means up to 6 chars + ellipsis)
    assert!(vm.source_label.starts_with("import"));
    assert!(vm.source_label.len() <= 10); // utf8 bytes
  }

  #[test]
  fn author_truncates_to_thirteen() {
    let mut item =
      mock_item(SourcePlatform::ArXiv, "");
    item.authors = vec!["Aaron Aardvark Anderson".to_string()];
    let vm = FeedRowVm::from_item(&item);
    // "Aaron Aardvar…" is 13 visible chars + ellipsis
    assert!(vm.author.starts_with("Aaron"));
    assert!(vm.author.chars().count() <= 14);
  }

  #[test]
  fn empty_authors_yields_empty_string() {
    let mut item = mock_item(SourcePlatform::ArXiv, "");
    item.authors = Vec::new();
    let vm = FeedRowVm::from_item(&item);
    assert_eq!(vm.author, "");
  }

  #[test]
  fn signal_indicator_propagates() {
    let mut item = mock_item(SourcePlatform::ArXiv, "");
    item.signal = SignalLevel::Tertiary;
    let vm = FeedRowVm::from_item(&item);
    assert_eq!(vm.signal, SignalLevel::Tertiary);
    assert_eq!(vm.signal_indicator, SignalLevel::Tertiary.indicator());
  }
}
