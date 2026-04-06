//! # opencode-storage
//!
//! SQLite-backed persistence layer for the opencode Rust port.
//!
//! Provides:
//! - [`Storage`] trait — the unified facade used by session/server layers.
//! - Repository types for each domain entity (project, session, message, etc.).
//! - [`SyncEventStore`] — append-only event-sourcing table.
//! - [`StorageFactory`] — connection and migration bootstrapper.
//!
//! Schema is kept compatible with the existing TypeScript SQLite database so
//! that migration between runtimes is non-destructive.

#![warn(missing_docs)]

pub mod pool;
pub mod store;
pub mod event_store;
pub mod repo;

pub use store::{Storage, StorageImpl};
pub use event_store::SyncEventStore;
pub use pool::connect;
