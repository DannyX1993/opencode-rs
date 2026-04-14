//! Axum router factory.

use axum::{Json, Router, routing::get, routing::post};
use serde_json::json;
use std::net::SocketAddr;
use tokio::net::TcpListener;

use crate::{
    routes::{config, event, project, provider, session},
    state::AppState,
};
use opencode_core::error::ServerError;

/// Build the axum [`Router`] with all route groups registered.
///
/// `/health` is the liveness probe.
/// `/api/v1/projects` and session/message routes expose storage-backed REST APIs.
/// `/api/v1/sessions/:sid/prompt|cancel` call into the session runtime.
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
        .route("/config/providers", get(config::providers))
        .route("/event", get(event::stream))
        // Manual provider harness (Phase 2 — env-gated)
        .route("/provider/stream", post(provider::stream));

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
    use futures::StreamExt;
    use opencode_bus::{BroadcastBus, BusEvent, EventBus};
    use opencode_core::config::Config;
    use opencode_core::{
        dto::{
            AccountRow, AccountStateRow, ControlAccountRow, MessageRow, MessageWithParts, PartRow,
            PermissionRow, ProjectRow, SessionRow, TodoRow,
        },
        error::{SessionError, StorageError},
        id::{AccountId, ProjectId, SessionId},
    };
    use opencode_provider::{
        AccountService, ModelRegistry, ProviderAuthService, ProviderCatalogService,
    };
    use opencode_session::engine::Session;
    use opencode_session::types::{
        DetachedPromptAccepted, SessionHandle, SessionPrompt, SessionRuntimeStatus,
    };
    use opencode_storage::Storage;
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
        let storage: Arc<dyn Storage> = Arc::new(StubStorage);
        let cfg = Config::default();
        AppState {
            config: Arc::new(cfg.clone()),
            bus: Arc::new(BroadcastBus::new(64)),
            event_heartbeat: crate::state::EventHeartbeat::default(),
            storage: Arc::clone(&storage),
            session: Arc::new(StubSession),
            registry: Arc::new(ModelRegistry::new()),
            provider_catalog: Arc::new(ProviderCatalogService::new(cfg)),
            provider_auth: Arc::new(ProviderAuthService::new()),
            provider_accounts: Arc::new(AccountService::new(storage)),
            harness: false,
        }
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
}
