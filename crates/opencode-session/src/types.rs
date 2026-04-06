//! Session engine types: requests, handles, frames, and state.

use opencode_core::id::SessionId;
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
