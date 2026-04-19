//! Axum router factory.

use axum::{Json, Router, middleware::from_fn_with_state, routing::get, routing::post};
use serde_json::json;
use std::net::SocketAddr;
use tokio::net::TcpListener;

use crate::{
    control_plane,
    routes::{config, event, permission, project, provider, question, session, workspace},
    state::AppState,
};
use opencode_core::error::ServerError;

/// Build the axum [`Router`] with all route groups registered.
///
/// `/health` is the liveness probe.
/// `/api/v1/projects` and session/message routes expose storage-backed REST APIs.
/// `/api/v1/sessions/:sid/prompt|cancel` call into the session runtime.
/// `/api/v1/permission*` and `/api/v1/question*` expose pending interactive runtime flows.
/// `/api/v1/provider/stream` is a manual harness route gated by env config.
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
        )
        .route("/sessions/{sid}/prompt", post(session::prompt))
        .route("/sessions/{sid}/cancel", post(session::cancel))
        // Session parity aliases (port-server-session-and-event-apis)
        .route("/session/status", get(session::list_runtime_status))
        .route("/session/{sid}/status", get(session::runtime_status))
        .route("/session/{sid}/abort", post(session::abort))
        .route("/session/{sid}/message", get(session::list_messages_alias))
        .route("/session/{sid}/prompt", post(session::prompt))
        .route("/provider", get(provider::list))
        .route("/provider/auth", get(provider::auth_methods))
        .route(
            "/provider/{provider}/oauth/authorize",
            post(provider::oauth_authorize),
        )
        .route(
            "/provider/{provider}/oauth/callback",
            post(provider::oauth_callback),
        )
        .route("/provider/account", get(provider::account_state))
        .route("/provider/account/use", post(provider::use_account))
        .route(
            "/provider/account/{account_id}",
            axum::routing::delete(provider::remove_account),
        )
        .route("/permission", get(permission::list))
        .route("/permission/reply", post(permission::reply))
        .route("/question", get(question::list))
        .route("/question/reply", post(question::reply))
        .route("/question/reject", post(question::reject))
        .route("/config", get(config::get_local).patch(config::patch_local))
        .route(
            "/global/config",
            get(config::get_global).patch(config::patch_global),
        )
        .route("/workspaces", get(workspace::list).post(workspace::create))
        .route(
            "/workspaces/{id}",
            get(workspace::get)
                .patch(workspace::patch)
                .delete(workspace::delete),
        )
        .route("/config/providers", get(config::providers))
        .route("/event", get(event::stream))
        // Manual provider harness (Phase 2 — env-gated)
        .route("/provider/stream", post(provider::stream));

    Router::new()
        .route("/health", get(health))
        .nest("/api/v1", api)
        // Control-plane sits in front of eligible API routes and either
        // forwards or allows local handlers to run unchanged.
        .layer(from_fn_with_state(state.clone(), control_plane::middleware))
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
    use futures::StreamExt;
    use opencode_bus::{BroadcastBus, BusEvent, EventBus};
    use opencode_core::config::Config;
    use opencode_core::config_service::ConfigService;
    use opencode_core::{
        dto::{
            AccountRow, AccountStateRow, ControlAccountRow, MessageRow, MessageWithParts, PartRow,
            PermissionRow, ProjectRow, SessionRow, TodoRow, WorkspaceRow,
        },
        error::{SessionError, StorageError},
        id::{AccountId, ProjectId, SessionId, WorkspaceId},
    };
    use opencode_provider::{AccountService, ModelRegistry, ProviderAuthService};
    use opencode_session::engine::Session;
    use opencode_session::types::{
        DetachedPromptAccepted, SessionHandle, SessionPrompt, SessionRuntimeStatus,
    };
    use opencode_storage::{Storage, StorageImpl, connect};
    use std::io::Write;
    use std::sync::Arc;
    use tokio::sync::Notify;
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
        async fn append_part(&self, _: PartRow) -> Result<(), StorageError> {
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
        async fn get_account(&self, _: AccountId) -> Result<Option<AccountRow>, StorageError> {
            Ok(None)
        }
        async fn remove_account(&self, _: AccountId) -> Result<(), StorageError> {
            Ok(())
        }
        async fn update_account_tokens(
            &self,
            _: AccountId,
            _: String,
            _: String,
            _: Option<i64>,
            _: i64,
        ) -> Result<(), StorageError> {
            Ok(())
        }
        async fn get_account_state(&self) -> Result<Option<AccountStateRow>, StorageError> {
            Ok(None)
        }
        async fn set_account_state(&self, _: AccountStateRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn get_control_account(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<ControlAccountRow>, StorageError> {
            Ok(None)
        }
        async fn get_active_control_account(
            &self,
        ) -> Result<Option<ControlAccountRow>, StorageError> {
            Ok(None)
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
        async fn prompt_detached(
            &self,
            _: SessionPrompt,
        ) -> Result<DetachedPromptAccepted, SessionError> {
            Err(SessionError::NotFound("stub".into()))
        }
        async fn status(&self, _: SessionId) -> Result<SessionRuntimeStatus, SessionError> {
            Err(SessionError::NotFound("stub".into()))
        }
        async fn list_statuses(
            &self,
        ) -> Result<std::collections::HashMap<SessionId, SessionRuntimeStatus>, SessionError>
        {
            Ok(std::collections::HashMap::new())
        }
    }

    fn state() -> AppState {
        state_with_config_service(ConfigService::with_cached_resolved(
            std::env::temp_dir(),
            None,
            Config::default(),
        ))
    }

    fn state_with_config_service(config_service: ConfigService) -> AppState {
        let storage: Arc<dyn Storage> = Arc::new(StubStorage);
        let bus = Arc::new(BroadcastBus::new(64));
        AppState {
            config_service: Arc::new(config_service),
            bus: Arc::clone(&bus),
            event_heartbeat: crate::state::EventHeartbeat::default(),
            storage: Arc::clone(&storage),
            session: Arc::new(StubSession),
            permission_runtime: Arc::new(
                opencode_session::permission_runtime::InMemoryPermissionRuntime::new(
                    Arc::clone(&storage),
                    Arc::clone(&bus),
                ),
            ),
            question_runtime: Arc::new(
                opencode_session::question_runtime::InMemoryQuestionRuntime::new(Arc::clone(&bus)),
            ),
            registry: Arc::new(ModelRegistry::new()),
            provider_catalog_models: Arc::new(Vec::new()),
            provider_auth: Arc::new(ProviderAuthService::new()),
            provider_accounts: Arc::new(AccountService::new(storage)),
            control_plane: crate::state::ControlPlaneConfig::default(),
            control_plane_proxy: Arc::new(crate::control_plane::proxy::HttpProxyService::new(
                reqwest::Client::new(),
                crate::state::ProxyPolicy::default(),
            )),
            harness: false,
        }
    }

    fn state_with_storage_and_control_plane(
        storage: Arc<dyn Storage>,
        control_plane: crate::state::ControlPlaneConfig,
    ) -> AppState {
        let config_service =
            ConfigService::with_cached_resolved(std::env::temp_dir(), None, Config::default());
        let bus = Arc::new(BroadcastBus::new(64));
        AppState {
            config_service: Arc::new(config_service),
            bus: Arc::clone(&bus),
            event_heartbeat: crate::state::EventHeartbeat::default(),
            storage: Arc::clone(&storage),
            session: Arc::new(StubSession),
            permission_runtime: Arc::new(
                opencode_session::permission_runtime::InMemoryPermissionRuntime::new(
                    Arc::clone(&storage),
                    Arc::clone(&bus),
                ),
            ),
            question_runtime: Arc::new(
                opencode_session::question_runtime::InMemoryQuestionRuntime::new(Arc::clone(&bus)),
            ),
            registry: Arc::new(ModelRegistry::new()),
            provider_catalog_models: Arc::new(Vec::new()),
            provider_auth: Arc::new(ProviderAuthService::new()),
            provider_accounts: Arc::new(AccountService::new(storage)),
            control_plane,
            control_plane_proxy: Arc::new(crate::control_plane::proxy::HttpProxyService::new(
                reqwest::Client::new(),
                crate::state::ProxyPolicy::default(),
            )),
            harness: false,
        }
    }

    fn write_jsonc(path: &std::path::Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut file = std::fs::File::create(path).unwrap();
        write!(file, "{content}").unwrap();
    }

    fn temp_test_dir(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("opencode-server-{label}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    async fn read_sse_frame(
        stream: &mut (impl futures::Stream<Item = Result<axum::body::Bytes, axum::Error>> + Unpin),
    ) -> String {
        let mut buffer = Vec::new();

        loop {
            let chunk = tokio::time::timeout(std::time::Duration::from_millis(100), stream.next())
                .await
                .expect("expected SSE bytes before timeout")
                .expect("stream should remain open")
                .expect("SSE chunk should be readable");
            buffer.extend_from_slice(&chunk);

            if buffer.windows(2).any(|window| window == b"\n\n") {
                return String::from_utf8(buffer).expect("SSE frame should be valid utf-8");
            }
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
    async fn workspace_collection_route_is_registered_under_api_namespace() {
        let app = build(state());
        let req = Request::builder()
            .uri("/api/v1/workspaces")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn workspace_item_route_rejects_unsupported_methods() {
        let app = build(state());
        let req = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/workspaces/{}",
                opencode_core::id::WorkspaceId::new()
            ))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn workspace_crud_persists_via_temp_sqlite_with_full_router() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let pool = connect(file.path()).await.unwrap();
        let storage: Arc<dyn Storage> = Arc::new(StorageImpl::new(pool));
        let project_id = ProjectId::new();
        storage
            .upsert_project(ProjectRow {
                id: project_id,
                worktree: "/tmp/project".into(),
                vcs: Some("git".into()),
                name: Some("project".into()),
                icon_url: None,
                icon_color: None,
                time_created: 1,
                time_updated: 1,
                time_initialized: None,
                sandboxes: serde_json::json!([]),
                commands: None,
            })
            .await
            .unwrap();

        let app = build(state_with_storage_and_control_plane(
            Arc::clone(&storage),
            crate::state::ControlPlaneConfig::new(
                "cp-local".into(),
                false,
                crate::state::ProxyPolicy::default(),
            ),
        ));

        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "type": "remote",
                            "project_id": project_id,
                            "name": "alpha",
                            "extra": {
                                "instance": "cp-remote",
                                "base_url": "https://cp-remote.example"
                            }
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::OK);
        let created_body = axum::body::to_bytes(created.into_body(), 8192)
            .await
            .unwrap();
        let created_json: serde_json::Value = serde_json::from_slice(&created_body).unwrap();
        let created_id = created_json["id"].as_str().unwrap().to_string();

        let fetched = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/{created_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(fetched.status(), StatusCode::OK);

        // Reopen the same SQLite file through a fresh storage + router instance to
        // verify persistence through the API boundary, not only in-memory state.
        let reopened_pool = connect(file.path()).await.unwrap();
        let reopened_storage: Arc<dyn Storage> = Arc::new(StorageImpl::new(reopened_pool));
        let reopened = build(state_with_storage_and_control_plane(
            Arc::clone(&reopened_storage),
            crate::state::ControlPlaneConfig::new(
                "cp-local".into(),
                false,
                crate::state::ProxyPolicy::default(),
            ),
        ));

        let persisted = reopened
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/{created_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(persisted.status(), StatusCode::OK);

        let deleted = reopened
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/workspaces/{created_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(deleted.status(), StatusCode::OK);

        let missing = reopened
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/{created_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn session_get_with_remote_selector_is_forwarded_by_control_plane() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path, query_param},
        };

        let upstream = MockServer::start().await;
        let sid = SessionId::new();
        let workspace_id = WorkspaceId::new();

        Mock::given(method("GET"))
            .and(path(format!("/api/v1/sessions/{sid}")))
            .and(query_param("workspace", workspace_id.to_string()))
            .respond_with(
                ResponseTemplate::new(202).set_body_json(serde_json::json!({"proxied": true})),
            )
            .mount(&upstream)
            .await;

        let file = tempfile::NamedTempFile::new().unwrap();
        let pool = connect(file.path()).await.unwrap();
        let storage: Arc<dyn Storage> = Arc::new(StorageImpl::new(pool));
        let project_id = ProjectId::new();
        storage
            .upsert_project(ProjectRow {
                id: project_id,
                worktree: "/tmp/project".into(),
                vcs: Some("git".into()),
                name: Some("project".into()),
                icon_url: None,
                icon_color: None,
                time_created: 1,
                time_updated: 1,
                time_initialized: None,
                sandboxes: serde_json::json!([]),
                commands: None,
            })
            .await
            .unwrap();
        storage
            .upsert_workspace(WorkspaceRow {
                id: workspace_id,
                r#type: "remote".into(),
                branch: None,
                name: Some("remote".into()),
                directory: None,
                extra: Some(serde_json::json!({
                    "instance": "cp-remote",
                    "base_url": upstream.uri(),
                })),
                project_id,
            })
            .await
            .unwrap();

        let app = build(state_with_storage_and_control_plane(
            storage,
            crate::state::ControlPlaneConfig::new(
                "cp-local".into(),
                false,
                crate::state::ProxyPolicy::default(),
            ),
        ));
        let req = Request::builder()
            .uri(format!("/api/v1/sessions/{sid}?workspace={workspace_id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        let received = upstream.received_requests().await.unwrap();
        assert_eq!(received.len(), 1, "request should be forwarded upstream");

        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["proxied"], true);
    }

    #[tokio::test]
    async fn session_get_without_selector_keeps_existing_local_behavior() {
        use wiremock::{MockServer, matchers::method};

        let upstream = MockServer::start().await;
        let sid = SessionId::new();

        // Guard rail: no forwarding should happen when selector is absent.
        wiremock::Mock::given(method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&upstream)
            .await;

        let file = tempfile::NamedTempFile::new().unwrap();
        let pool = connect(file.path()).await.unwrap();
        let storage: Arc<dyn Storage> = Arc::new(StorageImpl::new(pool));
        let app = build(state_with_storage_and_control_plane(
            storage,
            crate::state::ControlPlaneConfig::new(
                "cp-local".into(),
                false,
                crate::state::ProxyPolicy::default(),
            ),
        ));

        let req = Request::builder()
            .uri(format!("/api/v1/sessions/{sid}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let received = upstream.received_requests().await.unwrap();
        assert!(
            received.is_empty(),
            "selector-free requests must stay local"
        );
    }

    #[tokio::test]
    async fn websocket_upgrade_forwarding_returns_not_implemented() {
        let sid = SessionId::new();
        let workspace_id = WorkspaceId::new();

        let file = tempfile::NamedTempFile::new().unwrap();
        let pool = connect(file.path()).await.unwrap();
        let storage: Arc<dyn Storage> = Arc::new(StorageImpl::new(pool));
        let project_id = ProjectId::new();
        storage
            .upsert_project(ProjectRow {
                id: project_id,
                worktree: "/tmp/project".into(),
                vcs: Some("git".into()),
                name: Some("project".into()),
                icon_url: None,
                icon_color: None,
                time_created: 1,
                time_updated: 1,
                time_initialized: None,
                sandboxes: serde_json::json!([]),
                commands: None,
            })
            .await
            .unwrap();
        storage
            .upsert_workspace(WorkspaceRow {
                id: workspace_id,
                r#type: "remote".into(),
                branch: None,
                name: Some("remote".into()),
                directory: None,
                extra: Some(serde_json::json!({
                    "instance": "cp-remote",
                    "base_url": "https://remote.example",
                })),
                project_id,
            })
            .await
            .unwrap();

        let app = build(state_with_storage_and_control_plane(
            storage,
            crate::state::ControlPlaneConfig::new(
                "cp-local".into(),
                false,
                crate::state::ProxyPolicy::default(),
            ),
        ));

        let req = Request::builder()
            .uri(format!("/api/v1/sessions/{sid}?workspace={workspace_id}"))
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn rollout_rollback_force_local_switch_prevents_remote_forwarding() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

        let upstream = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&upstream)
            .await;

        let sid = SessionId::new();
        let workspace_id = WorkspaceId::new();
        let file = tempfile::NamedTempFile::new().unwrap();
        let pool = connect(file.path()).await.unwrap();
        let storage: Arc<dyn Storage> = Arc::new(StorageImpl::new(pool));
        let project_id = ProjectId::new();
        storage
            .upsert_project(ProjectRow {
                id: project_id,
                worktree: "/tmp/project".into(),
                vcs: Some("git".into()),
                name: Some("project".into()),
                icon_url: None,
                icon_color: None,
                time_created: 1,
                time_updated: 1,
                time_initialized: None,
                sandboxes: serde_json::json!([]),
                commands: None,
            })
            .await
            .unwrap();
        storage
            .upsert_workspace(WorkspaceRow {
                id: workspace_id,
                r#type: "remote".into(),
                branch: None,
                name: Some("remote".into()),
                directory: None,
                extra: Some(serde_json::json!({
                    "instance": "cp-remote",
                    "base_url": upstream.uri(),
                })),
                project_id,
            })
            .await
            .unwrap();

        // Rollback mode forces all selector-bearing requests to remain local,
        // even when workspace metadata points at another instance.
        let app = build(state_with_storage_and_control_plane(
            storage,
            crate::state::ControlPlaneConfig::new(
                "cp-local".into(),
                true,
                crate::state::ProxyPolicy::default(),
            ),
        ));

        let req = Request::builder()
            .uri(format!("/api/v1/sessions/{sid}?workspace={workspace_id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let forwarded = upstream.received_requests().await.unwrap();
        assert!(
            forwarded.is_empty(),
            "force-local rollback must suppress remote forwarding"
        );
    }

    #[tokio::test]
    async fn websocket_upgrade_forwarding_returns_parity_501_error_payload() {
        let sid = SessionId::new();
        let workspace_id = WorkspaceId::new();

        let file = tempfile::NamedTempFile::new().unwrap();
        let pool = connect(file.path()).await.unwrap();
        let storage: Arc<dyn Storage> = Arc::new(StorageImpl::new(pool));
        let project_id = ProjectId::new();
        storage
            .upsert_project(ProjectRow {
                id: project_id,
                worktree: "/tmp/project".into(),
                vcs: Some("git".into()),
                name: Some("project".into()),
                icon_url: None,
                icon_color: None,
                time_created: 1,
                time_updated: 1,
                time_initialized: None,
                sandboxes: serde_json::json!([]),
                commands: None,
            })
            .await
            .unwrap();
        storage
            .upsert_workspace(WorkspaceRow {
                id: workspace_id,
                r#type: "remote".into(),
                branch: None,
                name: Some("remote".into()),
                directory: None,
                extra: Some(serde_json::json!({
                    "instance": "cp-remote",
                    "base_url": "https://remote.example",
                })),
                project_id,
            })
            .await
            .unwrap();

        let app = build(state_with_storage_and_control_plane(
            storage,
            crate::state::ControlPlaneConfig::new(
                "cp-local".into(),
                false,
                crate::state::ProxyPolicy::default(),
            ),
        ));

        let req = Request::builder()
            .uri(format!("/api/v1/sessions/{sid}?workspace={workspace_id}"))
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);

        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            payload["error"]["code"],
            serde_json::Value::String("websocket_forwarding_deferred".into())
        );
    }

    #[test]
    fn control_plane_selector_module_exposes_query_first_resolution() {
        let uri: axum::http::Uri = "/api/v1/sessions/abc?workspace=query-wins".parse().unwrap();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "x-opencode-workspace",
            axum::http::HeaderValue::from_static("header-fallback"),
        );

        let selector = crate::control_plane::resolver::resolve_selector(&uri, &headers)
            .expect("selector resolution should succeed")
            .expect("query selector should be found");
        assert_eq!(selector.raw, "query-wins");
    }

    #[tokio::test]
    async fn local_config_route_returns_scoped_persisted_payload() {
        let temp = temp_test_dir("local-config-route");
        let project_dir = temp.join("project");
        let global_path = temp.join("home/.config/opencode/config.jsonc");
        write_jsonc(
            &project_dir.join(".opencode/config.jsonc"),
            r#"{ "model": "openai/gpt-5", "server": { "port": 7001 } }"#,
        );

        let service = ConfigService::with_global_config_path(project_dir, Some(global_path));
        let app = build(state_with_config_service(service));
        let req = Request::builder()
            .uri("/api/v1/config")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["scope"], "local");
        assert_eq!(value["config"]["model"], "openai/gpt-5");
        assert_eq!(value["config"]["server"]["port"], 7001);
    }

    #[tokio::test]
    async fn global_config_route_returns_scope_with_default_payload_when_missing() {
        let temp = temp_test_dir("global-config-route");
        let project_dir = temp.join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        let global_path = temp.join("home/.config/opencode/config.jsonc");

        let service = ConfigService::with_global_config_path(project_dir, Some(global_path));
        let app = build(state_with_config_service(service));
        let req = Request::builder()
            .uri("/api/v1/global/config")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["scope"], "global");
        assert_eq!(value["config"]["server"]["port"], 4141);
        assert_eq!(value["config"]["server"]["host"], "127.0.0.1");
    }

    #[tokio::test]
    async fn local_config_route_returns_scope_with_default_payload_when_missing() {
        let temp = temp_test_dir("local-config-default-route");
        let project_dir = temp.join("project");
        std::fs::create_dir_all(&project_dir).unwrap();

        let service = ConfigService::with_global_config_path(project_dir, None);
        let app = build(state_with_config_service(service));
        let req = Request::builder()
            .uri("/api/v1/config")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["scope"], "local");
        assert_eq!(value["config"]["server"]["port"], 4141);
        assert_eq!(value["config"]["server"]["host"], "127.0.0.1");
        assert!(value["config"]["model"].is_null());
    }

    #[tokio::test]
    async fn global_config_route_returns_scoped_persisted_payload() {
        let temp = temp_test_dir("global-config-persisted-route");
        let project_dir = temp.join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        let global_path = temp.join("home/.config/opencode/config.jsonc");
        write_jsonc(
            &global_path,
            r#"{ "model": "anthropic/claude-3-7-sonnet", "server": { "host": "0.0.0.0", "port": 8181 } }"#,
        );

        let service = ConfigService::with_global_config_path(project_dir, Some(global_path));
        let app = build(state_with_config_service(service));
        let req = Request::builder()
            .uri("/api/v1/global/config")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["scope"], "global");
        assert_eq!(value["config"]["model"], "anthropic/claude-3-7-sonnet");
        assert_eq!(value["config"]["server"]["host"], "0.0.0.0");
        assert_eq!(value["config"]["server"]["port"], 8181);
    }

    #[tokio::test]
    async fn local_config_patch_refreshes_provider_listing() {
        let temp = temp_test_dir("local-config-patch-refresh");
        let project_dir = temp.join("project");
        let global_path = temp.join("home/.config/opencode/config.jsonc");

        let mut cached = Config::default();
        cached.providers.openai = Some("sk-openai".into());
        let service = ConfigService::with_cached_resolved(project_dir, Some(global_path), cached);
        let app = build(state_with_config_service(service));

        let before = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/provider")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(before.status(), StatusCode::OK);
        let before_body = axum::body::to_bytes(before.into_body(), 8192)
            .await
            .unwrap();
        let before_json: serde_json::Value = serde_json::from_slice(&before_body).unwrap();
        assert_eq!(before_json["connected"], serde_json::json!(["openai"]));

        let patch = Request::builder()
            .method("PATCH")
            .uri("/api/v1/config")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "providers": { "anthropic": "sk-ant" }
                }))
                .unwrap(),
            ))
            .unwrap();
        let patch_resp = app.clone().oneshot(patch).await.unwrap();
        assert_eq!(patch_resp.status(), StatusCode::OK);
        let patch_body = axum::body::to_bytes(patch_resp.into_body(), 8192)
            .await
            .unwrap();
        let patch_json: serde_json::Value = serde_json::from_slice(&patch_body).unwrap();
        assert_eq!(patch_json["scope"], "local");
        assert_eq!(patch_json["config"]["providers"]["anthropic"], "sk-ant");

        let after = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/provider")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(after.status(), StatusCode::OK);
        let after_body = axum::body::to_bytes(after.into_body(), 8192).await.unwrap();
        let after_json: serde_json::Value = serde_json::from_slice(&after_body).unwrap();
        assert_eq!(after_json["connected"], serde_json::json!(["anthropic"]));
    }

    #[tokio::test]
    async fn global_config_patch_persists_and_reads_back_scoped_payload() {
        let temp = temp_test_dir("global-config-patch");
        let project_dir = temp.join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        let global_path = temp.join("home/.config/opencode/config.jsonc");

        let service = ConfigService::with_global_config_path(project_dir, Some(global_path));
        let app = build(state_with_config_service(service));

        let patch = Request::builder()
            .method("PATCH")
            .uri("/api/v1/global/config")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "server": { "host": "0.0.0.0", "port": 7331 }
                }))
                .unwrap(),
            ))
            .unwrap();
        let patch_resp = app.clone().oneshot(patch).await.unwrap();
        assert_eq!(patch_resp.status(), StatusCode::OK);
        let patch_body = axum::body::to_bytes(patch_resp.into_body(), 8192)
            .await
            .unwrap();
        let patch_json: serde_json::Value = serde_json::from_slice(&patch_body).unwrap();
        assert_eq!(patch_json["scope"], "global");
        assert_eq!(patch_json["config"]["server"]["host"], "0.0.0.0");
        assert_eq!(patch_json["config"]["server"]["port"], 7331);

        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/global/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
        let get_body = axum::body::to_bytes(get_resp.into_body(), 8192)
            .await
            .unwrap();
        let get_json: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
        assert_eq!(get_json["scope"], "global");
        assert_eq!(get_json["config"]["server"]["host"], "0.0.0.0");
        assert_eq!(get_json["config"]["server"]["port"], 7331);
    }

    #[tokio::test]
    async fn global_config_patch_refreshes_provider_listing() {
        let temp = temp_test_dir("global-config-patch-refresh");
        let project_dir = temp.join("project");
        let global_path = temp.join("home/.config/opencode/config.jsonc");

        let mut cached = Config::default();
        cached.providers.openai = Some("sk-openai".into());
        let service = ConfigService::with_cached_resolved(project_dir, Some(global_path), cached);
        let app = build(state_with_config_service(service));

        let before = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/provider")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(before.status(), StatusCode::OK);
        let before_body = axum::body::to_bytes(before.into_body(), 8192)
            .await
            .unwrap();
        let before_json: serde_json::Value = serde_json::from_slice(&before_body).unwrap();
        assert_eq!(before_json["connected"], serde_json::json!(["openai"]));

        let patch = Request::builder()
            .method("PATCH")
            .uri("/api/v1/global/config")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "providers": { "anthropic": "sk-ant" }
                }))
                .unwrap(),
            ))
            .unwrap();
        let patch_resp = app.clone().oneshot(patch).await.unwrap();
        assert_eq!(patch_resp.status(), StatusCode::OK);

        let after = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/provider")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(after.status(), StatusCode::OK);
        let after_body = axum::body::to_bytes(after.into_body(), 8192).await.unwrap();
        let after_json: serde_json::Value = serde_json::from_slice(&after_body).unwrap();
        assert_eq!(after_json["connected"], serde_json::json!(["anthropic"]));
    }

    #[tokio::test]
    async fn invalid_local_config_patch_returns_client_error_and_keeps_cached_provider_view() {
        let temp = temp_test_dir("invalid-local-config-patch");
        let project_dir = temp.join("project");
        let global_path = temp.join("home/.config/opencode/config.jsonc");

        let mut cached = Config::default();
        cached.providers.openai = Some("sk-openai".into());
        let service = ConfigService::with_cached_resolved(project_dir, Some(global_path), cached);
        let app = build(state_with_config_service(service));

        let before = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/provider")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(before.status(), StatusCode::OK);
        let before_body = axum::body::to_bytes(before.into_body(), 8192)
            .await
            .unwrap();
        let before_json: serde_json::Value = serde_json::from_slice(&before_body).unwrap();
        assert_eq!(before_json["connected"], serde_json::json!(["openai"]));

        let invalid_patch = Request::builder()
            .method("PATCH")
            .uri("/api/v1/config")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "server": { "port": "not-a-port" }
                }))
                .unwrap(),
            ))
            .unwrap();
        let invalid_resp = app.clone().oneshot(invalid_patch).await.unwrap();
        assert!(invalid_resp.status().is_client_error());

        let after = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/provider")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(after.status(), StatusCode::OK);
        let after_body = axum::body::to_bytes(after.into_body(), 8192).await.unwrap();
        let after_json: serde_json::Value = serde_json::from_slice(&after_body).unwrap();
        assert_eq!(after_json["connected"], serde_json::json!(["openai"]));
    }

    #[tokio::test]
    async fn invalid_global_config_patch_returns_client_error_and_keeps_persisted_payload() {
        let temp = temp_test_dir("invalid-global-config-patch");
        let project_dir = temp.join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        let global_path = temp.join("home/.config/opencode/config.jsonc");
        write_jsonc(
            &global_path,
            r#"{ "server": { "host": "127.0.0.1", "port": 9444 } }"#,
        );

        let service = ConfigService::with_global_config_path(project_dir, Some(global_path));
        let app = build(state_with_config_service(service));

        let invalid_patch = Request::builder()
            .method("PATCH")
            .uri("/api/v1/global/config")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "server": { "port": "not-a-port" }
                }))
                .unwrap(),
            ))
            .unwrap();
        let invalid_resp = app.clone().oneshot(invalid_patch).await.unwrap();
        assert!(invalid_resp.status().is_client_error());

        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/global/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(get_resp.into_body(), 8192)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["scope"], "global");
        assert_eq!(json["config"]["server"]["host"], "127.0.0.1");
        assert_eq!(json["config"]["server"]["port"], 9444);
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
            workspace_id: None,
            parent_id: None,
            slug: "test".into(),
            directory: "/tmp".into(),
            title: "Test".into(),
            version: "0".into(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
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

    // ── Phase 7: provider harness route tests ────────────────────────────────

    // ── Provider stub for failure path testing ────────────────────────────────

    struct FailProvider;

    #[async_trait]
    impl opencode_provider::LanguageModel for FailProvider {
        fn provider(&self) -> &'static str {
            "fail"
        }
        async fn models(
            &self,
        ) -> Result<Vec<opencode_provider::ModelInfo>, opencode_provider::ProviderError> {
            Ok(vec![])
        }
        async fn stream(
            &self,
            _: opencode_provider::ModelRequest,
        ) -> Result<
            opencode_core::context::BoxStream<
                Result<opencode_provider::ModelEvent, opencode_provider::ProviderError>,
            >,
            opencode_provider::ProviderError,
        > {
            Err(opencode_provider::ProviderError::Auth {
                provider: "fail".into(),
                msg: "injected auth failure".into(),
            })
        }
    }

    // RED 7.1 — harness disabled → 403
    #[tokio::test]
    async fn provider_stream_disabled_returns_403() {
        let app = build(state()); // harness: false
        let body = serde_json::json!({
            "provider": "openai",
            "model": "gpt-4o",
            "prompt": "hello"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/provider/stream")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // RED 7.2 — harness enabled, unknown provider → 404
    #[tokio::test]
    async fn provider_stream_unknown_provider_returns_404() {
        let mut s = state();
        s.harness = true;
        let app = build(s);
        let body = serde_json::json!({
            "provider": "does-not-exist",
            "model": "model-x",
            "prompt": "hello"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/provider/stream")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // RED 7.3 — harness enabled, registered provider, wiremock → 200 SSE
    #[tokio::test]
    async fn provider_stream_returns_sse_events() {
        use opencode_provider::{EnvAuthResolver, OpenAiProvider};
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path as wm_path},
        };

        let srv = MockServer::start().await;
        let fixture = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        Mock::given(method("POST"))
            .and(wm_path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(fixture),
            )
            .mount(&srv)
            .await;

        let auth = std::sync::Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_KEY_HARNESS_TEST_NONEXISTENT",
            Some("key".into()),
        ));
        let reg = ModelRegistry::new();
        reg.register(
            "openai",
            std::sync::Arc::new(OpenAiProvider::with_base_url(auth, srv.uri())),
        )
        .await;

        let mut s = state();
        s.harness = true;
        s.registry = std::sync::Arc::new(reg);

        let app = build(s);
        let body = serde_json::json!({
            "provider": "openai",
            "model": "gpt-4o",
            "prompt": "hello"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/provider/stream")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("text/event-stream"));
    }

    // RED 7.4 — harness enabled, provider.stream() fails → 502 BAD_GATEWAY
    #[tokio::test]
    async fn provider_stream_provider_error_returns_502() {
        let reg = ModelRegistry::new();
        reg.register("fail", std::sync::Arc::new(FailProvider))
            .await;

        let mut s = state();
        s.harness = true;
        s.registry = std::sync::Arc::new(reg);

        let app = build(s);
        let body = serde_json::json!({
            "provider": "fail",
            "model": "model-x",
            "prompt": "hello"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/provider/stream")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            val["error"].as_str().is_some(),
            "response must have error field"
        );
    }

    // RED 7.5 — SSE body contains serialised ModelEvent text
    #[tokio::test]
    async fn provider_stream_sse_body_contains_event_data() {
        use opencode_provider::{EnvAuthResolver, OpenAiProvider};
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path as wm_path},
        };

        let srv = MockServer::start().await;
        let fixture = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        Mock::given(method("POST"))
            .and(wm_path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(fixture),
            )
            .mount(&srv)
            .await;

        let auth = std::sync::Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_KEY_BODY_TEST_NONEXISTENT",
            Some("key".into()),
        ));
        let reg = ModelRegistry::new();
        reg.register(
            "openai",
            std::sync::Arc::new(OpenAiProvider::with_base_url(auth, srv.uri())),
        )
        .await;

        let mut s = state();
        s.harness = true;
        s.registry = std::sync::Arc::new(reg);

        let app = build(s);
        let body = serde_json::json!({
            "provider": "openai",
            "model": "gpt-4o",
            "prompt": "hello"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/provider/stream")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Read the full SSE body and verify it contains a TextDelta event.
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let body_str = std::str::from_utf8(&bytes).unwrap();
        assert!(
            body_str.contains("text_delta"),
            "SSE body must contain text_delta event, got: {body_str}"
        );
        assert!(
            body_str.contains("hi"),
            "SSE body must contain the prompt reply text, got: {body_str}"
        );
    }

    #[tokio::test]
    async fn instance_event_route_returns_sse_content_type() {
        let app = build(state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/event")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .expect("event route should set content-type")
            .to_str()
            .unwrap();
        assert!(ct.contains("text/event-stream"));
    }

    #[tokio::test]
    async fn instance_event_route_bootstrap_streams_wire_event_payload() {
        let app = build(state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/event")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let mut stream = resp.into_body().into_data_stream();
        let body_str = read_sse_frame(&mut stream).await;
        assert!(
            body_str.starts_with("data: {"),
            "expected SSE data frame: {body_str}"
        );
        assert!(body_str.contains("\"type\":\"server.connected\""));
        assert!(body_str.contains("\"properties\":{}"));
    }

    #[tokio::test]
    async fn instance_event_route_heartbeat_waits_for_idle_interval_before_wire_frame() {
        let mut state = state();
        let _bus_guard = Arc::clone(&state.bus);
        let heartbeat = Arc::new(Notify::new());
        state.event_heartbeat = crate::state::EventHeartbeat::Manual(Arc::clone(&heartbeat));
        let app = build(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/event")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let mut stream = resp.into_body().into_data_stream();
        let body_str = read_sse_frame(&mut stream).await;
        assert!(body_str.contains("\"type\":\"server.connected\""));

        match tokio::time::timeout(std::time::Duration::from_millis(10), stream.next()).await {
            Err(_) => {}
            Ok(Some(Ok(bytes))) => panic!(
                "heartbeat must not arrive before the manual trigger fires, got chunk: {}",
                std::str::from_utf8(&bytes).unwrap_or("<non-utf8>")
            ),
            Ok(Some(Err(err))) => panic!(
                "heartbeat must not arrive before the manual trigger fires, got body error: {err}"
            ),
            Ok(None) => panic!(
                "heartbeat must not arrive before the manual trigger fires, stream closed early"
            ),
        }

        heartbeat.notify_one();

        let heartbeat = read_sse_frame(&mut stream).await;
        assert!(heartbeat.contains("\"type\":\"server.heartbeat\""));
        assert!(heartbeat.contains("\"properties\":{}"));
    }

    #[tokio::test]
    async fn instance_event_route_streams_translated_bus_event_payload() {
        let state = state();
        let bus = std::sync::Arc::clone(&state.bus);
        let app = build(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/event")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let mut stream = resp.into_body().into_data_stream();
        let _ = read_sse_frame(&mut stream).await;
        let sid = SessionId::new();
        let mid = opencode_core::id::MessageId::new();
        let _ = bus.publish(BusEvent::MessageAdded {
            session_id: sid,
            message_id: mid,
        });
        let body_str = read_sse_frame(&mut stream).await;
        assert!(
            body_str.starts_with("data: {"),
            "expected SSE data frame: {body_str}"
        );
        assert!(body_str.contains("\"type\":\"message.added\""));
        assert!(body_str.contains(&format!("\"session_id\":\"{sid}\"")));
        assert!(body_str.contains(&format!("\"message_id\":\"{mid}\"")));
    }

    #[tokio::test]
    async fn instance_event_route_streams_permission_and_question_payloads() {
        let state = state();
        let bus = std::sync::Arc::clone(&state.bus);
        let app = build(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/event")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let mut stream = resp.into_body().into_data_stream();
        let _ = read_sse_frame(&mut stream).await;
        let sid = SessionId::new();

        let _ = bus.publish(BusEvent::PermissionAsked {
            session_id: sid,
            request_id: "perm-sse".into(),
            permission: "bash".into(),
            patterns: vec!["/tmp/**".into()],
            metadata: serde_json::json!({"source": "router-test"}),
            always: vec!["/tmp/**".into()],
            tool: None,
        });
        let permission_frame = read_sse_frame(&mut stream).await;
        assert!(permission_frame.contains("\"type\":\"permission.asked\""));
        assert!(permission_frame.contains("\"request_id\":\"perm-sse\""));

        let _ = bus.publish(BusEvent::QuestionRejected {
            session_id: sid,
            request_id: "q-sse".into(),
        });
        let question_frame = read_sse_frame(&mut stream).await;
        assert!(question_frame.contains("\"type\":\"question.rejected\""));
        assert!(question_frame.contains("\"request_id\":\"q-sse\""));
    }

    #[tokio::test]
    async fn permission_and_question_routes_are_registered() {
        let app = build(state());

        let permission_list = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/permission")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(permission_list.status(), StatusCode::NOT_FOUND);

        let question_list = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/question")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(question_list.status(), StatusCode::NOT_FOUND);
    }
}
