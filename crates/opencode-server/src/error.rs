//! Server-layer error types and `IntoResponse` mapping.

use axum::{Json, http::StatusCode, response::IntoResponse};
use opencode_core::error::StorageError;
use serde_json::json;

/// HTTP error response wrapper.
#[derive(Debug)]
pub struct HttpError {
    status: StatusCode,
    msg: String,
}

impl HttpError {
    /// 404 Not Found.
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            msg: msg.into(),
        }
    }

    /// 400 Bad Request.
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            msg: msg.into(),
        }
    }

    /// 500 Internal Server Error.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            msg: msg.into(),
        }
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(json!({ "error": self.msg }))).into_response()
    }
}

/// Map storage-layer errors to HTTP errors.
///
/// - [`StorageError::NotFound`] → 404 with entity/id in the message
/// - [`StorageError::Db`] / [`StorageError::Serde`] → 500
impl From<StorageError> for HttpError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::NotFound { entity, id } => {
                Self::not_found(format!("not found: {entity} {id}"))
            }
            StorageError::Db(msg) => Self::internal(msg),
            StorageError::Serde(msg) => Self::internal(msg),
            // `#[non_exhaustive]` guard — any future variants become 500
            _ => Self::internal(err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use opencode_core::error::StorageError;

    #[test]
    fn not_found_status() {
        let e = HttpError::not_found("missing resource");
        assert_eq!(e.status, StatusCode::NOT_FOUND);
        assert!(e.msg.contains("missing resource"));
    }

    #[test]
    fn bad_request_status() {
        let e = HttpError::bad_request("invalid input");
        assert_eq!(e.status, StatusCode::BAD_REQUEST);
        assert!(e.msg.contains("invalid input"));
    }

    #[test]
    fn internal_status() {
        let e = HttpError::internal("db error");
        assert_eq!(e.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(e.msg.contains("db error"));
    }

    #[test]
    fn debug_impl() {
        let e = HttpError::not_found("x");
        assert!(format!("{e:?}").contains("HttpError"));
    }

    // ── Task 4.1: StorageError::NotFound → 404 ───────────────────────────────

    #[test]
    fn storage_not_found_maps_to_404() {
        let storage_err = StorageError::NotFound {
            entity: "project",
            id: "abc-123".into(),
        };
        let http_err = HttpError::from(storage_err);
        assert_eq!(http_err.status, StatusCode::NOT_FOUND);
        assert!(http_err.msg.contains("project"));
    }

    // ── Task 4.2: StorageError::Db → 500 ─────────────────────────────────────

    #[test]
    fn storage_db_error_maps_to_500() {
        let storage_err = StorageError::Db("connection refused".into());
        let http_err = HttpError::from(storage_err);
        assert_eq!(http_err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(http_err.msg.contains("connection refused"));
    }

    // ── Task 4.3: StorageError::Serde → 500 ──────────────────────────────────

    #[test]
    fn storage_serde_error_maps_to_500() {
        let storage_err = StorageError::Serde("invalid json field".into());
        let http_err = HttpError::from(storage_err);
        assert_eq!(http_err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(http_err.msg.contains("invalid json field"));
    }

    // ── Triangulation: verify msg content for NotFound ───────────────────────

    #[test]
    fn storage_not_found_msg_includes_id() {
        let storage_err = StorageError::NotFound {
            entity: "session",
            id: "sess-xyz-999".into(),
        };
        let http_err = HttpError::from(storage_err);
        assert_eq!(http_err.status, StatusCode::NOT_FOUND);
        assert!(http_err.msg.contains("sess-xyz-999"));
    }
}
