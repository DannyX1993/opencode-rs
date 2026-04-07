//! Model catalog — fetches and caches the `models.dev` model list with TTL.

use crate::error::ProviderError;
use crate::types::ModelInfo;
use serde::{Deserialize, Serialize};
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
}
