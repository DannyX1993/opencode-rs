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
            start_server(cwd, port).await?;
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
        Command::Tool {
            name,
            args_json,
            output,
        } => match opencode_cli::tool_cmd::run(&name, args_json.as_deref(), &output, cwd).await {
            Ok(out) => print!("{out}"),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        },
    }
    Ok(())
}

/// Start the HTTP server on `port`, building minimal app state from `cwd`.
///
/// Reads `OPENCODE_MANUAL_HARNESS=1` from the environment to enable
/// the manual provider harness route.
///
/// # Errors
///
/// Returns an error if storage init, TCP bind, or serve fails.
pub async fn start_server(cwd: &Path, port: u16) -> Result<()> {
    use opencode_bus::BroadcastBus;
    use opencode_provider::{
        AnthropicProvider, EnvAuthResolver, GoogleProvider, ModelRegistry, OpenAiProvider,
    };
    use opencode_server::{AppState, build, serve};
    use opencode_session::engine::SessionEngine;
    use opencode_storage::{StorageImpl, connect};
    use std::net::SocketAddr;
    use std::sync::Arc;

    let cfg = Config::load(cwd).await?;
    let db = cwd.join("opencode.db");
    let pool = connect(&db).await?;
    let storage = StorageImpl::new(pool);
    let bus = BroadcastBus::new(64);
    let harness = std::env::var("OPENCODE_MANUAL_HARNESS").as_deref() == Ok("1");

    let registry = ModelRegistry::new();
    if harness {
        // Register standard providers from env keys.
        let openai_auth = OpenAiProvider::default_auth(None);
        registry
            .register("openai", Arc::new(OpenAiProvider::new(openai_auth)))
            .await;
        let anthropic_auth = Arc::new(EnvAuthResolver::new("anthropic", "ANTHROPIC_API_KEY", None));
        registry
            .register(
                "anthropic",
                Arc::new(AnthropicProvider::new(anthropic_auth)),
            )
            .await;
        let google_auth = GoogleProvider::default_auth(cfg.providers.google.clone());
        registry
            .register("google", Arc::new(GoogleProvider::new(google_auth)))
            .await;
    }

    let state = AppState {
        config: Arc::new(cfg),
        bus: Arc::new(bus),
        storage: Arc::new(storage),
        session: Arc::new(SessionEngine),
        registry: Arc::new(registry),
        harness,
    };

    let addr: SocketAddr = format!("0.0.0.0:{port}").parse()?;
    let router = build(state);
    tracing::info!(%addr, "opencode server listening");
    serve(router, addr).await?;
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

    // RED S.1 — `server` subcommand binds a real TCP socket and serves health
    #[tokio::test]
    async fn dispatch_server_binds_and_serves_health() {
        use std::net::TcpListener;

        // Grab an ephemeral port.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener); // release so start_server can bind

        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        let handle = tokio::spawn(async move {
            start_server(&path, port).await.unwrap();
        });

        // Give the server time to bind.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let url = format!("http://127.0.0.1:{port}/health");
        let resp = reqwest::get(&url).await.expect("health request failed");
        assert_eq!(resp.status().as_u16(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "ok");

        handle.abort();
    }

    #[tokio::test]
    async fn start_server_registers_google_provider_for_harness() {
        use reqwest::StatusCode;
        use std::net::TcpListener;

        // SAFETY: test-scoped env var, restored below.
        unsafe {
            std::env::set_var("OPENCODE_MANUAL_HARNESS", "1");
            std::env::remove_var("GOOGLE_API_KEY");
            std::env::remove_var("GEMINI_API_KEY");
        }

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        let handle = tokio::spawn(async move {
            start_server(&path, port).await.unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let cli = reqwest::Client::new();
        let resp = cli
            .post(format!("http://127.0.0.1:{port}/api/v1/provider/stream"))
            .json(&serde_json::json!({
                "provider": "google",
                "model": "gemini-2.0-flash",
                "prompt": "hello"
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
        let body: serde_json::Value = resp.json().await.unwrap();
        let err = body["error"].as_str().unwrap();
        assert!(err.contains("google"), "unexpected error: {err}");

        handle.abort();

        // SAFETY: cleanup for test-scoped env var.
        unsafe {
            std::env::remove_var("OPENCODE_MANUAL_HARNESS");
        }
    }

    // ── B.5 REFACTOR: Tool dispatch integration ───────────────────────────────

    #[tokio::test]
    async fn dispatch_tool_bash_echo() {
        let cli = Cli::try_parse_from([
            "opencode",
            "tool",
            "bash",
            "--args-json",
            r#"{"command":"echo wired","description":"dispatch test"}"#,
        ])
        .unwrap();
        let dir = TempDir::new().unwrap();
        dispatch(cli, dir.path()).await.unwrap();
    }

    #[tokio::test]
    async fn dispatch_tool_missing_name_returns_err() {
        let dir = TempDir::new().unwrap();
        let err = opencode_cli::tool_cmd::run("no_such_tool", None, "text", dir.path())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not") || err.to_string().contains("no_such_tool"));
    }
}
