//! Shell execution helper used by BashTool.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::time::{Duration, timeout};

/// Output from a shell command execution.
#[derive(Debug)]
pub struct ShellOut {
    /// Combined stdout + stderr output.
    pub stdout: String,
    /// Process exit code (`None` if the process was killed due to timeout).
    pub exit_code: Option<i32>,
    /// Whether the command was killed because it exceeded the timeout.
    pub timed_out: bool,
}

/// Run `cmd` in `cwd` using `shell`, with an optional `timeout_ms` and
/// extra `env` variables merged on top of the current environment.
///
/// # Errors
/// Returns `io::Error` on process spawn failure or I/O error while reading output.
pub async fn run_shell(
    cmd: &str,
    cwd: &Path,
    shell: &str,
    timeout_ms: u64,
    env: &HashMap<String, String>,
) -> std::io::Result<ShellOut> {
    let mut child = tokio::process::Command::new(shell)
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .envs(env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let mut stdout_handle = child.stdout.take().expect("stdout is piped");
    let mut stderr_handle = child.stderr.take().expect("stderr is piped");

    let dur = Duration::from_millis(timeout_ms);
    match timeout(dur, async {
        let mut out = String::new();
        let mut err = String::new();
        let _ = stdout_handle.read_to_string(&mut out).await;
        let _ = stderr_handle.read_to_string(&mut err).await;
        out.push_str(&err);
        let status = child.wait().await?;
        Ok::<(String, Option<i32>), std::io::Error>((out, status.code()))
    })
    .await
    {
        Ok(Ok((stdout, exit_code))) => Ok(ShellOut {
            stdout,
            exit_code,
            timed_out: false,
        }),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            // Timeout: kill the process
            let _ = child.kill().await;
            Ok(ShellOut {
                stdout: String::new(),
                exit_code: None,
                timed_out: true,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn tmp() -> std::path::PathBuf {
        std::env::temp_dir()
    }

    #[tokio::test]
    async fn echo_command() {
        let out = run_shell("echo hello", &tmp(), "/bin/sh", 5_000, &HashMap::new())
            .await
            .unwrap();
        assert!(out.stdout.contains("hello"));
        assert_eq!(out.exit_code, Some(0));
        assert!(!out.timed_out);
    }

    #[tokio::test]
    async fn exit_code_capture() {
        let out = run_shell("exit 42", &tmp(), "/bin/sh", 5_000, &HashMap::new())
            .await
            .unwrap();
        assert_eq!(out.exit_code, Some(42));
        assert!(!out.timed_out);
    }

    #[tokio::test]
    async fn timeout_expiry() {
        let out = run_shell("sleep 60", &tmp(), "/bin/sh", 200, &HashMap::new())
            .await
            .unwrap();
        assert!(out.timed_out);
        assert!(out.exit_code.is_none());
    }

    #[tokio::test]
    async fn empty_output() {
        let out = run_shell("true", &tmp(), "/bin/sh", 5_000, &HashMap::new())
            .await
            .unwrap();
        assert_eq!(out.stdout, "");
        assert_eq!(out.exit_code, Some(0));
    }
}
