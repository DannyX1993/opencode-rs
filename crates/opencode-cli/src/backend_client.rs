//! Backend client seam for command handlers.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use opencode_core::{
    dto::{ProjectRow, SessionDetachedPromptDto, SessionPromptRequestDto, SessionRow},
    id::ProjectId,
};
use opencode_provider::catalog::ProviderListDto;
use opencode_server::{AppState, build};
use opencode_session::types::SessionPrompt;
use std::borrow::Cow;
use tower::ServiceExt;

const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// Backend command client contract used by CLI command handlers.
#[async_trait]
pub trait BackendClient: Send + Sync {
    /// List projects used for cwd-aware session resolution.
    async fn list_projects(&self) -> Result<Vec<ProjectRow>>;

    /// List provider metadata from backend route contracts.
    async fn list_providers(&self) -> Result<ProviderListDto>;

    /// List sessions for a given project.
    async fn list_sessions(&self, project_id: ProjectId) -> Result<Vec<SessionRow>>;

    /// Create a session row for the given project.
    async fn create_session(&self, project_id: ProjectId, row: SessionRow) -> Result<()>;

    /// Submit a detached prompt request to the session runtime.
    async fn prompt_detached(&self, prompt: SessionPrompt) -> Result<SessionDetachedPromptDto>;
}

/// Local in-process backend adapter backed by the Axum router.
///
/// This adapter is used to keep CLI command behavior aligned with the same
/// HTTP route contracts consumed by external clients.
#[derive(Clone)]
pub struct LocalBackendClient {
    router: Router,
}

impl LocalBackendClient {
    /// Construct a local adapter from an existing [`AppState`].
    #[must_use]
    pub fn from_state(state: AppState) -> Self {
        Self {
            router: build(state),
        }
    }

    async fn request_json<T: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        uri: String,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        let has_body = body.is_some();
        let payload = match body {
            Some(value) => Body::from(serde_json::to_vec(&value)?),
            None => Body::empty(),
        };

        let mut builder = Request::builder().method(method).uri(uri);
        if has_body {
            builder = builder.header("content-type", "application/json");
        }

        let request = builder
            .body(payload)
            .map_err(|err| anyhow!("failed to build backend request: {err}"))?;
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .map_err(|err| anyhow!("backend route execution failed: {err}"))?;

        decode_json(response.status(), response.into_body()).await
    }

    async fn request_empty(
        &self,
        method: Method,
        uri: String,
        body: Option<serde_json::Value>,
    ) -> Result<()> {
        let has_body = body.is_some();
        let payload = match body {
            Some(value) => Body::from(serde_json::to_vec(&value)?),
            None => Body::empty(),
        };

        let mut builder = Request::builder().method(method).uri(uri);
        if has_body {
            builder = builder.header("content-type", "application/json");
        }

        let request = builder
            .body(payload)
            .map_err(|err| anyhow!("failed to build backend request: {err}"))?;
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .map_err(|err| anyhow!("backend route execution failed: {err}"))?;

        if response.status().is_success() {
            return Ok(());
        }

        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), MAX_RESPONSE_BYTES)
            .await
            .map_err(|err| anyhow!("failed to read backend response body: {err}"))?;
        let diagnostic = backend_error_diagnostic(&bytes);
        Err(anyhow!(
            "backend request failed (status {}): {}",
            status,
            diagnostic
        ))
    }
}

#[async_trait]
impl BackendClient for LocalBackendClient {
    async fn list_projects(&self) -> Result<Vec<ProjectRow>> {
        // Mirrors server project listing contract used for cwd resolution.
        self.request_json(Method::GET, "/api/v1/projects".to_string(), None)
            .await
    }

    async fn list_providers(&self) -> Result<ProviderListDto> {
        // Mirrors provider catalog route consumed by `providers list`.
        self.request_json(Method::GET, "/api/v1/provider".to_string(), None)
            .await
    }

    async fn list_sessions(&self, project_id: ProjectId) -> Result<Vec<SessionRow>> {
        self.request_json(
            Method::GET,
            format!("/api/v1/projects/{project_id}/sessions"),
            None,
        )
        .await
    }

    async fn create_session(&self, project_id: ProjectId, row: SessionRow) -> Result<()> {
        self.request_empty(
            Method::POST,
            format!("/api/v1/projects/{project_id}/sessions"),
            Some(serde_json::to_value(row)?),
        )
        .await
    }

    async fn prompt_detached(&self, prompt: SessionPrompt) -> Result<SessionDetachedPromptDto> {
        self.request_json(
            Method::POST,
            format!("/api/v1/sessions/{}/prompt", prompt.session_id),
            Some(serde_json::to_value(SessionPromptRequestDto {
                text: prompt.text,
                model: prompt.model,
                plan_mode: prompt.plan_mode,
                detached: true,
            })?),
        )
        .await
    }
}

async fn decode_json<T: serde::de::DeserializeOwned>(status: StatusCode, body: Body) -> Result<T> {
    let bytes = axum::body::to_bytes(body, MAX_RESPONSE_BYTES)
        .await
        .map_err(|err| anyhow!("failed to read backend response body: {err}"))?;

    if !status.is_success() {
        let diagnostic = backend_error_diagnostic(&bytes);
        return Err(anyhow!(
            "backend request failed (status {}): {}",
            status,
            diagnostic
        ));
    }

    let diagnostic = backend_error_diagnostic(&bytes);
    serde_json::from_slice::<T>(&bytes).map_err(|err| {
        anyhow!(
            "failed to decode backend response JSON: {err}; response body: {}",
            diagnostic
        )
    })
}

fn backend_error_diagnostic(bytes: &[u8]) -> Cow<'_, str> {
    if bytes.is_empty() {
        return Cow::Borrowed("<empty response body>");
    }

    String::from_utf8_lossy(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post;

    #[tokio::test]
    async fn request_empty_error_with_empty_body_uses_actionable_placeholder() {
        let router = Router::new().route(
            "/empty-error",
            post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        );
        let client = LocalBackendClient { router };

        let err = client
            .request_empty(Method::POST, "/empty-error".to_string(), None)
            .await
            .expect_err("non-success backend status should map to an error");

        assert!(
            err.to_string().contains("<empty response body>"),
            "expected diagnostic placeholder for empty backend error body"
        );
    }

    #[tokio::test]
    async fn request_empty_error_with_body_preserves_backend_diagnostic() {
        let router = Router::new().route(
            "/body-error",
            post(|| async { (StatusCode::CONFLICT, "session already exists for project") }),
        );
        let client = LocalBackendClient { router };

        let err = client
            .request_empty(
                Method::POST,
                "/body-error".to_string(),
                Some(serde_json::json!({"id": "abc"})),
            )
            .await
            .expect_err("non-success backend status should map to an error");

        let diagnostic = err.to_string();
        assert!(diagnostic.contains("status 409 Conflict"));
        assert!(diagnostic.contains("session already exists for project"));
    }

    #[tokio::test]
    async fn request_json_decode_failure_includes_response_preview() {
        let router = Router::new().route(
            "/bad-json",
            post(|| async { (StatusCode::OK, "{not-json}") }),
        );
        let client = LocalBackendClient { router };

        let err = client
            .request_json::<serde_json::Value>(Method::POST, "/bad-json".to_string(), None)
            .await
            .expect_err("invalid JSON should map to decode error");

        let message = err.to_string();
        assert!(message.contains("failed to decode backend response JSON"));
        assert!(
            message.contains("{not-json}"),
            "decode diagnostics should include backend payload preview"
        );
    }

    #[tokio::test]
    async fn request_json_type_mismatch_includes_response_preview() {
        let router = Router::new().route(
            "/type-mismatch",
            post(|| async { (StatusCode::OK, r#"{"ok":"yes"}"#) }),
        );
        let client = LocalBackendClient { router };

        let err = client
            .request_json::<bool>(Method::POST, "/type-mismatch".to_string(), None)
            .await
            .expect_err("type mismatch should map to decode error");

        let message = err.to_string();
        assert!(message.contains("failed to decode backend response JSON"));
        assert!(
            message.contains(r#"{"ok":"yes"}"#),
            "decode diagnostics should include typed-mismatch payload preview"
        );
    }
}
