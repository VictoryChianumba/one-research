use crate::app::{App, FeedTab};
use crate::commands::parser::SlashCommandInvocation;
use crate::discovery::intent::QueryIntent;

pub fn dispatch_slash_command(app: &mut App, cmd: SlashCommandInvocation) {
  match cmd {
    SlashCommandInvocation::ClearChat => {
      app.clear_chat_messages();
      app.status_message = Some("Cleared chat session".to_string());
    }
    SlashCommandInvocation::Discover { topic } => {
      if topic.is_empty() {
        app.push_chat_assistant_message("Usage: /discover TOPIC".to_string());
      } else {
        crate::workflows::discover::start(app, topic);
      }
    }
    SlashCommandInvocation::ClearHistory => {
      app.mutate_history(|h| h.clear());
      app.feed.history_list.reset();
      crate::store::history::save(&app.workspace.history);
      app.push_chat_assistant_message("Cleared activity history.".to_string());
      app.status_message = Some("Cleared history".to_string());
    }
    SlashCommandInvocation::ClearDiscoveries => {
      app.reset_discovery_items();
      app.feed.discovery.list.reset();
      app.feed.discovery.status = String::new();
      app.feed.discovery.loading = false;
      app.feed.discovery.session = crate::discovery::SessionHistory::default();
      crate::store::discovery_cache::save(&app.feed.discovery.items);
      crate::store::session::clear();
      app.push_chat_assistant_message(
        "Cleared discovery results and session history.".to_string(),
      );
      app.status_message = Some("Cleared discovery results".to_string());
      if app.feed.feed_tab == FeedTab::Discoveries {
        app.reset_active_feed_position();
      }
    }
    SlashCommandInvocation::AddArxivCategory { category } => {
      if category.is_empty() {
        app.push_chat_assistant_message(
          "Usage: /add ARXIV_CATEGORY".to_string(),
        );
      } else if app
        .config
        .sources
        .arxiv_categories
        .iter()
        .any(|c| c == &category)
      {
        app.push_chat_assistant_message(format!(
          "{category} is already configured."
        ));
      } else {
        app.config.sources.arxiv_categories.push(category.clone());
        app.config.save();
        app.push_chat_assistant_message(format!(
          "Added arXiv category: {category}"
        ));
      }
    }
    SlashCommandInvocation::AddFeed { url } => {
      if !crate::discovery::ai_query::is_http_url(&url) {
        app.push_chat_assistant_message(
          "Usage: /add-feed http(s)://feed-url".to_string(),
        );
      } else if app.config.sources.custom_feeds.iter().any(|f| f.url == url) {
        app.push_chat_assistant_message(
          "Feed is already configured.".to_string(),
        );
      } else {
        app.config.sources.custom_feeds.push(crate::config::CustomFeed {
          url: url.clone(),
          name: feed_name_from_url(&url),
          feed_type: "rss".to_string(),
        });
        app.config.save();
        app.push_chat_assistant_message(format!("Added RSS feed: {url}"));
      }
    }
    SlashCommandInvocation::Sota { topic } => {
      dispatch_discovery_with_intent(
        app,
        topic,
        QueryIntent::SotaLookup,
        "/sota",
      );
    }
    SlashCommandInvocation::ReadingList { topic } => {
      dispatch_discovery_with_intent(
        app,
        topic,
        QueryIntent::ReadingList,
        "/reading-list",
      );
    }
    SlashCommandInvocation::Code { topic } => {
      dispatch_discovery_with_intent(
        app,
        topic,
        QueryIntent::CodeSearch,
        "/code",
      );
    }
    SlashCommandInvocation::Compare { topic } => {
      dispatch_discovery_with_intent(
        app,
        topic,
        QueryIntent::Compare,
        "/compare",
      );
    }
    SlashCommandInvocation::Digest => {
      app.feed.discovery.forced_intent = Some(QueryIntent::Digest);
      crate::workflows::discover::start(
        app,
        "what happened in AI/ML this week".to_string(),
      );
    }
    SlashCommandInvocation::Author { name } => {
      dispatch_discovery_with_intent(
        app,
        name,
        QueryIntent::AuthorSearch,
        "/author",
      );
    }
    SlashCommandInvocation::Trending { topic } => {
      dispatch_discovery_with_intent(
        app,
        topic,
        QueryIntent::Trending,
        "/trending",
      );
    }
    SlashCommandInvocation::Watch { .. } => {
      app.push_chat_assistant_message(
        "/watch is coming soon. It will monitor a topic over time and surface new results on each launch.".to_string(),
      );
    }
    SlashCommandInvocation::ExportHistory { format } => {
      let Some(fmt) = crate::export::ExportFormat::from_arg(&format) else {
        app.push_chat_assistant_message(
          "Usage: /export-history [md|jsonl]".to_string(),
        );
        return;
      };
      let entries = app.filtered_history();
      match crate::export::export_history(&entries, fmt) {
        Ok(path) => {
          let msg =
            format!("Exported {} entries to {}", entries.len(), path.display());
          app.push_chat_assistant_message(msg.clone());
          app.status_message = Some(msg);
        }
        Err(e) => {
          app.push_chat_assistant_message(format!("Export failed: {e}"));
        }
      }
    }
    SlashCommandInvocation::ExportLibrary { format } => {
      let Some(fmt) = crate::export::ExportFormat::from_arg(&format) else {
        app.push_chat_assistant_message(
          "Usage: /export-library [md|jsonl]".to_string(),
        );
        return;
      };
      let label = app.feed.library_filter.label().to_string();
      let items = app.visible_items();
      match crate::export::export_library(&items, &label, fmt) {
        Ok(path) => {
          let msg =
            format!("Exported {} items to {}", items.len(), path.display());
          app.push_chat_assistant_message(msg.clone());
          app.status_message = Some(msg);
        }
        Err(e) => {
          app.push_chat_assistant_message(format!("Export failed: {e}"));
        }
      }
    }
    SlashCommandInvocation::Unknown { raw } => {
      app.push_chat_assistant_message(format!("Unknown slash command: {raw}"));
    }
  }
}

fn dispatch_discovery_with_intent(
  app: &mut App,
  topic: String,
  intent: QueryIntent,
  command: &str,
) {
  if topic.is_empty() {
    app.push_chat_assistant_message(format!("Usage: {command} TOPIC"));
  } else {
    app.feed.discovery.forced_intent = Some(intent);
    crate::workflows::discover::start(app, topic);
  }
}

fn feed_name_from_url(url: &str) -> String {
  url
    .trim_start_matches("https://")
    .trim_start_matches("http://")
    .split('/')
    .next()
    .unwrap_or("feed")
    .to_string()
}
