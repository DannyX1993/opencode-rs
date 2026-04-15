//! `/api/v1/question` route handlers.

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use opencode_session::types::QuestionReply;
use serde::{Deserialize, Serialize};

use crate::{error::HttpError, state::AppState};

#[derive(Debug, Clone, Serialize)]
struct ReplyResultDto {
    ok: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuestionRejectDto {
    #[serde(rename = "requestID")]
    request_id: String,
}

/// `GET /api/v1/question` — list pending runtime questions.
pub(crate) async fn list(State(s): State<AppState>) -> impl IntoResponse {
    match s.question_runtime.list().await {
        Ok(rows) => (StatusCode::OK, Json(rows)).into_response(),
        Err(err) => HttpError::from(err).into_response(),
    }
}

/// `POST /api/v1/question/reply` — reply to a pending question request.
pub(crate) async fn reply(
    State(s): State<AppState>,
    Json(payload): Json<QuestionReply>,
) -> impl IntoResponse {
    match s.question_runtime.reply(payload).await {
        Ok(ok) => (StatusCode::ACCEPTED, Json(ReplyResultDto { ok })).into_response(),
        Err(err) => HttpError::from(err).into_response(),
    }
}

/// `POST /api/v1/question/reject` — reject a pending question request.
pub(crate) async fn reject(
    State(s): State<AppState>,
    Json(payload): Json<QuestionRejectDto>,
) -> impl IntoResponse {
    match s.question_runtime.reject(payload.request_id).await {
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
        http::{Request, StatusCode},
        routing::{get, post},
    };
    use opencode_bus::BroadcastBus;
    use opencode_core::{
        config::Config,
        dto::{ProjectRow, SessionRow},
        error::SessionError,
        id::{MessageId, ProjectId, SessionId},
    };
    use opencode_provider::{AccountService, ProviderAuthService, ProviderCatalogService};
    use opencode_session::{
        engine::Session,
        permission_runtime::InMemoryPermissionRuntime,
        question_runtime::{InMemoryQuestionRuntime, QuestionRuntime},
        types::{
            DetachedPromptAccepted, QuestionInfo, QuestionOption, QuestionRequest,
            RuntimeToolCallRef, SessionHandle, SessionPrompt, SessionRuntimeStatus,
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
            "opencode-server-question-route-{}-{counter}.db",
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
        Arc<dyn QuestionRuntime>,
        SessionId,
        tokio::task::JoinHandle<Result<Vec<Vec<String>>, SessionError>>,
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
        let permission_runtime = Arc::new(InMemoryPermissionRuntime::new(
            Arc::clone(&storage),
            Arc::clone(&bus),
        ));
        let question_runtime: Arc<dyn QuestionRuntime> =
            Arc::new(InMemoryQuestionRuntime::new(Arc::clone(&bus)));

        let ask = {
            let runtime = Arc::clone(&question_runtime);
            tokio::spawn(async move {
                runtime
                    .ask(QuestionRequest {
                        id: "question-route".into(),
                        session_id,
                        questions: vec![
                            QuestionInfo {
                                question: "Pick environment".into(),
                                header: "Environment".into(),
                                options: vec![QuestionOption {
                                    label: "prod".into(),
                                    description: "Production".into(),
                                }],
                                multiple: Some(false),
                                custom: Some(false),
                            },
                            QuestionInfo {
                                question: "Confirm deployment".into(),
                                header: "Confirmation".into(),
                                options: vec![QuestionOption {
                                    label: "yes".into(),
                                    description: "Proceed now".into(),
                                }],
                                multiple: Some(false),
                                custom: Some(false),
                            },
                        ],
                        tool: Some(RuntimeToolCallRef {
                            message_id: MessageId::new(),
                            call_id: "call-question-route".into(),
                        }),
                    })
                    .await
            })
        };

        for _ in 0..20 {
            if question_runtime.list().await.unwrap().len() == 1 {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }

        let cfg = Config::default();
        let state = AppState {
            config: Arc::new(cfg.clone()),
            bus,
            event_heartbeat: crate::state::EventHeartbeat::default(),
            storage: Arc::clone(&storage),
            session: Arc::new(StubSession),
            permission_runtime,
            question_runtime: Arc::clone(&question_runtime),
            registry: Arc::new(opencode_provider::ModelRegistry::new()),
            provider_catalog: Arc::new(ProviderCatalogService::new(cfg)),
            provider_auth: Arc::new(ProviderAuthService::new()),
            provider_accounts: Arc::new(AccountService::new(storage)),
            harness: false,
        };

        let app = Router::new()
            .route("/question", get(list))
            .route("/question/reply", post(reply))
            .route("/question/reject", post(reject))
            .with_state(state);

        (app, question_runtime, session_id, ask)
    }

    #[tokio::test]
    async fn list_returns_pending_question_requests() {
        let (app, _runtime, session_id, ask) = test_app().await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/question")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let rows: Vec<opencode_session::types::QuestionRequest> =
            serde_json::from_slice(&body).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "question-route");
        assert_eq!(rows[0].session_id, session_id);
        assert_eq!(rows[0].questions.len(), 2);
        assert_eq!(rows[0].questions[0].question, "Pick environment");
        assert_eq!(rows[0].questions[1].question, "Confirm deployment");
        assert_eq!(
            rows[0].tool.as_ref().map(|tool| tool.call_id.as_str()),
            Some("call-question-route")
        );

        ask.abort();
    }

    #[tokio::test]
    async fn reply_and_reject_routes_return_contract_ok_flags() {
        let (app, runtime, session_id, ask) = test_app().await;

        let reply_payload = serde_json::json!({
            "sessionID": session_id,
            "requestID": "question-route",
            "answers": [["prod"], ["yes"]]
        });
        let reply_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/question/reply")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&reply_payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reply_resp.status(), StatusCode::ACCEPTED);
        let reply_body = axum::body::to_bytes(reply_resp.into_body(), 4096)
            .await
            .unwrap();
        let reply_json: serde_json::Value = serde_json::from_slice(&reply_body).unwrap();
        assert_eq!(reply_json["ok"], true);
        assert_eq!(
            ask.await.unwrap().unwrap(),
            vec![vec![String::from("prod")], vec![String::from("yes")]]
        );

        let reject_payload = serde_json::json!({"requestID": "missing-question"});
        let reject_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/question/reject")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&reject_payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reject_resp.status(), StatusCode::ACCEPTED);
        let reject_body = axum::body::to_bytes(reject_resp.into_body(), 4096)
            .await
            .unwrap();
        let reject_json: serde_json::Value = serde_json::from_slice(&reject_body).unwrap();
        assert_eq!(reject_json["ok"], false);

        assert!(runtime.list().await.unwrap().is_empty());
    }
}
