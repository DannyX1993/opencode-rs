//! `/api/v1/permission` route handlers.

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use opencode_session::types::PermissionReply;
use serde::Serialize;

use crate::{error::HttpError, state::AppState};

#[derive(Debug, Clone, Serialize)]
struct ReplyResultDto {
    ok: bool,
}

/// `GET /api/v1/permission` — list pending permission requests.
pub(crate) async fn list(State(s): State<AppState>) -> impl IntoResponse {
    match s.permission_runtime.list().await {
        Ok(rows) => (StatusCode::OK, Json(rows)).into_response(),
        Err(err) => HttpError::from(err).into_response(),
    }
}

/// `POST /api/v1/permission/reply` — reply to a pending permission request.
pub(crate) async fn reply(
    State(s): State<AppState>,
    Json(payload): Json<PermissionReply>,
) -> impl IntoResponse {
    match s.permission_runtime.reply(payload).await {
        Ok(ok) => (StatusCode::ACCEPTED, Json(ReplyResultDto { ok })).into_response(),
        Err(err) => HttpError::from(err).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{
        Router,
        body::Body,
        http::Request,
        routing::{get, post},
    };
    use opencode_bus::BroadcastBus;
    use opencode_core::{
        config::Config,
        dto::{ProjectRow, SessionRow},
        error::SessionError,
        id::{MessageId, ProjectId, SessionId},
    };
    use opencode_provider::{AccountService, ProviderAuthService};
    use opencode_session::{
        engine::Session,
        permission_runtime::{InMemoryPermissionRuntime, PermissionRuntime},
        question_runtime::InMemoryQuestionRuntime,
        types::{
            DetachedPromptAccepted, PermissionReplyKind, PermissionRequest, RuntimeToolCallRef,
            SessionHandle, SessionPrompt, SessionRuntimeStatus,
        },
    };
    use opencode_storage::{Storage, StorageImpl, connect};
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    use tokio::time::{Duration, sleep};
    use tower::ServiceExt;

    struct StubSession;

    #[async_trait]
    impl Session for StubSession {
        async fn prompt(&self, _: SessionPrompt) -> Result<SessionHandle, SessionError> {
            Err(SessionError::NotFound("stub".into()))
        }
        async fn prompt_detached(
            &self,
            _: SessionPrompt,
        ) -> Result<DetachedPromptAccepted, SessionError> {
            Err(SessionError::NotFound("stub".into()))
        }
        async fn cancel(&self, _: SessionId) -> Result<(), SessionError> {
            Err(SessionError::NoActiveRun("stub".into()))
        }
        async fn status(&self, _: SessionId) -> Result<SessionRuntimeStatus, SessionError> {
            Ok(SessionRuntimeStatus::Idle)
        }
        async fn list_statuses(
            &self,
        ) -> Result<std::collections::HashMap<SessionId, SessionRuntimeStatus>, SessionError>
        {
            Ok(std::collections::HashMap::new())
        }
    }

    fn project_row(id: ProjectId) -> ProjectRow {
        ProjectRow {
            id,
            worktree: "/tmp".into(),
            vcs: None,
            name: None,
            icon_url: None,
            icon_color: None,
            time_created: 0,
            time_updated: 0,
            time_initialized: None,
            sandboxes: serde_json::json!([]),
            commands: None,
        }
    }

    fn session_row(session_id: SessionId, project_id: ProjectId) -> SessionRow {
        SessionRow {
            id: session_id,
            project_id,
            workspace_id: None,
            parent_id: None,
            slug: "s".into(),
            directory: "/tmp".into(),
            title: "t".into(),
            version: "1".into(),
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
        }
    }

    static DB_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn make_storage() -> Arc<dyn Storage> {
        let counter = DB_COUNTER.fetch_add(1, Ordering::Relaxed);
        let db_path = std::env::temp_dir().join(format!(
            "opencode-server-permission-route-{}-{counter}.db",
            now_millis()
        ));
        let pool = connect(&db_path).await.unwrap();
        let storage: Arc<dyn Storage> = Arc::new(StorageImpl::new(pool));
        storage
    }

    fn now_millis() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64)
    }

    async fn test_app() -> (
        Router,
        Arc<dyn PermissionRuntime>,
        SessionId,
        tokio::task::JoinHandle<Result<(), SessionError>>,
    ) {
        let storage = make_storage().await;
        let project_id = ProjectId::new();
        let session_id = SessionId::new();
        storage
            .upsert_project(project_row(project_id))
            .await
            .unwrap();
        storage
            .create_session(session_row(session_id, project_id))
            .await
            .unwrap();

        let bus = Arc::new(BroadcastBus::new(64));
        let permission_runtime: Arc<dyn PermissionRuntime> = Arc::new(
            InMemoryPermissionRuntime::new(Arc::clone(&storage), Arc::clone(&bus)),
        );
        let question_runtime = Arc::new(InMemoryQuestionRuntime::new(Arc::clone(&bus)));

        let ask = {
            let runtime = Arc::clone(&permission_runtime);
            tokio::spawn(async move {
                runtime
                    .ask(PermissionRequest {
                        id: "perm-route".into(),
                        session_id,
                        permission: "bash".into(),
                        patterns: vec!["/tmp/**".into()],
                        metadata: serde_json::json!({"source": "route-test"}),
                        always: vec!["/tmp/**".into()],
                        tool: Some(RuntimeToolCallRef {
                            message_id: MessageId::new(),
                            call_id: "call-perm-route".into(),
                        }),
                    })
                    .await
            })
        };

        for _ in 0..20 {
            if permission_runtime.list().await.unwrap().len() == 1 {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }

        let cfg = Config::default();
        let state = AppState {
            config_service: Arc::new(
                opencode_core::config_service::ConfigService::with_cached_resolved(
                    std::env::temp_dir(),
                    None,
                    cfg,
                ),
            ),
            bus,
            event_heartbeat: crate::state::EventHeartbeat::default(),
            storage: Arc::clone(&storage),
            session: Arc::new(StubSession),
            permission_runtime: Arc::clone(&permission_runtime),
            question_runtime,
            registry: Arc::new(opencode_provider::ModelRegistry::new()),
            provider_catalog_models: Arc::new(Vec::new()),
            provider_auth: Arc::new(ProviderAuthService::new()),
            provider_accounts: Arc::new(AccountService::new(storage)),
            control_plane: crate::state::ControlPlaneConfig::default(),
            control_plane_proxy: Arc::new(crate::control_plane::proxy::HttpProxyService::new(
                reqwest::Client::new(),
                crate::state::ProxyPolicy::default(),
            )),
            harness: false,
        };

        let app = Router::new()
            .route("/permission", get(list))
            .route("/permission/reply", post(reply))
            .with_state(state);

        (app, permission_runtime, session_id, ask)
    }

    #[tokio::test]
    async fn list_returns_pending_permission_requests() {
        let (app, _runtime, session_id, ask) = test_app().await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/permission")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let rows: Vec<opencode_session::types::PermissionRequest> =
            serde_json::from_slice(&body).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "perm-route");
        assert_eq!(rows[0].session_id, session_id);
        assert_eq!(
            rows[0].metadata,
            serde_json::json!({"source": "route-test"})
        );
        assert_eq!(rows[0].always, vec![String::from("/tmp/**")]);
        assert_eq!(
            rows[0].tool.as_ref().map(|tool| tool.call_id.as_str()),
            Some("call-perm-route")
        );

        ask.abort();
    }

    #[tokio::test]
    async fn reply_returns_ok_true_for_known_pending_permission() {
        let (app, _runtime, session_id, ask) = test_app().await;

        let payload = serde_json::json!({
            "sessionID": session_id,
            "requestID": "perm-route",
            "reply": "once"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/permission/reply")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ok"], true);
        assert!(ask.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn reply_returns_ok_false_for_unknown_permission_request() {
        let (app, runtime, session_id, ask) = test_app().await;

        let payload = serde_json::json!({
            "sessionID": session_id,
            "requestID": "missing-request",
            "reply": "reject"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/permission/reply")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ok"], false);

        runtime
            .reply(PermissionReply {
                session_id,
                request_id: "perm-route".into(),
                reply: PermissionReplyKind::Reject,
            })
            .await
            .unwrap();
        assert!(ask.await.unwrap().is_err());
    }
}
