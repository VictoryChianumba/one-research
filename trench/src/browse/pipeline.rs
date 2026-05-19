//! Browse-fetch worker (ADR-010 §D3).
//!
//! One worker thread per `Enter` on a Category — calls the existing
//! [`arxiv::fetch`](crate::ingestion::arxiv::fetch) free function for
//! the chosen category code, then sends [`BrowseMessage::Items`] (or
//! [`BrowseMessage::Error`]) on the channel. Panic-isolated via the
//! same `panic_msg` shim discovery and ingestion use; a thread panic
//! becomes a routed `Error` rather than crashing the UI.

use std::sync::mpsc;

use super::BrowseMessage;
use crate::ingestion::arxiv;

/// Spawn a browse-fetch thread for one arXiv category code.
///
/// Sends a single `Items` or `Error` message on `tx`, then exits. The
/// caller is responsible for tracking inflight state — `tx` is cloned
/// per spawn, so multiple in-flight Browse fetches share the same
/// receiver in `BrowseModel.rx`.
pub fn spawn_browse_fetch(category: String, tx: mpsc::Sender<BrowseMessage>) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let category_panic = category.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let t = std::time::Instant::now();
      match arxiv::fetch(std::slice::from_ref(&category)) {
        Ok(items) => {
          log::info!(
            "browse::spawn_browse_fetch[{category}]: {} items in {}ms",
            items.len(),
            t.elapsed().as_millis(),
          );
          let _ = tx.send(BrowseMessage::Items { category, items });
        }
        Err(e) => {
          log::error!(
            "browse::spawn_browse_fetch[{category}]: failed in {}ms — {e}",
            t.elapsed().as_millis(),
          );
          let _ = tx.send(BrowseMessage::Error { category, error: e });
        }
      }
    }));
    if let Err(payload) = result {
      let msg = crate::panic_msg(payload);
      log::error!(
        "browse::spawn_browse_fetch[{category_panic}]: thread panicked — {msg}"
      );
      let _ = tx_panic.send(BrowseMessage::Error {
        category: category_panic,
        error: format!("browse thread panicked: {msg}"),
      });
    }
  });
}
