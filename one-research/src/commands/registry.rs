use chat::ChatSlashCommandSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCategory {
  Discovery,
  Sources,
  Planned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
  BuiltIn,
  Workflow,
  Stub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandId {
  ClearChat,
  Discover,
  ClearDiscoveries,
  ClearHistory,
  AddArxivCategory,
  AddFeed,
  Sota,
  ReadingList,
  Code,
  Compare,
  Digest,
  Author,
  Trending,
  Watch,
  ExportHistory,
  ExportLibrary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
  pub id: CommandId,
  pub command: &'static str,
  pub completion: &'static str,
  pub description: &'static str,
  pub category: CommandCategory,
  pub kind: CommandKind,
  /// Show this command in the discovery search bar palette.
  pub show_in_discovery: bool,
}

pub const COMMAND_SPECS: &[CommandSpec] = &[
  CommandSpec {
    id: CommandId::ClearChat,
    command: "/clear",
    completion: "/clear",
    description: "Clear the current chat session view",
    category: CommandCategory::Discovery,
    kind: CommandKind::BuiltIn,
    show_in_discovery: false,
  },
  CommandSpec {
    id: CommandId::Discover,
    command: "/discover",
    completion: "/discover ",
    description: "Find papers and sources for a topic",
    category: CommandCategory::Discovery,
    kind: CommandKind::Workflow,
    show_in_discovery: true,
  },
  CommandSpec {
    id: CommandId::ClearDiscoveries,
    command: "/clear discoveries",
    completion: "/clear discoveries",
    description: "Clear the discovery feed",
    category: CommandCategory::Discovery,
    kind: CommandKind::BuiltIn,
    show_in_discovery: false,
  },
  CommandSpec {
    id: CommandId::ClearHistory,
    command: "/clear history",
    completion: "/clear history",
    description: "Wipe the activity history",
    category: CommandCategory::Discovery,
    kind: CommandKind::BuiltIn,
    show_in_discovery: false,
  },
  CommandSpec {
    id: CommandId::AddArxivCategory,
    command: "/add",
    completion: "/add ",
    description: "Add an arXiv category permanently",
    category: CommandCategory::Sources,
    kind: CommandKind::BuiltIn,
    show_in_discovery: false,
  },
  CommandSpec {
    id: CommandId::AddFeed,
    command: "/add-feed",
    completion: "/add-feed ",
    description: "Add an RSS feed permanently",
    category: CommandCategory::Sources,
    kind: CommandKind::BuiltIn,
    show_in_discovery: false,
  },
  CommandSpec {
    id: CommandId::Sota,
    command: "/sota",
    completion: "/sota ",
    description: "State-of-the-art results and benchmark comparison",
    category: CommandCategory::Discovery,
    kind: CommandKind::Workflow,
    show_in_discovery: true,
  },
  CommandSpec {
    id: CommandId::ReadingList,
    command: "/reading-list",
    completion: "/reading-list ",
    description: "Ordered learning path for a topic",
    category: CommandCategory::Discovery,
    kind: CommandKind::Workflow,
    show_in_discovery: true,
  },
  CommandSpec {
    id: CommandId::Code,
    command: "/code",
    completion: "/code ",
    description: "Find implementations and code for a topic",
    category: CommandCategory::Discovery,
    kind: CommandKind::Workflow,
    show_in_discovery: true,
  },
  CommandSpec {
    id: CommandId::Compare,
    command: "/compare",
    completion: "/compare ",
    description: "Side-by-side comparison of two approaches or models",
    category: CommandCategory::Discovery,
    kind: CommandKind::Workflow,
    show_in_discovery: true,
  },
  CommandSpec {
    id: CommandId::Digest,
    command: "/digest",
    completion: "/digest",
    description: "What happened in AI/ML this week",
    category: CommandCategory::Discovery,
    kind: CommandKind::Workflow,
    show_in_discovery: true,
  },
  CommandSpec {
    id: CommandId::Author,
    command: "/author",
    completion: "/author ",
    description: "Find all papers by a specific researcher",
    category: CommandCategory::Discovery,
    kind: CommandKind::Workflow,
    show_in_discovery: true,
  },
  CommandSpec {
    id: CommandId::Trending,
    command: "/trending",
    completion: "/trending ",
    description: "Find trending papers on a topic",
    category: CommandCategory::Discovery,
    kind: CommandKind::Workflow,
    show_in_discovery: true,
  },
  CommandSpec {
    id: CommandId::Watch,
    command: "/watch",
    completion: "/watch ",
    description: "Coming soon: monitor a topic over time",
    category: CommandCategory::Planned,
    kind: CommandKind::Stub,
    show_in_discovery: true,
  },
  CommandSpec {
    id: CommandId::ExportHistory,
    command: "/export-history",
    completion: "/export-history ",
    description: "Export current history view to ~/.config/one-research/exports (md/jsonl)",
    category: CommandCategory::Discovery,
    kind: CommandKind::BuiltIn,
    show_in_discovery: false,
  },
  CommandSpec {
    id: CommandId::ExportLibrary,
    command: "/export-library",
    completion: "/export-library ",
    description: "Export current library view to ~/.config/one-research/exports (md/jsonl)",
    category: CommandCategory::Discovery,
    kind: CommandKind::BuiltIn,
    show_in_discovery: false,
  },
];

/// Memoized output of the discovery palette. Returns a `&'static [...]` so
/// callers iterate by reference instead of paying ~80 String clones per
/// palette draw + per nav keystroke (audit Perf HIGH H8). The spec list is
/// derived from the const COMMAND_SPECS, so the result is deterministic and
/// can be cached for the lifetime of the process.
pub fn discovery_slash_specs() -> &'static [ChatSlashCommandSpec] {
  static CACHE: std::sync::OnceLock<Vec<ChatSlashCommandSpec>> =
    std::sync::OnceLock::new();
  CACHE.get_or_init(|| {
    COMMAND_SPECS
      .iter()
      .filter(|spec| spec.show_in_discovery)
      .map(|spec| ChatSlashCommandSpec {
        command: spec.command.to_string(),
        completion: spec.completion.to_string(),
        description: spec.description.to_string(),
        badge: match spec.kind {
          CommandKind::Stub => "soon".to_string(),
          _ => String::new(),
        },
      })
      .collect()
  })
}

/// Memoized output of the full chat slash palette. Same shape as
/// `discovery_slash_specs` — single allocation at first call.
pub fn chat_slash_specs() -> &'static [ChatSlashCommandSpec] {
  static CACHE: std::sync::OnceLock<Vec<ChatSlashCommandSpec>> =
    std::sync::OnceLock::new();
  CACHE.get_or_init(|| {
    COMMAND_SPECS
      .iter()
      .map(|spec| ChatSlashCommandSpec {
        command: spec.command.to_string(),
        completion: spec.completion.to_string(),
        description: spec.description.to_string(),
        badge: match spec.category {
          CommandCategory::Discovery => "disc",
          CommandCategory::Sources => "src",
          CommandCategory::Planned => "soon",
        }
        .to_string(),
      })
      .collect()
  })
}
