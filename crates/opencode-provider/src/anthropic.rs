//! Anthropic provider — streams responses from the Anthropic Messages API.

use crate::auth::{AuthResolver, EnvAuthResolver};
use crate::error::ProviderError;
use crate::sse::SseDecoder;
use crate::types::{LanguageModel, ModelEvent, ModelInfo, ModelMessage, ModelRequest};
use async_anthropic::types::{
    ContentBlockDelta, CreateMessagesRequest, Message, MessageContent, MessageContentList,
    MessageRole, MessagesStreamEvent, Text, ToolResult, ToolUse,
};
use async_trait::async_trait;
use futures::StreamExt;
use opencode_core::context::BoxStream;
use reqwest::header::{HeaderMap, HeaderValue};
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;

const API_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

// ── Helper: convert ModelMessage → SDK Message ────────────────────────────────

/// Convert a [`ModelMessage`] to the SDK [`Message`] type.
///
/// Images are silently skipped — the Anthropic SDK crate marks image support as TODO.
/// Text, ToolUse, and ToolResult parts are mapped to their SDK counterparts.
fn to_sdk_message(msg: &ModelMessage) -> Message {
    use crate::types::ContentPart;
    let parts: Vec<MessageContent> = msg
        .content
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text { text } => Some(MessageContent::Text(Text { text: text.clone() })),
            ContentPart::Image { .. } => {
                // async-anthropic SDK does not yet support image content blocks.
                // Skip silently; callers relying on images must use a raw HTTP path.
                None
            }
            ContentPart::ToolUse { id, name, input } => Some(MessageContent::ToolUse(ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            })),
            ContentPart::ToolResult {
                tool_use_id,
                content,
            } => Some(MessageContent::ToolResult(ToolResult {
                tool_use_id: tool_use_id.clone(),
                content: Some(content.clone()),
                is_error: false,
            })),
        })
        .collect();
    let role = match msg.role.as_str() {
        "assistant" => MessageRole::Assistant,
        _ => MessageRole::User,
    };
    Message {
        role,
        content: MessageContentList(parts),
    }
}

/// Collect system messages into a single joined string for `CreateMessagesRequest.system`.
fn build_system(req: &ModelRequest) -> Option<String> {
    use crate::types::ContentPart;
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
}

// ── Map a single Anthropic SSE event (SDK type) to ModelEvent(s) ─────────────

/// Map a single `MessagesStreamEvent` (from the `async-anthropic` SDK) to 0..N `ModelEvent`s.
///
/// This is a pure function — easy to unit-test without network I/O.
pub fn map_event(ev: &MessagesStreamEvent) -> Vec<ModelEvent> {
    match ev {
        MessagesStreamEvent::MessageStart { message, .. } => {
            let Some(usage) = &message.usage else {
                return vec![];
            };
            vec![ModelEvent::Usage {
                input: usage.input_tokens.unwrap_or(0),
                output: usage.output_tokens.unwrap_or(0),
                cache_read: 0,
                cache_write: 0,
            }]
        }
        MessagesStreamEvent::ContentBlockStart {
            index,
            content_block,
        } => {
            if let MessageContent::ToolUse(tool) = content_block {
                vec![ModelEvent::ToolUseStart {
                    id: tool.id.clone(),
                    name: tool.name.clone(),
                }]
            } else {
                let _ = index;
                vec![]
            }
        }
        MessagesStreamEvent::ContentBlockDelta { index, delta } => match delta {
            ContentBlockDelta::TextDelta { text } => {
                if text.is_empty() {
                    vec![]
                } else {
                    vec![ModelEvent::TextDelta {
                        delta: text.clone(),
                    }]
                }
            }
            ContentBlockDelta::InputJsonDelta { partial_json } => {
                vec![ModelEvent::ToolUseInputDelta {
                    id: index.to_string(),
                    delta: partial_json.clone(),
                }]
            }
        },
        MessagesStreamEvent::ContentBlockStop { index } => {
            vec![ModelEvent::ToolUseEnd {
                id: index.to_string(),
            }]
        }
        MessagesStreamEvent::MessageDelta { delta, usage } => {
            let mut out = vec![];
            if let Some(u) = usage {
                out.push(ModelEvent::Usage {
                    input: 0,
                    output: u.output_tokens.unwrap_or(0),
                    cache_read: 0,
                    cache_write: 0,
                });
            }
            if let Some(reason) = &delta.stop_reason {
                if !reason.is_empty() && reason != "null" {
                    out.push(ModelEvent::Done {
                        reason: reason.clone(),
                    });
                }
            }
            out
        }
        MessagesStreamEvent::MessageStop => vec![ModelEvent::Done {
            reason: "end_turn".into(),
        }],
    }
}

// ── AnthropicProvider ─────────────────────────────────────────────────────────

/// Anthropic provider wrapping the `/v1/messages` streaming API.
pub struct AnthropicProvider {
    auth: Arc<dyn AuthResolver>,
    base_url: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    /// Create with the standard Anthropic endpoint.
    pub fn new(auth: Arc<dyn AuthResolver>) -> Self {
        Self::with_base_url(auth, API_BASE)
    }

    /// Create with a custom base URL (useful for tests with wiremock).
    pub fn with_base_url(auth: Arc<dyn AuthResolver>, base_url: impl Into<String>) -> Self {
        Self {
            auth,
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Default auth resolver reading `ANTHROPIC_API_KEY`.
    pub fn default_auth(config_key: Option<String>) -> Arc<dyn AuthResolver> {
        Arc::new(EnvAuthResolver::new(
            "anthropic",
            "ANTHROPIC_API_KEY",
            config_key,
        ))
    }

    fn headers(&self) -> Result<HeaderMap, ProviderError> {
        let key = self.auth.resolve()?;
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&key).map_err(|_| ProviderError::Auth {
                provider: "anthropic".into(),
                msg: "invalid key bytes".into(),
            })?,
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        Ok(headers)
    }
}

#[async_trait]
impl LanguageModel for AnthropicProvider {
    fn provider(&self) -> &'static str {
        "anthropic"
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        // Models are fetched via CatalogCache; this impl returns an empty list
        // (the registry uses CatalogCache independently).
        Ok(vec![])
    }

    async fn stream(
        &self,
        req: ModelRequest,
    ) -> Result<BoxStream<Result<ModelEvent, ProviderError>>, ProviderError> {
        let headers = self.headers()?;
        let body = CreateMessagesRequest {
            model: req.model.clone(),
            max_tokens: req.max_tokens.unwrap_or(4096) as i32,
            messages: req.messages.iter().map(to_sdk_message).collect(),
            system: build_system(&req),
            stream: true,
            temperature: req.temperature,
            metadata: None,
            stop_sequences: None,
            tool_choice: None,
            tools: None,
            top_k: None,
            top_p: None,
        };

        let url = format!("{}/v1/messages", self.base_url);
        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http("anthropic".into(), e.to_string()))?;

        if resp.status() == 429 {
            let retry = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());
            return Err(ProviderError::RateLimit {
                provider: "anthropic".into(),
                retry_after: retry,
            });
        }

        if resp.status() == 401 {
            return Err(ProviderError::Auth {
                provider: "anthropic".into(),
                msg: "401 Unauthorized".into(),
            });
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let detail = if body.is_empty() {
                format!("status {status}")
            } else {
                format!("status {status}: {body}")
            };
            return Err(ProviderError::Http("anthropic".into(), detail));
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
                        return;
                    }
                    match serde_json::from_str::<MessagesStreamEvent>(&ev.data) {
                        Ok(sdk_ev) => {
                            for model_ev in map_event(&sdk_ev) {
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ContentPart;
    use async_anthropic::types::{
        ContentBlockDelta, MessageContent, MessageDelta, MessagesStreamEvent, Text, ToolUse, Usage,
    };
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

    // ── Unit tests for map_event (pure, no network) ───────────────────────────

    // RED 4.1 — content_block_delta text
    #[test]
    fn map_text_delta() {
        let ev = MessagesStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentBlockDelta::TextDelta {
                text: "hello".into(),
            },
        };
        let events = map_event(&ev);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ModelEvent::TextDelta { delta } if delta == "hello"));
    }

    // RED 4.1 — content_block_start tool-use
    #[test]
    fn map_tool_use_start() {
        let ev = MessagesStreamEvent::ContentBlockStart {
            index: 1,
            content_block: MessageContent::ToolUse(ToolUse {
                id: "call_abc".into(),
                name: "bash".into(),
                input: serde_json::Value::Null,
            }),
        };
        let events = map_event(&ev);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ModelEvent::ToolUseStart { id, name } if id == "call_abc" && name == "bash")
        );
    }

    // RED 4.1 — content_block_stop
    #[test]
    fn map_tool_use_stop() {
        let ev = MessagesStreamEvent::ContentBlockStop { index: 1 };
        let events = map_event(&ev);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ModelEvent::ToolUseEnd { id } if id == "1"));
    }

    // RED 4.1 — message_delta usage
    #[test]
    fn map_message_delta_usage() {
        let ev = MessagesStreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: Some("end_turn".into()),
                stop_sequence: None,
            },
            usage: Some(Usage {
                input_tokens: None,
                output_tokens: Some(50),
            }),
        };
        let events = map_event(&ev);
        // Usage + Done
        assert!(events.len() >= 1);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ModelEvent::Usage { output, .. } if *output == 50))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ModelEvent::Done { reason } if reason == "end_turn"))
        );
    }

    // RED 4.1 — message_stop → Done
    #[test]
    fn map_message_stop() {
        let ev = MessagesStreamEvent::MessageStop;
        let events = map_event(&ev);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ModelEvent::Done { .. }));
    }

    // TRIANGULATE: empty text_delta produces no TextDelta event
    #[test]
    fn empty_text_delta_ignored() {
        let ev = MessagesStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentBlockDelta::TextDelta { text: "".into() },
        };
        assert_eq!(map_event(&ev).len(), 0);
    }

    // TRIANGULATE: content_block_start with text type yields no events
    #[test]
    fn text_content_block_start_yields_no_event() {
        let ev = MessagesStreamEvent::ContentBlockStart {
            index: 0,
            content_block: MessageContent::Text(Text { text: "".into() }),
        };
        assert_eq!(map_event(&ev).len(), 0);
    }

    // ── Integration test: wiremock SSE mock ──────────────────────────────────

    fn sse_fixture() -> String {
        vec![
            "event: message_start",
            r#"data: {"type":"message_start","message":{"id":"msg_01","model":"claude-3","role":"assistant","content":[],"usage":{"input_tokens":10,"output_tokens":0}}}"#,
            "",
            "event: content_block_start",
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            "",
            "event: content_block_delta",
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#,
            "",
            "event: content_block_delta",
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}"#,
            "",
            "event: content_block_stop",
            r#"data: {"type":"content_block_stop","index":0}"#,
            "",
            "event: message_delta",
            r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":25}}"#,
            "",
            "event: message_stop",
            r#"data: {"type":"message_stop"}"#,
            "",
        ]
        .join("\n")
    }

    // RED 4.2 — streaming integration test
    #[tokio::test]
    async fn stream_yields_text_and_done() {
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wm_path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_fixture()),
            )
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "anthropic",
            "ANTHROPIC_API_KEY_NONEXISTENT",
            Some("test-key".into()),
        ));
        let provider = AnthropicProvider::with_base_url(auth, srv.uri());
        let mut stream = provider
            .stream(text_req("claude-3-haiku-20240307", "hi"))
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
        assert_eq!(text, "Hello world");
        assert!(events.iter().any(|e| matches!(e, ModelEvent::Done { .. })));
        assert!(events.iter().any(|e| matches!(e, ModelEvent::Usage { .. })));
    }

    // RED 4.3 — 429 → ProviderError::RateLimit with retry_after
    #[tokio::test]
    async fn rate_limit_429_returns_error() {
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wm_path("/v1/messages"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "30"))
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "anthropic",
            "ANTHROPIC_API_KEY_NONEXISTENT",
            Some("test-key".into()),
        ));
        let provider = AnthropicProvider::with_base_url(auth, srv.uri());
        let result = provider
            .stream(text_req("claude-3-haiku-20240307", "hi"))
            .await;
        match result {
            Err(ProviderError::RateLimit {
                provider,
                retry_after: Some(30),
            }) => {
                assert_eq!(provider, "anthropic");
            }
            other => panic!("expected RateLimit, got {:?}", other.err()),
        }
    }

    // TRIANGULATE: 401 → ProviderError::Auth
    #[tokio::test]
    async fn unauthorized_401_returns_auth_error() {
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wm_path("/v1/messages"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "anthropic",
            "ANTHROPIC_API_KEY_NONEXISTENT",
            Some("test-key".into()),
        ));
        let provider = AnthropicProvider::with_base_url(auth, srv.uri());
        let result = provider
            .stream(text_req("claude-3-haiku-20240307", "hi"))
            .await;
        match result {
            Err(ProviderError::Auth { provider, .. }) => {
                assert_eq!(provider, "anthropic");
            }
            other => panic!("expected Auth error, got {:?}", other.err()),
        }
    }
}
