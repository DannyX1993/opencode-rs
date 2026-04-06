//! Tool trait and associated types.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Resource policy describing what a tool may access.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolPolicy {
    /// Paths this tool reads.
    pub reads: Vec<String>,
    /// Paths this tool writes.
    pub writes: Vec<String>,
    /// Whether this tool makes network calls.
    pub net: bool,
    /// Whether this tool must run exclusively (no parallel peers).
    pub exclusive: bool,
}

/// A single tool invocation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique correlation id for this call.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// JSON-encoded arguments.
    pub args: serde_json::Value,
}

/// The result of a tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Correlation id matching the [`ToolCall`].
    pub call_id: String,
    /// Whether the invocation succeeded.
    pub ok: bool,
    /// Result content (text or JSON).
    pub content: String,
}

/// Errors produced by tool invocations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ToolError {
    /// The requested resource was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// Permission was denied.
    #[error("permission denied for tool {tool}: {reason}")]
    PermissionDenied {
        /// Tool name.
        tool: String,
        /// Denial reason.
        reason: String,
    },

    /// Input argument validation failed.
    #[error("invalid args for {tool}: {msg}")]
    InvalidArgs {
        /// Tool name.
        tool: String,
        /// Validation message.
        msg: String,
    },

    /// The tool execution timed out.
    #[error("tool {tool} timed out after {seconds}s")]
    Timeout {
        /// Tool name.
        tool: String,
        /// Elapsed seconds.
        seconds: u64,
    },

    /// Generic execution failure.
    #[error("tool {tool} failed: {msg}")]
    Exec {
        /// Tool name.
        tool: String,
        /// Error description.
        msg: String,
    },
}

/// The primary tool abstraction.
///
/// Implementors must be `Send + Sync` and live behind `Arc<dyn Tool>`.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Stable tool name (snake_case, e.g. `"bash"`, `"read_file"`).
    fn name(&self) -> &'static str;

    /// Resource policy — used by the planner to detect conflicts.
    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default()
    }

    /// Execute the tool call.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError`] on any failure.
    async fn invoke(&self, call: ToolCall) -> Result<ToolResult, ToolError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_serialises() {
        let call = ToolCall {
            id: "abc".into(),
            name: "bash".into(),
            args: serde_json::json!({"cmd": "ls"}),
        };
        let json = serde_json::to_value(&call).unwrap();
        assert_eq!(json["name"], "bash");
        assert_eq!(json["args"]["cmd"], "ls");
    }

    #[test]
    fn tool_result_serialises() {
        let res = ToolResult {
            call_id: "abc".into(),
            ok: true,
            content: "foo".into(),
        };
        let json = serde_json::to_value(&res).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["content"], "foo");
    }

    #[test]
    fn tool_error_display() {
        let e = ToolError::NotFound("bash".into());
        assert!(e.to_string().contains("bash"));

        let e = ToolError::PermissionDenied {
            tool: "write".into(),
            reason: "readonly".into(),
        };
        assert!(e.to_string().contains("readonly"));

        let e = ToolError::InvalidArgs {
            tool: "read".into(),
            msg: "bad path".into(),
        };
        assert!(e.to_string().contains("bad path"));

        let e = ToolError::Timeout {
            tool: "run".into(),
            seconds: 30,
        };
        assert!(e.to_string().contains("30"));

        let e = ToolError::Exec {
            tool: "bash".into(),
            msg: "exit 1".into(),
        };
        assert!(e.to_string().contains("exit 1"));
    }

    #[test]
    fn tool_policy_default() {
        let p = ToolPolicy::default();
        assert!(p.reads.is_empty());
        assert!(!p.net);
        assert!(!p.exclusive);
    }
}
