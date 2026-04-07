//! Dispatch handler for the `tool` subcommand.
//!
//! Builds a [`ToolRegistry`] from the current working directory context,
//! invokes the requested tool, and prints the result.

use anyhow::{Context, Result};
use opencode_tool::{Ctx, ToolCall, ToolRegistry};
use std::path::Path;

/// Run a named built-in tool and print the result.
///
/// `name`      — tool name (e.g. `"read"`, `"bash"`)
/// `args_json` — optional JSON string of arguments (defaults to `{}`)
/// `output`    — output format: `"text"` or `"json"`
/// `cwd`       — current working directory (becomes both root and cwd in `Ctx`)
///
/// # Errors
///
/// Returns an error if JSON parsing fails or the registry invocation fails
/// with an unrecoverable error. Tool-level errors (NotFound, InvalidArgs, …)
/// are reported to stderr and return an [`Err`] with exit-code semantics.
pub async fn run(name: &str, args_json: Option<&str>, output: &str, cwd: &Path) -> Result<String> {
    if output != "text" && output != "json" {
        anyhow::bail!("invalid --output value '{output}': expected 'text' or 'json'");
    }

    let ctx = Ctx::default_for(cwd.to_path_buf());
    let reg = ToolRegistry::with_builtins(ctx);

    let raw = args_json.unwrap_or("{}");
    let args: serde_json::Value =
        serde_json::from_str(raw).with_context(|| format!("invalid JSON args: {raw}"))?;

    let call = ToolCall {
        id: "cli-1".into(),
        name: name.to_string(),
        args,
    };

    match reg.invoke(call).await {
        Ok(res) => {
            let out = if output == "json" {
                serde_json::to_string_pretty(&res)?
            } else {
                res.output.clone()
            };
            Ok(out)
        }
        Err(e) => Err(anyhow::anyhow!("{e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── B.3 RED tests — written before implementation ─────────────────────────

    #[tokio::test]
    async fn dispatch_bash_tool_echo_ok() {
        let dir = TempDir::new().unwrap();
        let result = run(
            "bash",
            Some(r#"{"command":"echo hello","description":"test echo"}"#),
            "text",
            dir.path(),
        )
        .await
        .unwrap();
        assert!(
            result.contains("hello"),
            "expected 'hello' in output, got: {result}"
        );
    }

    #[tokio::test]
    async fn dispatch_missing_tool_returns_error() {
        let dir = TempDir::new().unwrap();
        let err = run("nonexistent_tool", None, "text", dir.path())
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("nonexistent_tool") || err.to_string().contains("not"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn dispatch_invalid_json_args_returns_error() {
        let dir = TempDir::new().unwrap();
        let err = run("bash", Some("not-valid-json{{{"), "text", dir.path())
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("invalid JSON") || err.to_string().contains("JSON"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn output_text_format() {
        let dir = TempDir::new().unwrap();
        // create a temp file to read
        let file = dir.path().join("hello.txt");
        std::fs::write(&file, "hello world\n").unwrap();
        let result = run(
            "read",
            Some(&format!(r#"{{"filePath":"{}"}}"#, file.display())),
            "text",
            dir.path(),
        )
        .await
        .unwrap();
        assert!(
            result.contains("hello world"),
            "expected file content in output, got: {result}"
        );
        // text format should NOT start with '{' (not JSON envelope)
        assert!(
            !result.trim_start().starts_with('{'),
            "text format should not be JSON envelope"
        );
    }

    #[tokio::test]
    async fn output_json_format() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("hello.txt");
        std::fs::write(&file, "hello world\n").unwrap();
        let result = run(
            "read",
            Some(&format!(r#"{{"filePath":"{}"}}"#, file.display())),
            "json",
            dir.path(),
        )
        .await
        .unwrap();
        // JSON format should be a parseable JSON object
        let val: serde_json::Value =
            serde_json::from_str(&result).expect("expected JSON output, got: {result}");
        assert_eq!(val["is_err"], false);
        assert!(val["output"].as_str().unwrap().contains("hello world"));
    }

    #[tokio::test]
    async fn dispatch_read_tool_ok() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "test content\n").unwrap();
        let result = run(
            "read",
            Some(&format!(r#"{{"filePath":"{}"}}"#, file.display())),
            "text",
            dir.path(),
        )
        .await
        .unwrap();
        assert!(result.contains("test content"));
    }

    // ── C.1 RED: invalid --output validation ────────────────────────────────

    #[tokio::test]
    async fn invalid_output_format_returns_error() {
        let dir = TempDir::new().unwrap();
        let err = run(
            "bash",
            Some(r#"{"command":"echo hi","description":"t"}"#),
            "xml",
            dir.path(),
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("xml") || err.to_string().contains("output"),
            "expected error mentioning output format, got: {err}"
        );
    }

    #[tokio::test]
    async fn invalid_output_format_case_sensitive() {
        // "JSON" (uppercase) must also be rejected — only lowercase "json" is valid
        let dir = TempDir::new().unwrap();
        let err = run(
            "bash",
            Some(r#"{"command":"echo hi","description":"t"}"#),
            "JSON",
            dir.path(),
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("JSON") || err.to_string().contains("output"),
            "expected error mentioning output format, got: {err}"
        );
    }
}
