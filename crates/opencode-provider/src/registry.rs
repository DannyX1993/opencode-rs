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
    use crate::auth::{AuthResolver, EnvAuthResolver};
    use crate::error::ProviderError;
    use crate::types::{
        ContentPart, LanguageModel, ModelEvent, ModelInfo, ModelMessage, ModelRequest,
    };
    use async_trait::async_trait;
    use futures::StreamExt;
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

    fn simple_req(model: &str) -> ModelRequest {
        ModelRequest {
            model: model.into(),
            system: vec![],
            messages: vec![ModelMessage {
                role: "user".into(),
                content: vec![ContentPart::Text { text: "hi".into() }],
            }],
            tools: Default::default(),
            max_tokens: Some(16),
            temperature: None,
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

    // ── Phase 6: registry integration tests with real provider impls ─────────

    // RED 6.1 — stream openai provider through registry, wiremock SSE
    #[tokio::test]
    async fn registry_stream_openai_yields_text() {
        use crate::openai::OpenAiProvider;
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path as wm_path},
        };

        let srv = MockServer::start().await;
        let fixture = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        Mock::given(method("POST"))
            .and(wm_path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(fixture),
            )
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "openai",
            "OPENAI_KEY_NONEXISTENT_REG",
            Some("key".into()),
        ));
        let reg = ModelRegistry::new();
        reg.register(
            "openai",
            Arc::new(OpenAiProvider::with_base_url(auth, srv.uri())),
        )
        .await;

        let provider = reg.get("openai").await.unwrap();
        let mut stream = provider.stream(simple_req("gpt-4o")).await.unwrap();

        let mut text = String::new();
        let mut done = false;
        while let Some(ev) = stream.next().await {
            match ev.unwrap() {
                ModelEvent::TextDelta { delta } => text.push_str(&delta),
                ModelEvent::Done { .. } => done = true,
                _ => {}
            }
        }
        assert_eq!(text, "hi");
        assert!(done);
    }

    // RED 6.2 — stream anthropic provider through registry, wiremock SSE
    #[tokio::test]
    async fn registry_stream_anthropic_yields_text() {
        use crate::anthropic::AnthropicProvider;
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path as wm_path},
        };

        let srv = MockServer::start().await;
        // Anthropic SSE format: event: + data: pairs, followed by message_stop
        let fixture = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01\",\"model\":\"claude-3-5-sonnet-20241022\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hey\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        Mock::given(method("POST"))
            .and(wm_path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(fixture),
            )
            .mount(&srv)
            .await;

        let auth: Arc<dyn AuthResolver> = Arc::new(EnvAuthResolver::new(
            "anthropic",
            "ANTHROPIC_KEY_NONEXISTENT_REG",
            Some("key".into()),
        ));
        let reg = ModelRegistry::new();
        reg.register(
            "anthropic",
            Arc::new(AnthropicProvider::with_base_url(auth, srv.uri())),
        )
        .await;

        let provider = reg.get("anthropic").await.unwrap();
        let mut stream = provider
            .stream(simple_req("claude-3-5-sonnet-20241022"))
            .await
            .unwrap();

        let mut text = String::new();
        let mut done = false;
        while let Some(ev) = stream.next().await {
            match ev.unwrap() {
                ModelEvent::TextDelta { delta } => text.push_str(&delta),
                ModelEvent::Done { .. } => done = true,
                _ => {}
            }
        }
        assert_eq!(text, "hey");
        assert!(done);
    }

    // RED 6.3 — unknown provider id returns None from registry
    #[tokio::test]
    async fn registry_get_unknown_returns_none() {
        let reg = ModelRegistry::new();
        assert!(reg.get("does-not-exist").await.is_none());
    }

    // RED 6.4 — registry replaces provider when registered under same id
    #[tokio::test]
    async fn registry_register_replaces_provider() {
        let reg = ModelRegistry::new();
        reg.register("stub", Arc::new(Stub)).await;
        reg.register("stub", Arc::new(Stub)).await; // overwrite
        let providers = reg.list_providers().await;
        assert_eq!(providers.len(), 1);
    }
}
