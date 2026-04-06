//! opencode binary — testable dispatch logic.
//!
//! `main.rs` is a thin shim; all command dispatch lives here so tests can
//! exercise every branch without spawning a full process.

use anyhow::Result;
use opencode_cli::cli::{Cli, Command};
use opencode_core::config::Config;
use std::path::Path;

/// Dispatch the parsed CLI command, given the already-bootstrapped config.
///
/// Returns `Ok(())` on successful dispatch (even for stubs not yet implemented).
///
/// # Errors
///
/// Returns an error if config loading fails for the `Config` sub-command, or
/// if any other operation propagates an `anyhow::Error`.
pub async fn dispatch(cli: Cli, cwd: &Path) -> Result<()> {
    match cli.command.unwrap_or(Command::Run) {
        Command::Version => {
            println!("opencode {}", env!("CARGO_PKG_VERSION"));
        }
        Command::Run => {
            tracing::info!("TUI mode — not yet implemented");
        }
        Command::Server { port } => {
            tracing::info!(%port, "server mode — not yet implemented");
        }
        Command::Prompt { text, .. } => {
            tracing::info!(%text, "one-shot prompt — not yet implemented");
        }
        Command::Config { show: true } => {
            let cfg = Config::load(cwd).await?;
            println!("{}", serde_json::to_string_pretty(&cfg)?);
        }
        Command::Config { show: false } => {
            tracing::info!("config edit — not yet implemented");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use opencode_cli::cli::Cli;
    use tempfile::TempDir;

    async fn dispatch_from(args: &[&str]) -> Result<()> {
        let cli = Cli::try_parse_from(args).unwrap();
        let dir = TempDir::new().unwrap();
        dispatch(cli, dir.path()).await
    }

    #[tokio::test]
    async fn dispatch_version() {
        dispatch_from(&["opencode", "version"]).await.unwrap();
    }

    #[tokio::test]
    async fn dispatch_run() {
        dispatch_from(&["opencode", "run"]).await.unwrap();
    }

    #[tokio::test]
    async fn dispatch_server() {
        dispatch_from(&["opencode", "server", "--port", "9090"])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn dispatch_prompt() {
        dispatch_from(&["opencode", "prompt", "hello"])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn dispatch_config_show() {
        dispatch_from(&["opencode", "config", "--show"])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn dispatch_config_no_show() {
        dispatch_from(&["opencode", "config"]).await.unwrap();
    }

    #[tokio::test]
    async fn dispatch_no_subcommand_defaults_to_run() {
        dispatch_from(&["opencode"]).await.unwrap();
    }
}
