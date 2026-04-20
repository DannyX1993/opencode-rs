//! # opencode-core
//!
//! Lowest-level shared types used across the entire opencode Rust workspace.
//!
//! Provides:
//! - [`error`] — workspace-wide error hierarchy
//! - [`id`] — typed ID newtypes wrapping [`uuid::Uuid`]
//! - [`config`] — JSONC cascading configuration loader
//! - [`dto`] — shared data-transfer objects mirroring the TypeScript schema
//! - [`project`] — repository/worktree foundation contracts and probe seam
//! - [`tracing`] — bootstrap helpers for `tracing-subscriber`
//! - [`context`] — [`BoxStream`] alias, [`CancellationToken`] re-export, task-local session context

#![warn(missing_docs)]

pub mod config;
pub mod config_service;
pub mod context;
pub mod dto;
pub mod error;
pub mod id;
pub mod project;
pub mod tracing;

#[cfg(test)]
#[allow(unsafe_code)]
pub(crate) mod test_env {
    use std::sync::OnceLock;
    use tokio::sync::{Mutex, MutexGuard};

    const TEST_ENV_KEYS: [&str; 8] = [
        "OPENCODE_MODEL",
        "OPENCODE_LOG_LEVEL",
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "GOOGLE_API_KEY",
        "OPENCODE_SERVER_PORT",
        "OPENCODE_SERVER_HOST",
        "OPENCODE_AUTH_TOKEN",
    ];

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    pub(crate) async fn lock() -> MutexGuard<'static, ()> {
        env_lock().lock().await
    }

    pub(crate) fn clear() {
        for key in TEST_ENV_KEYS {
            // SAFETY: test-only environment setup/teardown for deterministic config loading.
            unsafe { std::env::remove_var(key) };
        }
    }
}

/// Convenience re-export of the most commonly used items.
pub mod prelude {
    pub use crate::{
        config::Config,
        config_service::{ConfigScope, ConfigService, ServerBindOverrides},
        context::SessionCtx,
        dto::*,
        error::{ConfigError, OpenCodeError, SessionError, StorageError},
        id::{MessageId, PartId, ProjectId, SessionId, TodoId},
        project::{
            ProjectFoundationRecord, ProjectProbeError, RepositoryProbe, RepositoryState,
            SyncBasis, WorktreeState,
        },
    };
    pub use ::tracing::{debug, error, info, instrument, warn};
    pub use tokio_util::sync::CancellationToken;
}
