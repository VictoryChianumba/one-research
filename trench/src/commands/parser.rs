use crate::commands::registry::{COMMAND_SPECS, CommandId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommandInvocation {
  ClearChat,
  Discover { topic: String },
  ClearDiscoveries,
  ClearHistory,
  AddArxivCategory { category: String },
  AddFeed { url: String },
  Sota { topic: String },
  ReadingList { topic: String },
  Code { topic: String },
  Compare { topic: String },
  Digest,
  Author { name: String },
  Trending { topic: String },
  Watch { topic: String },
  ExportHistory { format: String },
  ExportLibrary { format: String },
  Unknown { raw: String },
}

pub fn parse_slash_command(raw: &str) -> SlashCommandInvocation {
  let trimmed = raw.trim();

  match find_command(trimmed).map(|spec| spec.id) {
    Some(CommandId::ClearChat) => SlashCommandInvocation::ClearChat,
    Some(CommandId::Discover) => SlashCommandInvocation::Discover {
      topic: trimmed.strip_prefix("/discover").unwrap_or("").trim().to_string(),
    },
    Some(CommandId::ClearDiscoveries) => {
      SlashCommandInvocation::ClearDiscoveries
    }
    Some(CommandId::ClearHistory) => SlashCommandInvocation::ClearHistory,
    Some(CommandId::AddArxivCategory) => {
      SlashCommandInvocation::AddArxivCategory {
        category: trimmed.strip_prefix("/add").unwrap_or("").trim().to_string(),
      }
    }
    Some(CommandId::AddFeed) => SlashCommandInvocation::AddFeed {
      url: trimmed.strip_prefix("/add-feed").unwrap_or("").trim().to_string(),
    },
    Some(CommandId::Sota) => SlashCommandInvocation::Sota {
      topic: trimmed.strip_prefix("/sota").unwrap_or("").trim().to_string(),
    },
    Some(CommandId::ReadingList) => SlashCommandInvocation::ReadingList {
      topic: trimmed
        .strip_prefix("/reading-list")
        .unwrap_or("")
        .trim()
        .to_string(),
    },
    Some(CommandId::Code) => SlashCommandInvocation::Code {
      topic: trimmed.strip_prefix("/code").unwrap_or("").trim().to_string(),
    },
    Some(CommandId::Compare) => SlashCommandInvocation::Compare {
      topic: trimmed.strip_prefix("/compare").unwrap_or("").trim().to_string(),
    },
    Some(CommandId::Digest) => SlashCommandInvocation::Digest,
    Some(CommandId::Author) => SlashCommandInvocation::Author {
      name: trimmed.strip_prefix("/author").unwrap_or("").trim().to_string(),
    },
    Some(CommandId::Trending) => SlashCommandInvocation::Trending {
      topic: trimmed.strip_prefix("/trending").unwrap_or("").trim().to_string(),
    },
    Some(CommandId::Watch) => SlashCommandInvocation::Watch {
      topic: trimmed.strip_prefix("/watch").unwrap_or("").trim().to_string(),
    },
    Some(CommandId::ExportHistory) => SlashCommandInvocation::ExportHistory {
      format: trimmed
        .strip_prefix("/export-history")
        .unwrap_or("")
        .trim()
        .to_string(),
    },
    Some(CommandId::ExportLibrary) => SlashCommandInvocation::ExportLibrary {
      format: trimmed
        .strip_prefix("/export-library")
        .unwrap_or("")
        .trim()
        .to_string(),
    },
    None => SlashCommandInvocation::Unknown { raw: trimmed.to_string() },
  }
}

fn find_command(
  raw: &str,
) -> Option<&'static crate::commands::registry::CommandSpec> {
  // Prefer the longest matching command so "/clear history" doesn't accidentally
  // match the bare "/clear" prefix.
  COMMAND_SPECS
    .iter()
    .filter(|spec| {
      raw == spec.command
        || raw
          .strip_prefix(spec.command)
          .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
    })
    .max_by_key(|spec| spec.command.len())
}
