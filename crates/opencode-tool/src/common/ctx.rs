//! Execution context threaded through all built-in tools.

use std::path::PathBuf;

/// Default command timeout: 120 seconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// Execution context shared across tool invocations.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// Project root directory.
    pub root: PathBuf,
    /// Current working directory for this invocation.
    pub cwd: PathBuf,
    /// Directory where truncated output files are saved.
    pub out_dir: PathBuf,
    /// Shell executable path (e.g. `/bin/bash`).
    pub shell: String,
    /// Default command timeout in milliseconds.
    pub timeout: u64,
}

impl Ctx {
    /// Create a new context with all fields specified.
    pub fn new(root: PathBuf, cwd: PathBuf, out_dir: PathBuf, shell: String, timeout: u64) -> Self {
        Self {
            root,
            cwd,
            out_dir,
            shell,
            timeout,
        }
    }

    /// Create a context with sensible defaults for the current process.
    pub fn default_for(root: PathBuf) -> Self {
        let cwd = root.clone();
        let out_dir = std::env::temp_dir().join("opencode-tool-output");
        let shell = default_shell();
        Self {
            root,
            cwd,
            out_dir,
            shell,
            timeout: DEFAULT_TIMEOUT_MS,
        }
    }
}

/// Return the system default shell path.
fn default_shell() -> String {
    if cfg!(windows) {
        "cmd.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn field_round_trip() {
        let root = PathBuf::from("/tmp/root");
        let cwd = PathBuf::from("/tmp/cwd");
        let out = PathBuf::from("/tmp/out");
        let ctx = Ctx::new(
            root.clone(),
            cwd.clone(),
            out.clone(),
            "/bin/bash".into(),
            5_000,
        );
        assert_eq!(ctx.root, root);
        assert_eq!(ctx.cwd, cwd);
        assert_eq!(ctx.out_dir, out);
        assert_eq!(ctx.shell, "/bin/bash");
        assert_eq!(ctx.timeout, 5_000);
    }

    #[test]
    fn default_timeout_is_120s() {
        let root = PathBuf::from("/tmp");
        let ctx = Ctx::default_for(root.clone());
        assert_eq!(ctx.timeout, DEFAULT_TIMEOUT_MS);
        assert_eq!(ctx.root, root);
        assert_eq!(ctx.cwd, root);
    }

    #[test]
    fn default_timeout_constant_value() {
        assert_eq!(DEFAULT_TIMEOUT_MS, 120_000);
    }
}
