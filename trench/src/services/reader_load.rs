//! Fulltext fetch service. Spawns a worker thread that pulls the full
//! paper / article body from the appropriate provider (arxiv, semantic
//! scholar, generic HTTP), wrapping it in panic protection so a worker
//! failure routes back as `Err(...)` rather than killing the app.

use std::sync::mpsc;

use crate::ingestion;
use crate::models;
use crate::panic_msg;

pub(crate) fn spawn_fulltext_fetch(
  item: models::FeedItem,
  tx: mpsc::Sender<Result<tread::PaperData, String>>,
) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let _ = tx.send(ingestion::fulltext::fetch(&item));
    }));
    if let Err(payload) = result {
      let msg = panic_msg(payload);
      log::error!("spawn_fulltext_fetch: thread panicked — {msg}");
      let _ = tx_panic.send(Err(format!("fulltext thread panicked: {msg}")));
    }
  });
}
