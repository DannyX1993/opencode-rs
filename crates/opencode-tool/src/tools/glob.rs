//! GlobTool — find files matching a glob pattern using native traversal.
//!
//! [IMPROVEMENT] Uses `globset` + `walkdir` instead of shelling out to `rg`,
//! removing the runtime binary dependency while preserving identical output semantics.

use crate::common::Ctx;
use crate::types::{Tool, ToolCall, ToolError, ToolResult};
use async_trait::async_trait;
use globset::GlobBuilder;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use walkdir::WalkDir;

const LIMIT: usize = 100;

/// Finds files matching a glob pattern, sorted by modification time (newest first).
pub struct GlobTool {
    /// Shared execution context.
    pub ctx: Arc<Ctx>,
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &'static str {
        "glob"
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let pattern = call.args["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "glob".into(),
                msg: "pattern required".into(),
            })?;

        let search: PathBuf = match call.args["path"].as_str() {
            Some(p) => {
                let pb = Path::new(p);
                if pb.is_absolute() {
                    pb.to_path_buf()
                } else {
                    self.ctx.cwd.join(p)
                }
            }
            None => self.ctx.cwd.clone(),
        };

        let glob = GlobBuilder::new(pattern)
            .case_insensitive(false)
            .build()
            .map_err(|e| ToolError::InvalidArgs {
                tool: "glob".into(),
                msg: e.to_string(),
            })?
            .compile_matcher();

        let mut matches: Vec<(PathBuf, SystemTime)> = Vec::new();
        let mut truncated = false;

        for entry in WalkDir::new(&search)
            .follow_links(true)
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path().to_path_buf();
            let rel = path.strip_prefix(&search).unwrap_or(&path);
            if glob.is_match(rel) || glob.is_match(&path) {
                if matches.len() >= LIMIT {
                    truncated = true;
                    break;
                }
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                matches.push((path.canonicalize().unwrap_or(path), mtime));
            }
        }

        matches.sort_by(|a, b| b.1.cmp(&a.1));

        let title = search
            .strip_prefix(&self.ctx.root)
            .unwrap_or(&search)
            .to_string_lossy()
            .into_owned();

        let mut lines: Vec<String> = matches
            .iter()
            .map(|(p, _)| p.display().to_string())
            .collect();
        if lines.is_empty() {
            lines.push("No files found".into());
        } else if truncated {
            lines.push(String::new());
            lines.push(format!(
                "(Results are truncated: showing first {} results. Consider using a more specific path or pattern.)",
                LIMIT
            ));
        }

        Ok(ToolResult {
            call_id: call.id,
            is_err: false,
            output: lines.join("\n"),
            title,
            metadata: serde_json::json!({ "count": matches.len(), "truncated": truncated }),
            output_path: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
            name: "glob".into(),
            args,
        }
    }

    #[tokio::test]
    async fn matching_files_returned() {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("a.rs"), "").unwrap();
        fs::write(d.path().join("b.rs"), "").unwrap();
        fs::write(d.path().join("c.txt"), "").unwrap();
        let tool = GlobTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                serde_json::json!({"pattern": "*.rs", "path": d.path().to_str().unwrap()}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("a.rs"));
        assert!(res.output.contains("b.rs"));
        assert!(!res.output.contains("c.txt"));
    }

    #[tokio::test]
    async fn no_match_returns_no_files_found() {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("hello.txt"), "").unwrap();
        let tool = GlobTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                serde_json::json!({"pattern": "*.rs", "path": d.path().to_str().unwrap()}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert_eq!(res.output, "No files found");
    }

    #[tokio::test]
    async fn cap_at_100() {
        let d = TempDir::new().unwrap();
        for i in 0..120 {
            fs::write(d.path().join(format!("f{:03}.rs", i)), "").unwrap();
        }
        let tool = GlobTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                serde_json::json!({"pattern": "*.rs", "path": d.path().to_str().unwrap()}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert_eq!(res.metadata["truncated"], true);
        assert!(res.output.contains("truncated"));
    }

    #[tokio::test]
    async fn missing_pattern_invalid_args() {
        let d = TempDir::new().unwrap();
        let tool = GlobTool { ctx: ctx(&d) };
        let err = tool.invoke(call(serde_json::json!({}))).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs { .. }));
    }
}
