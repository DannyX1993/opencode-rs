//! Bootstrap sequence: initialise all services in dependency order.
//!
//! Phase 0 stub — `bootstrap()` loads config and sets up tracing only.
//! Storage, provider registry, and session engine hookups arrive in Phase 8.

use anyhow::Result;
use opencode_core::config::Config;
use std::path::Path;

/// Bootstrap the opencode runtime for `project_dir`.
///
/// # Errors
///
/// Returns an error if config loading fails.
pub async fn bootstrap(project_dir: &Path) -> Result<Config> {
    let cfg = Config::load(project_dir).await?;
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
}
