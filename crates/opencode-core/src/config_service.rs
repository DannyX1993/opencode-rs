//! Runtime config service with layered resolution and scoped persistence.

use crate::config::{
    Config, apply_env_overrides, default_global_config_path, load_optional_jsonc,
    local_config_path, merge_configs,
};
use crate::error::ConfigError;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Config file scope for raw reads/writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigScope {
    /// Project-local config file (`.opencode/config.jsonc`).
    Local,
    /// Global user config file (`$HOME/.config/opencode/config.jsonc`).
    Global,
}

/// Optional CLI bind overrides applied after layered resolution.
#[derive(Debug, Clone, Default)]
pub struct ServerBindOverrides {
    /// CLI host override.
    pub host: Option<String>,
    /// CLI port override.
    pub port: Option<u16>,
}

/// Shared runtime config service with cached layered resolution.
#[derive(Debug)]
pub struct ConfigService {
    project_dir: PathBuf,
    global_path: Option<PathBuf>,
    resolved_cache: RwLock<Option<Config>>,
}

impl ConfigService {
    /// Build a service for `project_dir` with default global config path resolution.
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self::with_global_config_path(project_dir, default_global_config_path())
    }

    /// Build a service for `project_dir` with an explicit global config path.
    #[must_use]
    pub fn with_global_config_path(project_dir: PathBuf, global_path: Option<PathBuf>) -> Self {
        Self {
            project_dir,
            global_path,
            resolved_cache: RwLock::new(None),
        }
    }

    /// Build a service with a pre-seeded resolved cache.
    ///
    /// Useful for dependency injection in tests where filesystem config
    /// persistence is not part of the behavior under test.
    #[must_use]
    pub fn with_cached_resolved(
        project_dir: PathBuf,
        global_path: Option<PathBuf>,
        resolved: Config,
    ) -> Self {
        Self {
            project_dir,
            global_path,
            resolved_cache: RwLock::new(Some(resolved)),
        }
    }

    /// Resolve layered runtime config, using cache when available.
    ///
    /// Merge order: defaults < global < local < env overrides.
    pub async fn resolve(&self) -> Result<Config, ConfigError> {
        if let Some(cached) = self.resolved_cache.read().expect("cache poisoned").clone() {
            return Ok(cached);
        }

        let mut resolved = Config::default();

        if let Some(global_path) = &self.global_path {
            if let Some(global_cfg) = load_optional_jsonc(global_path).await? {
                resolved = merge_configs(resolved, global_cfg);
            }
        }

        let local_path = local_config_path(&self.project_dir);
        if let Some(local_cfg) = load_optional_jsonc(&local_path).await? {
            resolved = merge_configs(resolved, local_cfg);
        }

        apply_env_overrides(&mut resolved);

        *self.resolved_cache.write().expect("cache poisoned") = Some(resolved.clone());
        Ok(resolved)
    }

    /// Read the raw persisted config for one scope.
    ///
    /// Missing files resolve to `Config::default()`.
    pub async fn read_scope(&self, scope: ConfigScope) -> Result<Config, ConfigError> {
        match self.scope_path(scope) {
            Some(path) => Ok(load_optional_jsonc(&path).await?.unwrap_or_default()),
            None => Ok(Config::default()),
        }
    }

    /// Merge and persist config for one scope, then invalidate resolved cache.
    pub async fn update_scope(
        &self,
        scope: ConfigScope,
        input: &Config,
    ) -> Result<Config, ConfigError> {
        let path = self
            .scope_path(scope)
            .ok_or(ConfigError::Missing { field: "HOME" })?;

        let current = load_optional_jsonc(&path).await?.unwrap_or_default();
        let merged = merge_configs(current, input.clone());

        write_config_file(&path, &merged).await?;
        self.invalidate();

        Ok(merged)
    }

    /// Resolve bind host+port and then apply optional CLI overrides.
    pub async fn resolve_bind(
        &self,
        cli: ServerBindOverrides,
    ) -> Result<(String, u16), ConfigError> {
        let resolved = self.resolve().await?;
        let host = cli.host.unwrap_or(resolved.server.host);
        let port = cli.port.unwrap_or(resolved.server.port);
        Ok((host, port))
    }

    /// Invalidate cached resolved config.
    pub fn invalidate(&self) {
        *self.resolved_cache.write().expect("cache poisoned") = None;
    }

    fn scope_path(&self, scope: ConfigScope) -> Option<PathBuf> {
        match scope {
            ConfigScope::Local => Some(local_config_path(&self.project_dir)),
            ConfigScope::Global => self.global_path.clone(),
        }
    }
}

async fn write_config_file(path: &Path, cfg: &Config) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let text = serde_json::to_string_pretty(cfg).map_err(|err| ConfigError::Invalid {
        field: "config",
        reason: err.to_string(),
    })?;
    tokio::fs::write(path, format!("{text}\n")).await?;
    Ok(())
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::{ConfigScope, ConfigService, ServerBindOverrides};
    use crate::config::Config;
    use crate::test_env;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_jsonc(path: &std::path::Path, content: &str) {
        let mut file = std::fs::File::create(path).unwrap();
        write!(file, "{content}").unwrap();
    }

    #[tokio::test]
    async fn resolve_uses_global_then_local_then_env_precedence() {
        let _guard = test_env::lock().await;
        test_env::clear();
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let project = tmp.path().join("project");

        std::fs::create_dir_all(home.join(".config/opencode")).unwrap();
        std::fs::create_dir_all(project.join(".opencode")).unwrap();

        write_jsonc(
            &home.join(".config/opencode/config.jsonc"),
            r#"{ "server": { "host": "10.0.0.1", "port": 5000, "auth_token": "global-token" } }"#,
        );
        write_jsonc(
            &project.join(".opencode/config.jsonc"),
            r#"{ "server": { "host": "10.0.0.2", "port": 6000 } }"#,
        );

        unsafe { std::env::set_var("OPENCODE_SERVER_PORT", "7000") };
        let service = ConfigService::with_global_config_path(
            project,
            Some(home.join(".config/opencode/config.jsonc")),
        );

        let resolved = service.resolve().await.unwrap();
        assert_eq!(resolved.server.host, "10.0.0.2");
        assert_eq!(resolved.server.port, 7000);
        assert_eq!(resolved.server.auth_token.as_deref(), Some("global-token"));

        unsafe { std::env::remove_var("OPENCODE_SERVER_PORT") };
    }

    #[tokio::test]
    async fn update_scope_invalidates_cache_on_success() {
        let _guard = test_env::lock().await;
        test_env::clear();
        let dir = TempDir::new().unwrap();
        let service = ConfigService::with_global_config_path(
            dir.path().to_path_buf(),
            Some(dir.path().join("home/.config/opencode/config.jsonc")),
        );

        let _ = service.resolve().await.unwrap();

        let mut patch = Config::default();
        patch
            .providers
            .extra
            .insert("fresh".to_string(), serde_json::json!("value"));
        let persisted = service
            .update_scope(ConfigScope::Local, &patch)
            .await
            .unwrap();
        assert_eq!(
            persisted.providers.extra.get("fresh"),
            Some(&serde_json::json!("value"))
        );

        let resolved = service.resolve().await.unwrap();
        assert_eq!(
            resolved.providers.extra.get("fresh"),
            Some(&serde_json::json!("value"))
        );
    }

    #[tokio::test]
    async fn failed_update_keeps_previous_cached_config() {
        let _guard = test_env::lock().await;
        test_env::clear();
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("project");
        let global = dir.path().join("home/.config/opencode/config.jsonc");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join(".opencode"), "not-a-directory").unwrap();

        let service = ConfigService::with_global_config_path(project.clone(), Some(global));

        let mut initial = Config::default();
        initial
            .providers
            .extra
            .insert("baseline".to_string(), serde_json::json!(true));
        let _ = service
            .update_scope(ConfigScope::Global, &initial)
            .await
            .unwrap();

        let cached = service.resolve().await.unwrap();
        assert_eq!(
            cached.providers.extra.get("baseline"),
            Some(&serde_json::json!(true))
        );

        let mut patch = Config::default();
        patch
            .providers
            .extra
            .insert("next".to_string(), serde_json::json!(123));
        let err = service.update_scope(ConfigScope::Local, &patch).await;
        assert!(err.is_err());

        let after = service.resolve().await.unwrap();
        assert_eq!(
            after.providers.extra.get("baseline"),
            Some(&serde_json::json!(true))
        );
        assert!(!after.providers.extra.contains_key("next"));
    }

    #[tokio::test]
    async fn read_scope_returns_defaults_when_scope_file_missing() {
        let _guard = test_env::lock().await;
        test_env::clear();
        let dir = TempDir::new().unwrap();
        let service = ConfigService::with_global_config_path(
            dir.path().to_path_buf(),
            Some(dir.path().join("home/.config/opencode/config.jsonc")),
        );

        let local = service.read_scope(ConfigScope::Local).await.unwrap();
        let global = service.read_scope(ConfigScope::Global).await.unwrap();

        assert_eq!(local.server.port, 4141);
        assert_eq!(global.server.port, 4141);
        assert!(local.model.is_none());
        assert!(global.model.is_none());
    }

    #[tokio::test]
    async fn resolve_bind_applies_cli_overrides_only_for_host_and_port() {
        let _guard = test_env::lock().await;
        test_env::clear();
        let dir = TempDir::new().unwrap();
        let service = ConfigService::with_global_config_path(
            dir.path().to_path_buf(),
            Some(dir.path().join("home/.config/opencode/config.jsonc")),
        );

        let mut patch = Config::default();
        patch.server.host = "10.1.1.1".to_string();
        patch.server.port = 5000;
        patch.server.auth_token = Some("cfg-token".to_string());
        let _ = service
            .update_scope(ConfigScope::Local, &patch)
            .await
            .unwrap();

        let (host, port) = service
            .resolve_bind(ServerBindOverrides {
                host: Some("0.0.0.0".to_string()),
                port: Some(9000),
            })
            .await
            .unwrap();

        assert_eq!(host, "0.0.0.0");
        assert_eq!(port, 9000);
        let resolved = service.resolve().await.unwrap();
        assert_eq!(resolved.server.auth_token.as_deref(), Some("cfg-token"));
    }

    #[tokio::test]
    async fn resolve_bind_keeps_resolved_host_when_cli_host_not_provided() {
        let _guard = test_env::lock().await;
        test_env::clear();
        let dir = TempDir::new().unwrap();
        let service = ConfigService::with_global_config_path(
            dir.path().to_path_buf(),
            Some(dir.path().join("home/.config/opencode/config.jsonc")),
        );

        let mut patch = Config::default();
        patch.server.host = "127.0.0.9".to_string();
        patch.server.port = 5000;
        let _ = service
            .update_scope(ConfigScope::Local, &patch)
            .await
            .unwrap();

        let (host, port) = service
            .resolve_bind(ServerBindOverrides {
                host: None,
                port: Some(9001),
            })
            .await
            .unwrap();

        assert_eq!(host, "127.0.0.9");
        assert_eq!(port, 9001);
    }

    #[tokio::test]
    async fn resolve_uses_defaults_when_files_and_env_are_absent() {
        let _guard = test_env::lock().await;
        test_env::clear();
        let dir = TempDir::new().unwrap();
        let service = ConfigService::with_global_config_path(dir.path().to_path_buf(), None);

        let resolved = service.resolve().await.unwrap();
        assert_eq!(resolved.server.host, "127.0.0.1");
        assert_eq!(resolved.server.port, 4141);
        assert!(resolved.server.auth_token.is_none());
    }

    #[tokio::test]
    async fn resolve_applies_env_override_for_auth_token_after_file_merges() {
        let _guard = test_env::lock().await;
        test_env::clear();
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let project = tmp.path().join("project");

        std::fs::create_dir_all(home.join(".config/opencode")).unwrap();
        std::fs::create_dir_all(project.join(".opencode")).unwrap();

        write_jsonc(
            &home.join(".config/opencode/config.jsonc"),
            r#"{ "server": { "auth_token": "global-token" } }"#,
        );
        write_jsonc(
            &project.join(".opencode/config.jsonc"),
            r#"{ "server": { "auth_token": "local-token" } }"#,
        );

        unsafe { std::env::set_var("OPENCODE_AUTH_TOKEN", "env-token") };
        let service = ConfigService::with_global_config_path(
            project,
            Some(home.join(".config/opencode/config.jsonc")),
        );

        let resolved = service.resolve().await.unwrap();
        assert_eq!(resolved.server.auth_token.as_deref(), Some("env-token"));

        unsafe { std::env::remove_var("OPENCODE_AUTH_TOKEN") };
    }

    #[tokio::test]
    async fn resolve_bind_keeps_resolved_port_when_cli_port_not_provided() {
        let _guard = test_env::lock().await;
        test_env::clear();
        unsafe {
            std::env::remove_var("OPENCODE_SERVER_PORT");
            std::env::remove_var("OPENCODE_SERVER_HOST");
            std::env::remove_var("OPENCODE_AUTH_TOKEN");
        }

        let dir = TempDir::new().unwrap();
        let service = ConfigService::with_global_config_path(
            dir.path().to_path_buf(),
            Some(dir.path().join("home/.config/opencode/config.jsonc")),
        );

        let mut patch = Config::default();
        patch.server.host = "127.0.0.8".to_string();
        patch.server.port = 5055;
        let _ = service
            .update_scope(ConfigScope::Local, &patch)
            .await
            .unwrap();

        let (host, port) = service
            .resolve_bind(ServerBindOverrides {
                host: Some("0.0.0.0".to_string()),
                port: None,
            })
            .await
            .unwrap();

        assert_eq!(host, "0.0.0.0");
        assert_eq!(port, 5055);
    }
}
