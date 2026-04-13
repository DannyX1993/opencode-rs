//! Model catalog — fetches and caches the `models.dev` model list with TTL.

use crate::error::ProviderError;
use crate::types::ModelInfo;
use opencode_core::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// TTL for the on-disk catalog cache (5 minutes, matching TS source).
const TTL: Duration = Duration::from_secs(5 * 60);

/// Default catalog URL.
const DEFAULT_URL: &str = "https://models.dev";

/// A single entry from the models.dev JSON array.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct CatalogEntry {
    id: String,
    name: String,
    #[serde(default)]
    context: u32,
    #[serde(rename = "max_tokens", default)]
    max_tokens: u32,
    #[serde(default)]
    vision: bool,
    /// Provider attachment field (kept for round-trip fidelity).
    #[serde(default)]
    attachment: bool,
}

impl From<CatalogEntry> for ModelInfo {
    fn from(e: CatalogEntry) -> Self {
        ModelInfo {
            id: e.id,
            name: e.name,
            context_window: e.context,
            max_output: e.max_tokens,
            vision: e.vision,
        }
    }
}

/// File-backed, TTL-aware cache for the model catalog.
pub struct CatalogCache {
    path: PathBuf,
    url: String,
    ttl: Duration,
    client: reqwest::Client,
}

/// Provider metadata exposed by provider/config endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfoDto {
    /// Provider identifier.
    pub id: String,
    /// Human-readable provider name.
    pub name: String,
    /// Provider model catalog keyed by model id.
    pub models: BTreeMap<String, ModelInfo>,
}

/// Provider catalog response for `/api/v1/provider`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderListDto {
    /// All providers visible under config filters.
    pub all: Vec<ProviderInfoDto>,
    /// Default model id per visible provider.
    pub default: BTreeMap<String, String>,
    /// Provider ids that currently have configured connectivity.
    pub connected: Vec<String>,
}

/// Provider catalog response for `/api/v1/config/providers`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigProvidersDto {
    /// Providers that are both visible and connected.
    pub providers: Vec<ProviderInfoDto>,
    /// Default model id per connected provider.
    pub default: BTreeMap<String, String>,
}

/// Builds provider metadata independently from runtime model execution.
pub struct ProviderCatalogService {
    cfg: Config,
    providers: Vec<ProviderInfoDto>,
}

impl ProviderCatalogService {
    /// Create a catalog service for the provided config.
    #[must_use]
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            providers: builtin_providers(),
        }
    }

    /// Create a catalog service that overlays provider models from cached
    /// `models.dev` entries when available.
    #[must_use]
    pub fn new_with_models(cfg: Config, models: Vec<ModelInfo>) -> Self {
        let mut providers: BTreeMap<String, ProviderInfoDto> = builtin_providers()
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();

        for (id, entries) in providers_from_models(models) {
            let name = providers
                .get(&id)
                .map(|item| item.name.clone())
                .unwrap_or_else(|| provider_name(&id));
            providers.insert(
                id.clone(),
                ProviderInfoDto {
                    id,
                    name,
                    models: entries,
                },
            );
        }

        Self {
            cfg,
            providers: providers.into_values().collect(),
        }
    }

    /// Return all visible providers plus defaults and connectivity.
    pub fn list(&self) -> Result<ProviderListDto, ProviderError> {
        let all = self.visible_providers();
        let connected = self.connected_ids(&all);
        Ok(ProviderListDto {
            default: defaults(&all)?,
            all,
            connected,
        })
    }

    /// Return only connected providers plus defaults.
    pub fn config_providers(&self) -> Result<ConfigProvidersDto, ProviderError> {
        let all = self.visible_providers();
        let connected = self.connected_ids(&all);
        let providers: Vec<_> = all
            .into_iter()
            .filter(|item| connected.binary_search(&item.id).is_ok())
            .collect();
        Ok(ConfigProvidersDto {
            default: defaults(&providers)?,
            providers,
        })
    }

    fn visible_providers(&self) -> Vec<ProviderInfoDto> {
        let enabled = self.cfg.enabled_providers.as_ref();
        let disabled = self.cfg.disabled_providers.as_ref();
        self.providers
            .iter()
            .filter(|item| {
                enabled.is_none_or(|allowed| allowed.iter().any(|id| id == &item.id))
                    && disabled.is_none_or(|blocked| blocked.iter().all(|id| id != &item.id))
            })
            .cloned()
            .collect()
    }

    fn connected_ids(&self, providers: &[ProviderInfoDto]) -> Vec<String> {
        let mut ids: Vec<_> = providers
            .iter()
            .filter_map(|item| match item.id.as_str() {
                "anthropic"
                    if self
                        .cfg
                        .providers
                        .anthropic
                        .as_deref()
                        .is_some_and(|value| !value.is_empty()) =>
                {
                    Some(item.id.clone())
                }
                "openai"
                    if self
                        .cfg
                        .providers
                        .openai
                        .as_deref()
                        .is_some_and(|value| !value.is_empty()) =>
                {
                    Some(item.id.clone())
                }
                "google"
                    if self
                        .cfg
                        .providers
                        .google
                        .as_deref()
                        .is_some_and(|value| !value.is_empty()) =>
                {
                    Some(item.id.clone())
                }
                _ => None,
            })
            .collect();
        ids.sort();
        ids
    }
}

fn defaults(providers: &[ProviderInfoDto]) -> Result<BTreeMap<String, String>, ProviderError> {
    providers
        .iter()
        .map(|item| {
            let default = sort_models(item.models.values().cloned().collect())
                .into_iter()
                .next()
                .ok_or_else(|| {
                    ProviderError::Http(item.id.clone(), "provider has no available models".into())
                })?;
            Ok((item.id.clone(), default.id))
        })
        .collect()
}

fn sort_models(mut models: Vec<ModelInfo>) -> Vec<ModelInfo> {
    const PRIORITY: [&str; 4] = ["gpt-5", "claude-sonnet-4", "big-pickle", "gemini-3-pro"];

    models.sort_by(|left, right| {
        let left_rank = PRIORITY
            .iter()
            .position(|needle| left.id.contains(needle))
            .map_or(-1_i32, |idx| (PRIORITY.len() - idx) as i32);
        let right_rank = PRIORITY
            .iter()
            .position(|needle| right.id.contains(needle))
            .map_or(-1_i32, |idx| (PRIORITY.len() - idx) as i32);

        right_rank
            .cmp(&left_rank)
            .then_with(|| left.id.contains("latest").cmp(&right.id.contains("latest")))
            .then_with(|| right.id.cmp(&left.id))
    });
    models
}

fn builtin_providers() -> Vec<ProviderInfoDto> {
    [anthropic(), google(), openai()].into_iter().collect()
}

fn anthropic() -> ProviderInfoDto {
    ProviderInfoDto {
        id: "anthropic".into(),
        name: "Anthropic".into(),
        models: [
            model(
                "claude-sonnet-4-5",
                "Claude Sonnet 4.5",
                200_000,
                8_192,
                true,
            ),
            model("claude-3-5-haiku", "Claude 3.5 Haiku", 200_000, 8_192, true),
        ]
        .into_iter()
        .collect(),
    }
}

fn openai() -> ProviderInfoDto {
    ProviderInfoDto {
        id: "openai".into(),
        name: "OpenAI".into(),
        models: [
            model("gpt-5", "GPT-5", 200_000, 8_192, true),
            model("gpt-4o", "GPT-4o", 128_000, 4_096, true),
        ]
        .into_iter()
        .collect(),
    }
}

fn google() -> ProviderInfoDto {
    ProviderInfoDto {
        id: "google".into(),
        name: "Google".into(),
        models: [
            model("gemini-3-pro", "Gemini 3 Pro", 1_000_000, 8_192, true),
            model(
                "gemini-2.0-flash",
                "Gemini 2.0 Flash",
                1_000_000,
                8_192,
                true,
            ),
        ]
        .into_iter()
        .collect(),
    }
}

fn model(
    id: &str,
    name: &str,
    context_window: u32,
    max_output: u32,
    vision: bool,
) -> (String, ModelInfo) {
    (
        id.into(),
        ModelInfo {
            id: id.into(),
            name: name.into(),
            context_window,
            max_output,
            vision,
        },
    )
}

fn providers_from_models(models: Vec<ModelInfo>) -> BTreeMap<String, BTreeMap<String, ModelInfo>> {
    models
        .into_iter()
        .fold(BTreeMap::new(), |mut acc, mut model| {
            let raw_id = model.id.clone();
            let Some((provider, model_id)) = raw_id.split_once('/') else {
                return acc;
            };
            if provider.is_empty() || model_id.is_empty() {
                acc
            } else {
                model.id = model_id.to_string();
                acc.entry(provider.to_string())
                    .or_insert_with(BTreeMap::new)
                    .insert(model.id.clone(), model);
                acc
            }
        })
}

fn provider_name(id: &str) -> String {
    match id {
        "openai" => "OpenAI".into(),
        "anthropic" => "Anthropic".into(),
        "google" => "Google".into(),
        _ => id.to_string(),
    }
}

impl CatalogCache {
    /// Create a new cache backed by `path`, fetching from `url` with default TTL.
    pub fn new(path: impl Into<PathBuf>, url: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            url: url.into(),
            ttl: TTL,
            client: reqwest::Client::new(),
        }
    }

    /// Create using the default `models.dev` URL.
    pub fn default_url(path: impl Into<PathBuf>) -> Self {
        Self::new(path, DEFAULT_URL)
    }

    /// Override the TTL (useful for tests).
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Load model list, using the cache when fresh or re-fetching when stale/missing.
    ///
    /// Set `force = true` to bypass the TTL and always re-fetch.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] on network or parse failures.
    pub async fn load(&self, force: bool) -> Result<Vec<ModelInfo>, ProviderError> {
        if !force && self.is_fresh() {
            return self.read_cache();
        }
        let fresh = self.fetch().await?;
        self.write_cache(&fresh)?;
        Ok(fresh)
    }

    /// Load models from the on-disk cache only.
    ///
    /// Returns `Ok(None)` when the cache file does not exist.
    pub fn load_cached(&self) -> Result<Option<Vec<ModelInfo>>, ProviderError> {
        if !self.path.exists() {
            return Ok(None);
        }
        self.read_cache().map(Some)
    }

    fn is_fresh(&self) -> bool {
        let Some(m) = std::fs::metadata(&self.path).ok() else {
            return false;
        };
        let Some(mod_time) = m.modified().ok() else {
            return false;
        };
        SystemTime::now()
            .duration_since(mod_time)
            .map(|age| age < self.ttl)
            .unwrap_or(false)
    }

    fn read_cache(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let bytes = std::fs::read(&self.path).map_err(|e| ProviderError::Stream(e.to_string()))?;
        let entries: Vec<CatalogEntry> =
            serde_json::from_slice(&bytes).map_err(|e| ProviderError::Stream(e.to_string()))?;
        Ok(entries.into_iter().map(ModelInfo::from).collect())
    }

    fn write_cache(&self, models: &[ModelInfo]) -> Result<(), ProviderError> {
        let entries: Vec<CatalogEntry> = models
            .iter()
            .map(|m| CatalogEntry {
                id: m.id.clone(),
                name: m.name.clone(),
                context: m.context_window,
                max_tokens: m.max_output,
                vision: m.vision,
                attachment: false,
            })
            .collect();
        let bytes =
            serde_json::to_vec(&entries).map_err(|e| ProviderError::Stream(e.to_string()))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ProviderError::Stream(e.to_string()))?;
        }
        std::fs::write(&self.path, bytes).map_err(|e| ProviderError::Stream(e.to_string()))
    }

    async fn fetch(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let url = format!("{}/models.json", self.url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ProviderError::Http("catalog".into(), e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ProviderError::Http(
                "catalog".into(),
                format!("status {}", resp.status()),
            ));
        }
        let entries: Vec<CatalogEntry> = resp
            .json()
            .await
            .map_err(|e| ProviderError::Stream(e.to_string()))?;
        Ok(entries.into_iter().map(ModelInfo::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencode_core::config::Config;
    use std::time::Duration;
    use tempfile::tempdir;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path as wm_path},
    };

    fn fixture() -> serde_json::Value {
        serde_json::json!([
            {
                "id": "claude-3-haiku-20240307",
                "name": "Claude 3 Haiku",
                "context": 200000,
                "max_tokens": 4096,
                "vision": true
            },
            {
                "id": "gpt-4o",
                "name": "GPT-4o",
                "context": 128000,
                "max_tokens": 4096,
                "vision": true
            }
        ])
    }

    // RED 3.1 — missing cache file triggers fetch
    #[tokio::test]
    async fn missing_cache_triggers_fetch() {
        let dir = tempdir().unwrap();
        let srv = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wm_path("/models.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fixture()))
            .mount(&srv)
            .await;

        let cache = CatalogCache::new(dir.path().join("models.json"), srv.uri());
        let models = cache.load(false).await.unwrap();

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "claude-3-haiku-20240307");
        assert_eq!(models[1].id, "gpt-4o");
    }

    // RED 3.1 — fresh cache returns models without fetching
    #[tokio::test]
    async fn fresh_cache_returns_without_fetch() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("models.json");
        let srv = MockServer::start().await;

        let seeded = serde_json::json!([{
            "id": "cached-model",
            "name": "Cached",
            "context": 1000,
            "max_tokens": 512,
            "vision": false
        }]);
        std::fs::write(&p, seeded.to_string()).unwrap();

        // No mock registered — if fetch is called wiremock will return 404 and panic
        let cache = CatalogCache::new(p, srv.uri());
        let models = cache.load(false).await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "cached-model");
    }

    // RED 3.1 — stale cache (TTL=0) triggers re-fetch
    #[tokio::test]
    async fn stale_cache_triggers_refetch() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("models.json");
        let srv = MockServer::start().await;

        let stale = serde_json::json!([{
            "id": "stale-model",
            "name": "Stale",
            "context": 1000,
            "max_tokens": 512,
            "vision": false
        }]);
        std::fs::write(&p, stale.to_string()).unwrap();

        Mock::given(method("GET"))
            .and(wm_path("/models.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fixture()))
            .mount(&srv)
            .await;

        let cache = CatalogCache::new(&p, srv.uri()).with_ttl(Duration::ZERO);
        let models = cache.load(false).await.unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "claude-3-haiku-20240307");
    }

    // RED 3.2 — integration: mock GET /models.json → parsed Vec<ModelInfo>
    #[tokio::test]
    async fn fetch_parses_models_from_mock_server() {
        let srv = MockServer::start().await;
        let dir = tempdir().unwrap();
        Mock::given(method("GET"))
            .and(wm_path("/models.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fixture()))
            .mount(&srv)
            .await;

        let cache = CatalogCache::new(dir.path().join("models.json"), srv.uri());
        let models = cache.load(true).await.unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "Claude 3 Haiku");
        assert!(models[0].vision);
        assert_eq!(models[0].context_window, 200000);
    }

    // TRIANGULATE: force=true bypasses fresh cache
    #[tokio::test]
    async fn force_bypasses_fresh_cache() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("models.json");
        let srv = MockServer::start().await;

        let cached = serde_json::json!([{
            "id": "old-model",
            "name": "Old",
            "context": 1000,
            "max_tokens": 512,
            "vision": false
        }]);
        std::fs::write(&p, cached.to_string()).unwrap();

        Mock::given(method("GET"))
            .and(wm_path("/models.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fixture()))
            .mount(&srv)
            .await;

        let cache = CatalogCache::new(&p, srv.uri());
        let models = cache.load(true).await.unwrap();
        assert_eq!(models.len(), 2);
    }

    #[test]
    fn provider_catalog_filters_enabled_and_disabled_providers() {
        let cfg = Config {
            enabled_providers: Some(vec!["openai".into(), "google".into()]),
            disabled_providers: Some(vec!["google".into()]),
            ..Config::default()
        };

        let catalog = ProviderCatalogService::new(cfg);
        let listed = catalog.list().unwrap();

        let ids: Vec<_> = listed.all.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, vec!["openai"]);
        assert!(listed.default.contains_key("openai"));
        assert!(!listed.default.contains_key("google"));
    }

    #[test]
    fn provider_catalog_reports_connected_providers_from_configured_keys() {
        let cfg = Config {
            providers: opencode_core::config::ProvidersConfig {
                openai: Some("sk-openai".into()),
                google: Some("google-key".into()),
                ..Default::default()
            },
            ..Config::default()
        };

        let catalog = ProviderCatalogService::new(cfg);
        let listed = catalog.list().unwrap();

        assert_eq!(
            listed.connected,
            vec!["google".to_string(), "openai".to_string()]
        );
    }

    #[test]
    fn provider_catalog_uses_sorted_model_defaults_per_provider() {
        let catalog = ProviderCatalogService::new(Config::default());
        let listed = catalog.list().unwrap();

        assert_eq!(
            listed.default.get("anthropic"),
            Some(&"claude-sonnet-4-5".to_string())
        );
        assert_eq!(listed.default.get("openai"), Some(&"gpt-5".to_string()));
        assert_eq!(
            listed.default.get("google"),
            Some(&"gemini-3-pro".to_string())
        );
    }

    #[test]
    fn config_provider_catalog_only_includes_connected_entries() {
        let cfg = Config {
            providers: opencode_core::config::ProvidersConfig {
                anthropic: Some("sk-anthropic".into()),
                ..Default::default()
            },
            ..Config::default()
        };

        let catalog = ProviderCatalogService::new(cfg);
        let listed = catalog.config_providers().unwrap();

        assert_eq!(listed.providers.len(), 1);
        assert_eq!(listed.providers[0].id, "anthropic");
        assert_eq!(
            listed.default.get("anthropic"),
            Some(&"claude-sonnet-4-5".to_string())
        );
    }

    #[test]
    fn provider_catalog_can_overlay_models_from_cache_entries() {
        let cfg = Config {
            providers: opencode_core::config::ProvidersConfig {
                openai: Some("sk-openai".into()),
                ..Default::default()
            },
            enabled_providers: Some(vec!["openai".into()]),
            ..Config::default()
        };

        let catalog = ProviderCatalogService::new_with_models(
            cfg,
            vec![ModelInfo {
                id: "openai/gpt-cache-only".into(),
                name: "Cached model".into(),
                context_window: 32_768,
                max_output: 2_048,
                vision: true,
            }],
        );
        let listed = catalog.config_providers().unwrap();

        assert_eq!(listed.providers.len(), 1);
        assert_eq!(listed.providers[0].id, "openai");
        assert_eq!(
            listed.default.get("openai"),
            Some(&"gpt-cache-only".to_string())
        );
    }
}
