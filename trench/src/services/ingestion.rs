use std::sync::mpsc;

use crate::models::{self, ContentType, SourcePlatform};
use crate::{config, ingestion, panic_msg, store};
use ingestion::message::FetchMessage;

/// Uniform per-source runner. Logs the start, dispatches `fetch_fn`, then
/// either extends the shared accumulator and emits Items + SourceComplete,
/// or routes a SourceError. Mutex-poison recovery follows the W3 voice
/// pattern — a poisoned lock on `all_items` is recovered rather than
/// crashing the refresh.
fn run_source<F>(
  name: &str,
  tx: &mpsc::Sender<FetchMessage>,
  all_items: &std::sync::Mutex<Vec<models::FeedItem>>,
  fetch_fn: F,
) where
  F: FnOnce() -> Result<Vec<models::FeedItem>, String>,
{
  log::info!("source {name}: starting fetch");
  match fetch_fn() {
    Ok(items) => {
      log::info!("source {name}: completed, {} items", items.len());
      // Clone outside the lock so the critical section is just the
      // `.extend(...)` move. Two consumers (accumulator + channel) so
      // one clone is unavoidable; the win is shrinking the lock window
      // and avoiding torn-state risk on panic mid-clone.
      let to_extend = items.clone();
      all_items.lock().unwrap_or_else(|e| e.into_inner()).extend(to_extend);
      let _ = tx.send(FetchMessage::Items(items));
      let _ = tx.send(FetchMessage::SourceComplete(name.to_string()));
    }
    Err(e) => {
      log::error!("source {name}: failed — {e}");
      let _ = tx.send(FetchMessage::SourceError(name.to_string(), e));
    }
  }
}

/// Wrap a per-source thread body in `catch_unwind` so a panic does not
/// kill its siblings. Routes the panic to a SourceError + SourceComplete
/// pair so the loading-spinner clears and the UI surfaces the error.
fn run_source_protected<F>(name: &str, tx: &mpsc::Sender<FetchMessage>, body: F)
where
  F: FnOnce(),
{
  let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
  if let Err(payload) = outcome {
    let msg = panic_msg(payload);
    log::error!("source {name}: thread panicked — {msg}");
    let _ = tx.send(FetchMessage::SourceError(
      name.to_string(),
      format!("source thread panicked: {msg}"),
    ));
    let _ = tx.send(FetchMessage::SourceComplete(name.to_string()));
  }
}

pub(crate) fn spawn_fetch(
  tx: mpsc::Sender<FetchMessage>,
  config: config::Config,
) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let enabled = |name: &str| -> bool {
        config.sources.enabled_sources.get(name).copied().unwrap_or(true)
      };

      // Shared accumulator for enrichment. Each scope thread locks briefly
      // to extend; critical section is one Vec::extend.
      let all_items: std::sync::Mutex<Vec<models::FeedItem>> =
        std::sync::Mutex::new(Vec::new());

      log::warn!(
        "source anthropic: no RSS feed available; skipping \
         (https://www.anthropic.com/news has no feed link)"
      );

      let rss_feeds: &[(&str, &str, SourcePlatform, ContentType)] = &[
        (
          "openai",
          "https://openai.com/blog/rss.xml",
          SourcePlatform::Blog,
          ContentType::Article,
        ),
        (
          "deepmind",
          "https://deepmind.google/blog/rss.xml",
          SourcePlatform::Blog,
          ContentType::Article,
        ),
        (
          "import_ai",
          "https://importai.substack.com/feed",
          SourcePlatform::Newsletter,
          ContentType::Digest,
        ),
        (
          "bair",
          "https://bair.berkeley.edu/blog/feed.xml",
          SourcePlatform::Blog,
          ContentType::Article,
        ),
        (
          "mit_news_ai",
          "https://news.mit.edu/rss/topic/machine-learning",
          SourcePlatform::Blog,
          ContentType::Article,
        ),
      ];

      let all_items_ref = &all_items;
      let enabled_ref = &enabled;
      let arxiv_categories: &[String] = &config.sources.arxiv_categories;
      let core_key: Option<&str> = config.core_api_key.as_deref();
      let custom_feeds: &[config::CustomFeed] = &config.sources.custom_feeds;

      // Concurrency groups:
      //   A — arxiv-family (sequential within: arxiv → huggingface),
      //       both touch export.arxiv.org so we keep them serial
      //       within the group to stay under the 3s/req rate envelope.
      //   B — openreview (alone, api.openreview.net).
      //   C — core (alone, api.core.ac.uk; needs API key).
      //   D — RSS feeds + custom feeds, one thread per feed; each is
      //       on a distinct host.
      // thread::scope auto-joins all spawns at scope exit, so enrichment
      // (which needs the full all_items) naturally runs after fetches.
      std::thread::scope(|scope| {
        // Group A
        let tx_a = tx.clone();
        scope.spawn(move || {
          run_source_protected("arxiv-family", &tx_a, || {
            run_source("arxiv", &tx_a, all_items_ref, || {
              ingestion::arxiv::fetch(arxiv_categories)
            });
            if enabled_ref("huggingface") {
              run_source(
                "huggingface",
                &tx_a,
                all_items_ref,
                ingestion::huggingface::fetch,
              );
            } else {
              log::info!("source huggingface: disabled — skipping");
              let _ = tx_a
                .send(FetchMessage::SourceComplete("huggingface".to_string()));
            }
          });
        });

        // Group B
        let tx_b = tx.clone();
        scope.spawn(move || {
          run_source_protected("openreview", &tx_b, || {
            if enabled_ref("openreview") {
              run_source(
                "openreview",
                &tx_b,
                all_items_ref,
                ingestion::openreview::fetch,
              );
            } else {
              log::info!("source openreview: disabled — skipping");
              let _ = tx_b
                .send(FetchMessage::SourceComplete("openreview".to_string()));
            }
          });
        });

        // Group C
        let tx_c = tx.clone();
        scope.spawn(move || {
          run_source_protected("core", &tx_c, || {
            if !enabled_ref("core") {
              log::info!("source core: disabled — skipping");
              let _ =
                tx_c.send(FetchMessage::SourceComplete("core".to_string()));
              return;
            }
            let Some(key) = core_key else {
              log::info!("source core: no API key configured — skipping");
              let _ =
                tx_c.send(FetchMessage::SourceComplete("core".to_string()));
              return;
            };
            run_source("core", &tx_c, all_items_ref, || {
              ingestion::core::fetch(key)
            });
          });
        });

        // Group D — built-in RSS feeds
        for (name, url, platform, content_type) in rss_feeds.iter() {
          let tx_d = tx.clone();
          let platform = platform.clone();
          let content_type = content_type.clone();
          scope.spawn(move || {
            run_source_protected(name, &tx_d, || {
              if !enabled_ref(name) {
                log::info!("source {name}: disabled — skipping");
                let _ =
                  tx_d.send(FetchMessage::SourceComplete(name.to_string()));
                return;
              }
              run_source(name, &tx_d, all_items_ref, || {
                ingestion::rss::fetch(name, url, platform, content_type)
              });
            });
          });
        }

        // Group D — user-configured custom feeds
        for feed in custom_feeds.iter() {
          let tx_d = tx.clone();
          scope.spawn(move || {
            run_source_protected(&feed.name, &tx_d, || {
              run_source(&feed.name, &tx_d, all_items_ref, || {
                ingestion::rss::fetch(
                  &feed.name,
                  &feed.url,
                  SourcePlatform::Rss,
                  ContentType::Article,
                )
              });
            });
          });
        }
      });

      // All scope threads have joined; recover ownership from the Mutex.
      let mut all_items =
        all_items.into_inner().unwrap_or_else(|e| e.into_inner());

      log::info!(
        "background: {} total items collected across all sources",
        all_items.len()
      );

      let mut ecache = store::enrichment_cache::load();
      ingestion::semantic_scholar::enrich(
        &mut all_items,
        &mut ecache,
        config.semantic_scholar_key.as_deref(),
      );
      ingestion::huggingface::enrich_with_repos(&mut all_items);
      let with_repo =
        all_items.iter().filter(|i| i.github_repo.is_some()).count();
      log::info!(
        "ingestion complete: {with_repo}/{} items have github_repo set",
        all_items.len()
      );
      let _ = tx.send(FetchMessage::EnrichedItems(all_items));
      let _ = tx.send(FetchMessage::SourceComplete("enriching".to_string()));
      let _ = tx.send(FetchMessage::AllComplete);
    }));
    if let Err(payload) = result {
      let msg = panic_msg(payload);
      log::error!("spawn_fetch: background thread panicked — {msg}");
      let _ = tx_panic.send(FetchMessage::SourceError(
        "background".to_string(),
        format!("background thread panicked: {msg}"),
      ));
      let _ = tx_panic.send(FetchMessage::AllComplete);
    }
  });
}
