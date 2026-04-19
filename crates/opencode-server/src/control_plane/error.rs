//! Control-plane specific errors and HTTP mapping.

use axum::{Json, http::StatusCode, response::IntoResponse};
use serde_json::json;

/// Error surface for selector resolution, routing, and proxy forwarding.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ControlPlaneError {
    /// Selector could not be parsed from query/header policy input.
    #[error("invalid workspace selector: {message}")]
    SelectorMalformed {
        /// Human-readable validation reason.
        message: String,
    },
    /// Selector parsed, but no workspace exists for that identifier.
    #[error("workspace selector '{selector}' not found")]
    WorkspaceNotFound {
        /// Raw selector value from request input.
        selector: String,
    },
    /// Workspace metadata exists but does not satisfy routing requirements.
    #[error("workspace metadata for selector '{selector}' is invalid: {message}")]
    WorkspaceMetadataInvalid {
        /// Raw selector value from request input.
        selector: String,
        /// Metadata validation detail.
        message: String,
    },
    /// Upstream forwarding timed out after bounded retries.
    #[error("upstream request timed out after {attempts} attempt(s)")]
    UpstreamTimeout {
        /// Total attempts including retries.
        attempts: u32,
    },
    /// Upstream transport failed for a non-timeout reason.
    #[error("upstream request failed: {message}")]
    UpstreamFailure {
        /// Transport failure detail from reqwest.
        message: String,
    },
    /// Deferred protocol parity: WebSocket forwarding intentionally unsupported.
    #[error("websocket forwarding is deferred in this control-plane slice")]
    WebSocketForwardingDeferred,
    /// Unexpected internal failure in routing/proxy plumbing.
    #[error("control-plane internal error: {message}")]
    Internal {
        /// Internal failure detail.
        message: String,
    },
}

impl ControlPlaneError {
    /// HTTP status associated with this error category.
    #[must_use]
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::SelectorMalformed { .. } => StatusCode::BAD_REQUEST,
            Self::WorkspaceNotFound { .. } => StatusCode::NOT_FOUND,
            Self::WorkspaceMetadataInvalid { .. } => StatusCode::BAD_REQUEST,
            Self::UpstreamTimeout { .. } => StatusCode::GATEWAY_TIMEOUT,
            Self::UpstreamFailure { .. } => StatusCode::BAD_GATEWAY,
            Self::WebSocketForwardingDeferred => StatusCode::NOT_IMPLEMENTED,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Stable machine-readable error code for clients and telemetry.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::SelectorMalformed { .. } => "selector_malformed",
            Self::WorkspaceNotFound { .. } => "workspace_not_found",
            Self::WorkspaceMetadataInvalid { .. } => "workspace_metadata_invalid",
            Self::UpstreamTimeout { .. } => "upstream_timeout",
            Self::UpstreamFailure { .. } => "upstream_failure",
            Self::WebSocketForwardingDeferred => "websocket_forwarding_deferred",
            Self::Internal { .. } => "control_plane_internal",
        }
    }
}

impl IntoResponse for ControlPlaneError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status_code();
        let mut payload = json!({
            "error": {
                "code": self.code(),
                "message": self.to_string(),
            }
        });

        if let Self::UpstreamTimeout { attempts } = self {
            payload["error"]["attempts"] = json!(attempts);
        }

        (status, Json(payload)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body, response::IntoResponse};
    use serde_json::Value;

    #[test]
    fn timeout_maps_to_gateway_timeout() {
        assert_eq!(
            ControlPlaneError::UpstreamTimeout { attempts: 3 }.status_code(),
            StatusCode::GATEWAY_TIMEOUT
        );
    }

    #[test]
    fn websocket_deferral_maps_to_not_implemented() {
        assert_eq!(
            ControlPlaneError::WebSocketForwardingDeferred.status_code(),
            StatusCode::NOT_IMPLEMENTED
        );
    }

    #[tokio::test]
    async fn timeout_response_uses_gateway_error_code_and_attempt_count() {
        let response = ControlPlaneError::UpstreamTimeout { attempts: 2 }.into_response();
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);

        let bytes = body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("timeout error body");
        let payload: Value = serde_json::from_slice(&bytes).expect("valid error json");

        assert_eq!(payload["error"]["code"], "upstream_timeout");
        assert_eq!(payload["error"]["attempts"], 2);
    }
}
