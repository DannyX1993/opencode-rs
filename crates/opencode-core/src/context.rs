//! Async context helpers shared across crates.
//!
//! Provides:
//! - [`BoxStream`] — a type alias for a pinned, boxed, `Send` async stream.
//! - [`CancellationToken`] re-export — for propagating graceful-shutdown signals.
//! - [`SessionCtx`] — task-local session context so that nested tasks can
//!   read the active session id without threading it through every function.

use futures::Stream;
use std::pin::Pin;
pub use tokio_util::sync::CancellationToken;

/// Convenience alias for a heap-allocated async stream.
///
/// Used throughout the provider/session/server layers where streams need to be
/// stored in structs or returned from `async-trait` methods.
pub type BoxStream<T> = Pin<Box<dyn Stream<Item = T> + Send + 'static>>;

/// Task-local session ID for the current async task tree.
///
/// Set via [`SessionCtx::scope`] at the session-engine entry point so that
/// tracing spans, logging, and nested tool calls can all read it cheaply.
///
/// # Examples
///
/// ```
/// use opencode_core::context::SessionCtx;
/// use opencode_core::id::SessionId;
///
/// # #[tokio::main]
/// # async fn main() {
/// let id = SessionId::new();
/// SessionCtx::scope(id, async {
///     let current = SessionCtx::current();
///     assert_eq!(current, Some(id));
/// }).await;
/// # }
/// ```
pub struct SessionCtx;

tokio::task_local! {
    static SESSION_ID: crate::id::SessionId;
}

impl SessionCtx {
    /// Run `fut` with `id` set as the active session context.
    pub async fn scope<F, O>(id: crate::id::SessionId, fut: F) -> O
    where
        F: std::future::Future<Output = O>,
    {
        SESSION_ID.scope(id, fut).await
    }

    /// Read the current task-local session ID, if set.
    #[must_use]
    pub fn current() -> Option<crate::id::SessionId> {
        SESSION_ID.try_with(|v| *v).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::SessionId;

    #[tokio::test]
    async fn session_ctx_stores_and_retrieves() {
        let id = SessionId::new();
        assert_eq!(SessionCtx::current(), None);
        SessionCtx::scope(id, async {
            assert_eq!(SessionCtx::current(), Some(id));
        })
        .await;
        assert_eq!(SessionCtx::current(), None);
    }
}
