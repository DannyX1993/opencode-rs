//! Run-state primitives for session runtime exclusivity and cancellation.

use opencode_core::{context::CancellationToken, error::SessionError, id::SessionId};
use std::sync::Mutex;
use std::{collections::HashMap, sync::Arc};

/// Read-only snapshot of a session run lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunSnapshot {
    /// Whether a run lease is currently active.
    pub is_active: bool,
    /// Whether the active run has been cancellation-signalled.
    pub is_cancelled: bool,
}

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
            .get(&session_id)
            .cloned();
        if let Some(token) = token {
            if token.is_cancelled() {
                return false;
            }
            token.cancel();
            true
        } else {
            false
        }
    }

    /// Snapshot run occupancy/cancellation for one session.
    #[must_use]
    pub async fn snapshot(&self, session_id: SessionId) -> RunSnapshot {
        let token = self
            .inner
            .sessions
            .lock()
            .expect("run-state mutex poisoned")
            .get(&session_id)
            .cloned();
        match token {
            Some(token) => RunSnapshot {
                is_active: true,
                is_cancelled: token.is_cancelled(),
            },
            None => RunSnapshot {
                is_active: false,
                is_cancelled: false,
            },
        }
    }

    /// List all sessions that currently have an active run lease.
    #[must_use]
    pub async fn list_active_sessions(&self) -> Vec<SessionId> {
        self.inner
            .sessions
            .lock()
            .expect("run-state mutex poisoned")
            .keys()
            .copied()
            .collect()
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

        let while_guard_live = runs.acquire(session_id).await;
        assert!(matches!(while_guard_live, Err(SessionError::Busy(_))));

        drop(guard);
        let reacquired_after_drop = runs.acquire(session_id).await;
        assert!(reacquired_after_drop.is_ok());
    }

    #[tokio::test]
    async fn snapshot_reports_idle_busy_and_cancelled_state() {
        let runs = RunState::default();
        let sid = SessionId::new();

        let idle = runs.snapshot(sid).await;
        assert!(!idle.is_active);
        assert!(!idle.is_cancelled);

        let guard = runs.acquire(sid).await.unwrap();
        let busy = runs.snapshot(sid).await;
        assert!(busy.is_active);
        assert!(!busy.is_cancelled);

        assert!(runs.cancel(sid).await);
        let cancelled = runs.snapshot(sid).await;
        assert!(cancelled.is_active);
        assert!(cancelled.is_cancelled);

        drop(guard);
        let dropped = runs.snapshot(sid).await;
        assert!(!dropped.is_active);
        assert!(!dropped.is_cancelled);
    }

    #[tokio::test]
    async fn list_active_sessions_tracks_guard_lifecycle() {
        let runs = RunState::default();
        let sid_a = SessionId::new();
        let sid_b = SessionId::new();

        let guard_a = runs.acquire(sid_a).await.unwrap();
        let guard_b = runs.acquire(sid_b).await.unwrap();

        let active = runs.list_active_sessions().await;
        assert_eq!(active.len(), 2);
        assert!(active.contains(&sid_a));
        assert!(active.contains(&sid_b));

        drop(guard_a);
        let active_after_first_drop = runs.list_active_sessions().await;
        assert_eq!(active_after_first_drop, vec![sid_b]);

        drop(guard_b);
        assert!(runs.list_active_sessions().await.is_empty());
    }
}
