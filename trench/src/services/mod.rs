//! Side-effect executors. Services own threads, sockets, and the wire
//! protocol to external systems (GitHub, arXiv, RSS feeds, AI providers,
//! local disk). Surfaces emit Effects when something async needs to
//! start; services own the receivers and emit completion events back
//! into the app loop.
//!
//! Strict definition of what belongs here: **owns a thread, a socket,
//! or a non-trivial blocking I/O op**. Anything else is data, surface
//! state, or a presentation helper. This rule keeps `services/` from
//! drifting back toward a generic utility bucket.

pub mod discovery;
pub mod ingestion;
pub mod reader_load;
pub mod repo_load;

pub(crate) use discovery::{spawn_ai_discovery, spawn_discovery};
pub(crate) use ingestion::spawn_fetch;
pub(crate) use reader_load::spawn_fulltext_fetch;
pub(crate) use repo_load::{spawn_repo_dir, spawn_repo_file, spawn_repo_open};
