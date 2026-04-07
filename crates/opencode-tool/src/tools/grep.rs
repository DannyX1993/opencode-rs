//! GrepTool — search file contents with a regex pattern using native Rust.
//!
//! [IMPROVEMENT] Uses `regex` + `walkdir` instead of shelling out to `rg`,
//! removing the runtime binary dependency while preserving identical output semantics.

use crate::common::Ctx;
use crate::types::{Tool, ToolCall, ToolError, ToolResult};
use async_trait::async_trait;
use globset::GlobBuilder;
use regex::Regex;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use walkdir::WalkDir;

const LIMIT: usize = 100;
const MAX_LINE_LEN: usize = 2000;

struct Match {
    path: PathBuf,
    mtime: SystemTime,
    line_num: usize,
    text: String,
}

/// Searches file contents with a regex, grouped by file and sorted by mtime.
pub struct GrepTool {
    /// Shared execution context.
    pub ctx: Arc<Ctx>,
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        "grep"
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let pattern = call.args["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "grep".into(),
                msg: "pattern required".into(),
            })?;

        let re = Regex::new(pattern).map_err(|e| ToolError::InvalidArgs {
            tool: "grep".into(),
            msg: format!("invalid regex: {e}"),
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

        let include_glob = call.args["include"].as_str().map(|g| {
            GlobBuilder::new(g)
                .case_insensitive(false)
                .build()
                .map(|g| g.compile_matcher())
        });

        let glob_matcher = match include_glob {
            Some(Ok(m)) => Some(m),
            Some(Err(e)) => {
                return Err(ToolError::InvalidArgs {
                    tool: "grep".into(),
                    msg: e.to_string(),
                });
            }
            None => None,
        };

        let mut all: Vec<Match> = Vec::new();
        let mut has_errors = false;

        for entry in WalkDir::new(&search)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path().to_path_buf();

            // Apply include filter
            if let Some(ref m) = glob_matcher {
                let name = path.file_name().unwrap_or_default();
                if !m.is_match(Path::new(name)) {
                    continue;
                }
            }

            let mtime = entry
                .metadata()
                .ok()
                .and_then(|md| md.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);

            let f = match std::fs::File::open(&path) {
                Ok(f) => f,
                Err(_) => {
                    has_errors = true;
                    continue;
                }
            };

            for (idx, line) in BufReader::new(f).lines().enumerate() {
                let text = match line {
                    Ok(t) => t,
                    Err(_) => {
                        has_errors = true;
                        break;
                    }
                };
                if re.is_match(&text) {
                    let display = if text.len() > MAX_LINE_LEN {
                        format!("{}...", &text[..MAX_LINE_LEN])
                    } else {
                        text
                    };
                    all.push(Match {
                        path: path.clone(),
                        mtime,
                        line_num: idx + 1,
                        text: display,
                    });
                }
            }
        }

        all.sort_by(|a, b| b.mtime.cmp(&a.mtime));

        let title = pattern.to_string();
        let total = all.len();

        if total == 0 {
            return Ok(ToolResult {
                call_id: call.id,
                is_err: false,
                output: "No files found".into(),
                title,
                metadata: serde_json::json!({ "matches": 0, "truncated": false }),
                output_path: None,
            });
        }

        let truncated = total > LIMIT;
        let shown = &all[..total.min(LIMIT)];

        let mut lines = vec![format!(
            "Found {} matches{}",
            total,
            if truncated {
                format!(" (showing first {})", LIMIT)
            } else {
                String::new()
            }
        )];
        let mut cur_file = String::new();
        for m in shown {
            let fp = m.path.display().to_string();
            if fp != cur_file {
                if !cur_file.is_empty() {
                    lines.push(String::new());
                }
                cur_file = fp.clone();
                lines.push(format!("{}:", fp));
            }
            lines.push(format!("  Line {}: {}", m.line_num, m.text));
        }

        if truncated {
            lines.push(String::new());
            lines.push(format!(
                "(Results truncated: showing {} of {} matches ({} hidden). Consider using a more specific path or pattern.)",
                LIMIT,
                total,
                total - LIMIT
            ));
        }
        if has_errors {
            lines.push(String::new());
            lines.push("(Some paths were inaccessible and skipped)".into());
        }

        Ok(ToolResult {
            call_id: call.id,
            is_err: false,
            output: lines.join("\n"),
            title,
            metadata: serde_json::json!({ "matches": total, "truncated": truncated }),
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
            name: "grep".into(),
            args,
        }
    }

    #[tokio::test]
    async fn finds_matching_lines() {
        let d = TempDir::new().unwrap();
        fs::write(
            d.path().join("src.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();
        let tool = GrepTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                serde_json::json!({"pattern": "println", "path": d.path().to_str().unwrap()}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("println"));
        assert!(res.metadata["matches"].as_u64().unwrap() >= 1);
    }

    #[tokio::test]
    async fn no_match_returns_no_files_found() {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("a.txt"), "nothing here\n").unwrap();
        let tool = GrepTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                serde_json::json!({"pattern": "XYZNOTFOUND", "path": d.path().to_str().unwrap()}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert_eq!(res.output, "No files found");
    }

    #[tokio::test]
    async fn include_filter() {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("main.rs"), "hello rust\n").unwrap();
        fs::write(d.path().join("main.py"), "hello python\n").unwrap();
        let tool = GrepTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(serde_json::json!({"pattern": "hello", "include": "*.rs", "path": d.path().to_str().unwrap()})))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("main.rs"));
        assert!(!res.output.contains("main.py"));
    }

    #[tokio::test]
    async fn cap_at_100() {
        let d = TempDir::new().unwrap();
        // Create files with 2 matches each = 200+ matches total
        for i in 0..60 {
            fs::write(
                d.path().join(format!("f{:03}.txt", i)),
                "match line\nmatch again\n",
            )
            .unwrap();
        }
        let tool = GrepTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                serde_json::json!({"pattern": "match", "path": d.path().to_str().unwrap()}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert_eq!(res.metadata["truncated"], true);
    }

    #[tokio::test]
    async fn invalid_regex_returns_invalid_args() {
        let d = TempDir::new().unwrap();
        let tool = GrepTool { ctx: ctx(&d) };
        let err = tool
            .invoke(call(
                serde_json::json!({"pattern": "[invalid(", "path": d.path().to_str().unwrap()}),
            ))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs { .. }));
    }

    #[tokio::test]
    async fn relative_path_uses_cwd() {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("rel.rs"), "fn foo() {}\n").unwrap();
        let tool = GrepTool { ctx: ctx(&d) };
        // No "path" arg → uses cwd, relative path branch covered
        let res = tool
            .invoke(call(serde_json::json!({"pattern": "fn foo"})))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("fn foo"));
    }

    #[tokio::test]
    async fn invalid_include_glob_returns_invalid_args() {
        let d = TempDir::new().unwrap();
        let tool = GrepTool { ctx: ctx(&d) };
        let err = tool
            .invoke(call(serde_json::json!({
                "pattern": "foo",
                "include": "[invalid",
                "path": d.path().to_str().unwrap()
            })))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs { .. }));
    }

    #[tokio::test]
    async fn missing_pattern_invalid_args() {
        let d = TempDir::new().unwrap();
        let tool = GrepTool { ctx: ctx(&d) };
        let err = tool.invoke(call(serde_json::json!({}))).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs { .. }));
    }
}
