//! opencode binary — testable dispatch logic.
//!
//! `main.rs` is a thin shim; all command dispatch lives here so tests can
//! exercise every branch without spawning a full process.

use anyhow::{Result, anyhow};
use opencode_cli::cli::{Cli, Command, ProvidersCommand, SessionCommand};
use opencode_core::config_service::{ConfigService, ServerBindOverrides};
use opencode_server::control_plane::proxy::HttpProxyService;
use opencode_server::state::{ControlPlaneConfig, ProxyPolicy};
use std::{path::Path, time::Duration};

/// Command execution result for scriptable command handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    /// User-facing payload emitted on standard output.
    pub stdout: String,
    /// User/actionable diagnostics emitted on standard error.
    pub stderr: String,
    /// Process-compatible command exit status.
    pub exit_code: i32,
}

impl CommandOutcome {
    /// Build a successful command outcome with `exit_code == 0`.
    #[must_use]
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: 0,
        }
    }

    /// Build a failed command outcome with a non-zero exit code.
    #[must_use]
    pub fn failure(stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            exit_code,
        }
    }
}

/// Execute the parsed CLI command and return a deterministic command outcome.
///
/// Core scriptable commands intentionally preserve backend-aligned semantics:
/// stdout carries command payloads, stderr carries actionable diagnostics, and
/// exit code mapping stays stable for shell automation.
pub async fn run_command(cli: Cli, cwd: &Path) -> CommandOutcome {
    match cli.command.unwrap_or(Command::Run {
        text: Vec::new(),
        output: "text".to_string(),
        timeout_ms: 30_000,
    }) {
        Command::Version => {
            CommandOutcome::success(format!("opencode {}\n", env!("CARGO_PKG_VERSION")))
        }
        Command::Run {
            text,
            output,
            timeout_ms,
        } => {
            if text.is_empty() {
                tracing::info!("TUI mode — not yet implemented");
                return CommandOutcome::success(String::new());
            }

            let backend = match opencode_cli::bootstrap::bootstrap_backend_client(cwd).await {
                Ok(client) => client,
                Err(error) => return CommandOutcome::failure(format!("error: {error}\n"), 1),
            };

            let joined = text.join(" ");
            let outcome = opencode_cli::commands::run::run(
                &backend,
                cwd,
                joined.as_str(),
                output.as_str(),
                Duration::from_millis(timeout_ms),
            )
            .await;
            CommandOutcome {
                stdout: outcome.stdout,
                stderr: outcome.stderr,
                exit_code: outcome.exit_code,
            }
        }
        Command::Serve { host, port } => {
            let outcome = opencode_cli::commands::serve::run(cwd, host, port).await;
            CommandOutcome {
                stdout: outcome.stdout,
                stderr: outcome.stderr,
                exit_code: outcome.exit_code,
            }
        }
        // Keep `prompt` and non-interactive `run <text...>` on the same code
        // path so timeout/output/acceptance behavior cannot drift.
        Command::Prompt {
            text,
            output,
            timeout_ms,
        } => {
            let backend = match opencode_cli::bootstrap::bootstrap_backend_client(cwd).await {
                Ok(client) => client,
                Err(error) => return CommandOutcome::failure(format!("error: {error}\n"), 1),
            };
            let outcome = opencode_cli::commands::run::run(
                &backend,
                cwd,
                text.as_str(),
                output.as_str(),
                Duration::from_millis(timeout_ms),
            )
            .await;
            CommandOutcome {
                stdout: outcome.stdout,
                stderr: outcome.stderr,
                exit_code: outcome.exit_code,
            }
        }
        Command::Config { show: true } => {
            let result = async {
                let cfg = ConfigService::new(cwd.to_path_buf()).resolve().await?;
                anyhow::Ok(format!("{}\n", serde_json::to_string_pretty(&cfg)?))
            }
            .await;
            match result {
                Ok(stdout) => CommandOutcome::success(stdout),
                Err(error) => CommandOutcome::failure(format!("error: {error}\n"), 1),
            }
        }
        Command::Config { show: false } => {
            tracing::info!("config edit — not yet implemented");
            CommandOutcome::success(String::new())
        }
        Command::Tool {
            name,
            args_json,
            output,
        } => match opencode_cli::tool_cmd::run(&name, args_json.as_deref(), &output, cwd).await {
            Ok(stdout) => CommandOutcome::success(stdout),
            Err(error) => CommandOutcome::failure(format!("error: {error}\n"), 1),
        },
        Command::Providers { command } => match command {
            ProvidersCommand::List { output } => {
                let backend = match opencode_cli::bootstrap::bootstrap_backend_client(cwd).await {
                    Ok(client) => client,
                    Err(error) => return CommandOutcome::failure(format!("error: {error}\n"), 1),
                };
                let outcome =
                    opencode_cli::commands::providers_list::run(&backend, output.as_str()).await;
                CommandOutcome {
                    stdout: outcome.stdout,
                    stderr: outcome.stderr,
                    exit_code: outcome.exit_code,
                }
            }
        },
        Command::Session { command } => match command {
            SessionCommand::List => {
                let backend = match opencode_cli::bootstrap::bootstrap_backend_client(cwd).await {
                    Ok(client) => client,
                    Err(error) => return CommandOutcome::failure(format!("error: {error}\n"), 1),
                };
                let outcome = opencode_cli::commands::session_list::run(&backend, cwd).await;
                CommandOutcome {
                    stdout: outcome.stdout,
                    stderr: outcome.stderr,
                    exit_code: outcome.exit_code,
                }
            }
        },
    }
}

/// Backwards-compatible dispatch API for tests using `anyhow::Result`.
///
/// # Errors
///
/// Returns an error when the command outcome exit code is non-zero.
pub async fn dispatch(cli: Cli, cwd: &Path) -> Result<()> {
    let outcome = run_command(cli, cwd).await;
    if outcome.exit_code == 0 {
        return Ok(());
    }

    Err(anyhow!(outcome.stderr.trim().to_string()))
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
    let control_plane = resolve_control_plane_config();
    let control_plane_proxy = Arc::new(HttpProxyService::new(
        reqwest::Client::new(),
        control_plane.proxy.clone(),
    ));

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
        control_plane,
        control_plane_proxy,
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

fn resolve_control_plane_config() -> ControlPlaneConfig {
    let defaults = ControlPlaneConfig::default();
    let instance_id = std::env::var("OPENCODE_CONTROL_PLANE_INSTANCE_ID")
        .unwrap_or_else(|_| defaults.instance_id.clone());
    let force_local_only =
        env_bool("OPENCODE_CONTROL_PLANE_LOCAL_ONLY").unwrap_or(defaults.force_local_only);
    let timeout = env_u64("OPENCODE_CONTROL_PLANE_PROXY_TIMEOUT_MS")
        .map(Duration::from_millis)
        .unwrap_or(defaults.proxy.timeout);
    let max_retries = env_u64("OPENCODE_CONTROL_PLANE_PROXY_RETRIES")
        .map_or(defaults.proxy.max_retries, |value| value as u32);
    let backoff = env_u64("OPENCODE_CONTROL_PLANE_PROXY_BACKOFF_MS")
        .map(Duration::from_millis)
        .unwrap_or(defaults.proxy.backoff);

    ControlPlaneConfig::new(
        instance_id,
        force_local_only,
        ProxyPolicy::bounded(timeout, max_retries, backoff),
    )
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
}

fn env_bool(name: &str) -> Option<bool> {
    match std::env::var(name).ok()?.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
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
        let error = dispatch_from(&["opencode", "prompt", "hello"])
            .await
            .expect_err("prompt without cwd project should fail deterministically");
        assert!(
            error
                .to_string()
                .contains("could not resolve project for cwd")
        );
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
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.14.0");
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

    #[test]
    fn start_server_control_plane_config_defaults_when_env_missing() {
        // SAFETY: isolated test-scoped env cleanup.
        unsafe {
            std::env::remove_var("OPENCODE_CONTROL_PLANE_INSTANCE_ID");
            std::env::remove_var("OPENCODE_CONTROL_PLANE_LOCAL_ONLY");
            std::env::remove_var("OPENCODE_CONTROL_PLANE_PROXY_TIMEOUT_MS");
            std::env::remove_var("OPENCODE_CONTROL_PLANE_PROXY_RETRIES");
            std::env::remove_var("OPENCODE_CONTROL_PLANE_PROXY_BACKOFF_MS");
        }

        let cfg = resolve_control_plane_config();
        assert_eq!(cfg.instance_id, "local");
        assert!(!cfg.force_local_only);
        assert_eq!(cfg.proxy.timeout, Duration::from_secs(5));
        assert_eq!(cfg.proxy.max_retries, 2);
        assert_eq!(cfg.proxy.backoff, Duration::from_millis(200));
    }

    #[test]
    fn start_server_control_plane_config_reads_and_bounds_env_overrides() {
        // SAFETY: isolated test-scoped env setup/cleanup.
        unsafe {
            std::env::set_var("OPENCODE_CONTROL_PLANE_INSTANCE_ID", "cp-a");
            std::env::set_var("OPENCODE_CONTROL_PLANE_LOCAL_ONLY", "true");
            std::env::set_var("OPENCODE_CONTROL_PLANE_PROXY_TIMEOUT_MS", "10");
            std::env::set_var("OPENCODE_CONTROL_PLANE_PROXY_RETRIES", "77");
            std::env::set_var("OPENCODE_CONTROL_PLANE_PROXY_BACKOFF_MS", "1");
        }

        let cfg = resolve_control_plane_config();
        assert_eq!(cfg.instance_id, "cp-a");
        assert!(cfg.force_local_only);
        assert_eq!(cfg.proxy.timeout, Duration::from_millis(100));
        assert_eq!(cfg.proxy.max_retries, 5);
        assert_eq!(cfg.proxy.backoff, Duration::from_millis(10));

        // SAFETY: cleanup for process-global env vars used above.
        unsafe {
            std::env::remove_var("OPENCODE_CONTROL_PLANE_INSTANCE_ID");
            std::env::remove_var("OPENCODE_CONTROL_PLANE_LOCAL_ONLY");
            std::env::remove_var("OPENCODE_CONTROL_PLANE_PROXY_TIMEOUT_MS");
            std::env::remove_var("OPENCODE_CONTROL_PLANE_PROXY_RETRIES");
            std::env::remove_var("OPENCODE_CONTROL_PLANE_PROXY_BACKOFF_MS");
        }
    }
}
