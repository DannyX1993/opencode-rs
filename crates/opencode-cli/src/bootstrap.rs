//! Bootstrap sequence: initialise all services in dependency order.
//!
//! Phase 0 stub — `bootstrap()` loads config and sets up tracing only.
//! Storage, provider registry, and session engine hookups arrive in Phase 8.

use anyhow::Result;
use opencode_core::config::Config;
use opencode_core::config_service::ConfigService;
use std::path::Path;

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
