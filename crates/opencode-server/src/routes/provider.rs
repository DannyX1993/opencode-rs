//! `POST /api/v1/provider/stream` — manual validation harness.
//!
//! Only active when the `OPENCODE_MANUAL_HARNESS` environment variable is set
//! to `"1"`.  All other requests receive **403 Forbidden**.
//!
//! This endpoint is intentionally NOT part of the public API surface.
//! Its purpose is to let operators exercise real provider round-trips with
//! `curl` or Postman without requiring the full CLI stack to be running.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response, Sse, sse::Event},
};
use futures::StreamExt;
use opencode_core::id::AccountId;
use opencode_provider::types::{ContentPart, ModelMessage, ModelRequest};
use opencode_provider::{AuthorizeInput, CallbackInput, PersistAccountInput, ProviderError};
use serde::Deserialize;
use serde_json::json;
use tokio_stream::wrappers::ReceiverStream;

use crate::error::HttpError;
use crate::state::AppState;

// ── Request body ─────────────────────────────────────────────────────────────

/// Request body for the manual stream endpoint.
#[derive(Deserialize)]
pub struct StreamBody {
    /// Provider id (e.g. `"anthropic"`, `"openai"`).
    pub provider: String,
    /// Model id (e.g. `"claude-3-5-sonnet-20241022"`, `"gpt-4o"`).
    pub model: String,
    /// The user prompt text.
    pub prompt: String,
    /// Optional max tokens cap.
    pub max_tokens: Option<u32>,
}

/// Request body for selecting the active provider account/org.
#[derive(Debug, Deserialize)]
pub struct UseAccountBody {
    /// Persisted account id to activate.
    pub account_id: AccountId,
    /// Optional persisted org id for that account.
    #[serde(default)]
    pub active_org_id: Option<String>,
}

/// Request body for OAuth callback completion.
#[derive(Debug, Deserialize)]
pub struct OAuthCallbackBody {
    /// Selected auth-method index.
    pub method: usize,
    /// OAuth authorization code, when required.
    #[serde(default)]
    pub code: Option<String>,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `POST /api/v1/provider/stream`
///
/// Streams `ModelEvent` values as Server-Sent Events, one JSON object per
/// event.  Returns 403 unless `OPENCODE_MANUAL_HARNESS=1`.
pub async fn stream(State(state): State<AppState>, Json(body): Json<StreamBody>) -> Response {
    if !state.harness {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "harness disabled" })),
        )
            .into_response();
    }

    let provider = match state.registry.get(&body.provider).await {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("unknown provider: {}", body.provider) })),
            )
                .into_response();
        }
    };

    let req = ModelRequest {
        model: body.model,
        system: vec![],
        messages: vec![ModelMessage {
            role: "user".into(),
            content: vec![ContentPart::Text { text: body.prompt }],
        }],
        tools: Default::default(),
        max_tokens: body.max_tokens.or(Some(1024)),
        temperature: None,
    };

    let model_stream = match provider.stream(req).await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    // Convert BoxStream<Result<ModelEvent, ProviderError>> → SSE stream.
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, std::convert::Infallible>>(64);

    tokio::spawn(async move {
        let mut s = model_stream;
        while let Some(item) = s.next().await {
            let ev = match item {
                Ok(ev) => {
                    let data = match serde_json::to_string(&ev) {
                        Ok(d) => d,
                        Err(_) => continue,
                    };
                    Event::default().data(data)
                }
                Err(e) => Event::default()
                    .event("error")
                    .data(json!({ "error": e.to_string() }).to_string()),
            };
            if tx.send(Ok(ev)).await.is_err() {
                break;
            }
        }
    });

    Sse::new(ReceiverStream::new(rx)).into_response()
}

/// `GET /api/v1/provider` — list visible providers with defaults/connectivity.
pub async fn list(State(state): State<AppState>) -> impl IntoResponse {
    match state.provider_catalog.list() {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(err) => map_provider_error(err).into_response(),
    }
}

/// `GET /api/v1/provider/auth` — list supported provider auth methods.
pub async fn auth_methods(State(state): State<AppState>) -> impl IntoResponse {
    (StatusCode::OK, Json(state.provider_auth.methods())).into_response()
}

/// `POST /api/v1/provider/:provider/oauth/authorize` — start provider auth.
pub async fn oauth_authorize(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(input): Json<AuthorizeInput>,
) -> impl IntoResponse {
    match state.provider_auth.authorize(&provider, input) {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(err) => map_provider_error(err).into_response(),
    }
}

/// `POST /api/v1/provider/:provider/oauth/callback` — complete provider auth.
pub async fn oauth_callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(input): Json<OAuthCallbackBody>,
) -> impl IntoResponse {
    let callback = CallbackInput {
        method: input.method,
        code: input.code,
    };

    if let Err(err) = state.provider_auth.callback(&provider, callback) {
        return map_provider_error(err).into_response();
    }

    let id = AccountId::new();
    let now = chrono_like_now();
    let persist = PersistAccountInput {
        id,
        email: format!("{provider}@opencode.dev"),
        url: format!("https://{provider}.opencode.dev"),
        access_token: format!("{provider}-access-token"),
        refresh_token: format!("{provider}-refresh-token"),
        token_expiry: Some(now + 3_600_000),
        active_org_id: None,
        time_created: now,
        time_updated: now,
    };

    match state.provider_accounts.persist(persist).await {
        Ok(()) => (StatusCode::OK, Json(true)).into_response(),
        Err(err) => map_provider_error(err).into_response(),
    }
}

/// `GET /api/v1/provider/account` — list persisted provider account state.
pub async fn account_state(State(state): State<AppState>) -> impl IntoResponse {
    match state.provider_accounts.state().await {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(err) => map_provider_error(err).into_response(),
    }
}

/// `POST /api/v1/provider/account/use` — switch active provider account/org.
pub async fn use_account(
    State(state): State<AppState>,
    Json(body): Json<UseAccountBody>,
) -> impl IntoResponse {
    match state
        .provider_accounts
        .set_active(body.account_id, body.active_org_id)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(true)).into_response(),
        Err(err) => map_provider_error(err).into_response(),
    }
}

/// `DELETE /api/v1/provider/account/:account_id` — remove a persisted account.
pub async fn remove_account(
    State(state): State<AppState>,
    Path(account_id): Path<AccountId>,
) -> impl IntoResponse {
    match state.provider_accounts.remove(account_id).await {
        Ok(()) => (StatusCode::OK, Json(true)).into_response(),
        Err(err) => map_provider_error(err).into_response(),
    }
}

fn chrono_like_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn map_provider_error(err: ProviderError) -> HttpError {
    match err {
        ProviderError::Auth { msg, .. } => HttpError::bad_request(msg),
        ProviderError::ContextLength { model } => {
            HttpError::bad_request(format!("context length exceeded for model {model}"))
        }
        ProviderError::RateLimit {
            provider,
            retry_after,
        } => HttpError::conflict(format!(
            "rate limited by {provider}{}",
            retry_after
                .map(|secs| format!(" (retry after {secs}s)"))
                .unwrap_or_default()
        )),
        ProviderError::Http(_, msg) | ProviderError::Stream(msg) => HttpError::internal(msg),
        ProviderError::CircuitOpen { provider, model } => {
            HttpError::conflict(format!("circuit breaker open for {provider}/{model}"))
        }
        _ => HttpError::internal(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{
        Router,
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use opencode_bus::BroadcastBus;
    use opencode_core::{
        config::Config,
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
    use opencode_session::{
        engine::Session,
        types::{SessionHandle, SessionPrompt},
    };
    use opencode_storage::Storage;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    #[derive(Default)]
    struct StubStorage {
        accounts: Mutex<Vec<AccountRow>>,
        state: Mutex<Option<AccountStateRow>>,
    }

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
        async fn upsert_account(&self, row: AccountRow) -> Result<(), StorageError> {
            let mut accounts = self.accounts.lock().unwrap();
            accounts.retain(|item| item.id != row.id);
            accounts.push(row);
            Ok(())
        }
        async fn list_accounts(&self) -> Result<Vec<AccountRow>, StorageError> {
            Ok(self.accounts.lock().unwrap().clone())
        }
        async fn get_account(&self, id: AccountId) -> Result<Option<AccountRow>, StorageError> {
            Ok(self
                .accounts
                .lock()
                .unwrap()
                .iter()
                .find(|item| item.id == id)
                .cloned())
        }
        async fn remove_account(&self, id: AccountId) -> Result<(), StorageError> {
            self.accounts.lock().unwrap().retain(|item| item.id != id);
            let mut state = self.state.lock().unwrap();
            if state.as_ref().and_then(|item| item.active_account_id) == Some(id) {
                *state = Some(AccountStateRow {
                    id: 1,
                    active_account_id: None,
                    active_org_id: None,
                });
            }
            Ok(())
        }
        async fn update_account_tokens(
            &self,
            id: AccountId,
            access_token: String,
            refresh_token: String,
            token_expiry: Option<i64>,
            time_updated: i64,
        ) -> Result<(), StorageError> {
            let mut accounts = self.accounts.lock().unwrap();
            let Some(row) = accounts.iter_mut().find(|item| item.id == id) else {
                return Err(StorageError::NotFound {
                    entity: "account",
                    id: id.to_string(),
                });
            };
            row.access_token = access_token;
            row.refresh_token = refresh_token;
            row.token_expiry = token_expiry;
            row.time_updated = time_updated;
            Ok(())
        }
        async fn get_account_state(&self) -> Result<Option<AccountStateRow>, StorageError> {
            Ok(self.state.lock().unwrap().clone())
        }
        async fn set_account_state(&self, row: AccountStateRow) -> Result<(), StorageError> {
            *self.state.lock().unwrap() = Some(row);
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
    }

    fn app() -> Router {
        let storage: Arc<dyn Storage> = Arc::new(StubStorage::default());
        let mut cfg = Config::default();
        cfg.providers.openai = Some("sk-openai".into());
        let state = AppState {
            config: Arc::new(cfg.clone()),
            bus: Arc::new(BroadcastBus::new(64)),
            storage: Arc::clone(&storage),
            session: Arc::new(StubSession),
            registry: Arc::new(ModelRegistry::new()),
            provider_catalog: Arc::new(ProviderCatalogService::new(cfg)),
            provider_auth: Arc::new(ProviderAuthService::new()),
            provider_accounts: Arc::new(AccountService::new(storage)),
            harness: false,
        };
        crate::router::build(state)
    }

    #[tokio::test]
    async fn provider_routes_list_returns_catalog_and_connected_state() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/provider")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(value["default"]["openai"].is_string());
        assert_eq!(value["connected"], serde_json::json!(["openai"]));
    }

    #[tokio::test]
    async fn provider_routes_auth_returns_supported_methods() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/provider/auth")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["openai"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn provider_routes_unsupported_authorize_does_not_write_account_state() {
        let app = app();

        let before_state = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/provider/account")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(before_state.status(), StatusCode::OK);
        let before_body = axum::body::to_bytes(before_state.into_body(), 8192)
            .await
            .unwrap();
        let before_json: serde_json::Value = serde_json::from_slice(&before_body).unwrap();
        assert!(before_json["accounts"].as_array().unwrap().is_empty());
        assert!(before_json["active"].is_null());

        let unsupported = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/provider/unsupported/oauth/authorize")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({ "method": 0 })).unwrap(),
            ))
            .unwrap();
        let unsupported_resp = app.clone().oneshot(unsupported).await.unwrap();
        assert_eq!(unsupported_resp.status(), StatusCode::OK);
        let unsupported_body = axum::body::to_bytes(unsupported_resp.into_body(), 8192)
            .await
            .unwrap();
        let unsupported_json: serde_json::Value =
            serde_json::from_slice(&unsupported_body).unwrap();
        assert!(unsupported_json.is_null());

        let after_state = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/provider/account")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(after_state.status(), StatusCode::OK);
        let after_body = axum::body::to_bytes(after_state.into_body(), 8192)
            .await
            .unwrap();
        let after_json: serde_json::Value = serde_json::from_slice(&after_body).unwrap();
        assert_eq!(after_json, before_json);
    }

    #[tokio::test]
    async fn provider_routes_authorize_returns_oauth_handoff() {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/provider/openai/oauth/authorize")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({ "method": 1 })).unwrap(),
            ))
            .unwrap();

        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["method"], "code");
        assert!(value["url"].as_str().unwrap().contains("openai"));
    }

    #[tokio::test]
    async fn provider_routes_callback_persists_account_state() {
        let app = app();

        let authorize = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/provider/openai/oauth/authorize")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({ "method": 1 })).unwrap(),
            ))
            .unwrap();
        let authorize_resp = app.clone().oneshot(authorize).await.unwrap();
        assert_eq!(authorize_resp.status(), StatusCode::OK);

        let callback = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/provider/openai/oauth/callback")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "method": 1,
                    "code": "oauth-code"
                }))
                .unwrap(),
            ))
            .unwrap();
        let callback_resp = app.clone().oneshot(callback).await.unwrap();
        assert_eq!(callback_resp.status(), StatusCode::OK);

        let state = Request::builder()
            .uri("/api/v1/provider/account")
            .body(Body::empty())
            .unwrap();
        let state_resp = app.oneshot(state).await.unwrap();
        assert_eq!(state_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(state_resp.into_body(), 8192)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["accounts"].as_array().unwrap().len(), 1);
        assert!(value["active"]["account_id"].is_string());
    }

    #[tokio::test]
    async fn provider_routes_callback_missing_pending_flow_returns_bad_request() {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/provider/openai/oauth/callback")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "method": 1,
                    "code": "oauth-code"
                }))
                .unwrap(),
            ))
            .unwrap();

        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            value["error"]
                .as_str()
                .unwrap()
                .contains("no pending oauth flow")
        );
    }

    #[tokio::test]
    async fn provider_routes_config_providers_returns_connected_defaults_only() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/config/providers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["providers"].as_array().unwrap().len(), 1);
        assert_eq!(value["providers"][0]["id"], "openai");
        assert_eq!(value["default"]["openai"], "gpt-5");
    }

    #[tokio::test]
    async fn provider_routes_use_and_remove_account_updates_active_state() {
        let app = app();

        let authorize = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/provider/openai/oauth/authorize")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({ "method": 1 })).unwrap(),
            ))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(authorize).await.unwrap().status(),
            StatusCode::OK
        );

        let callback = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/provider/openai/oauth/callback")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "method": 1,
                    "code": "oauth-code"
                }))
                .unwrap(),
            ))
            .unwrap();
        let callback_resp = app.clone().oneshot(callback).await.unwrap();
        assert_eq!(callback_resp.status(), StatusCode::OK);
        let callback_body = axum::body::to_bytes(callback_resp.into_body(), 8192)
            .await
            .unwrap();
        let callback_json: serde_json::Value = serde_json::from_slice(&callback_body).unwrap();
        assert_eq!(callback_json, serde_json::json!(true));

        let state_req = Request::builder()
            .uri("/api/v1/provider/account")
            .body(Body::empty())
            .unwrap();
        let state_resp = app.clone().oneshot(state_req).await.unwrap();
        let state_body = axum::body::to_bytes(state_resp.into_body(), 8192)
            .await
            .unwrap();
        let state_json: serde_json::Value = serde_json::from_slice(&state_body).unwrap();
        let id = state_json["accounts"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let use_req = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/provider/account/use")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "account_id": id,
                    "active_org_id": null
                }))
                .unwrap(),
            ))
            .unwrap();
        let use_resp = app.clone().oneshot(use_req).await.unwrap();
        assert_eq!(use_resp.status(), StatusCode::OK);
        let use_body = axum::body::to_bytes(use_resp.into_body(), 8192)
            .await
            .unwrap();
        let use_json: serde_json::Value = serde_json::from_slice(&use_body).unwrap();
        assert_eq!(use_json, serde_json::json!(true));

        let delete_req = Request::builder()
            .method(Method::DELETE)
            .uri(format!("/api/v1/provider/account/{id}"))
            .body(Body::empty())
            .unwrap();
        let delete_resp = app.clone().oneshot(delete_req).await.unwrap();
        assert_eq!(delete_resp.status(), StatusCode::OK);
        let delete_body = axum::body::to_bytes(delete_resp.into_body(), 8192)
            .await
            .unwrap();
        let delete_json: serde_json::Value = serde_json::from_slice(&delete_body).unwrap();
        assert_eq!(delete_json, serde_json::json!(true));

        let final_state_req = Request::builder()
            .uri("/api/v1/provider/account")
            .body(Body::empty())
            .unwrap();
        let final_state_resp = app.oneshot(final_state_req).await.unwrap();
        let final_state_body = axum::body::to_bytes(final_state_resp.into_body(), 8192)
            .await
            .unwrap();
        let final_state_json: serde_json::Value =
            serde_json::from_slice(&final_state_body).unwrap();
        assert!(final_state_json["accounts"].as_array().unwrap().is_empty());
        assert!(final_state_json["active"].is_null());
    }
}
