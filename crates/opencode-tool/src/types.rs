//! Tool trait and associated types.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
    pub is_err: bool,
    /// Result output (text or JSON).
    pub output: String,
    /// Short human-readable title (relative path or description).
    pub title: String,
    /// Arbitrary structured metadata.
    pub metadata: serde_json::Value,
    /// Path to the file where truncated output was saved, if any.
    pub output_path: Option<PathBuf>,
}

impl ToolResult {
    /// Construct a successful result.
    pub fn ok(call_id: String, title: String, output: String) -> Self {
        Self {
            call_id,
            is_err: false,
            output,
            title,
            metadata: serde_json::Value::Null,
            output_path: None,
        }
    }

    /// Construct an error result.
    pub fn err(call_id: String, msg: String) -> Self {
        Self {
            call_id,
            is_err: true,
            output: msg,
            title: String::new(),
            metadata: serde_json::Value::Null,
            output_path: None,
        }
    }
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

    /// Offset parameter is out of range for the file.
    #[error("offset {offset} out of range: file has {count} lines")]
    OffsetOutOfRange {
        /// Requested offset.
        offset: usize,
        /// Actual line count.
        count: usize,
    },

    /// File is binary and cannot be read as text.
    #[error("cannot read binary file: {0}")]
    BinaryFile(String),

    /// Feature not supported on the current platform.
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),
}

/// The primary tool abstraction.
///
/// Implementors must be `Send + Sync` and live behind `Arc<dyn Tool>`.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Stable tool name (snake_case, e.g. `"bash"`, `"read"`).
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
    fn tool_result_ok_serialises() {
        let res = ToolResult::ok("abc".into(), "test".into(), "foo".into());
        let json = serde_json::to_value(&res).unwrap();
        assert_eq!(json["is_err"], false);
        assert_eq!(json["output"], "foo");
        assert_eq!(json["title"], "test");
    }

    #[test]
    fn tool_result_err_serialises() {
        let res = ToolResult::err("abc".into(), "something failed".into());
        assert!(res.is_err);
        assert_eq!(res.output, "something failed");
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

        let e = ToolError::OffsetOutOfRange {
            offset: 10,
            count: 3,
        };
        assert!(e.to_string().contains("10"));
        assert!(e.to_string().contains("3"));

        let e = ToolError::BinaryFile("/path/to/file".into());
        assert!(e.to_string().contains("binary"));

        let e = ToolError::UnsupportedPlatform("win32".into());
        assert!(e.to_string().contains("win32"));
    }

    #[test]
    fn tool_policy_default() {
        let p = ToolPolicy::default();
        assert!(p.reads.is_empty());
        assert!(!p.net);
        assert!(!p.exclusive);
    }
}
