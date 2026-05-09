//! Authoritative in-memory application model. Pure state — no
//! threads, no sockets, no disk writes. Side effects live in
//! [`crate::services`].
//!
//! Currently a single `Workspace` struct; later phases may shard
//! into feed_store / history_store / tag_store if cross-shard
//! coordination becomes painful.

pub mod workspace_store;

pub use workspace_store::Workspace;
