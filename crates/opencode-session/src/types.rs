//! Session engine types: requests, handles, frames, and state.

use opencode_core::id::{MessageId, SessionId};
use serde::{Deserialize, Serialize};

/// A request to begin a new prompt turn in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPrompt {
    /// Session to prompt into.
    pub session_id: SessionId,
    /// User message text.
    pub text: String,
    /// Optional model override for this turn.
    #[serde(default)]
    pub model: Option<String>,
    /// Run in plan-only mode (no write tools).
    #[serde(default)]
    pub plan_mode: bool,
}

/// An opaque handle returned when a prompt is submitted.
///
/// Callers can use the `session_id` to subscribe to bus events for streaming
/// updates.
#[derive(Debug, Clone)]
pub struct SessionHandle {
    /// The session this handle belongs to.
    pub session_id: SessionId,
    /// Assistant message id allocated for this turn, when available.
    pub assistant_message_id: Option<MessageId>,
    /// Optional model actually selected for this turn.
    pub resolved_model: Option<String>,
}

impl SessionHandle {
    /// Create a new handle with empty metadata.
    pub fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            assistant_message_id: None,
            resolved_model: None,
        }
    }

    /// Attach assistant message id metadata.
    pub fn with_assistant_message_id(mut self, message_id: MessageId) -> Self {
        self.assistant_message_id = Some(message_id);
        self
    }

    /// Attach resolved model metadata.
    pub fn with_resolved_model(mut self, model: impl Into<String>) -> Self {
        self.resolved_model = Some(model.into());
        self
    }
}

/// A streamed frame of output from the agent loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SessionFrame {
    /// A text delta from the LLM.
    TextDelta {
        /// The appended text.
        delta: String,
    },
    /// A tool call was dispatched.
    ToolCall {
        /// Tool name.
        tool: String,
        /// Correlation id.
        call_id: String,
    },
    /// A tool call completed.
    ToolResult {
        /// Correlation id.
        call_id: String,
        /// Whether it succeeded.
        ok: bool,
    },
    /// The turn is complete.
    Done {
        /// Total input tokens used in this turn.
        input_tokens: u32,
        /// Total output tokens generated in this turn.
        output_tokens: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_handle_can_be_created_without_metadata() {
        let sid = SessionId::new();
        let handle = SessionHandle::new(sid);

        assert_eq!(handle.session_id, sid);
        assert!(handle.assistant_message_id.is_none());
        assert!(handle.resolved_model.is_none());
    }

    #[test]
    fn session_handle_supports_runtime_metadata() {
        let sid = SessionId::new();
        let mid = MessageId::new();

        let handle = SessionHandle::new(sid)
            .with_assistant_message_id(mid)
            .with_resolved_model("gpt-4o-mini");

        assert_eq!(handle.session_id, sid);
        assert_eq!(handle.assistant_message_id, Some(mid));
        assert_eq!(handle.resolved_model.as_deref(), Some("gpt-4o-mini"));
    }
}
