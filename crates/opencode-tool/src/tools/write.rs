//! WriteTool — write content to a file, creating parent directories as needed.

use crate::common::Ctx;
use crate::types::{Tool, ToolCall, ToolError, ToolResult};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Creates or overwrites a file with the given content.
pub struct WriteTool {
    /// Shared execution context.
    pub ctx: Arc<Ctx>,
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &'static str {
        "write"
    }

    fn description(&self) -> &'static str {
        "Create or overwrite a file with provided text content."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "filePath": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["filePath", "content"],
            "additionalProperties": false
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let content = call.args["content"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "write".into(),
                msg: "content required".into(),
            })?
            .to_string();

        let file_path = call.args["filePath"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "write".into(),
                msg: "filePath required".into(),
            })?;

        let path: PathBuf = if Path::new(file_path).is_absolute() {
            file_path.into()
        } else {
            self.ctx.cwd.join(file_path)
        };

        let existed = path.exists();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ToolError::Exec {
                tool: "write".into(),
                msg: e.to_string(),
            })?;
        }

        std::fs::write(&path, &content).map_err(|e| ToolError::Exec {
            tool: "write".into(),
            msg: e.to_string(),
        })?;

        let title = path
            .strip_prefix(&self.ctx.root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();

        Ok(ToolResult {
            call_id: call.id,
            is_err: false,
            output: "Wrote file successfully.".into(),
            title,
            metadata: serde_json::json!({ "filepath": path.display().to_string(), "existed": existed }),
            output_path: None,
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
            name: "write".into(),
            args,
        }
    }

    #[tokio::test]
    async fn create_new_file() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("new.txt");
        let tool = WriteTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                serde_json::json!({"content": "hello", "filePath": p.to_str().unwrap()}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert_eq!(res.output, "Wrote file successfully.");
        assert_eq!(res.metadata["existed"], false);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "hello");
    }

    #[tokio::test]
    async fn overwrite_existing_file() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("existing.txt");
        std::fs::write(&p, "old content").unwrap();
        let tool = WriteTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                serde_json::json!({"content": "new content", "filePath": p.to_str().unwrap()}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert_eq!(res.metadata["existed"], true);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "new content");
    }

    #[tokio::test]
    async fn relative_path_resolved_via_cwd() {
        let d = TempDir::new().unwrap();
        let tool = WriteTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                serde_json::json!({"content": "data", "filePath": "relative.txt"}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(d.path().join("relative.txt").exists());
    }

    #[tokio::test]
    async fn missing_content_arg_invalid() {
        let d = TempDir::new().unwrap();
        let tool = WriteTool { ctx: ctx(&d) };
        let err = tool
            .invoke(call(serde_json::json!({"filePath": "/tmp/f.txt"})))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs { .. }));
    }

    #[tokio::test]
    async fn missing_filepath_arg_invalid() {
        let d = TempDir::new().unwrap();
        let tool = WriteTool { ctx: ctx(&d) };
        let err = tool
            .invoke(call(serde_json::json!({"content": "x"})))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs { .. }));
    }
}
