//! Bootstrap sequence: initialise all services in dependency order.
//!
//! Phase 0 stub — `bootstrap()` loads config and sets up tracing only.
//! Storage, provider registry, and session engine hookups arrive in Phase 8.

use anyhow::Result;
use opencode_bus::BroadcastBus;
use opencode_core::config::Config;
use opencode_core::config_service::ConfigService;
use opencode_provider::{
    AccountService, AnthropicProvider, CatalogCache, EnvAuthResolver, GoogleProvider,
    ModelRegistry, OpenAiProvider, ProviderAuthService,
};
use opencode_server::AppState;
use opencode_server::control_plane::proxy::HttpProxyService;
use opencode_server::state::{ControlPlaneConfig, EventHeartbeat};
use opencode_session::{
    engine::SessionEngine, permission_runtime::InMemoryPermissionRuntime,
    question_runtime::InMemoryQuestionRuntime,
};
use opencode_storage::{Storage, StorageImpl, connect};
use std::path::Path;
use std::sync::Arc;

use crate::backend_client::LocalBackendClient;

/// Bootstrap the opencode runtime for `project_dir`.
///
/// # Errors
///
/// Returns an error if config loading fails.
pub async fn bootstrap(project_dir: &Path) -> Result<Config> {
    let service = ConfigService::new(project_dir.to_path_buf());
    let cfg = bootstrap_with_service(&service).await?;
    Ok(cfg)
}

/// Bootstrap runtime/tracing from an existing [`ConfigService`].
///
/// # Errors
///
/// Returns an error if config resolution fails.
pub async fn bootstrap_with_service(service: &ConfigService) -> Result<Config> {
    let cfg = service.resolve().await?;
    opencode_core::tracing::init(&cfg);
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        model   = ?cfg.model,
        "opencode starting"
    );
    Ok(cfg)
}

/// Bootstrap in-process backend routing and return a local command client.
///
/// # Errors
///
/// Returns an error when runtime dependencies (config/storage/state) fail to initialize.
pub async fn bootstrap_backend_client(project_dir: &Path) -> Result<LocalBackendClient> {
    let state = bootstrap_app_state(project_dir).await?;
    Ok(LocalBackendClient::from_state(state))
}

/// Bootstrap app state for local router serving/commands.
///
/// # Errors
///
/// Returns an error when config, storage, or runtime dependencies cannot initialize.
pub async fn bootstrap_app_state(project_dir: &Path) -> Result<AppState> {
    let config_service = Arc::new(ConfigService::new(project_dir.to_path_buf()));
    let cfg = config_service.resolve().await?;
    let db = project_dir.join("opencode.db");
    let pool = connect(&db).await?;
    let storage: Arc<dyn Storage> = Arc::new(StorageImpl::new(pool));
    let bus = Arc::new(BroadcastBus::new(64));
    let harness = std::env::var("OPENCODE_MANUAL_HARNESS").as_deref() == Ok("1");

    let registry = Arc::new(ModelRegistry::new());
    if harness {
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
        cfg.model.clone(),
        Arc::clone(&permission_runtime),
        Arc::clone(&question_runtime),
    ));
    let models = CatalogCache::default_url(project_dir.join(".opencode/models.json"))
        .load_cached()
        .ok()
        .flatten()
        .unwrap_or_default();
    let provider_catalog_models = Arc::new(models);
    let provider_auth = Arc::new(ProviderAuthService::new());
    let provider_accounts = Arc::new(AccountService::new(Arc::clone(&storage)));

    let control_plane = ControlPlaneConfig::default();
    let control_plane_proxy = Arc::new(HttpProxyService::new(
        reqwest::Client::new(),
        control_plane.proxy.clone(),
    ));

    Ok(AppState {
        config_service,
        bus,
        event_heartbeat: EventHeartbeat::default(),
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencode_core::config_service::ConfigService;
    use tempfile::TempDir;

    #[tokio::test]
    async fn bootstrap_succeeds_on_empty_dir() {
        let dir = TempDir::new().unwrap();
        let cfg = bootstrap(dir.path())
            .await
            .expect("bootstrap should succeed");
        assert_eq!(cfg.log_level, "info");
        assert_eq!(cfg.server.port, 4141);
    }

    #[tokio::test]
    async fn bootstrap_with_service_resolves_layered_config() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".opencode")).unwrap();
        std::fs::write(
            dir.path().join(".opencode/config.jsonc"),
            r#"{ "log_level": "debug", "server": { "port": 5005 } }"#,
        )
        .unwrap();

        let service = ConfigService::new(dir.path().to_path_buf());
        let cfg = bootstrap_with_service(&service)
            .await
            .expect("bootstrap should resolve via config service");

        assert_eq!(cfg.log_level, "debug");
        assert_eq!(cfg.server.port, 5005);
    }
}
