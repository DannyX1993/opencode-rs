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
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response, Sse, sse::Event},
};
use futures::StreamExt;
use opencode_provider::types::{ContentPart, ModelMessage, ModelRequest};
use serde::Deserialize;
use serde_json::json;
use tokio_stream::wrappers::ReceiverStream;

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
