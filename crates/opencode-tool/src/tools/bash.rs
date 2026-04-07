//! BashTool — run shell commands with timeout and truncated output.

use crate::common::Ctx;
use crate::common::shell::run_shell;
use crate::common::truncate::{self, Direction};
use crate::types::{Tool, ToolCall, ToolError, ToolResult};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Executes shell commands using the system shell.
pub struct BashTool {
    /// Shared execution context.
    pub ctx: Arc<Ctx>,
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let cmd = call.args["command"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "bash".into(),
                msg: "command required".into(),
            })?
            .to_string();

        let desc = call.args["description"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "bash".into(),
                msg: "description required".into(),
            })?
            .to_string();

        if desc.trim().is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: "bash".into(),
                msg: "description must not be empty".into(),
            });
        }

        let timeout_ms = match call.args["timeout"].as_i64() {
            Some(t) if t < 0 => {
                return Err(ToolError::InvalidArgs {
                    tool: "bash".into(),
                    msg: format!("Invalid timeout value: {t}. Timeout must be a positive number."),
                });
            }
            Some(t) => t as u64,
            None => self.ctx.timeout,
        };

        let cwd: PathBuf = match call.args["workdir"].as_str() {
            Some(w) => {
                let p = Path::new(w);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    self.ctx.cwd.join(w)
                }
            }
            None => self.ctx.cwd.clone(),
        };

        let out = run_shell(&cmd, &cwd, &self.ctx.shell, timeout_ms, &HashMap::new())
            .await
            .map_err(|e| ToolError::Exec {
                tool: "bash".into(),
                msg: e.to_string(),
            })?;

        let mut output = out.stdout;

        let mut meta_lines: Vec<String> = Vec::new();
        if out.timed_out {
            meta_lines.push(format!(
                "bash tool terminated command after exceeding timeout {} ms",
                timeout_ms
            ));
        }
        if !meta_lines.is_empty() {
            output.push_str("\n\n<bash_metadata>\n");
            output.push_str(&meta_lines.join("\n"));
            output.push_str("\n</bash_metadata>");
        }

        // Apply truncation
        let trunc = truncate::truncate(
            &output,
            truncate::MAX_LINES,
            truncate::MAX_BYTES,
            Direction::Head,
            &self.ctx.out_dir,
            &format!("bash-{}.txt", call.id),
        )
        .map_err(|e| ToolError::Exec {
            tool: "bash".into(),
            msg: e.to_string(),
        })?;

        let exit_code = out.exit_code;

        Ok(ToolResult {
            call_id: call.id,
            is_err: false,
            output: trunc.content,
            title: desc.clone(),
            metadata: serde_json::json!({
                "exit": exit_code,
                "description": desc,
            }),
            output_path: trunc.output_path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ctx(dir: &TempDir) -> Arc<Ctx> {
        Arc::new(Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            dir.path().join("out"),
            "/bin/sh".into(),
            5_000,
        ))
    }

    fn call(args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "1".into(),
            name: "bash".into(),
            args,
        }
    }

    #[tokio::test]
    async fn echo_command() {
        let d = TempDir::new().unwrap();
        let tool = BashTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                serde_json::json!({"command": "echo hello", "description": "Echo test"}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("hello"));
        assert_eq!(res.metadata["exit"], 0);
    }

    #[tokio::test]
    async fn exit_code_capture() {
        let d = TempDir::new().unwrap();
        let tool = BashTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                serde_json::json!({"command": "exit 5", "description": "Exit test"}),
            ))
            .await
            .unwrap();
        assert_eq!(res.metadata["exit"], 5);
    }

    #[tokio::test]
    async fn timeout_adds_metadata() {
        let d = TempDir::new().unwrap();
        let tool = BashTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(serde_json::json!({"command": "sleep 60", "description": "Timeout test", "timeout": 200})))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("bash_metadata"));
        assert!(res.output.contains("terminated"));
    }

    #[tokio::test]
    async fn negative_timeout_invalid() {
        let d = TempDir::new().unwrap();
        let tool = BashTool { ctx: ctx(&d) };
        let err = tool
            .invoke(call(
                serde_json::json!({"command": "echo x", "description": "bad", "timeout": -1}),
            ))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs { .. }));
    }

    #[tokio::test]
    async fn empty_description_invalid() {
        let d = TempDir::new().unwrap();
        let tool = BashTool { ctx: ctx(&d) };
        let err = tool
            .invoke(call(
                serde_json::json!({"command": "echo x", "description": "   "}),
            ))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs { .. }));
    }

    #[tokio::test]
    async fn custom_timeout_positive() {
        let d = TempDir::new().unwrap();
        let tool = BashTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(serde_json::json!({"command": "echo done", "description": "Custom timeout", "timeout": 10_000})))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("done"));
    }

    #[tokio::test]
    async fn workdir_absolute() {
        let d = TempDir::new().unwrap();
        let tool = BashTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(serde_json::json!({
                "command": "pwd",
                "description": "Check workdir",
                "workdir": d.path().to_str().unwrap()
            })))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert_eq!(res.metadata["exit"], 0);
    }

    #[tokio::test]
    async fn missing_command_invalid() {
        let d = TempDir::new().unwrap();
        let tool = BashTool { ctx: ctx(&d) };
        let err = tool
            .invoke(call(serde_json::json!({"description": "no cmd"})))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs { .. }));
    }
}
