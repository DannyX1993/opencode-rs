//! Run-state primitives for session runtime exclusivity and cancellation.

use opencode_core::{context::CancellationToken, error::SessionError, id::SessionId};
use std::sync::Mutex;
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Default)]
struct RunStateInner {
    sessions: Mutex<HashMap<SessionId, CancellationToken>>,
}

/// Lease that keeps a session run active until dropped.
#[derive(Debug)]
pub struct RunGuard {
    session_id: SessionId,
    token: CancellationToken,
    inner: Arc<RunStateInner>,
}

impl RunGuard {
    /// Cancellation token associated with this active run.
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.token.clone()
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.inner
            .sessions
            .lock()
            .expect("run-state mutex poisoned")
            .remove(&self.session_id);
    }
}

/// In-memory per-session run-state coordinator.
#[derive(Debug, Default, Clone)]
pub struct RunState {
    inner: Arc<RunStateInner>,
}

impl RunState {
    /// Acquire exclusive run ownership for `session_id`.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::Busy`] when another run is already active.
    pub async fn acquire(&self, session_id: SessionId) -> Result<RunGuard, SessionError> {
        let mut guard = self
            .inner
            .sessions
            .lock()
            .expect("run-state mutex poisoned");
        if guard.contains_key(&session_id) {
            return Err(SessionError::Busy(session_id.to_string()));
        }

        let token = CancellationToken::new();
        guard.insert(session_id, token.clone());

        Ok(RunGuard {
            session_id,
            token,
            inner: Arc::clone(&self.inner),
        })
    }

    /// Cancel an active run for `session_id`.
    ///
    /// Returns `true` when an active run existed and was cancelled.
    #[must_use]
    pub async fn cancel(&self, session_id: SessionId) -> bool {
        let token = self
            .inner
            .sessions
            .lock()
            .expect("run-state mutex poisoned")
            .remove(&session_id);
        if let Some(token) = token {
            token.cancel();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencode_core::error::SessionError;
    use std::sync::Arc;

    #[tokio::test]
    async fn acquire_enforces_single_active_run_per_session() {
        let runs = RunState::default();
        let session_id = SessionId::new();

        let _guard = runs.acquire(session_id).await.unwrap();
        let err = runs.acquire(session_id).await.unwrap_err();

        assert!(matches!(err, SessionError::Busy(_)));
    }

    #[tokio::test]
    async fn concurrent_acquire_denies_second_prompt() {
        let runs = Arc::new(RunState::default());
        let session_id = SessionId::new();

        let a = Arc::clone(&runs);
        let b = Arc::clone(&runs);

        let first = tokio::spawn(async move {
            let _guard = a.acquire(session_id).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let second = tokio::spawn(async move { b.acquire(session_id).await.unwrap_err() });

        first.await.unwrap();
        let err = second.await.unwrap();
        assert!(matches!(err, SessionError::Busy(_)));
    }

    #[tokio::test]
    async fn cancel_is_idempotent_and_releases_session() {
        let runs = RunState::default();
        let session_id = SessionId::new();

        let guard = runs.acquire(session_id).await.unwrap();

        assert!(runs.cancel(session_id).await);
        assert!(guard.cancellation_token().is_cancelled());
        assert!(!runs.cancel(session_id).await);

        let reacquired = runs.acquire(session_id).await;
        assert!(reacquired.is_ok());
    }
}
