//! LsTool — list directory tree respecting ignore patterns.

use crate::common::Ctx;
use crate::types::{Tool, ToolCall, ToolError, ToolResult};
use async_trait::async_trait;
use ignore::WalkBuilder;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Default ignore patterns (mirrors TypeScript IGNORE_PATTERNS).
pub const IGNORE_PATTERNS: &[&str] = &[
    "node_modules/",
    "__pycache__/",
    ".git/",
    "dist/",
    "build/",
    "target/",
    "vendor/",
    "bin/",
    "obj/",
    ".idea/",
    ".vscode/",
    ".zig-cache/",
    "zig-out",
    ".coverage",
    "coverage/",
    "tmp/",
    "temp/",
    ".cache/",
    "cache/",
    "logs/",
    ".venv/",
    "venv/",
    "env/",
];

const LIMIT: usize = 100;

/// Lists files in a directory as a tree, respecting gitignore and custom patterns.
pub struct LsTool {
    /// Shared execution context.
    pub ctx: Arc<Ctx>,
}

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &'static str {
        "list"
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
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

        if !search.exists() {
            return Err(ToolError::NotFound(search.display().to_string()));
        }

        let extra: Vec<String> = call.args["ignore"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let files = collect_files(&search, &extra);
        let truncated = files.len() >= LIMIT;
        let files = &files[..files.len().min(LIMIT)];

        let output = render_tree(&search, files);

        let title = search
            .strip_prefix(&self.ctx.root)
            .unwrap_or(&search)
            .to_string_lossy()
            .into_owned();

        Ok(ToolResult {
            call_id: call.id,
            is_err: false,
            output,
            title,
            metadata: serde_json::json!({ "count": files.len(), "truncated": truncated }),
            output_path: None,
        })
    }
}

fn collect_files(root: &Path, extra_ignore: &[String]) -> Vec<PathBuf> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false);

    // Add default ignore patterns via override globs
    let mut ov = ignore::overrides::OverrideBuilder::new(root);
    for pat in IGNORE_PATTERNS {
        let neg = format!("!{}", pat);
        let _ = ov.add(&neg);
    }
    for pat in extra_ignore {
        let neg = format!("!{}", pat);
        let _ = ov.add(&neg);
    }
    if let Ok(built) = ov.build() {
        builder.overrides(built);
    }

    let mut files: Vec<PathBuf> = Vec::new();
    for entry in builder.build().flatten() {
        if entry.file_type().is_some_and(|t| t.is_file()) {
            if let Ok(rel) = entry.path().strip_prefix(root) {
                files.push(rel.to_path_buf());
            }
        }
        if files.len() > LIMIT {
            break;
        }
    }
    files
}

fn render_tree(root: &Path, files: &[PathBuf]) -> String {
    // Collect directories and files per dir
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    let mut by_dir: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for file in files {
        let dir = file
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parts: Vec<&str> = if dir.is_empty() {
            vec![]
        } else {
            dir.split('/').collect()
        };

        for i in 0..=parts.len() {
            let d = if i == 0 {
                ".".to_string()
            } else {
                parts[..i].join("/")
            };
            dirs.insert(d);
        }

        let entry = by_dir.entry(dir).or_default();
        entry.push(
            file.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        );
    }

    let mut output = format!("{}/\n", root.display());
    output.push_str(&render_dir(".", &dirs, &by_dir, 0));
    output
}

fn render_dir(
    dir: &str,
    dirs: &BTreeSet<String>,
    by_dir: &BTreeMap<String, Vec<String>>,
    depth: usize,
) -> String {
    let indent = "  ".repeat(depth);
    let mut out = String::new();

    if depth > 0 {
        let name = dir.split('/').next_back().unwrap_or(dir);
        out.push_str(&format!("{}{}/\n", indent, name));
    }

    let child_indent = "  ".repeat(depth + 1);

    // Render subdirectories first
    let children: Vec<&str> = dirs
        .iter()
        .filter(|d| {
            if dir == "." {
                !d.contains('/') && *d != "."
            } else {
                d.starts_with(&format!("{}/", dir)) && {
                    let rest = &d[dir.len() + 1..];
                    !rest.contains('/')
                }
            }
        })
        .map(|s| s.as_str())
        .collect();

    for child in &children {
        out.push_str(&render_dir(child, dirs, by_dir, depth + 1));
    }

    // Render files
    let empty = vec![];
    let files = by_dir
        .get(if dir == "." { "" } else { dir })
        .unwrap_or(&empty);
    let mut sorted = files.clone();
    sorted.sort();
    for file in sorted {
        out.push_str(&format!("{}{}\n", child_indent, file));
    }

    out
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
            name: "list".into(),
            args,
        }
    }

    #[tokio::test]
    async fn basic_tree_output() {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("hello.rs"), "fn main() {}").unwrap();
        fs::create_dir(d.path().join("src")).unwrap();
        fs::write(d.path().join("src").join("lib.rs"), "").unwrap();
        let tool = LsTool { ctx: ctx(&d) };
        let res = tool.invoke(call(serde_json::json!({}))).await.unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("hello.rs"));
        assert!(res.output.contains("lib.rs"));
    }

    #[tokio::test]
    async fn respects_ignore_pattern() {
        let d = TempDir::new().unwrap();
        let nm = d.path().join("node_modules");
        fs::create_dir(&nm).unwrap();
        fs::write(nm.join("pkg.js"), "").unwrap();
        fs::write(d.path().join("main.rs"), "").unwrap();
        let tool = LsTool { ctx: ctx(&d) };
        let res = tool.invoke(call(serde_json::json!({}))).await.unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("main.rs"));
        // node_modules is ignored by default
        assert!(!res.output.contains("pkg.js"));
    }

    #[tokio::test]
    async fn missing_dir_returns_not_found() {
        let d = TempDir::new().unwrap();
        let tool = LsTool { ctx: ctx(&d) };
        let err = tool
            .invoke(call(serde_json::json!({"path": "/nonexistent/path/12345"})))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn relative_path_uses_cwd() {
        let d = TempDir::new().unwrap();
        let sub = d.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("file.txt"), "").unwrap();
        let tool = LsTool { ctx: ctx(&d) };
        // Pass relative path — resolved against cwd
        let res = tool
            .invoke(call(serde_json::json!({"path": "sub"})))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("file.txt"));
    }

    #[tokio::test]
    async fn extra_ignore_filters_file() {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("keep.rs"), "").unwrap();
        fs::write(d.path().join("skip.log"), "").unwrap();
        let tool = LsTool { ctx: ctx(&d) };
        let res = tool
            .invoke(call(serde_json::json!({"ignore": ["*.log"]})))
            .await
            .unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("keep.rs"));
        assert!(!res.output.contains("skip.log"));
    }

    #[tokio::test]
    async fn cap_at_100() {
        let d = TempDir::new().unwrap();
        for i in 0..150 {
            fs::write(d.path().join(format!("f{:03}.txt", i)), "").unwrap();
        }
        let tool = LsTool { ctx: ctx(&d) };
        let res = tool.invoke(call(serde_json::json!({}))).await.unwrap();
        assert!(!res.is_err);
        let count = res.metadata["count"].as_u64().unwrap_or(0);
        assert!(count <= 100);
        assert_eq!(res.metadata["truncated"], true);
    }
}
