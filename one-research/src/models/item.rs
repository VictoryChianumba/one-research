#[derive(
  Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SourcePlatform {
  ArXiv,
  Twitter,
  Blog,
  Newsletter,
  HuggingFace,
  Rss,
  OpenReview,
  Core,
}

impl SourcePlatform {
  pub fn short_label(&self) -> &'static str {
    match self {
      Self::ArXiv => "arXiv",
      Self::Twitter => "twit",
      Self::Blog => "blog",
      Self::Newsletter => "news",
      Self::HuggingFace => "hf",
      Self::Rss => "rss",
      Self::OpenReview => "OR",
      Self::Core => "CORE",
    }
  }
}

#[derive(
  Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ContentType {
  Paper,
  Thread,
  Article,
  Repo,
  Digest,
}

impl ContentType {
  pub fn short_label(&self) -> &'static str {
    match self {
      Self::Paper => "paper",
      Self::Thread => "thread",
      Self::Article => "article",
      Self::Repo => "repo",
      Self::Digest => "digest",
    }
  }
}

#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SignalLevel {
  Primary,
  Secondary,
  Tertiary,
}

impl SignalLevel {
  pub fn indicator(&self) -> &'static str {
    match self {
      Self::Primary => "*",
      Self::Secondary => "·",
      Self::Tertiary => " ",
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowState {
  Inbox,
  Queued,
  DeepRead,
  Archived,
}

impl WorkflowState {
  pub fn short_label(&self) -> &'static str {
    match self {
      Self::Inbox => "inbox",
      Self::Queued => "queued",
      Self::DeepRead => "read",
      Self::Archived => "archived",
    }
  }
}

// Manual Deserialize that maps unknown values (legacy variants like
// `"skimmed"`, or anything else) to `Inbox`. This prevents a single removed
// or renamed enum variant from failing to deserialize the whole state file
// and silently zeroing user workflow tags via `unwrap_or_default()`.
impl<'de> serde::Deserialize<'de> for WorkflowState {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    let s = String::deserialize(deserializer)?;
    Ok(match s.as_str() {
      "inbox" => Self::Inbox,
      "queued" => Self::Queued,
      "deepread" => Self::DeepRead,
      "archived" => Self::Archived,
      _ => Self::Inbox,
    })
  }
}

#[cfg(test)]
mod workflow_state_tests {
  use super::WorkflowState;

  #[test]
  fn deserializes_known_variants() {
    let cases = [
      ("\"inbox\"", WorkflowState::Inbox),
      ("\"queued\"", WorkflowState::Queued),
      ("\"deepread\"", WorkflowState::DeepRead),
      ("\"archived\"", WorkflowState::Archived),
    ];
    for (json, expected) in cases {
      let got: WorkflowState = serde_json::from_str(json).unwrap();
      assert_eq!(got, expected, "input: {json}");
    }
  }

  #[test]
  fn deserializes_legacy_skimmed_to_inbox() {
    let got: WorkflowState = serde_json::from_str("\"skimmed\"").unwrap();
    assert_eq!(got, WorkflowState::Inbox);
  }

  #[test]
  fn deserializes_unknown_to_inbox() {
    for s in ["\"garbage\"", "\"\"", "\"INBOX\"", "\"future_variant\""] {
      let got: WorkflowState = serde_json::from_str(s).unwrap();
      assert_eq!(got, WorkflowState::Inbox, "input: {s}");
    }
  }

  #[test]
  fn serializes_to_lowercase_variant_name() {
    assert_eq!(
      serde_json::to_string(&WorkflowState::Inbox).unwrap(),
      "\"inbox\""
    );
    assert_eq!(
      serde_json::to_string(&WorkflowState::Queued).unwrap(),
      "\"queued\""
    );
    assert_eq!(
      serde_json::to_string(&WorkflowState::DeepRead).unwrap(),
      "\"deepread\""
    );
    assert_eq!(
      serde_json::to_string(&WorkflowState::Archived).unwrap(),
      "\"archived\""
    );
  }

  #[test]
  fn sanitize_in_place_populates_lowercase_search_cache() {
    use super::{
      ContentType, FeedItem, SignalLevel, SourcePlatform, WorkflowState,
    };
    let mut item = FeedItem {
      id: "x".into(),
      title: "Mixed CASE Title".into(),
      source_platform: SourcePlatform::ArXiv,
      content_type: ContentType::Paper,
      domain_tags: vec![],
      signal: SignalLevel::Primary,
      published_at: String::new(),
      authors: vec!["Alice Smith".into(), "BOB JONES".into()],
      summary_short: String::new(),
      workflow_state: WorkflowState::Inbox,
      url: "u".into(),
      upvote_count: 0,
      github_repo: None,
      github_owner: None,
      github_repo_name: None,
      benchmark_results: vec![],
      full_content: None,
      source_name: String::new(),
      title_lower: String::new(),
      authors_lower: Vec::new(),
    };
    item.sanitize_in_place();
    assert_eq!(item.title_lower, "mixed case title");
    assert_eq!(
      item.authors_lower,
      vec!["alice smith".to_string(), "bob jones".to_string()]
    );
  }

  #[test]
  fn cached_lowercase_fields_are_serde_skip() {
    use super::{
      ContentType, FeedItem, SignalLevel, SourcePlatform, WorkflowState,
    };
    let item = FeedItem {
      id: "x".into(),
      title: "T".into(),
      source_platform: SourcePlatform::ArXiv,
      content_type: ContentType::Paper,
      domain_tags: vec![],
      signal: SignalLevel::Primary,
      published_at: String::new(),
      authors: vec![],
      summary_short: String::new(),
      workflow_state: WorkflowState::Inbox,
      url: "u".into(),
      upvote_count: 0,
      github_repo: None,
      github_owner: None,
      github_repo_name: None,
      benchmark_results: vec![],
      full_content: None,
      source_name: String::new(),
      title_lower: "this should not appear in the json".into(),
      authors_lower: vec!["nor should this".into()],
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(
      !json.contains("this should not appear"),
      "title_lower leaked into serialized output: {json}"
    );
    assert!(
      !json.contains("nor should this"),
      "authors_lower leaked into serialized output: {json}"
    );
    // Round-trip should produce empty cache fields (default), since the
    // serialized form omits them and Deserialize fills with Default.
    let back: FeedItem = serde_json::from_str(&json).unwrap();
    assert_eq!(back.title_lower, "");
    assert_eq!(back.authors_lower, Vec::<String>::new());
  }

  #[test]
  fn round_trips_via_json() {
    for v in [
      WorkflowState::Inbox,
      WorkflowState::Queued,
      WorkflowState::DeepRead,
      WorkflowState::Archived,
    ] {
      let s = serde_json::to_string(&v).unwrap();
      let back: WorkflowState = serde_json::from_str(&s).unwrap();
      assert_eq!(v, back);
    }
  }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchmarkResult {
  pub task: String,
  pub dataset: String,
  pub metric: String,
  pub score: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FeedItem {
  pub id: String,
  pub title: String,
  pub source_platform: SourcePlatform,
  pub content_type: ContentType,
  pub domain_tags: Vec<String>,
  pub signal: SignalLevel,
  pub published_at: String,
  pub authors: Vec<String>,
  pub summary_short: String,
  pub workflow_state: WorkflowState,
  pub url: String,
  #[serde(default)]
  pub upvote_count: u32,
  #[serde(default)]
  pub github_repo: Option<String>,
  #[serde(default)]
  pub github_owner: Option<String>,
  #[serde(default)]
  pub github_repo_name: Option<String>,
  #[serde(default)]
  pub benchmark_results: Vec<BenchmarkResult>,
  #[serde(default)]
  pub full_content: Option<String>,
  #[serde(default)]
  pub source_name: String,

  /// Lowercased title cached for the search filter. `#[serde(skip)]` keeps
  /// `cache.json` size unchanged; populated by `sanitize_in_place` on every
  /// ingestion + every cache load. Eliminates ~13K `to_lowercase` allocs
  /// per typed search character on a ~2,600-item cache.
  #[serde(skip)]
  pub title_lower: String,
  /// Lowercased author names, parallel to `self.authors`. Same role as
  /// `title_lower`.
  #[serde(skip)]
  pub authors_lower: Vec<String>,
}

impl FeedItem {
  /// Derive signal level from source and upvotes.
  pub fn compute_signal(&self) -> SignalLevel {
    match self.source_platform {
      SourcePlatform::ArXiv => SignalLevel::Primary,
      SourcePlatform::HuggingFace => {
        if self.upvote_count >= 10 {
          SignalLevel::Primary
        } else if self.upvote_count >= 1 {
          SignalLevel::Secondary
        } else {
          SignalLevel::Tertiary
        }
      }
      SourcePlatform::Blog
      | SourcePlatform::Twitter
      | SourcePlatform::OpenReview => SignalLevel::Secondary,
      SourcePlatform::Newsletter
      | SourcePlatform::Rss
      | SourcePlatform::Core => SignalLevel::Tertiary,
    }
  }

  /// Strip terminal escape sequences and bare control bytes from every string
  /// field that originates from a network source. Should be called once at
  /// ingestion (after the FeedItem is fully assembled) and again at cache
  /// load (defense-in-depth for items persisted before this fix shipped).
  ///
  /// Idempotent: re-sanitizing already-clean text leaves it unchanged.
  ///
  /// `url` and `id` are included even though they're typically app-constructed:
  /// RSS ingestion writes the raw `<link>` href (or text body) directly into
  /// `url`, so a hostile feed can ship ESC bytes that survive into the cache
  /// and render in the details pane / clipboard / repo viewer (audit Sec
  /// HIGH #2, LOW #19).
  pub fn sanitize_in_place(&mut self) {
    use crate::sanitize::sanitize_terminal_text;
    self.title = sanitize_terminal_text(&self.title);
    self.summary_short = sanitize_terminal_text(&self.summary_short);
    self.source_name = sanitize_terminal_text(&self.source_name);
    self.url = sanitize_terminal_text(&self.url);
    self.id = sanitize_terminal_text(&self.id);
    for author in &mut self.authors {
      *author = sanitize_terminal_text(author);
    }
    for tag in &mut self.domain_tags {
      *tag = sanitize_terminal_text(tag);
    }
    if let Some(content) = &mut self.full_content {
      *content = sanitize_terminal_text(content);
    }
    // Refresh the search-filter cache. Both fields are #[serde(skip)] so
    // they need to be rebuilt at every entry point that produces or loads
    // a FeedItem; sanitize_in_place is the natural chokepoint.
    self.title_lower = self.title.to_lowercase();
    self.authors_lower =
      self.authors.iter().map(|a| a.to_lowercase()).collect();
  }
}
