//! ReadTool — read file contents or list directory entries.

use crate::common::Ctx;
use crate::common::fs::{self, MAX_BYTES};
use crate::types::{Tool, ToolCall, ToolError, ToolResult};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

/// Reads a file or lists a directory, with optional offset and line limit.
pub struct ReadTool {
    /// Shared execution context.
    pub ctx: Arc<Ctx>,
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &'static str {
        "read"
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let file_path: String = call.args["filePath"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "read".into(),
                msg: "filePath required".into(),
            })?
            .to_string();

        let offset = call.args["offset"].as_u64().unwrap_or(1) as usize;
        let limit = call.args["limit"].as_u64().unwrap_or(2000) as usize;

        if offset < 1 {
            return Err(ToolError::InvalidArgs {
                tool: "read".into(),
                msg: "offset must be >= 1".into(),
            });
        }

        let path: PathBuf = if std::path::Path::new(&file_path).is_absolute() {
            file_path.clone().into()
        } else {
            self.ctx.cwd.join(&file_path)
        };

        if !path.exists() {
            return Err(ToolError::NotFound(file_path));
        }

        let title = path
            .strip_prefix(&self.ctx.root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();

        // Directory listing
        if path.is_dir() {
            return read_dir(&path, offset, limit, &call.id, &title);
        }

        // Binary check
        let binary = fs::is_binary(&path).map_err(|e| ToolError::Exec {
            tool: "read".into(),
            msg: e.to_string(),
        })?;
        if binary {
            return Err(ToolError::BinaryFile(file_path));
        }

        // Read lines
        let file = fs::read_lines(&path, offset, limit).map_err(|e| ToolError::Exec {
            tool: "read".into(),
            msg: e.to_string(),
        })?;

        // Offset out of range check
        if file.count < offset.saturating_sub(1) && !(file.count == 0 && offset == 1) {
            return Err(ToolError::OffsetOutOfRange {
                offset,
                count: file.count,
            });
        }

        let last = offset + file.raw.len().saturating_sub(1);
        let next = last + 1;
        let truncated = file.more || file.cut;

        let mut output = format!(
            "<path>{}</path>\n<type>file</type>\n<content>\n",
            path.display()
        );
        output.push_str(
            &file
                .raw
                .iter()
                .enumerate()
                .map(|(i, l)| format!("{}: {}", i + offset, l))
                .collect::<Vec<_>>()
                .join("\n"),
        );

        if file.cut {
            output.push_str(&format!(
                "\n\n(Output capped at {} KB. Showing lines {}-{}. Use offset={} to continue.)",
                MAX_BYTES / 1024,
                offset,
                last,
                next
            ));
        } else if file.more {
            output.push_str(&format!(
                "\n\n(Showing lines {}-{} of {}. Use offset={} to continue.)",
                offset, last, file.count, next
            ));
        } else {
            output.push_str(&format!("\n\n(End of file - total {} lines)", file.count));
        }
        output.push_str("\n</content>");

        let meta = serde_json::json!({ "truncated": truncated });

        Ok(ToolResult {
            call_id: call.id,
            is_err: false,
            output,
            title,
            metadata: meta,
            output_path: None,
        })
    }
}

fn read_dir(
    path: &std::path::Path,
    offset: usize,
    limit: usize,
    id: &str,
    title: &str,
) -> Result<ToolResult, ToolError> {
    let mut entries: Vec<String> = std::fs::read_dir(path)
        .map_err(|e| ToolError::Exec {
            tool: "read".into(),
            msg: e.to_string(),
        })?
        .filter_map(|e| e.ok())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() {
                format!("{}/", name)
            } else {
                name
            }
        })
        .collect();
    entries.sort();

    let start = offset.saturating_sub(1);
    let sliced: Vec<String> = entries.iter().skip(start).take(limit).cloned().collect();
    let truncated = start + sliced.len() < entries.len();

    let footer = if truncated {
        format!(
            "\n(Showing {} of {} entries. Use 'offset' parameter to read beyond entry {})",
            sliced.len(),
            entries.len(),
            offset + sliced.len()
        )
    } else {
        format!("\n({} entries)", entries.len())
    };

    let output = format!(
        "<path>{}</path>\n<type>directory</type>\n<entries>\n{}{}\n</entries>",
        path.display(),
        sliced.join("\n"),
        footer,
    );

    Ok(ToolResult {
        call_id: id.to_string(),
        is_err: false,
        output,
        title: title.to_string(),
        metadata: serde_json::json!({ "truncated": truncated }),
        output_path: None,
    })
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

    fn call(id: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: "read".into(),
            args,
        }
    }

    #[tokio::test]
    async fn read_text_file() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("hello.txt");
        fs::write(&p, "line1\nline2\nline3\n").unwrap();
        let tool = ReadTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                "1",
                serde_json::json!({"filePath": p.to_str().unwrap()}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("1: line1"));
        assert!(res.output.contains("End of file - total 3 lines"));
    }

    #[tokio::test]
    async fn read_directory() {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("a.txt"), "a").unwrap();
        fs::write(d.path().join("b.txt"), "b").unwrap();
        let tool = ReadTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                "1",
                serde_json::json!({"filePath": d.path().to_str().unwrap()}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("directory"));
        assert!(res.output.contains("a.txt"));
        assert!(res.output.contains("b.txt"));
    }

    #[tokio::test]
    async fn missing_file_returns_not_found() {
        let d = TempDir::new().unwrap();
        let tool = ReadTool { ctx: ctx(&d) };
        let err = tool
            .invoke(call(
                "1",
                serde_json::json!({"filePath": d.path().join("nope.txt").to_str().unwrap()}),
            ))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn binary_file_rejected() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("file.bin");
        fs::write(&p, b"hello\x00world").unwrap();
        let tool = ReadTool { ctx: ctx(&d) };
        let err = tool
            .invoke(call(
                "1",
                serde_json::json!({"filePath": p.to_str().unwrap()}),
            ))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::BinaryFile(_)));
    }

    #[tokio::test]
    async fn offset_with_limit() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("multi.txt");
        fs::write(&p, "a\nb\nc\nd\ne\n").unwrap();
        let tool = ReadTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                "1",
                serde_json::json!({"filePath": p.to_str().unwrap(), "offset": 3, "limit": 2}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("3: c"));
        assert!(res.output.contains("4: d"));
        assert!(!res.output.contains("1: a"));
    }

    #[tokio::test]
    async fn relative_path_resolved_via_cwd() {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("rel.txt"), "hello\n").unwrap();
        let tool = ReadTool { ctx: ctx(&d) };
        // Pass relative path — should resolve against cwd
        let res = tool
            .invoke(call("1", serde_json::json!({"filePath": "rel.txt"})))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("hello"));
    }

    #[tokio::test]
    async fn missing_filepath_arg_invalid() {
        let d = TempDir::new().unwrap();
        let tool = ReadTool { ctx: ctx(&d) };
        let err = tool
            .invoke(call("1", serde_json::json!({})))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs { .. }));
    }

    #[tokio::test]
    async fn directory_truncated_when_many_entries() {
        let d = TempDir::new().unwrap();
        // Create enough entries to trigger truncation with limit=2
        for c in ['a', 'b', 'c', 'd', 'e'] {
            fs::write(d.path().join(format!("{}.txt", c)), "").unwrap();
        }
        let tool = ReadTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                "1",
                serde_json::json!({"filePath": d.path().to_str().unwrap(), "limit": 2}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        // With limit=2 and 5 entries, should show truncation message
        assert!(res.output.contains("Showing") || res.output.contains("entries"));
        assert_eq!(res.metadata["truncated"], true);
    }

    #[tokio::test]
    async fn large_file_shows_more_message() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("big.txt");
        // Write 2005 lines so limit=2000 triggers "more"
        let content: String = (1..=2005).map(|i| format!("line{}\n", i)).collect();
        fs::write(&p, content).unwrap();
        let tool = ReadTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(
                "1",
                serde_json::json!({"filePath": p.to_str().unwrap()}),
            ))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("Showing lines"));
        assert!(res.output.contains("to continue"));
    }
}
