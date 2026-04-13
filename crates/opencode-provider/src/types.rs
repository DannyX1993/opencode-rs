//! Core provider trait and streaming types.

use async_trait::async_trait;
use opencode_core::context::BoxStream;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::error::ProviderError;

/// Metadata about a single LLM model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Fully qualified model id (e.g. `"anthropic/claude-opus-4-5"`).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Context window in tokens.
    pub context_window: u32,
    /// Maximum output tokens.
    pub max_output: u32,
    /// Supports vision/image inputs.
    #[serde(default)]
    pub vision: bool,
}

/// A single LLM request payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequest {
    /// Target model id (e.g. `"claude-3-haiku-20240307"`).
    pub model: String,
    /// System prompt.
    pub system: Vec<ModelMessage>,
    /// Conversation history + current user turn.
    pub messages: Vec<ModelMessage>,
    /// Tools available to the model.
    #[serde(default)]
    pub tools: BTreeMap<String, serde_json::Value>,
    /// Maximum tokens to generate.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Temperature (0.0–1.0).
    #[serde(default)]
    pub temperature: Option<f32>,
}

/// A single conversation message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMessage {
    /// Role: "system" | "user" | "assistant" | "tool".
    pub role: String,
    /// Message content parts.
    pub content: Vec<ContentPart>,
}

/// A content part within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    /// Plain text.
    Text {
        /// Text content.
        text: String,
    },
    /// Image (URL or base64).
    Image {
        /// MIME type.
        mime: String,
        /// Data (URL or base64).
        data: String,
    },
    /// Tool-use request from the model.
    ToolUse {
        /// Tool call correlation id.
        id: String,
        /// Tool name.
        name: String,
        /// Arguments JSON.
        input: serde_json::Value,
        /// Google/Gemini replay metadata for tool calls.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
    },
    /// Tool result provided back to the model.
    ToolResult {
        /// Correlation id matching the `ToolUse`.
        tool_use_id: String,
        /// Result content.
        content: String,
    },
}

/// A streamed event from the provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ModelEvent {
    /// A text delta.
    TextDelta {
        /// The appended text.
        delta: String,
    },
    /// A tool-use block started.
    ToolUseStart {
        /// Call id.
        id: String,
        /// Tool name.
        name: String,
        /// Provider-specific replay metadata (Gemini thought signature).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
    },
    /// Input JSON accumulation for a tool-use.
    ToolUseInputDelta {
        /// Call id.
        id: String,
        /// Partial JSON string.
        delta: String,
    },
    /// Tool-use block complete.
    ToolUseEnd {
        /// Call id.
        id: String,
    },
    /// Token usage for this turn.
    Usage {
        /// Input tokens.
        input: u32,
        /// Output tokens.
        output: u32,
        /// Cache read tokens (Anthropic).
        #[serde(default)]
        cache_read: u32,
        /// Cache write tokens (Anthropic).
        #[serde(default)]
        cache_write: u32,
    },
    /// The model has finished generating.
    Done {
        /// Stop reason.
        reason: String,
    },
}

/// The primary language model abstraction.
///
/// Implementors must be `Send + Sync` and live behind `Arc<dyn LanguageModel>`.
#[async_trait]
pub trait LanguageModel: Send + Sync {
    /// Provider slug (e.g. `"anthropic"`).
    fn provider(&self) -> &'static str;

    /// Fetch available models.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] on auth or network failure.
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError>;

    /// Stream a model response.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] if the request cannot be initiated.
    async fn stream(
        &self,
        req: ModelRequest,
    ) -> Result<BoxStream<Result<ModelEvent, ProviderError>>, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(model: &str) -> ModelRequest {
        ModelRequest {
            model: model.into(),
            system: vec![],
            messages: vec![],
            tools: Default::default(),
            max_tokens: None,
            temperature: None,
        }
    }

    #[test]
    fn model_request_has_model_field() {
        let r = req("claude-3-haiku-20240307");
        assert_eq!(r.model, "claude-3-haiku-20240307");
    }

    #[test]
    fn model_request_roundtrips_json() {
        let r = req("gpt-4o");
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"model\":\"gpt-4o\""));
        let decoded: ModelRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.model, "gpt-4o");
    }

    #[test]
    fn model_request_different_models_distinct() {
        let a = req("claude-3-haiku-20240307");
        let b = req("gpt-4o-mini");
        assert_ne!(a.model, b.model);
    }
}
