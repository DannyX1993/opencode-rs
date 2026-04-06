//! [`Session`] trait and stub [`SessionEngine`].
//!
//! Full agent-loop implementation arrives in Phase 4.

use async_trait::async_trait;
use opencode_core::{error::SessionError, id::SessionId};

use crate::types::{SessionHandle, SessionPrompt};

/// The session engine abstraction.
///
/// Callers inject `Arc<dyn Session>` so implementations can be swapped for
/// testing.
#[async_trait]
pub trait Session: Send + Sync {
    /// Submit a prompt and return a handle for tracking the turn.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError`] if the session cannot be found or the
    /// provider fails to initialise.
    async fn prompt(&self, req: SessionPrompt) -> Result<SessionHandle, SessionError>;

    /// Cancel an in-progress prompt turn.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::NotFound`] if no active turn exists.
    async fn cancel(&self, session_id: SessionId) -> Result<(), SessionError>;
}

/// Stub session engine — panics if called.
///
/// Replace with the real `SessionEngine` in Phase 4.
pub struct SessionEngine;

#[async_trait]
impl Session for SessionEngine {
    async fn prompt(&self, _req: SessionPrompt) -> Result<SessionHandle, SessionError> {
        Err(SessionError::NotFound(
            "SessionEngine not yet implemented".into(),
        ))
    }

    async fn cancel(&self, _session_id: SessionId) -> Result<(), SessionError> {
        Err(SessionError::NotFound(
            "SessionEngine not yet implemented".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencode_core::id::SessionId;

    #[tokio::test]
    async fn prompt_returns_not_found() {
        let engine = SessionEngine;
        let req = SessionPrompt {
            session_id: SessionId::new(),
            text: "hello".into(),
            model: None,
            plan_mode: false,
        };
        let err = engine.prompt(req).await.unwrap_err();
        assert!(matches!(err, SessionError::NotFound(_)));
    }

    #[tokio::test]
    async fn cancel_returns_not_found() {
        let engine = SessionEngine;
        let err = engine.cancel(SessionId::new()).await.unwrap_err();
        assert!(matches!(err, SessionError::NotFound(_)));
    }
}
