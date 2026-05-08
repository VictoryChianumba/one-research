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

fn arg_after(input: &str, prefix: &str) -> String {
  input.strip_prefix(prefix).unwrap_or("").trim().to_string()
}

pub fn parse_slash_command(raw: &str) -> SlashCommandInvocation {
  let trimmed = raw.trim();

  match find_command(trimmed).map(|spec| spec.id) {
    Some(CommandId::ClearChat) => SlashCommandInvocation::ClearChat,
    Some(CommandId::Discover) => SlashCommandInvocation::Discover {
      topic: arg_after(trimmed, "/discover"),
    },
    Some(CommandId::ClearDiscoveries) => {
      SlashCommandInvocation::ClearDiscoveries
    }
    Some(CommandId::ClearHistory) => SlashCommandInvocation::ClearHistory,
    Some(CommandId::AddArxivCategory) => {
      SlashCommandInvocation::AddArxivCategory {
        category: arg_after(trimmed, "/add"),
      }
    }
    Some(CommandId::AddFeed) => SlashCommandInvocation::AddFeed {
      url: arg_after(trimmed, "/add-feed"),
    },
    Some(CommandId::Sota) => SlashCommandInvocation::Sota {
      topic: arg_after(trimmed, "/sota"),
    },
    Some(CommandId::ReadingList) => SlashCommandInvocation::ReadingList {
      topic: arg_after(trimmed, "/reading-list"),
    },
    Some(CommandId::Code) => SlashCommandInvocation::Code {
      topic: arg_after(trimmed, "/code"),
    },
    Some(CommandId::Compare) => SlashCommandInvocation::Compare {
      topic: arg_after(trimmed, "/compare"),
    },
    Some(CommandId::Digest) => SlashCommandInvocation::Digest,
    Some(CommandId::Author) => SlashCommandInvocation::Author {
      name: arg_after(trimmed, "/author"),
    },
    Some(CommandId::Trending) => SlashCommandInvocation::Trending {
      topic: arg_after(trimmed, "/trending"),
    },
    Some(CommandId::Watch) => SlashCommandInvocation::Watch {
      topic: arg_after(trimmed, "/watch"),
    },
    Some(CommandId::ExportHistory) => SlashCommandInvocation::ExportHistory {
      format: arg_after(trimmed, "/export-history"),
    },
    Some(CommandId::ExportLibrary) => SlashCommandInvocation::ExportLibrary {
      format: arg_after(trimmed, "/export-library"),
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
