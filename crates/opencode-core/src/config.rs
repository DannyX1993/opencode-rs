//! JSONC configuration loading with cascading overrides.
//!
//! Load order (each later source overrides earlier):
//! 1. `~/.config/opencode/config.jsonc` — global user config
//! 2. `.opencode/config.jsonc`          — project-local config
//! 3. Environment variables             — `OPENCODE_*` prefix
//!
//! # Examples
//!
//! ```no_run
//! use opencode_core::config::Config;
//!
//! # async fn run() -> anyhow::Result<()> {
//! let cfg = Config::load(std::path::Path::new(".")).await?;
//! println!("model: {:?}", cfg.model);
//! # Ok(())
//! # }
//! ```

use crate::error::ConfigError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Root configuration structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Default LLM model identifier (e.g. `"anthropic/claude-opus-4-5"`).
    #[serde(default)]
    pub model: Option<String>,

    /// Provider-specific API keys and settings.
    #[serde(default)]
    pub providers: ProvidersConfig,

    /// Provider ids hidden from public provider/config listings.
    #[serde(default)]
    pub disabled_providers: Option<Vec<String>>,

    /// Optional provider allow-list applied to public provider/config listings.
    #[serde(default)]
    pub enabled_providers: Option<Vec<String>>,

    /// Server / transport settings.
    #[serde(default)]
    pub server: ServerConfig,

    /// Tracing/log level override (default: `"info"`).
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Whether to emit structured JSON logs.
    #[serde(default)]
    pub log_json: bool,

    /// Working directory override; defaults to `cwd`.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: None,
            providers: ProvidersConfig::default(),
            disabled_providers: None,
            enabled_providers: None,
            server: ServerConfig::default(),
            log_level: default_log_level(),
            log_json: false,
            cwd: None,
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

/// Provider credentials and settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProvidersConfig {
    /// Anthropic API key (also read from `ANTHROPIC_API_KEY`).
    #[serde(default)]
    pub anthropic: Option<String>,

    /// OpenAI API key (also read from `OPENAI_API_KEY`).
    #[serde(default)]
    pub openai: Option<String>,

    /// Google / Gemini API key (also read from `GOOGLE_API_KEY`).
    #[serde(default)]
    pub google: Option<String>,

    /// Generic catch-all for other providers keyed by provider id.
    #[serde(default)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

/// HTTP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// TCP port to listen on (default: `4141`).
    #[serde(default = "default_port")]
    pub port: u16,

    /// Bind address (default: `"127.0.0.1"`).
    #[serde(default = "default_host")]
    pub host: String,

    /// Optional shared secret for BasicAuth.
    #[serde(default)]
    pub auth_token: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
            auth_token: None,
        }
    }
}

fn default_port() -> u16 {
    4141
}
fn default_host() -> String {
    "127.0.0.1".to_string()
}

impl Config {
    /// Load and merge config from all sources for `project_dir`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if any JSONC file fails to parse.
    pub async fn load(project_dir: &Path) -> Result<Self, ConfigError> {
        let mut merged = Self::default();

        // 1. Global user config
        if let Some(home) = dirs_home() {
            let global = home.join(".config").join("opencode").join("config.jsonc");
            if global.exists() {
                let partial = load_jsonc(&global).await?;
                merged = merge(merged, partial);
            }
        }

        // 2. Project-local config
        let local = project_dir.join(".opencode").join("config.jsonc");
        if local.exists() {
            let partial = load_jsonc(&local).await?;
            merged = merge(merged, partial);
        }

        // 3. Environment variables
        apply_env(&mut merged);

        Ok(merged)
    }
}

/// Read and parse a JSONC file into a `Config`.
async fn load_jsonc(path: &Path) -> Result<Config, ConfigError> {
    let text = fs::read_to_string(path).await?;
    let json: serde_json::Value =
        jsonc_parser::parse_to_serde_value::<serde_json::Value>(&text, &Default::default())
            .map_err(|e| ConfigError::Parse {
                path: path.display().to_string(),
                msg: format!("{e:?}"),
            })?;
    serde_json::from_value(json).map_err(|e| ConfigError::Parse {
        path: path.display().to_string(),
        msg: e.to_string(),
    })
}

/// Deep-merge `b` into `a` (fields in `b` win where `Some`).
fn merge(mut a: Config, b: Config) -> Config {
    if b.model.is_some() {
        a.model = b.model;
    }
    if b.disabled_providers.is_some() {
        a.disabled_providers = b.disabled_providers;
    }
    if b.enabled_providers.is_some() {
        a.enabled_providers = b.enabled_providers;
    }
    if b.log_level != "info" {
        a.log_level = b.log_level;
    }
    if b.log_json {
        a.log_json = b.log_json;
    }
    if b.cwd.is_some() {
        a.cwd = b.cwd;
    }
    if b.server.auth_token.is_some() {
        a.server.auth_token = b.server.auth_token;
    }

    if let Some(v) = b.providers.anthropic {
        a.providers.anthropic = Some(v);
    }
    if let Some(v) = b.providers.openai {
        a.providers.openai = Some(v);
    }
    if let Some(v) = b.providers.google {
        a.providers.google = Some(v);
    }
    a.providers.extra.extend(b.providers.extra);
    a
}

/// Override config fields from `OPENCODE_*` environment variables.
fn apply_env(cfg: &mut Config) {
    if let Ok(v) = std::env::var("OPENCODE_MODEL") {
        cfg.model = Some(v);
    }
    if let Ok(v) = std::env::var("OPENCODE_LOG_LEVEL") {
        cfg.log_level = v;
    }
    if let Ok(v) = std::env::var("ANTHROPIC_API_KEY") {
        cfg.providers.anthropic = Some(v);
    }
    if let Ok(v) = std::env::var("OPENAI_API_KEY") {
        cfg.providers.openai = Some(v);
    }
    if let Ok(v) = std::env::var("GOOGLE_API_KEY") {
        cfg.providers.google = Some(v);
    }
    if let Ok(v) = std::env::var("OPENCODE_SERVER_PORT") {
        if let Ok(p) = v.parse() {
            cfg.server.port = p;
        }
    }
    if let Ok(v) = std::env::var("OPENCODE_AUTH_TOKEN") {
        cfg.server.auth_token = Some(v);
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_jsonc(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        let mut f = std::fs::File::create(path).unwrap();
        write!(f, "{content}").unwrap();
    }

    #[tokio::test]
    async fn defaults_load_ok() {
        let dir = TempDir::new().unwrap();
        let cfg = Config::load(dir.path()).await.unwrap();
        assert_eq!(cfg.log_level, "info");
        assert_eq!(cfg.server.port, 4141);
    }

    #[tokio::test]
    async fn local_overrides_default() {
        let dir = TempDir::new().unwrap();
        let oc = dir.path().join(".opencode");
        std::fs::create_dir_all(&oc).unwrap();
        write_jsonc(
            &oc,
            "config.jsonc",
            r#"{ "model": "openai/gpt-4o", "log_level": "debug" }"#,
        );
        let cfg = Config::load(dir.path()).await.unwrap();
        assert_eq!(cfg.model.as_deref(), Some("openai/gpt-4o"));
        assert_eq!(cfg.log_level, "debug");
    }

    #[tokio::test]
    async fn env_var_overrides() {
        // SAFETY: test-only; single-threaded tokio test runner.
        unsafe { std::env::set_var("OPENCODE_MODEL", "anthropic/claude-opus-4-5") };
        let dir = TempDir::new().unwrap();
        let cfg = Config::load(dir.path()).await.unwrap();
        assert_eq!(cfg.model.as_deref(), Some("anthropic/claude-opus-4-5"));
        unsafe { std::env::remove_var("OPENCODE_MODEL") };
    }

    #[tokio::test]
    async fn jsonc_comments_are_stripped() {
        let dir = TempDir::new().unwrap();
        let oc = dir.path().join(".opencode");
        std::fs::create_dir_all(&oc).unwrap();
        write_jsonc(&oc, "config.jsonc", r#"{ /* comment */ "log_json": true }"#);
        let cfg = Config::load(dir.path()).await.unwrap();
        assert!(cfg.log_json);
    }

    #[tokio::test]
    async fn provider_keys_merge() {
        let dir = TempDir::new().unwrap();
        let oc = dir.path().join(".opencode");
        std::fs::create_dir_all(&oc).unwrap();
        write_jsonc(
            &oc,
            "config.jsonc",
            r#"{ "providers": { "anthropic": "sk-ant-123", "openai": "sk-oai-456", "google": "gk-789" } }"#,
        );
        let cfg = Config::load(dir.path()).await.unwrap();
        assert_eq!(cfg.providers.anthropic.as_deref(), Some("sk-ant-123"));
        assert_eq!(cfg.providers.openai.as_deref(), Some("sk-oai-456"));
        assert_eq!(cfg.providers.google.as_deref(), Some("gk-789"));
    }

    #[tokio::test]
    async fn provider_filters_merge_from_local_config() {
        let dir = TempDir::new().unwrap();
        let oc = dir.path().join(".opencode");
        std::fs::create_dir_all(&oc).unwrap();
        write_jsonc(
            &oc,
            "config.jsonc",
            r#"{ "disabled_providers": ["google"], "enabled_providers": ["openai", "anthropic"] }"#,
        );
        let cfg = Config::load(dir.path()).await.unwrap();
        assert_eq!(cfg.disabled_providers, Some(vec!["google".to_string()]));
        assert_eq!(
            cfg.enabled_providers,
            Some(vec!["openai".to_string(), "anthropic".to_string()])
        );
    }

    #[tokio::test]
    async fn provider_env_vars_override() {
        unsafe {
            std::env::set_var("ANTHROPIC_API_KEY", "ant-env");
            std::env::set_var("OPENAI_API_KEY", "oai-env");
            std::env::set_var("GOOGLE_API_KEY", "ggl-env");
        }
        let dir = TempDir::new().unwrap();
        let cfg = Config::load(dir.path()).await.unwrap();
        assert_eq!(cfg.providers.anthropic.as_deref(), Some("ant-env"));
        assert_eq!(cfg.providers.openai.as_deref(), Some("oai-env"));
        assert_eq!(cfg.providers.google.as_deref(), Some("ggl-env"));
        unsafe {
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("GOOGLE_API_KEY");
        }
    }

    #[tokio::test]
    async fn server_port_env_var() {
        unsafe { std::env::set_var("OPENCODE_SERVER_PORT", "9999") };
        let dir = TempDir::new().unwrap();
        let cfg = Config::load(dir.path()).await.unwrap();
        assert_eq!(cfg.server.port, 9999);
        unsafe { std::env::remove_var("OPENCODE_SERVER_PORT") };
    }

    #[tokio::test]
    async fn auth_token_env_var() {
        unsafe { std::env::set_var("OPENCODE_AUTH_TOKEN", "mysecret") };
        let dir = TempDir::new().unwrap();
        let cfg = Config::load(dir.path()).await.unwrap();
        assert_eq!(cfg.server.auth_token.as_deref(), Some("mysecret"));
        unsafe { std::env::remove_var("OPENCODE_AUTH_TOKEN") };
    }

    #[tokio::test]
    async fn log_level_env_var() {
        unsafe { std::env::set_var("OPENCODE_LOG_LEVEL", "warn") };
        let dir = TempDir::new().unwrap();
        let cfg = Config::load(dir.path()).await.unwrap();
        assert_eq!(cfg.log_level, "warn");
        unsafe { std::env::remove_var("OPENCODE_LOG_LEVEL") };
    }

    #[tokio::test]
    async fn cwd_field_merges() {
        let dir = TempDir::new().unwrap();
        let oc = dir.path().join(".opencode");
        std::fs::create_dir_all(&oc).unwrap();
        write_jsonc(&oc, "config.jsonc", r#"{ "cwd": "/some/path" }"#);
        let cfg = Config::load(dir.path()).await.unwrap();
        assert!(cfg.cwd.is_some());
    }

    #[tokio::test]
    async fn invalid_jsonc_returns_error() {
        let dir = TempDir::new().unwrap();
        let oc = dir.path().join(".opencode");
        std::fs::create_dir_all(&oc).unwrap();
        write_jsonc(&oc, "config.jsonc", "{ bad json !!!}");
        let result = Config::load(dir.path()).await;
        assert!(result.is_err());
    }
}
