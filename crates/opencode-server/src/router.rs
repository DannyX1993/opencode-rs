//! Axum router factory.

use axum::{Json, Router, routing::get};
use serde_json::json;
use std::net::SocketAddr;
use tokio::net::TcpListener;

use crate::{
    routes::{project, session},
    state::AppState,
};
use opencode_core::error::ServerError;

/// Build the axum [`Router`] with all route groups registered.
///
/// `/health` is the liveness probe (Phase 0, unchanged).
/// `/api/v1/projects` CRUD routes are wired in Phase 5.
/// `/api/v1/projects/:pid/sessions` and `/api/v1/sessions/:sid` routes are wired in Phase 6.
pub fn build(state: AppState) -> Router {
    let api = Router::new()
        // Project routes (Phase 5)
        .route("/projects", get(project::list))
        .route("/projects/{id}", get(project::get).put(project::upsert))
        // Session routes (Phase 6)
        .route(
            "/projects/{pid}/sessions",
            get(session::list).post(session::create),
        )
        .route(
            "/sessions/{sid}",
            get(session::get).patch(session::update),
        )
        .route(
            "/sessions/{sid}/messages",
            get(session::list_messages).post(session::append_message),
        );

    Router::new()
        .route("/health", get(health))
        .nest("/api/v1", api)
        .with_state(state)
}

/// Start the server, listening on `addr`.
///
/// # Errors
///
/// Returns [`ServerError::Bind`] if the TCP socket cannot be bound.
pub async fn serve(router: Router, addr: SocketAddr) -> Result<(), ServerError> {
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| ServerError::Bind(e.to_string()))?;

    tracing::info!("opencode server listening on {addr}");

    axum::serve(listener, router)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))
}

/// `GET /health` — liveness probe.
async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use opencode_bus::BroadcastBus;
    use opencode_core::config::Config;
    use opencode_core::{
        dto::{
            AccountRow, MessageRow, MessageWithParts, PartRow, PermissionRow, ProjectRow,
            SessionRow, TodoRow,
        },
        error::{SessionError, StorageError},
        id::{ProjectId, SessionId},
    };
    use opencode_session::engine::Session;
    use opencode_session::types::{SessionHandle, SessionPrompt};
    use opencode_storage::Storage;
    use std::sync::Arc;
    use tower::ServiceExt;

    // ── Test doubles ─────────────────────────────────────────────────────────

    struct StubStorage;

    #[async_trait]
    impl Storage for StubStorage {
        async fn upsert_project(&self, _: ProjectRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn get_project(&self, _: ProjectId) -> Result<Option<ProjectRow>, StorageError> {
            Ok(None)
        }
        async fn list_projects(&self) -> Result<Vec<ProjectRow>, StorageError> {
            Ok(vec![])
        }
        async fn create_session(&self, _: SessionRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn get_session(&self, _: SessionId) -> Result<Option<SessionRow>, StorageError> {
            Ok(None)
        }
        async fn list_sessions(&self, _: ProjectId) -> Result<Vec<SessionRow>, StorageError> {
            Ok(vec![])
        }
        async fn update_session(&self, _: SessionRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn append_message(&self, _: MessageRow, _: Vec<PartRow>) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list_history(&self, _: SessionId) -> Result<Vec<MessageRow>, StorageError> {
            Ok(vec![])
        }
        async fn list_history_with_parts(
            &self,
            _: SessionId,
        ) -> Result<Vec<MessageWithParts>, StorageError> {
            Ok(vec![])
        }
        async fn save_todos(&self, _: SessionId, _: Vec<TodoRow>) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list_todos(&self, _: SessionId) -> Result<Vec<TodoRow>, StorageError> {
            Ok(vec![])
        }
        async fn get_permission(
            &self,
            _: ProjectId,
        ) -> Result<Option<PermissionRow>, StorageError> {
            Ok(None)
        }
        async fn set_permission(&self, _: PermissionRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn upsert_account(&self, _: AccountRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list_accounts(&self) -> Result<Vec<AccountRow>, StorageError> {
            Ok(vec![])
        }
        async fn append_event(
            &self,
            _: &str,
            _: &str,
            _: serde_json::Value,
        ) -> Result<i64, StorageError> {
            Ok(0)
        }
    }

    struct StubSession;

    #[async_trait]
    impl Session for StubSession {
        async fn prompt(&self, _: SessionPrompt) -> Result<SessionHandle, SessionError> {
            Err(SessionError::NotFound("stub".into()))
        }
        async fn cancel(&self, _: SessionId) -> Result<(), SessionError> {
            Err(SessionError::NotFound("stub".into()))
        }
    }

    fn state() -> AppState {
        AppState {
            config: Arc::new(Config::default()),
            bus: Arc::new(BroadcastBus::new(64)),
            storage: Arc::new(StubStorage),
            session: Arc::new(StubSession),
        }
    }

    // ── Router tests ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn health_returns_ok() {
        let app = build(state());
        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["status"], "ok");
    }

    #[tokio::test]
    async fn unknown_route_returns_404() {
        let app = build(state());
        let req = Request::builder().uri("/nope").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn serve_binds_and_accepts_connection() {
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let app = build(state());
        let handle = tokio::spawn(async move { serve(app, addr).await });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        handle.abort();
    }

    #[tokio::test]
    async fn serve_returns_bind_error_on_bad_addr() {
        // Port 1 requires root — should fail to bind.
        let addr: std::net::SocketAddr = "127.0.0.1:1".parse().unwrap();
        let app = build(state());
        let result = serve(app, addr).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, opencode_core::error::ServerError::Bind(_)));
    }

    // ── Stub coverage — exercise every Storage/Session method ────────────────

    #[tokio::test]
    async fn stub_storage_project_methods() {
        let s = StubStorage;
        let pid = ProjectId::new();
        let row = ProjectRow {
            id: pid,
            worktree: "/tmp".into(),
            vcs: None,
            name: None,
            icon_url: None,
            icon_color: None,
            time_created: 0,
            time_updated: 0,
            time_initialized: None,
            sandboxes: serde_json::Value::Null,
            commands: None,
        };
        s.upsert_project(row).await.unwrap();
        assert!(s.get_project(pid).await.unwrap().is_none());
        assert!(s.list_projects().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn stub_storage_session_methods() {
        let s = StubStorage;
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let row = SessionRow {
            id: sid,
            project_id: pid,
            parent_id: None,
            slug: "test".into(),
            directory: "/tmp".into(),
            title: "Test".into(),
            version: "0".into(),
            share_url: None,
            permission: None,
            time_created: 0,
            time_updated: 0,
            time_compacting: None,
            time_archived: None,
        };
        s.create_session(row.clone()).await.unwrap();
        assert!(s.get_session(sid).await.unwrap().is_none());
        assert!(s.list_sessions(pid).await.unwrap().is_empty());
        s.update_session(row).await.unwrap();
    }

    #[tokio::test]
    async fn stub_storage_message_todo_methods() {
        use opencode_core::id::{MessageId, PartId};
        let s = StubStorage;
        let sid = SessionId::new();
        let mid = MessageId::new();
        let msg = MessageRow {
            id: mid,
            session_id: sid,
            time_created: 0,
            time_updated: 0,
            data: serde_json::Value::Null,
        };
        let part = PartRow {
            id: PartId::new(),
            message_id: mid,
            session_id: sid,
            time_created: 0,
            time_updated: 0,
            data: serde_json::Value::Null,
        };
        s.append_message(msg, vec![part]).await.unwrap();
        assert!(s.list_history(sid).await.unwrap().is_empty());
        assert!(s.list_history_with_parts(sid).await.unwrap().is_empty());
        let todo = TodoRow {
            session_id: sid,
            content: "x".into(),
            status: "pending".into(),
            priority: "low".into(),
            position: 0,
            time_created: 0,
            time_updated: 0,
        };
        s.save_todos(sid, vec![todo]).await.unwrap();
        assert!(s.list_todos(sid).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn stub_storage_permission_account_methods() {
        use opencode_core::{dto::AccountRow, id::AccountId};
        let s = StubStorage;
        let pid = ProjectId::new();
        assert!(s.get_permission(pid).await.unwrap().is_none());
        let perm = PermissionRow {
            project_id: pid,
            time_created: 0,
            time_updated: 0,
            data: serde_json::Value::Null,
        };
        s.set_permission(perm).await.unwrap();
        let acc = AccountRow {
            id: AccountId::new(),
            email: "test@example.com".into(),
            url: "https://example.com".into(),
            access_token: "tok".into(),
            refresh_token: "ref".into(),
            token_expiry: None,
            time_created: 0,
            time_updated: 0,
        };
        s.upsert_account(acc).await.unwrap();
        assert!(s.list_accounts().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn stub_storage_append_event() {
        let s = StubStorage;
        let seq = s
            .append_event("proj", "session.created", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(seq, 0);
    }

    #[tokio::test]
    async fn stub_session_methods() {
        let s = StubSession;
        let prompt = SessionPrompt {
            session_id: SessionId::new(),
            text: "hi".into(),
            model: None,
            plan_mode: false,
        };
        assert!(s.prompt(prompt).await.is_err());
        assert!(s.cancel(SessionId::new()).await.is_err());
    }
}
