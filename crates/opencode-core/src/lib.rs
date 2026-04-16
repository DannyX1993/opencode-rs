//! # opencode-core
//!
//! Lowest-level shared types used across the entire opencode Rust workspace.
//!
//! Provides:
//! - [`error`] — workspace-wide error hierarchy
//! - [`id`] — typed ID newtypes wrapping [`uuid::Uuid`]
//! - [`config`] — JSONC cascading configuration loader
//! - [`dto`] — shared data-transfer objects mirroring the TypeScript schema
//! - [`tracing`] — bootstrap helpers for `tracing-subscriber`
//! - [`context`] — [`BoxStream`] alias, [`CancellationToken`] re-export, task-local session context

#![warn(missing_docs)]

pub mod config;
pub mod config_service;
pub mod context;
pub mod dto;
pub mod error;
pub mod id;
pub mod tracing;

/// Convenience re-export of the most commonly used items.
pub mod prelude {
    pub use crate::{
        config::Config,
        config_service::{ConfigScope, ConfigService, ServerBindOverrides},
        context::SessionCtx,
        dto::*,
        error::{ConfigError, OpenCodeError, SessionError, StorageError},
        id::{MessageId, PartId, ProjectId, SessionId, TodoId},
    };
    pub use ::tracing::{debug, error, info, instrument, warn};
    pub use tokio_util::sync::CancellationToken;
}
