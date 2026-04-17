//! opencode binary — testable dispatch logic.
//!
//! `main.rs` is a thin shim; all command dispatch lives here so tests can
//! exercise every branch without spawning a full process.

use anyhow::Result;
use opencode_cli::cli::{Cli, Command};
use opencode_core::config_service::{ConfigService, ServerBindOverrides};
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
        Command::Server { host, port } => {
            start_server(cwd, ServerBindOverrides { host, port }).await?;
        }
        Command::Prompt { text, .. } => {
            tracing::info!(%text, "one-shot prompt — not yet implemented");
        }
        Command::Config { show: true } => {
            let cfg = ConfigService::new(cwd.to_path_buf()).resolve().await?;
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

/// Start the HTTP server using resolved bind settings for `cwd`.
///
/// Reads `OPENCODE_MANUAL_HARNESS=1` from the environment to enable
/// the manual provider harness route.
///
/// # Errors
///
/// Returns an error if storage init, TCP bind, or serve fails.
pub async fn start_server(cwd: &Path, cli_bind: ServerBindOverrides) -> Result<()> {
    use opencode_bus::BroadcastBus;
    use opencode_provider::{
        AccountService, AnthropicProvider, CatalogCache, EnvAuthResolver, GoogleProvider,
        ModelRegistry, OpenAiProvider, ProviderAuthService,
    };
    use opencode_server::{AppState, build, serve};
    use opencode_session::{
        engine::SessionEngine, permission_runtime::InMemoryPermissionRuntime,
        question_runtime::InMemoryQuestionRuntime,
    };
    use opencode_storage::{StorageImpl, connect};
    use std::net::SocketAddr;
    use std::sync::Arc;

    // Shared runtime config service used by startup and request handlers.
    // We resolve once here for startup-only concerns, and keep the service in
    // AppState so routes can always read latest resolved/scoped config.
    let config_service = Arc::new(ConfigService::new(cwd.to_path_buf()));
    let cfg = config_service.resolve().await?;
    let db = cwd.join("opencode.db");
    let pool = connect(&db).await?;
    let storage: Arc<dyn opencode_storage::Storage> = Arc::new(StorageImpl::new(pool));
    let bus = Arc::new(BroadcastBus::new(64));
    let harness = std::env::var("OPENCODE_MANUAL_HARNESS").as_deref() == Ok("1");

    let registry = Arc::new(ModelRegistry::new());
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

    let default_model = cfg.model.clone();
    let permission_runtime: Arc<dyn opencode_session::permission_runtime::PermissionRuntime> =
        Arc::new(InMemoryPermissionRuntime::new(
            Arc::clone(&storage),
            Arc::clone(&bus),
        ));
    let question_runtime: Arc<dyn opencode_session::question_runtime::QuestionRuntime> =
        Arc::new(InMemoryQuestionRuntime::new(Arc::clone(&bus)));
    let session = Arc::new(SessionEngine::with_runtimes(
        Arc::clone(&storage),
        Arc::clone(&bus),
        Arc::clone(&registry),
        default_model,
        Arc::clone(&permission_runtime),
        Arc::clone(&question_runtime),
    ));
    let models = CatalogCache::default_url(cwd.join(".opencode/models.json"))
        .load_cached()
        .ok()
        .flatten()
        .unwrap_or_default();
    let provider_catalog_models = Arc::new(models);
    let provider_auth = Arc::new(ProviderAuthService::new());
    let provider_accounts = Arc::new(AccountService::new(Arc::clone(&storage)));

    let state = AppState {
        config_service: Arc::clone(&config_service),
        bus,
        event_heartbeat: opencode_server::state::EventHeartbeat::default(),
        storage,
        session,
        permission_runtime,
        question_runtime,
        registry,
        provider_catalog_models,
        provider_auth,
        provider_accounts,
        harness,
    };

    // Bind policy: CLI host/port overrides > resolved config > defaults.
    let (host, port) = config_service.resolve_bind(cli_bind).await?;
    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let router = build(state);
    tracing::info!(%addr, "opencode server listening");
    serve(router, addr).await?;
    Ok(())
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::*;
    use clap::Parser;
    use opencode_cli::cli::Cli;
    use tempfile::TempDir;
    use tokio::time::{Duration, Instant};

    async fn dispatch_from(args: &[&str]) -> Result<()> {
        let cli = Cli::try_parse_from(args).unwrap();
        let dir = TempDir::new().unwrap();
        dispatch(cli, dir.path()).await
    }

    async fn wait_for_server_ready(port: u16) {
        let client = reqwest::Client::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        let health_url = format!("http://127.0.0.1:{port}/health");
        loop {
            let last_error = match client.get(&health_url).send().await {
                Ok(response) if response.status().is_success() => return,
                Ok(response) => format!("unexpected status {}", response.status()),
                Err(error) => error.to_string(),
            };

            if Instant::now() >= deadline {
                panic!("server did not become ready at {health_url}: {last_error}");
            }

            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    #[test]
    fn startup_tests_avoid_fixed_sleep_waits() {
        let source = include_str!("lib.rs");
        let forbidden = [
            "tokio::time::sleep",
            "(std::time::Duration::from_millis(",
            "100",
            ")).await",
        ]
        .concat();
        assert!(
            !source.contains(&forbidden),
            "startup tests should use wait_for_server_ready instead of fixed sleep"
        );
    }

    #[test]
    fn startup_tests_reuse_readiness_polling_helper() {
        let source = include_str!("lib.rs");
        let readiness_calls = source.matches("wait_for_server_ready(port).await;").count();
        assert!(
            readiness_calls >= 6,
            "expected startup tests to poll readiness in multiple scenarios, found {readiness_calls}"
        );
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

    #[test]
    fn package_version_matches_next_minor_release() {
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.12.0");
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
            start_server(
                &path,
                opencode_core::config_service::ServerBindOverrides {
                    host: Some("127.0.0.1".to_string()),
                    port: Some(port),
                },
            )
            .await
            .unwrap();
        });

        // Give the server time to bind.
        wait_for_server_ready(port).await;

        let url = format!("http://127.0.0.1:{port}/health");
        let resp = reqwest::get(&url).await.expect("health request failed");
        assert_eq!(resp.status().as_u16(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "ok");

        handle.abort();
    }

    #[tokio::test]
    async fn start_server_uses_resolved_bind_when_cli_overrides_missing() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".opencode")).unwrap();
        std::fs::write(
            dir.path().join(".opencode/config.jsonc"),
            format!(r#"{{ "server": {{ "host": "127.0.0.1", "port": {port} }} }}"#),
        )
        .unwrap();

        let path = dir.path().to_path_buf();
        let handle = tokio::spawn(async move {
            start_server(
                &path,
                opencode_core::config_service::ServerBindOverrides {
                    host: None,
                    port: None,
                },
            )
            .await
            .unwrap();
        });

        wait_for_server_ready(port).await;

        let url = format!("http://127.0.0.1:{port}/health");
        let resp = reqwest::get(&url).await.expect("health request failed");
        assert_eq!(resp.status().as_u16(), 200);

        handle.abort();
    }

    #[tokio::test]
    async fn start_server_cli_bind_overrides_win_while_config_drives_provider_view() {
        use std::net::TcpListener;

        let config_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let config_port = config_listener.local_addr().unwrap().port();
        drop(config_listener);

        let cli_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let cli_port = cli_listener.local_addr().unwrap().port();
        drop(cli_listener);

        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".opencode")).unwrap();
        std::fs::write(
            dir.path().join(".opencode/config.jsonc"),
            format!(
                r#"{{
                    "server": {{ "host": "127.0.0.1", "port": {config_port} }},
                    "providers": {{ "openai": "sk-openai" }}
                }}"#
            ),
        )
        .unwrap();

        let path = dir.path().to_path_buf();
        let handle = tokio::spawn(async move {
            start_server(
                &path,
                opencode_core::config_service::ServerBindOverrides {
                    host: Some("127.0.0.1".to_string()),
                    port: Some(cli_port),
                },
            )
            .await
            .unwrap();
        });

        wait_for_server_ready(cli_port).await;

        let health_resp = reqwest::get(format!("http://127.0.0.1:{cli_port}/health"))
            .await
            .expect("health request failed");
        assert_eq!(health_resp.status().as_u16(), 200);

        let provider_resp = reqwest::get(format!("http://127.0.0.1:{cli_port}/api/v1/provider"))
            .await
            .expect("provider request failed");
        assert_eq!(provider_resp.status().as_u16(), 200);
        let provider_json: serde_json::Value = provider_resp.json().await.unwrap();
        assert_eq!(provider_json["connected"], serde_json::json!(["openai"]));

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
            start_server(
                &path,
                opencode_core::config_service::ServerBindOverrides {
                    host: Some("127.0.0.1".to_string()),
                    port: Some(port),
                },
            )
            .await
            .unwrap();
        });

        wait_for_server_ready(port).await;

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

    #[tokio::test]
    async fn start_server_uses_runtime_session_for_prompt_route() {
        use reqwest::StatusCode;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        let handle = tokio::spawn(async move {
            start_server(
                &path,
                opencode_core::config_service::ServerBindOverrides {
                    host: Some("127.0.0.1".to_string()),
                    port: Some(port),
                },
            )
            .await
            .unwrap();
        });

        wait_for_server_ready(port).await;

        let sid = opencode_core::id::SessionId::new();
        let resp = reqwest::Client::new()
            .post(format!(
                "http://127.0.0.1:{port}/api/v1/sessions/{sid}/prompt"
            ))
            .json(&serde_json::json!({"text": "hello runtime"}))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body: serde_json::Value = resp.json().await.unwrap();
        let err = body["error"].as_str().unwrap();
        assert!(
            err.contains(&sid.to_string()),
            "expected runtime not-found to include session id, got: {err}"
        );

        handle.abort();
    }

    #[tokio::test]
    async fn start_server_exposes_provider_and_config_catalog_routes() {
        use reqwest::StatusCode;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".opencode")).unwrap();
        std::fs::write(
            dir.path().join(".opencode/config.jsonc"),
            r#"{
                "providers": { "openai": "sk-openai" },
                "enabled_providers": ["openai", "google"],
                "disabled_providers": ["google"]
            }"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".opencode/models.json"),
            r#"[
                {
                    "id": "openai/gpt-cache-only",
                    "name": "Cached OpenAI",
                    "context": 32768,
                    "max_tokens": 2048,
                    "vision": true,
                    "attachment": false
                }
            ]"#,
        )
        .unwrap();
        let path = dir.path().to_path_buf();
        let handle = tokio::spawn(async move {
            start_server(
                &path,
                opencode_core::config_service::ServerBindOverrides {
                    host: Some("127.0.0.1".to_string()),
                    port: Some(port),
                },
            )
            .await
            .unwrap();
        });

        wait_for_server_ready(port).await;

        let cli = reqwest::Client::new();
        let provider_resp = cli
            .get(format!("http://127.0.0.1:{port}/api/v1/provider"))
            .send()
            .await
            .unwrap();
        assert_eq!(provider_resp.status(), StatusCode::OK);
        let provider_json: serde_json::Value = provider_resp.json().await.unwrap();
        assert_eq!(provider_json["all"].as_array().unwrap().len(), 1);
        assert_eq!(provider_json["all"][0]["id"], "openai");
        assert_eq!(provider_json["connected"], serde_json::json!(["openai"]));

        let config_resp = cli
            .get(format!("http://127.0.0.1:{port}/api/v1/config/providers"))
            .send()
            .await
            .unwrap();
        assert_eq!(config_resp.status(), StatusCode::OK);
        let config_json: serde_json::Value = config_resp.json().await.unwrap();
        assert_eq!(config_json["providers"].as_array().unwrap().len(), 1);
        assert_eq!(config_json["providers"][0]["id"], "openai");
        assert_eq!(config_json["default"]["openai"], "gpt-cache-only");

        handle.abort();
    }

    #[tokio::test]
    async fn start_server_persists_callback_state_across_provider_requests() {
        use reqwest::StatusCode;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        let handle = tokio::spawn(async move {
            start_server(
                &path,
                opencode_core::config_service::ServerBindOverrides {
                    host: Some("127.0.0.1".to_string()),
                    port: Some(port),
                },
            )
            .await
            .unwrap();
        });

        wait_for_server_ready(port).await;

        let cli = reqwest::Client::new();
        let authorize_resp = cli
            .post(format!(
                "http://127.0.0.1:{port}/api/v1/provider/openai/oauth/authorize"
            ))
            .json(&serde_json::json!({ "method": 1 }))
            .send()
            .await
            .unwrap();
        assert_eq!(authorize_resp.status(), StatusCode::OK);

        let callback_resp = cli
            .post(format!(
                "http://127.0.0.1:{port}/api/v1/provider/openai/oauth/callback"
            ))
            .json(&serde_json::json!({
                "method": 1,
                "code": "oauth-code"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(callback_resp.status(), StatusCode::OK);

        let state_resp = cli
            .get(format!("http://127.0.0.1:{port}/api/v1/provider/account"))
            .send()
            .await
            .unwrap();
        assert_eq!(state_resp.status(), StatusCode::OK);
        let state_json: serde_json::Value = state_resp.json().await.unwrap();
        assert_eq!(state_json["accounts"].as_array().unwrap().len(), 1);
        assert!(state_json["active"]["account_id"].is_string());

        handle.abort();
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
