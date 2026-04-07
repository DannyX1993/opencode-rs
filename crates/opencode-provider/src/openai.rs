//! OpenAI provider — streams from `/v1/responses` (native OpenAI) or
//! `/v1/chat/completions` (OpenAI-compatible endpoints like Groq, Ollama).
//!
//! Branch logic: if `base_url` is the native OpenAI endpoint
//! (`api.openai.com`), the Responses API path is used. All other base URLs
//! fall back to the chat-completions path for broad compatibility.

use crate::auth::{AuthResolver, EnvAuthResolver};
use crate::error::ProviderError;
use crate::sse::SseDecoder;
use crate::types::{LanguageModel, ModelEvent, ModelInfo, ModelMessage, ModelRequest};
use async_openai::types::chat::{
    ChatCompletionRequestAssistantMessage, ChatCompletionRequestAssistantMessageContent,
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestSystemMessageContent, ChatCompletionRequestUserMessage,
    ChatCompletionRequestUserMessageContent, ChatCompletionStreamOptions,
    CreateChatCompletionRequest,
};
use async_openai::types::responses::{
    CreateResponse, EasyInputContent, EasyInputMessage, InputItem, InputParam, ResponseStreamEvent,
    Role,
};
use async_trait::async_trait;
use futures::StreamExt;
use opencode_core::context::BoxStream;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;

const DEFAULT_BASE: &str = "https://api.openai.com";

// ── Chat-completions SSE response event shapes ────────────────────────────────
// These are used for parsing SSE *response* frames, not for building requests.

#[derive(Deserialize, Debug)]
struct OpenAiChunk {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize, Debug)]
struct OpenAiChoice {
    delta: OpenAiDelta,
}

#[derive(Deserialize, Debug, Default)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize, Debug)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

// ── Helper: convert ModelMessage → ChatCompletionRequestMessage ───────────────

fn to_chat_message(msg: &ModelMessage) -> ChatCompletionRequestMessage {
    use crate::types::ContentPart;
    // Collapse all text parts to a single string (simplified; multi-modal
    // image support is a future enhancement for the chat-completions path).
    let text: String = msg
        .content
        .iter()
        .filter_map(|p| {
            if let ContentPart::Text { text } = p {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();

    match msg.role.as_str() {
        "system" => ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
            content: ChatCompletionRequestSystemMessageContent::Text(text),
            name: None,
        }),
        "assistant" => {
            ChatCompletionRequestMessage::Assistant(ChatCompletionRequestAssistantMessage {
                content: Some(ChatCompletionRequestAssistantMessageContent::Text(text)),
                ..Default::default()
            })
        }
        _ => ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
            content: ChatCompletionRequestUserMessageContent::Text(text),
            name: None,
        }),
    }
}

// ── Helper: build Responses API input from ModelRequest ───────────────────────

fn build_responses_input(req: &ModelRequest) -> (InputParam, Option<String>) {
    use crate::types::ContentPart;
    // System messages become the `instructions` field (single string).
    let instructions: Option<String> = {
        let s: String = req
            .system
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|p| {
                if let ContentPart::Text { text } = p {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if s.is_empty() { None } else { Some(s) }
    };

    let items: Vec<InputItem> = req
        .messages
        .iter()
        .map(|msg| {
            let text: String = msg
                .content
                .iter()
                .filter_map(|p| {
                    if let ContentPart::Text { text } = p {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            let role = match msg.role.as_str() {
                "assistant" => Role::Assistant,
                "system" | "developer" => Role::Developer,
                _ => Role::User,
            };
            InputItem::EasyMessage(EasyInputMessage {
                role,
                content: EasyInputContent::Text(text),
                ..Default::default()
            })
        })
        .collect();

    (InputParam::Items(items), instructions)
}

// ── Map a single chat-completions chunk to ModelEvent(s) ─────────────────────

/// Map a parsed OpenAI chat-completions SSE chunk to 0..N `ModelEvent`s.
///
/// Pure function — easy to unit-test without network I/O.
fn map_chunk(chunk: &OpenAiChunk) -> Vec<ModelEvent> {
    let mut events = vec![];

    for choice in &chunk.choices {
        if let Some(text) = &choice.delta.content {
            if !text.is_empty() {
                events.push(ModelEvent::TextDelta {
                    delta: text.clone(),
                });
            }
        }
    }

    if let Some(usage) = &chunk.usage {
        events.push(ModelEvent::Usage {
            input: usage.prompt_tokens,
            output: usage.completion_tokens,
            cache_read: 0,
            cache_write: 0,
        });
    }

    events
}

// ── Map a Responses-API SSE event to ModelEvent(s) ───────────────────────────

/// Outcome of mapping a single Responses-API SSE frame.
///
/// - `Ok(events)` — zero or more [`ModelEvent`]s to forward downstream.
/// - `Err(e)` — the event is a terminal failure; surface as a stream error.
pub enum ResponsesEventOutcome {
    /// Zero or more model events to forward downstream.
    Events(Vec<ModelEvent>),
    /// A terminal failure; surface as a stream error.
    Error(ProviderError),
}

/// Deserialize a Responses-API SSE frame (event type + JSON data) into a
/// typed [`ResponseStreamEvent`] using the SDK enum, then map to a
/// [`ResponsesEventOutcome`].
///
/// The SSE `event:` field becomes the `type` discriminator that the SDK's
/// `#[serde(tag = "type")]` enum expects.
///
/// Pure function — easy to unit-test without network I/O.
pub fn map_responses_event(event_type: &str, data: &str) -> ResponsesEventOutcome {
    // Inject `"type"` into the JSON object so the tagged enum can deserialize.
    let Ok(mut val) = serde_json::from_str::<serde_json::Value>(data) else {
        return ResponsesEventOutcome::Events(vec![]);
    };
    if let Some(obj) = val.as_object_mut() {
        obj.insert("type".into(), serde_json::Value::String(event_type.into()));
    } else {
        return ResponsesEventOutcome::Events(vec![]);
    }
    let Ok(ev) = serde_json::from_value::<ResponseStreamEvent>(val) else {
        return ResponsesEventOutcome::Events(vec![]);
    };
    match ev {
        ResponseStreamEvent::ResponseOutputTextDelta(e) => {
            if e.delta.is_empty() {
                ResponsesEventOutcome::Events(vec![])
            } else {
                ResponsesEventOutcome::Events(vec![ModelEvent::TextDelta { delta: e.delta }])
            }
        }
        ResponseStreamEvent::ResponseOutputTextDone(e) => {
            // Emitted when text content is finalized.  We surface the text as
            // a fallback only when no incremental deltas were already emitted;
            // the caller tracks that via a `saw_delta` flag and can discard
            // this if deltas were already forwarded.  We always return it here
            // so the caller has full control.
            if e.text.is_empty() {
                ResponsesEventOutcome::Events(vec![])
            } else {
                ResponsesEventOutcome::Events(vec![ModelEvent::TextDelta { delta: e.text }])
            }
        }
        ResponseStreamEvent::ResponseCompleted(e) => {
            let mut out = vec![];
            if let Some(u) = e.response.usage {
                out.push(ModelEvent::Usage {
                    input: u.input_tokens,
                    output: u.output_tokens,
                    cache_read: u.input_tokens_details.cached_tokens,
                    cache_write: 0,
                });
            }
            let reason = format!("{:?}", e.response.status).to_lowercase();
            out.push(ModelEvent::Done { reason });
            ResponsesEventOutcome::Events(out)
        }
        ResponseStreamEvent::ResponseIncomplete(e) => {
            // Surface partial usage (if present) then a Done with the
            // incomplete reason so callers can display partial output.
            let mut out = vec![];
            if let Some(u) = e.response.usage {
                out.push(ModelEvent::Usage {
                    input: u.input_tokens,
                    output: u.output_tokens,
                    cache_read: u.input_tokens_details.cached_tokens,
                    cache_write: 0,
                });
            }
            let reason = e
                .response
                .incomplete_details
                .map(|d| format!("incomplete:{}", d.reason))
                .unwrap_or_else(|| "incomplete".into());
            out.push(ModelEvent::Done { reason });
            ResponsesEventOutcome::Events(out)
        }
        ResponseStreamEvent::ResponseFailed(e) => {
            let msg = e
                .response
                .error
                .map(|err| format!("{}: {}", err.code, err.message))
                .unwrap_or_else(|| "response failed".into());
            ResponsesEventOutcome::Error(ProviderError::Stream(msg))
        }
        ResponseStreamEvent::ResponseError(e) => {
            let msg = match e.code {
                Some(c) => format!("{c}: {}", e.message),
                None => e.message,
            };
            ResponsesEventOutcome::Error(ProviderError::Stream(msg))
        }
        // All other lifecycle events are intentionally ignored.
        _ => ResponsesEventOutcome::Events(vec![]),
    }
}

// ── OpenAiProvider ────────────────────────────────────────────────────────────

/// OpenAI provider.
///
/// Uses the native Responses API (`/v1/responses`) when the base URL is the
/// official OpenAI endpoint, and chat-completions (`/v1/chat/completions`) for
/// all other compatible endpoints (Groq, Ollama, etc.).
pub struct OpenAiProvider {
    auth: Arc<dyn AuthResolver>,
    base_url: String,
    client: reqwest::Client,
}

/// Returns `true` when `base_url` points to the native OpenAI endpoint.
fn is_native_openai(base_url: &str) -> bool {
    base_url.contains("api.openai.com")
}

// ── OpenAiProvider struct ─────────────────────────────────────────────────────

impl OpenAiProvider {
    /// Create with the standard OpenAI endpoint (uses Responses API).
    pub fn new(auth: Arc<dyn AuthResolver>) -> Self {
        Self::with_base_url(auth, DEFAULT_BASE)
    }

    /// Create with a custom base URL (useful for wiremock tests and compatible APIs).
    pub fn with_base_url(auth: Arc<dyn AuthResolver>, base_url: impl Into<String>) -> Self {
        Self {
            auth,
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Default auth resolver reading `OPENAI_API_KEY`.
    pub fn default_auth(config_key: Option<String>) -> Arc<dyn AuthResolver> {
        Arc::new(EnvAuthResolver::new("openai", "OPENAI_API_KEY", config_key))
    }

    fn headers(&self) -> Result<HeaderMap, ProviderError> {
        let key = self.auth.resolve()?;
        let mut headers = HeaderMap::new();
        let bearer = format!("Bearer {key}");
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&bearer).map_err(|_| ProviderError::Auth {
                provider: "openai".into(),
                msg: "invalid key bytes".into(),
            })?,
        );
        Ok(headers)
    }

    /// Stream via the Responses API (`POST /v1/responses`).
    async fn stream_responses(
        &self,
        req: ModelRequest,
        headers: HeaderMap,
    ) -> Result<BoxStream<Result<ModelEvent, ProviderError>>, ProviderError> {
        let (input, instructions) = build_responses_input(&req);
        let body = CreateResponse {
            model: Some(req.model.clone()),
            input,
            instructions,
            max_output_tokens: req.max_tokens,
            temperature: req.temperature,
            store: Some(false),
            stream: Some(true),
            ..Default::default()
        };

        let url = format!("{}/v1/responses", self.base_url);
        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http("openai".into(), e.to_string()))?;

        if resp.status() == 401 {
            return Err(ProviderError::Auth {
                provider: "openai".into(),
                msg: "401 Unauthorized".into(),
            });
        }

        if resp.status() == 429 {
            let retry = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());
            return Err(ProviderError::RateLimit {
                provider: "openai".into(),
                retry_after: retry,
            });
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            let detail = if body_text.is_empty() {
                format!("status {status}")
            } else {
                format!("status {status}: {body_text}")
            };
            return Err(ProviderError::Http("openai".into(), detail));
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ModelEvent, ProviderError>>(64);
        let mut bytes_stream = resp.bytes_stream();

        tokio::spawn(async move {
            let mut dec = SseDecoder::new();
            // Track whether any incremental text delta was forwarded so that
            // the `response.output_text.done` fallback can be suppressed when
            // deltas already covered the full text.
            let mut saw_delta = false;
            while let Some(chunk) = bytes_stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(Err(ProviderError::Stream(e.to_string()))).await;
                        return;
                    }
                };
                let text = match std::str::from_utf8(&bytes) {
                    Ok(s) => s.to_string(),
                    Err(e) => {
                        let _ = tx.send(Err(ProviderError::Stream(e.to_string()))).await;
                        return;
                    }
                };
                for ev in dec.feed(&text) {
                    let is_text_done = ev.event == "response.output_text.done";
                    match map_responses_event(&ev.event, &ev.data) {
                        ResponsesEventOutcome::Error(e) => {
                            let _ = tx.send(Err(e)).await;
                            return;
                        }
                        ResponsesEventOutcome::Events(evs) => {
                            // Suppress output_text.done text if deltas already covered it.
                            let skip = is_text_done && saw_delta;
                            let is_done = evs.iter().any(|e| matches!(e, ModelEvent::Done { .. }));
                            for model_ev in evs {
                                // Track that at least one text delta was emitted.
                                if matches!(model_ev, ModelEvent::TextDelta { .. }) {
                                    if !is_text_done {
                                        saw_delta = true;
                                    }
                                    if skip {
                                        continue;
                                    }
                                }
                                if tx.send(Ok(model_ev)).await.is_err() {
                                    return;
                                }
                            }
                            if is_done {
                                return;
                            }
                        }
                    }
                }
            }
        });

        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    /// Stream via chat completions (`POST /v1/chat/completions`).
    async fn stream_chat(
        &self,
        req: ModelRequest,
        headers: HeaderMap,
    ) -> Result<BoxStream<Result<ModelEvent, ProviderError>>, ProviderError> {
        let mut messages: Vec<ChatCompletionRequestMessage> =
            req.system.iter().map(to_chat_message).collect();
        messages.extend(req.messages.iter().map(to_chat_message));

        #[allow(deprecated)]
        let body = CreateChatCompletionRequest {
            model: req.model.clone(),
            messages,
            stream: Some(true),
            stream_options: Some(ChatCompletionStreamOptions {
                include_usage: Some(true),
                include_obfuscation: None,
            }),
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            ..Default::default()
        };

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http("openai".into(), e.to_string()))?;

        if resp.status() == 401 {
            return Err(ProviderError::Auth {
                provider: "openai".into(),
                msg: "401 Unauthorized".into(),
            });
        }

        if resp.status() == 429 {
            let retry = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());
            return Err(ProviderError::RateLimit {
                provider: "openai".into(),
                retry_after: retry,
            });
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            let detail = if body_text.is_empty() {
                format!("status {status}")
            } else {
                format!("status {status}: {body_text}")
            };
            return Err(ProviderError::Http("openai".into(), detail));
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ModelEvent, ProviderError>>(64);
        let mut bytes_stream = resp.bytes_stream();

        tokio::spawn(async move {
            let mut dec = SseDecoder::new();
            while let Some(chunk) = bytes_stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(Err(ProviderError::Stream(e.to_string()))).await;
                        return;
                    }
                };
                let text = match std::str::from_utf8(&bytes) {
                    Ok(s) => s.to_string(),
                    Err(e) => {
                        let _ = tx.send(Err(ProviderError::Stream(e.to_string()))).await;
                        return;
                    }
                };
                for ev in dec.feed(&text) {
                    if ev.data == "[DONE]" {
                        let _ = tx
                            .send(Ok(ModelEvent::Done {
                                reason: "stop".into(),
                            }))
                            .await;
                        return;
                    }
                    match serde_json::from_str::<OpenAiChunk>(&ev.data) {
                        Ok(c) => {
                            for model_ev in map_chunk(&c) {
                                if tx.send(Ok(model_ev)).await.is_err() {
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx
                                .send(Err(ProviderError::Stream(format!(
                                    "parse error: {e} in {:?}",
                                    ev.data
                                ))))
                                .await;
                        }
                    }
                }
            }
        });

        Ok(Box::pin(ReceiverStream::new(rx)))
    }
}

#[async_trait]
impl LanguageModel for OpenAiProvider {
    fn provider(&self) -> &'static str {
        "openai"
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![])
    }

    async fn stream(
        &self,
        req: ModelRequest,
    ) -> Result<BoxStream<Result<ModelEvent, ProviderError>>, ProviderError> {
        let headers = self.headers()?;
        if is_native_openai(&self.base_url) {
            self.stream_responses(req, headers).await
        } else {
            self.stream_chat(req, headers).await
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ContentPart;
    use futures::StreamExt;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path as wm_path},
    };

    fn text_req(model: &str, text: &str) -> ModelRequest {
        ModelRequest {
            model: model.into(),
            system: vec![],
            messages: vec![ModelMessage {
                role: "user".into(),
                content: vec![ContentPart::Text { text: text.into() }],
            }],
            tools: Default::default(),
            max_tokens: Some(256),
            temperature: None,
        }
    }

    // ── Unit tests for map_chunk (pure) ───────────────────────────────────────

    // RED 5.1 — choices[].delta.content text delta
    #[test]
    fn map_text_delta() {
        let chunk = OpenAiChunk {
            choices: vec![OpenAiChoice {
                delta: OpenAiDelta {
                    content: Some("hello".into()),
                },
            }],
            usage: None,
        };
        let events = map_chunk(&chunk);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ModelEvent::TextDelta { delta } if delta == "hello"));
    }

    // RED 5.1 — usage chunk → Usage event
    #[test]
    fn map_usage_chunk() {
        let chunk = OpenAiChunk {
            choices: vec![],
            usage: Some(OpenAiUsage {
                prompt_tokens: 20,
                completion_tokens: 40,
            }),
        };
        let events = map_chunk(&chunk);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            ModelEvent::Usage {
                input: 20,
                output: 40,
                ..
            }
        ));
    }

    // RED 5.1 — empty delta content is ignored
    #[test]
    fn empty_delta_ignored() {
        let chunk = OpenAiChunk {
            choices: vec![OpenAiChoice {
                delta: OpenAiDelta {
                    content: Some("".into()),
                },
            }],
            usage: None,
        };
        assert_eq!(map_chunk(&chunk).len(), 0);
    }

    // TRIANGULATE: null content (role-only delta) produces no event
    #[test]
    fn null_content_delta_ignored() {
        let chunk = OpenAiChunk {
            choices: vec![OpenAiChoice {
                delta: OpenAiDelta { content: None },
            }],
            usage: None,
        };
        assert_eq!(map_chunk(&chunk).len(), 0);
    }

    // ── Integration test: wiremock SSE mock ──────────────────────────────────

    fn sse_fixture() -> String {
        // Each SSE event block must end with a blank line (\n\n).
        // We give each line its own \n\n so the decoder flushes every block,
        // including the final [DONE] sentinel.
        concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":null}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":30}}\n\n",
            "data: [DONE]\n\n",
        )
        .to_string()
    }

    // RED 5.2 — streaming integration test
    #[tokio::test]
    async fn stream_yields_text_and_done() {
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wm_path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_fixture()),
            )
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_API_KEY_NONEXISTENT",
            Some("test-key".into()),
        ));
        let provider = OpenAiProvider::with_base_url(auth, srv.uri());
        let mut stream = provider.stream(text_req("gpt-4o", "hi")).await.unwrap();

        let mut events = vec![];
        while let Some(ev) = stream.next().await {
            events.push(ev.unwrap());
        }

        let text: String = events
            .iter()
            .filter_map(|e| {
                if let ModelEvent::TextDelta { delta } = e {
                    Some(delta.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(text, "Hello world");
        assert!(events.iter().any(|e| matches!(e, ModelEvent::Done { .. })));
        assert!(events.iter().any(|e| matches!(e, ModelEvent::Usage { .. })));
    }

    // RED 5.3 — 401 → ProviderError::Auth
    #[tokio::test]
    async fn unauthorized_401_returns_auth_error() {
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wm_path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_API_KEY_NONEXISTENT",
            Some("test-key".into()),
        ));
        let provider = OpenAiProvider::with_base_url(auth, srv.uri());
        let result = provider.stream(text_req("gpt-4o", "hi")).await;
        match result {
            Err(ProviderError::Auth { provider, .. }) => {
                assert_eq!(provider, "openai");
            }
            other => panic!("expected Auth, got {:?}", other.err()),
        }
    }

    // TRIANGULATE: missing auth (both env and config absent) → ProviderError::Auth
    #[tokio::test]
    async fn missing_auth_returns_auth_error() {
        // SAFETY: single-threaded test, unique key
        unsafe { std::env::remove_var("OPENAI_API_KEY_MISSING_TEST_UNIQUE") };
        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_API_KEY_MISSING_TEST_UNIQUE",
            None,
        ));
        let provider = OpenAiProvider::new(auth);
        let result = provider.stream(text_req("gpt-4o", "hi")).await;
        match result {
            Err(ProviderError::Auth { .. }) => {}
            other => panic!("expected Auth, got {:?}", other.err()),
        }
    }

    // ── Unit tests for to_chat_message (pure) ────────────────────────────────

    // RED C1 — user role message → ChatCompletionRequestMessage::User
    #[test]
    fn to_chat_message_user_role() {
        let msg = ModelMessage {
            role: "user".into(),
            content: vec![ContentPart::Text {
                text: "describe it".into(),
            }],
        };
        let out = to_chat_message(&msg);
        assert!(
            matches!(out, ChatCompletionRequestMessage::User(_)),
            "expected User variant"
        );
    }

    // TRIANGULATE — assistant role → ChatCompletionRequestMessage::Assistant
    #[test]
    fn to_chat_message_assistant_role() {
        let msg = ModelMessage {
            role: "assistant".into(),
            content: vec![ContentPart::Text {
                text: "sure".into(),
            }],
        };
        let out = to_chat_message(&msg);
        assert!(
            matches!(out, ChatCompletionRequestMessage::Assistant(_)),
            "expected Assistant variant"
        );
    }

    // RED C2 — 429 rate-limit response → ProviderError::RateLimit
    #[tokio::test]
    async fn rate_limit_429_returns_rate_limit_error() {
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wm_path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "30"))
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_API_KEY_NONEXISTENT",
            Some("test-key".into()),
        ));
        let provider = OpenAiProvider::with_base_url(auth, srv.uri());
        let result = provider.stream(text_req("gpt-4o", "hi")).await;
        match result {
            Err(ProviderError::RateLimit {
                provider,
                retry_after,
            }) => {
                assert_eq!(provider, "openai");
                assert_eq!(retry_after, Some(30));
            }
            other => panic!("expected RateLimit, got {:?}", other.err()),
        }
    }

    // TRIANGULATE — 429 without retry-after header → None retry
    #[tokio::test]
    async fn rate_limit_429_no_retry_header() {
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wm_path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_API_KEY_NONEXISTENT",
            Some("test-key".into()),
        ));
        let provider = OpenAiProvider::with_base_url(auth, srv.uri());
        let result = provider.stream(text_req("gpt-4o", "hi")).await;
        match result {
            Err(ProviderError::RateLimit {
                retry_after: None, ..
            }) => {}
            other => panic!("expected RateLimit with None retry, got {:?}", other.err()),
        }
    }

    // RED C3 — non-200/401/429 response → ProviderError::Http
    #[tokio::test]
    async fn server_error_500_returns_http_error() {
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wm_path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_API_KEY_NONEXISTENT",
            Some("test-key".into()),
        ));
        let provider = OpenAiProvider::with_base_url(auth, srv.uri());
        let result = provider.stream(text_req("gpt-4o", "hi")).await;
        match result {
            Err(ProviderError::Http(provider, msg)) => {
                assert_eq!(provider, "openai");
                assert!(msg.contains("500"), "error should mention status: {msg}");
            }
            other => panic!("expected Http error, got {:?}", other.err()),
        }
    }

    // RED C4 — parse error in SSE stream → ProviderError::Stream in stream
    #[tokio::test]
    async fn stream_parse_error_yields_stream_error() {
        let srv = MockServer::start().await;
        // Malformed JSON in data field
        let fixture = "data: {invalid json}\n\ndata: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(wm_path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(fixture),
            )
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_API_KEY_NONEXISTENT",
            Some("test-key".into()),
        ));
        let provider = OpenAiProvider::with_base_url(auth, srv.uri());
        let mut stream = provider.stream(text_req("gpt-4o", "hi")).await.unwrap();

        let mut saw_error = false;
        while let Some(ev) = stream.next().await {
            if ev.is_err() {
                saw_error = true;
            }
        }
        assert!(saw_error, "malformed SSE data must produce a stream error");
    }

    // RED C5 — system messages are prepended to request messages
    #[tokio::test]
    async fn stream_includes_system_messages() {
        let srv = MockServer::start().await;
        let fixture = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
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

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_API_KEY_NONEXISTENT",
            Some("test-key".into()),
        ));
        let provider = OpenAiProvider::with_base_url(auth, srv.uri());

        let req = ModelRequest {
            model: "gpt-4o".into(),
            system: vec![ModelMessage {
                role: "system".into(),
                content: vec![ContentPart::Text {
                    text: "You are helpful.".into(),
                }],
            }],
            messages: vec![ModelMessage {
                role: "user".into(),
                content: vec![ContentPart::Text { text: "hi".into() }],
            }],
            tools: Default::default(),
            max_tokens: Some(64),
            temperature: None,
        };

        let mut stream = provider.stream(req).await.unwrap();
        let mut text = String::new();
        while let Some(ev) = stream.next().await {
            if let Ok(ModelEvent::TextDelta { delta }) = ev {
                text.push_str(&delta);
            }
        }
        assert_eq!(text, "ok");
    }

    // RED C6 — provider() and models() return expected values
    #[tokio::test]
    async fn provider_name_and_models() {
        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_API_KEY_NONEXISTENT",
            Some("key".into()),
        ));
        let p = OpenAiProvider::new(auth.clone());
        assert_eq!(p.provider(), "openai");
        let models = p.models().await.unwrap();
        assert!(models.is_empty(), "models() should return empty vec");
    }

    // RED C7 — default_auth constructs a resolver (no panic)
    #[test]
    fn default_auth_constructs_resolver() {
        let _auth = OpenAiProvider::default_auth(Some("explicit-key".into()));
        // If it compiles and doesn't panic, auth resolver was created.
        // Verify it returns Auth error for non-existent env var
        let auth2 = OpenAiProvider::default_auth(None);
        // Resolve should return Err (OPENAI_API_KEY not set in test env)
        // We just verify it doesn't panic:
        let _ = auth2.resolve();
    }

    // ── Unit tests for map_responses_event (pure) ────────────────────────────

    // Minimal valid delta payload — includes mandatory SDK fields.
    fn delta_data(text: &str) -> String {
        format!(
            r#"{{"sequence_number":1,"item_id":"item_1","output_index":0,"content_index":0,"delta":{}}}"#,
            serde_json::to_string(text).unwrap()
        )
    }

    // Minimal valid completed payload — includes mandatory SDK fields.
    fn completed_data(input: u32, output: u32) -> String {
        format!(
            r#"{{"sequence_number":99,"response":{{"id":"r1","object":"response","created_at":0,"model":"gpt-4o","status":"completed","output":[],"usage":{{"input_tokens":{input},"output_tokens":{output},"input_tokens_details":{{"cached_tokens":0}},"output_tokens_details":{{"reasoning_tokens":0}},"total_tokens":{}}}}}}}"#,
            input + output
        )
    }

    // Minimal valid failed payload — includes mandatory SDK fields with an error object.
    fn failed_data(code: &str, message: &str) -> String {
        format!(
            r#"{{"sequence_number":99,"response":{{"id":"r1","object":"response","created_at":0,"model":"gpt-4o","status":"failed","output":[],"error":{{"code":"{code}","message":"{message}"}}}}}}"#
        )
    }

    // Minimal valid incomplete payload — with incomplete_details.
    fn incomplete_data(reason: &str) -> String {
        format!(
            r#"{{"sequence_number":99,"response":{{"id":"r1","object":"response","created_at":0,"model":"gpt-4o","status":"incomplete","output":[],"incomplete_details":{{"reason":"{reason}"}}}}}}"#
        )
    }

    // Helper: extract events from a successful outcome (panics on Error).
    fn unwrap_events(outcome: ResponsesEventOutcome) -> Vec<ModelEvent> {
        match outcome {
            ResponsesEventOutcome::Events(evs) => evs,
            ResponsesEventOutcome::Error(e) => panic!("unexpected Error outcome: {e:?}"),
        }
    }

    // Helper: extract the error from an error outcome (panics on Events).
    fn unwrap_err(outcome: ResponsesEventOutcome) -> ProviderError {
        match outcome {
            ResponsesEventOutcome::Error(e) => e,
            ResponsesEventOutcome::Events(evs) => {
                panic!("expected Error outcome, got events: {evs:?}")
            }
        }
    }

    // text delta event → TextDelta
    #[test]
    fn responses_text_delta_maps_to_text_delta() {
        let evs = unwrap_events(map_responses_event(
            "response.output_text.delta",
            &delta_data("hello"),
        ));
        assert_eq!(evs.len(), 1);
        assert!(matches!(&evs[0], ModelEvent::TextDelta { delta } if delta == "hello"));
    }

    // empty delta is silently dropped
    #[test]
    fn responses_empty_delta_ignored() {
        let evs = unwrap_events(map_responses_event(
            "response.output_text.delta",
            &delta_data(""),
        ));
        assert_eq!(evs.len(), 0);
    }

    // response.completed with usage → Usage + Done
    #[test]
    fn responses_completed_emits_usage_and_done() {
        let evs = unwrap_events(map_responses_event(
            "response.completed",
            &completed_data(10, 20),
        ));
        assert_eq!(evs.len(), 2);
        assert!(matches!(
            &evs[0],
            ModelEvent::Usage {
                input: 10,
                output: 20,
                ..
            }
        ));
        assert!(matches!(&evs[1], ModelEvent::Done { reason } if reason == "completed"));
    }

    // response.completed with no usage still emits Done
    #[test]
    fn responses_completed_no_usage_still_done() {
        // Omit usage from the response object.
        let data = r#"{"sequence_number":1,"response":{"id":"r1","object":"response","created_at":0,"model":"gpt-4o","status":"completed","output":[]}}"#;
        let evs = unwrap_events(map_responses_event("response.completed", data));
        assert_eq!(evs.len(), 1);
        assert!(matches!(&evs[0], ModelEvent::Done { .. }));
    }

    // unknown event types are silently ignored
    #[test]
    fn responses_unknown_event_ignored() {
        // These will fail SDK deserialization (missing mandatory fields) → silently ignored.
        let evs = unwrap_events(map_responses_event("response.in_progress", r#"{}"#));
        assert_eq!(evs.len(), 0);
        let evs2 = unwrap_events(map_responses_event("response.created", r#"{}"#));
        assert_eq!(evs2.len(), 0);
    }

    // ── New tests: ResponseOutputTextDone, ResponseFailed, ResponseIncomplete, ResponseError ──

    // response.output_text.done with non-empty text → TextDelta (fallback)
    #[test]
    fn responses_text_done_emits_text_delta() {
        let data = r#"{"sequence_number":2,"item_id":"item_1","output_index":0,"content_index":0,"text":"full text"}"#;
        let evs = unwrap_events(map_responses_event("response.output_text.done", data));
        assert_eq!(evs.len(), 1);
        assert!(matches!(&evs[0], ModelEvent::TextDelta { delta } if delta == "full text"));
    }

    // response.output_text.done with empty text → no events
    #[test]
    fn responses_text_done_empty_ignored() {
        let data = r#"{"sequence_number":2,"item_id":"item_1","output_index":0,"content_index":0,"text":""}"#;
        let evs = unwrap_events(map_responses_event("response.output_text.done", data));
        assert_eq!(evs.len(), 0);
    }

    // response.failed with error object → ProviderError::Stream
    #[test]
    fn responses_failed_emits_stream_error() {
        let err = unwrap_err(map_responses_event(
            "response.failed",
            &failed_data("server_error", "something broke"),
        ));
        match err {
            ProviderError::Stream(msg) => {
                assert!(
                    msg.contains("server_error"),
                    "error should contain code: {msg}"
                );
                assert!(
                    msg.contains("something broke"),
                    "error should contain message: {msg}"
                );
            }
            other => panic!("expected Stream error, got {other:?}"),
        }
    }

    // response.failed with no error field → generic message
    #[test]
    fn responses_failed_no_error_field() {
        let data = r#"{"sequence_number":99,"response":{"id":"r1","object":"response","created_at":0,"model":"gpt-4o","status":"failed","output":[]}}"#;
        let err = unwrap_err(map_responses_event("response.failed", data));
        match err {
            ProviderError::Stream(msg) => {
                assert!(
                    msg.contains("failed"),
                    "error message should mention failure: {msg}"
                );
            }
            other => panic!("expected Stream error, got {other:?}"),
        }
    }

    // response.incomplete with incomplete_details → Done with incomplete:<reason>
    #[test]
    fn responses_incomplete_emits_done_with_reason() {
        let evs = unwrap_events(map_responses_event(
            "response.incomplete",
            &incomplete_data("max_output_tokens"),
        ));
        assert_eq!(evs.len(), 1);
        assert!(
            matches!(&evs[0], ModelEvent::Done { reason } if reason == "incomplete:max_output_tokens"),
            "got: {:?}",
            evs[0]
        );
    }

    // response.incomplete without incomplete_details → Done with "incomplete"
    #[test]
    fn responses_incomplete_no_details() {
        let data = r#"{"sequence_number":99,"response":{"id":"r1","object":"response","created_at":0,"model":"gpt-4o","status":"incomplete","output":[]}}"#;
        let evs = unwrap_events(map_responses_event("response.incomplete", data));
        assert_eq!(evs.len(), 1);
        assert!(matches!(&evs[0], ModelEvent::Done { reason } if reason == "incomplete"));
    }

    // error SSE event with code → ProviderError::Stream containing code and message
    #[test]
    fn responses_error_event_with_code() {
        let data = r#"{"sequence_number":1,"code":"rate_limit_exceeded","message":"slow down","param":null}"#;
        let err = unwrap_err(map_responses_event("error", data));
        match err {
            ProviderError::Stream(msg) => {
                assert!(
                    msg.contains("rate_limit_exceeded"),
                    "should contain code: {msg}"
                );
                assert!(msg.contains("slow down"), "should contain message: {msg}");
            }
            other => panic!("expected Stream error, got {other:?}"),
        }
    }

    // error SSE event without code → ProviderError::Stream containing just message
    #[test]
    fn responses_error_event_no_code() {
        let data = r#"{"sequence_number":1,"message":"internal error","param":null}"#;
        let err = unwrap_err(map_responses_event("error", data));
        match err {
            ProviderError::Stream(msg) => {
                assert!(
                    msg.contains("internal error"),
                    "should contain message: {msg}"
                );
            }
            other => panic!("expected Stream error, got {other:?}"),
        }
    }

    // ── Wiremock integration test: Responses API path ────────────────────────

    fn responses_sse_fixture() -> String {
        // Typed SSE events as produced by the OpenAI Responses API.
        // Includes output_text.done after deltas — the stream loop should suppress
        // the done text since deltas were already forwarded.
        let delta1 = delta_data("Hello");
        let delta2 = delta_data(" world");
        let done_text = r#"{"sequence_number":3,"item_id":"item_1","output_index":0,"content_index":0,"text":"Hello world"}"#;
        let completed = completed_data(5, 12);
        format!(
            "event: response.created\ndata: {{}}\n\n\
             event: response.in_progress\ndata: {{}}\n\n\
             event: response.output_text.delta\ndata: {delta1}\n\n\
             event: response.output_text.delta\ndata: {delta2}\n\n\
             event: response.output_text.done\ndata: {done_text}\n\n\
             event: response.completed\ndata: {completed}\n\n"
        )
    }

    // Integration: stream_responses yields TextDelta, Usage, Done from typed SSE events.
    // output_text.done is suppressed because deltas were already forwarded (no duplicates).
    // We call stream_responses directly because the wiremock URL isn't api.openai.com.
    #[tokio::test]
    async fn responses_stream_yields_text_usage_done() {
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wm_path("/v1/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(responses_sse_fixture()),
            )
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_API_KEY_NONEXISTENT",
            Some("test-key".into()),
        ));
        let provider = OpenAiProvider::with_base_url(auth, srv.uri());
        let headers = provider.headers().unwrap();
        let mut stream = provider
            .stream_responses(text_req("gpt-5-nano", "hi"), headers)
            .await
            .unwrap();

        let mut events = vec![];
        while let Some(ev) = stream.next().await {
            events.push(ev.unwrap());
        }

        let text: String = events
            .iter()
            .filter_map(|e| {
                if let ModelEvent::TextDelta { delta } = e {
                    Some(delta.as_str())
                } else {
                    None
                }
            })
            .collect();
        // "Hello world" exactly — output_text.done suppressed (no duplication).
        assert_eq!(text, "Hello world");
        assert!(events.iter().any(|e| matches!(e, ModelEvent::Done { .. })));
        assert!(events.iter().any(|e| matches!(
            e,
            ModelEvent::Usage {
                input: 5,
                output: 12,
                ..
            }
        )));
    }

    // Integration: response.failed SSE event surfaces as stream Error, not silent hang.
    #[tokio::test]
    async fn responses_stream_failed_event_yields_stream_error() {
        let srv = MockServer::start().await;
        let failed = failed_data("server_error", "model blew up");
        let fixture = format!(
            "event: response.output_text.delta\ndata: {}\n\n\
             event: response.failed\ndata: {failed}\n\n",
            delta_data("partial")
        );
        Mock::given(method("POST"))
            .and(wm_path("/v1/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(fixture),
            )
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_API_KEY_NONEXISTENT",
            Some("test-key".into()),
        ));
        let provider = OpenAiProvider::with_base_url(auth, srv.uri());
        let headers = provider.headers().unwrap();
        let mut stream = provider
            .stream_responses(text_req("gpt-5-nano", "hi"), headers)
            .await
            .unwrap();

        let mut saw_text = false;
        let mut saw_error = false;
        while let Some(ev) = stream.next().await {
            match ev {
                Ok(ModelEvent::TextDelta { .. }) => saw_text = true,
                Err(ProviderError::Stream(msg)) => {
                    assert!(
                        msg.contains("server_error"),
                        "msg should contain code: {msg}"
                    );
                    saw_error = true;
                }
                _ => {}
            }
        }
        assert!(
            saw_text,
            "partial text delta before failure should be surfaced"
        );
        assert!(saw_error, "response.failed must produce a stream error");
    }

    // Integration: error SSE event (without prior delta) surfaces as stream Error.
    #[tokio::test]
    async fn responses_stream_error_event_yields_stream_error() {
        let srv = MockServer::start().await;
        let fixture = "event: error\ndata: {\"sequence_number\":1,\"code\":\"quota_exceeded\",\"message\":\"out of quota\",\"param\":null}\n\n";
        Mock::given(method("POST"))
            .and(wm_path("/v1/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(fixture),
            )
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_API_KEY_NONEXISTENT",
            Some("test-key".into()),
        ));
        let provider = OpenAiProvider::with_base_url(auth, srv.uri());
        let headers = provider.headers().unwrap();
        let mut stream = provider
            .stream_responses(text_req("gpt-5-nano", "hi"), headers)
            .await
            .unwrap();

        let mut saw_error = false;
        while let Some(ev) = stream.next().await {
            if let Err(ProviderError::Stream(msg)) = ev {
                assert!(
                    msg.contains("quota_exceeded"),
                    "msg should contain code: {msg}"
                );
                saw_error = true;
            }
        }
        assert!(saw_error, "error SSE event must produce a stream error");
    }
}
