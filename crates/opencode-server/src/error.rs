//! Server-layer error types and `IntoResponse` mapping.

use axum::{Json, http::StatusCode, response::IntoResponse};
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

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
}
