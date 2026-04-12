//! Workspace-wide error hierarchy.
//!
//! Domain-specific error enums (`ProviderError`, `ToolError`, etc.) live in
//! their respective crates and implement [`From`] conversions into
//! [`OpenCodeError`] so callers can use `?` freely at the session/server layer.

use thiserror::Error;

/// Top-level opencode error — wraps all domain error variants.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OpenCodeError {
    /// Configuration loading or validation failure.
    #[error("config error: {0}")]
    Config(#[from] ConfigError),

    /// Storage / database failure.
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    /// Session engine failure.
    #[error("session error: {0}")]
    Session(#[from] SessionError),

    /// Server / HTTP failure.
    #[error("server error: {0}")]
    Server(#[from] ServerError),

    /// Catch-all for unexpected internal failures (used at binary edge).
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// Errors produced by config loading and validation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// JSONC parse error.
    #[error("parse error in {path}: {msg}")]
    Parse {
        /// File path that failed to parse.
        path: String,
        /// Parser error message.
        msg: String,
    },

    /// A required field was missing.
    #[error("missing required config field: {field}")]
    Missing {
        /// Name of the missing field.
        field: &'static str,
    },

    /// A field value failed validation.
    #[error("invalid value for {field}: {reason}")]
    Invalid {
        /// Name of the invalid field.
        field: &'static str,
        /// Description of why it is invalid.
        reason: String,
    },

    /// File I/O error while reading a config file.
    #[error("io error reading config: {0}")]
    Io(#[from] std::io::Error),
}

/// Errors produced by the storage layer (SQLite, repos).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StorageError {
    /// Row not found for the given id.
    #[error("not found: {entity} {id}")]
    NotFound {
        /// Entity type name (e.g. "session").
        entity: &'static str,
        /// Entity ID value.
        id: String,
    },

    /// Database driver or migration error.
    #[error("database error: {0}")]
    Db(String),

    /// Serialization / deserialization failure on a stored value.
    #[error("serde error: {0}")]
    Serde(String),
}

/// Errors produced by the session engine.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SessionError {
    /// The requested session does not exist.
    #[error("session not found: {0}")]
    NotFound(String),

    /// The session was cancelled before completion.
    #[error("session cancelled")]
    Cancelled,

    /// Context window exhausted and compaction failed.
    #[error("context overflow in session {id}")]
    ContextOverflow {
        /// ID of the affected session.
        id: String,
    },

    /// A prompt was requested while another run is active for the same session.
    #[error("session busy: {0}")]
    Busy(String),

    /// Provider returned a runtime failure while processing a prompt stream.
    #[error("session provider error: {0}")]
    Provider(String),

    /// Runtime failed unexpectedly while orchestrating the turn.
    #[error("session runtime internal error: {0}")]
    RuntimeInternal(String),

    /// Cancel was requested but there is no active run for the session.
    #[error("no active run for session: {0}")]
    NoActiveRun(String),
}

/// Errors produced by the HTTP server layer.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ServerError {
    /// Failed to bind to the requested address.
    #[error("failed to bind server: {0}")]
    Bind(String),

    /// Internal server error (catch-all).
    #[error("internal server error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::SessionError;

    #[test]
    fn session_busy_error_message_is_deterministic() {
        let err = SessionError::Busy("sess-123".into());
        assert_eq!(err.to_string(), "session busy: sess-123");
    }

    #[test]
    fn session_runtime_internal_error_message_is_deterministic() {
        let err = SessionError::RuntimeInternal("stream processor crashed".into());
        assert_eq!(
            err.to_string(),
            "session runtime internal error: stream processor crashed"
        );
    }

    #[test]
    fn session_provider_error_message_is_deterministic() {
        let err = SessionError::Provider("model unavailable".into());
        assert_eq!(err.to_string(), "session provider error: model unavailable");
    }

    #[test]
    fn cancel_without_active_run_error_message_is_deterministic() {
        let err = SessionError::NoActiveRun("sess-777".into());
        assert_eq!(err.to_string(), "no active run for session: sess-777");
    }
}
