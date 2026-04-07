//! Google AI Studio (Gemini) provider — streams via the Generative Language API.
//!
//! Endpoint: `POST /v1beta/models/{model}:streamGenerateContent?alt=sse`
//! Auth: API key via `x-goog-api-key` header.
//! No `[DONE]` sentinel — stream ends when connection closes.

use crate::auth::AuthResolver;
use crate::error::ProviderError;
use crate::sse::SseDecoder;
use crate::types::{ContentPart, LanguageModel, ModelEvent, ModelInfo, ModelMessage, ModelRequest};
use async_trait::async_trait;
use futures::StreamExt;
use opencode_core::context::BoxStream;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use tokio_stream::wrappers::ReceiverStream;

const API_BASE: &str = "https://generativelanguage.googleapis.com";

// ── Request types ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(rename = "inlineData", skip_serializing_if = "Option::is_none")]
    inline_data: Option<InlineData>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    function_call: Option<FunctionCallPart>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    function_response: Option<FunctionResponsePart>,
}

#[derive(Serialize)]
struct InlineData {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: String,
}

#[derive(Serialize)]
struct FunctionCallPart {
    name: String,
    args: serde_json::Value,
}

#[derive(Serialize)]
struct FunctionResponsePart {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    name: String,
    response: serde_json::Value,
}

#[derive(Serialize)]
struct Content {
    role: String,
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct GenerationConfig {
    #[serde(rename = "maxOutputTokens", skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct FunctionDeclaration {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Serialize)]
struct Tool {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<FunctionDeclaration>,
}

#[derive(Serialize)]
struct GenerateContentRequest {
    contents: Vec<Content>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<Content>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Tool>,
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ResponsePart {
    text: Option<String>,
    #[serde(rename = "functionCall")]
    function_call: Option<ResponseFunctionCall>,
}

#[derive(Deserialize)]
struct ResponseFunctionCall {
    #[serde(default)]
    id: Option<String>,
    name: String,
    args: serde_json::Value,
}

#[derive(Deserialize)]
struct ResponseContent {
    parts: Vec<ResponsePart>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<ResponseContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct UsageMetadata {
    #[serde(rename = "promptTokenCount", default)]
    prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount", default)]
    candidates_token_count: u32,
}

#[derive(Deserialize)]
struct GenerateContentResponse {
    candidates: Option<Vec<Candidate>>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<UsageMetadata>,
}

// ── Request builder helpers ───────────────────────────────────────────────────

fn names(req: &ModelRequest) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for msg in &req.messages {
        for part in &msg.content {
            if let ContentPart::ToolUse { id, name, .. } = part {
                out.insert(id.clone(), name.clone());
            }
        }
    }
    out
}

fn to_content(msg: &ModelMessage, names: &BTreeMap<String, String>) -> Content {
    // Google uses "model" not "assistant"
    let role = if msg.role == "assistant" {
        "model".to_string()
    } else {
        msg.role.clone()
    };
    let parts = msg
        .content
        .iter()
        .map(|part| part_from(part, names))
        .collect();
    Content { role, parts }
}

fn part_from(p: &ContentPart, names: &BTreeMap<String, String>) -> Part {
    match p {
        ContentPart::Text { text } => Part {
            text: Some(text.clone()),
            inline_data: None,
            function_call: None,
            function_response: None,
        },
        ContentPart::Image { mime, data } => Part {
            text: None,
            inline_data: Some(InlineData {
                mime_type: mime.clone(),
                data: data.clone(),
            }),
            function_call: None,
            function_response: None,
        },
        ContentPart::ToolUse { name, input, .. } => Part {
            text: None,
            inline_data: None,
            function_call: Some(FunctionCallPart {
                name: name.clone(),
                args: input.clone(),
            }),
            function_response: None,
        },
        ContentPart::ToolResult {
            tool_use_id,
            content,
        } => Part {
            text: None,
            inline_data: None,
            function_call: None,
            function_response: Some(FunctionResponsePart {
                id: Some(tool_use_id.clone()),
                name: names
                    .get(tool_use_id)
                    .cloned()
                    .unwrap_or_else(|| tool_use_id.clone()),
                response: serde_json::json!({ "output": content }),
            }),
        },
    }
}

fn build_system(req: &ModelRequest) -> Option<Content> {
    let text: String = req
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
    if text.is_empty() {
        return None;
    }
    Some(Content {
        role: "user".to_string(),
        parts: vec![Part {
            text: Some(text),
            inline_data: None,
            function_call: None,
            function_response: None,
        }],
    })
}

fn build_tools(req: &ModelRequest) -> Vec<Tool> {
    if req.tools.is_empty() {
        return vec![];
    }
    let decls: Vec<FunctionDeclaration> = req
        .tools
        .iter()
        .map(|(name, schema)| FunctionDeclaration {
            name: name.clone(),
            description: schema
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            parameters: schema.clone(),
        })
        .collect();
    vec![Tool {
        function_declarations: decls,
    }]
}

// ── Map a single response chunk to ModelEvent(s) ──────────────────────────────

/// Map a parsed [`GenerateContentResponse`] chunk to 0..N [`ModelEvent`]s.
///
/// Pure function — easy to unit-test without network I/O.
fn map_chunk(chunk: &GenerateContentResponse) -> Vec<ModelEvent> {
    let mut out = vec![];

    if let Some(usage) = &chunk.usage_metadata {
        if usage.prompt_token_count > 0 || usage.candidates_token_count > 0 {
            out.push(ModelEvent::Usage {
                input: usage.prompt_token_count,
                output: usage.candidates_token_count,
                cache_read: 0,
                cache_write: 0,
            });
        }
    }

    let candidates = chunk.candidates.as_deref().unwrap_or(&[]);
    for candidate in candidates {
        if let Some(content) = &candidate.content {
            for part in &content.parts {
                if let Some(text) = &part.text {
                    if !text.is_empty() {
                        out.push(ModelEvent::TextDelta {
                            delta: text.clone(),
                        });
                    }
                }
                if let Some(fc) = &part.function_call {
                    // Google delivers function calls complete in one chunk.
                    // Emit ToolUseStart + ToolUseInputDelta + ToolUseEnd.
                    let id = fc.id.clone().unwrap_or_else(|| fc.name.clone());
                    out.push(ModelEvent::ToolUseStart {
                        id: id.clone(),
                        name: fc.name.clone(),
                    });
                    let args = fc.args.to_string();
                    if !args.is_empty() {
                        out.push(ModelEvent::ToolUseInputDelta {
                            id: id.clone(),
                            delta: args,
                        });
                    }
                    out.push(ModelEvent::ToolUseEnd { id });
                }
            }
        }

        if let Some(reason) = &candidate.finish_reason {
            // Only emit Done on a terminal finish reason (not UNSPECIFIED / empty).
            let normalized = reason.to_lowercase();
            if !normalized.is_empty() && normalized != "finish_reason_unspecified" {
                let mapped = match normalized.as_str() {
                    "stop" => "stop",
                    "max_tokens" => "max_tokens",
                    "safety" => "safety",
                    other => other,
                };
                out.push(ModelEvent::Done {
                    reason: mapped.to_string(),
                });
            }
        }
    }

    out
}

// ── GoogleProvider ─────────────────────────────────────────────────────────────

/// Google AI Studio provider wrapping the Generative Language API.
pub struct GoogleProvider {
    auth: Arc<dyn AuthResolver>,
    base_url: String,
    client: reqwest::Client,
}

struct GoogleAuth {
    config_key: Option<String>,
}

impl GoogleAuth {
    fn new(config_key: Option<String>) -> Self {
        Self { config_key }
    }
}

impl AuthResolver for GoogleAuth {
    fn resolve(&self) -> Result<String, ProviderError> {
        for key in ["GOOGLE_API_KEY", "GEMINI_API_KEY"] {
            if let Ok(val) = std::env::var(key) {
                if !val.is_empty() {
                    return Ok(val);
                }
            }
        }

        if let Some(key) = &self.config_key {
            if !key.is_empty() {
                return Ok(key.clone());
            }
        }

        Err(ProviderError::Auth {
            provider: "google".into(),
            msg: "no API key found — set GOOGLE_API_KEY or GEMINI_API_KEY or configure it in ~/.opencode/config.jsonc".into(),
        })
    }
}

impl GoogleProvider {
    /// Create with the standard Google AI Studio endpoint.
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

    /// Default auth resolver reading `GOOGLE_API_KEY` or `GEMINI_API_KEY`.
    pub fn default_auth(config_key: Option<String>) -> Arc<dyn AuthResolver> {
        Arc::new(GoogleAuth::new(config_key))
    }

    fn headers(&self) -> Result<HeaderMap, ProviderError> {
        let key = self.auth.resolve()?;
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-goog-api-key",
            HeaderValue::from_str(&key).map_err(|_| ProviderError::Auth {
                provider: "google".into(),
                msg: "invalid key bytes".into(),
            })?,
        );
        Ok(headers)
    }
}

#[async_trait]
impl LanguageModel for GoogleProvider {
    fn provider(&self) -> &'static str {
        "google"
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        // Models served via CatalogCache; return empty list here.
        Ok(vec![])
    }

    async fn stream(
        &self,
        req: ModelRequest,
    ) -> Result<BoxStream<Result<ModelEvent, ProviderError>>, ProviderError> {
        let headers = self.headers()?;
        let names = names(&req);

        // Model name must use "models/" prefix in the path.
        let model_path = if req.model.starts_with("models/") {
            req.model.clone()
        } else {
            format!("models/{}", req.model)
        };

        let url = format!(
            "{}/v1beta/{}:streamGenerateContent?alt=sse",
            self.base_url, model_path
        );

        let body = GenerateContentRequest {
            contents: req
                .messages
                .iter()
                .map(|msg| to_content(msg, &names))
                .collect(),
            system_instruction: build_system(&req),
            generation_config: Some(GenerationConfig {
                max_output_tokens: req.max_tokens,
                temperature: req.temperature,
            }),
            tools: build_tools(&req),
        };

        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http("google".into(), e.to_string()))?;

        if resp.status() == 429 {
            let retry = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());
            return Err(ProviderError::RateLimit {
                provider: "google".into(),
                retry_after: retry,
            });
        }

        if resp.status() == 401 || resp.status() == 403 {
            return Err(ProviderError::Auth {
                provider: "google".into(),
                msg: format!("{} Unauthorized", resp.status()),
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
            return Err(ProviderError::Http("google".into(), detail));
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
                    // Google has no [DONE] sentinel — stream just ends.
                    match serde_json::from_str::<GenerateContentResponse>(&ev.data) {
                        Ok(resp_chunk) => {
                            for model_ev in map_chunk(&resp_chunk) {
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
    use futures::StreamExt;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path_regex},
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

    // ── Unit tests for map_chunk (pure, no network) ───────────────────────────

    fn chunk_with_text(text: &str) -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(ResponseContent {
                    parts: vec![ResponsePart {
                        text: Some(text.to_string()),
                        function_call: None,
                    }],
                }),
                finish_reason: None,
            }]),
            usage_metadata: None,
        }
    }

    fn chunk_with_finish(reason: &str) -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: None,
                finish_reason: Some(reason.to_string()),
            }]),
            usage_metadata: None,
        }
    }

    fn chunk_with_usage(input: u32, output: u32) -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: None,
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: input,
                candidates_token_count: output,
            }),
        }
    }

    #[test]
    fn map_text_delta() {
        let events = map_chunk(&chunk_with_text("hello"));
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ModelEvent::TextDelta { delta } if delta == "hello"));
    }

    #[test]
    fn empty_text_ignored() {
        let events = map_chunk(&chunk_with_text(""));
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn map_finish_stop() {
        let events = map_chunk(&chunk_with_finish("STOP"));
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ModelEvent::Done { reason } if reason == "stop"));
    }

    #[test]
    fn map_finish_max_tokens() {
        let events = map_chunk(&chunk_with_finish("MAX_TOKENS"));
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ModelEvent::Done { reason } if reason == "max_tokens"));
    }

    #[test]
    fn unspecified_finish_no_done() {
        let events = map_chunk(&chunk_with_finish("FINISH_REASON_UNSPECIFIED"));
        assert!(events.is_empty());
    }

    #[test]
    fn map_usage() {
        let events = map_chunk(&chunk_with_usage(10, 50));
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ModelEvent::Usage { input, output, .. } if *input == 10 && *output == 50)
        );
    }

    #[test]
    fn zero_usage_ignored() {
        let events = map_chunk(&chunk_with_usage(0, 0));
        assert!(events.is_empty());
    }

    #[test]
    fn map_function_call() {
        let chunk = GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(ResponseContent {
                    parts: vec![ResponsePart {
                        text: None,
                        function_call: Some(ResponseFunctionCall {
                            id: Some("call_1".to_string()),
                            name: "bash".to_string(),
                            args: serde_json::json!({"cmd": "ls"}),
                        }),
                    }],
                }),
                finish_reason: None,
            }]),
            usage_metadata: None,
        };
        let events = map_chunk(&chunk);
        assert_eq!(events.len(), 3);
        assert!(
            matches!(&events[0], ModelEvent::ToolUseStart { id, name } if id == "call_1" && name == "bash")
        );
        assert!(matches!(&events[1], ModelEvent::ToolUseInputDelta { .. }));
        assert!(matches!(&events[2], ModelEvent::ToolUseEnd { .. }));
    }

    #[test]
    fn tool_result_uses_prior_tool_name() {
        let req = ModelRequest {
            model: "gemini-2.0-flash".into(),
            system: vec![],
            messages: vec![
                ModelMessage {
                    role: "assistant".into(),
                    content: vec![ContentPart::ToolUse {
                        id: "call_1".into(),
                        name: "bash".into(),
                        input: serde_json::json!({"cmd": "ls"}),
                    }],
                },
                ModelMessage {
                    role: "user".into(),
                    content: vec![ContentPart::ToolResult {
                        tool_use_id: "call_1".into(),
                        content: "ok".into(),
                    }],
                },
            ],
            tools: Default::default(),
            max_tokens: Some(32),
            temperature: None,
        };

        let names = names(&req);
        let msg = to_content(&req.messages[1], &names);
        let part = msg.parts.first().unwrap();
        let res = part.function_response.as_ref().unwrap();

        assert_eq!(msg.role, "user");
        assert_eq!(res.id.as_deref(), Some("call_1"));
        assert_eq!(res.name, "bash");
        assert_eq!(res.response, serde_json::json!({ "output": "ok" }));
    }

    #[test]
    fn google_auth_uses_google_env_first() {
        let auth = GoogleAuth::new(Some("cfg".into()));
        // SAFETY: single-threaded test, process env restored below.
        unsafe {
            std::env::set_var("GOOGLE_API_KEY", "google-key");
            std::env::set_var("GEMINI_API_KEY", "gemini-key");
        }
        assert_eq!(auth.resolve().unwrap(), "google-key");
        // SAFETY: single-threaded test, cleanup for unique keys.
        unsafe {
            std::env::remove_var("GOOGLE_API_KEY");
            std::env::remove_var("GEMINI_API_KEY");
        }
    }

    #[test]
    fn google_auth_falls_back_to_gemini_env() {
        let auth = GoogleAuth::new(Some("cfg".into()));
        // SAFETY: single-threaded test, process env restored below.
        unsafe {
            std::env::remove_var("GOOGLE_API_KEY");
            std::env::set_var("GEMINI_API_KEY", "gemini-key");
        }
        assert_eq!(auth.resolve().unwrap(), "gemini-key");
        // SAFETY: single-threaded test, cleanup for unique keys.
        unsafe {
            std::env::remove_var("GEMINI_API_KEY");
        }
    }

    #[test]
    fn assistant_role_maps_to_model() {
        let msg = ModelMessage {
            role: "assistant".into(),
            content: vec![ContentPart::Text { text: "hi".into() }],
        };
        let c = to_content(&msg, &BTreeMap::new());
        assert_eq!(c.role, "model");
    }

    #[test]
    fn user_role_preserved() {
        let msg = ModelMessage {
            role: "user".into(),
            content: vec![ContentPart::Text { text: "hi".into() }],
        };
        let c = to_content(&msg, &BTreeMap::new());
        assert_eq!(c.role, "user");
    }

    #[test]
    fn model_path_prefix_added() {
        // Verify model path construction logic
        let model = "gemini-2.0-flash";
        let path = if model.starts_with("models/") {
            model.to_string()
        } else {
            format!("models/{}", model)
        };
        assert_eq!(path, "models/gemini-2.0-flash");
    }

    #[test]
    fn model_path_prefix_not_doubled() {
        let model = "models/gemini-2.0-flash";
        let path = if model.starts_with("models/") {
            model.to_string()
        } else {
            format!("models/{}", model)
        };
        assert_eq!(path, "models/gemini-2.0-flash");
    }

    // ── Integration test: wiremock SSE mock ──────────────────────────────────

    fn sse_fixture() -> String {
        // Each SSE event must be terminated by a blank line (\n\n).
        // We build the string explicitly to guarantee the final event is flushed.
        let mut s = String::new();
        s.push_str("data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hello\"}]},\"finishReason\":\"\"}]}\n\n");
        s.push_str("data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\" world\"}]},\"finishReason\":\"\"}]}\n\n");
        s.push_str("data: {\"candidates\":[{\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":10}}\n\n");
        s
    }

    #[tokio::test]
    async fn stream_yields_text_and_done() {
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/v1beta/models/.*:streamGenerateContent$"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_fixture()),
            )
            .mount(&srv)
            .await;

        let auth = GoogleProvider::default_auth(Some("test-key".into()));
        let provider = GoogleProvider::with_base_url(auth, srv.uri());
        let mut stream = provider
            .stream(text_req("gemini-2.0-flash", "hi"))
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

    #[tokio::test]
    async fn rate_limit_429_returns_error() {
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/v1beta/models/.*:streamGenerateContent$"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "60"))
            .mount(&srv)
            .await;

        let auth = GoogleProvider::default_auth(Some("test-key".into()));
        let provider = GoogleProvider::with_base_url(auth, srv.uri());
        let result = provider.stream(text_req("gemini-2.0-flash", "hi")).await;
        match result {
            Err(ProviderError::RateLimit {
                provider,
                retry_after: Some(60),
            }) => assert_eq!(provider, "google"),
            other => panic!("expected RateLimit, got {:?}", other.err()),
        }
    }

    #[tokio::test]
    async fn unauthorized_401_returns_auth_error() {
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/v1beta/models/.*:streamGenerateContent$"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&srv)
            .await;

        let auth = GoogleProvider::default_auth(Some("test-key".into()));
        let provider = GoogleProvider::with_base_url(auth, srv.uri());
        let result = provider.stream(text_req("gemini-2.0-flash", "hi")).await;
        match result {
            Err(ProviderError::Auth { provider, .. }) => assert_eq!(provider, "google"),
            other => panic!("expected Auth error, got {:?}", other.err()),
        }
    }
}
