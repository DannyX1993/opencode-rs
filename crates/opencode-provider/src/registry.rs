//! Model registry — a thread-safe map of provider id → `Arc<dyn LanguageModel>`.

use crate::error::ProviderError;
use crate::types::{LanguageModel, ModelInfo};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

/// Thread-safe registry of all registered providers.
pub struct ModelRegistry {
    providers: RwLock<HashMap<String, Arc<dyn LanguageModel>>>,
}

impl ModelRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
        }
    }

    /// Register a provider under `id`.
    pub async fn register(&self, id: impl Into<String>, model: Arc<dyn LanguageModel>) {
        self.providers.write().await.insert(id.into(), model);
    }

    /// Retrieve a registered provider.
    pub async fn get(&self, id: &str) -> Option<Arc<dyn LanguageModel>> {
        self.providers.read().await.get(id).cloned()
    }

    /// List all registered provider ids.
    pub async fn list_providers(&self) -> Vec<String> {
        self.providers.read().await.keys().cloned().collect()
    }

    /// Aggregate model list from all providers.
    ///
    /// # Errors
    ///
    /// Returns the first provider error encountered.
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let guard = self.providers.read().await;
        let mut out = Vec::new();
        for p in guard.values() {
            out.extend(p.models().await?);
        }
        Ok(out)
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ProviderError;
    use crate::types::{LanguageModel, ModelEvent, ModelInfo, ModelRequest};
    use async_trait::async_trait;
    use opencode_core::context::BoxStream;
    use std::sync::Arc;

    struct Stub;

    #[async_trait]
    impl LanguageModel for Stub {
        fn provider(&self) -> &'static str {
            "stub"
        }

        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![ModelInfo {
                id: "stub/model-1".into(),
                name: "Model One".into(),
                context_window: 4096,
                max_output: 1024,
                vision: false,
            }])
        }

        async fn stream(
            &self,
            _req: ModelRequest,
        ) -> Result<BoxStream<Result<ModelEvent, ProviderError>>, ProviderError> {
            Err(ProviderError::Http("stub".into(), "not implemented".into()))
        }
    }

    #[tokio::test]
    async fn register_and_get() {
        let reg = ModelRegistry::new();
        reg.register("stub", Arc::new(Stub)).await;
        assert!(reg.get("stub").await.is_some());
        assert!(reg.get("missing").await.is_none());
    }

    #[tokio::test]
    async fn list_providers() {
        let reg = ModelRegistry::new();
        reg.register("a", Arc::new(Stub)).await;
        reg.register("b", Arc::new(Stub)).await;
        let mut providers = reg.list_providers().await;
        providers.sort();
        assert_eq!(providers, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn list_models_aggregates() {
        let reg = ModelRegistry::new();
        reg.register("stub", Arc::new(Stub)).await;
        let models = reg.list_models().await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "stub/model-1");
    }

    #[tokio::test]
    async fn empty_registry_list_models() {
        let reg = ModelRegistry::new();
        let models = reg.list_models().await.unwrap();
        assert!(models.is_empty());
    }
}
