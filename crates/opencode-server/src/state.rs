//! Shared server application state injected into all route handlers.

use std::{sync::Arc, time::Duration};

use opencode_bus::BroadcastBus;
use opencode_core::{config::Config, config_service::ConfigService, error::ConfigError};
use opencode_provider::{
    AccountService, ModelRegistry, ProviderAuthService, ProviderCatalogService, types::ModelInfo,
};
use opencode_session::{
    engine::Session, permission_runtime::PermissionRuntime, question_runtime::QuestionRuntime,
};
use opencode_storage::Storage;
use tokio::sync::Notify;

/// Heartbeat source used by the SSE event route.
#[derive(Clone)]
pub enum EventHeartbeat {
    /// Emit heartbeats on a fixed idle interval in production.
    Interval(Duration),
    /// Emit heartbeats only when explicitly triggered by tests.
    Manual(Arc<Notify>),
}

impl Default for EventHeartbeat {
    fn default() -> Self {
        Self::Interval(Duration::from_secs(15))
    }
}

/// Shared state cloned into every Axum request handler.
#[derive(Clone)]
pub struct AppState {
    /// Shared runtime config service.
    pub config_service: Arc<ConfigService>,
    /// In-process event bus.
    pub bus: Arc<BroadcastBus>,
    /// SSE heartbeat driver for `/api/v1/event`.
    pub event_heartbeat: EventHeartbeat,
    /// Persistent storage facade.
    pub storage: Arc<dyn Storage>,
    /// Session engine.
    pub session: Arc<dyn Session>,
    /// Permission runtime service shared with the session engine.
    pub permission_runtime: Arc<dyn PermissionRuntime>,
    /// Question runtime service shared with the session engine.
    pub question_runtime: Arc<dyn QuestionRuntime>,
    /// LLM provider registry (may be empty when harness is disabled).
    pub registry: Arc<ModelRegistry>,
    /// Cached provider model metadata loaded at startup.
    pub provider_catalog_models: Arc<Vec<ModelInfo>>,
    /// Provider auth discovery + oauth orchestration service.
    pub provider_auth: Arc<ProviderAuthService>,
    /// Provider account persistence + active-state service.
    pub provider_accounts: Arc<AccountService>,
    /// When `true`, the manual provider harness route is active.
    /// Set by reading `OPENCODE_MANUAL_HARNESS=1` at startup.
    pub harness: bool,
}

impl AppState {
    /// Resolve latest layered runtime config through [`ConfigService`].
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when config files/env overrides cannot be read
    /// or validated.
    pub async fn resolved_config(&self) -> Result<Config, ConfigError> {
        self.config_service.resolve().await
    }

    /// Build a provider catalog view from the latest resolved config.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when resolving runtime config fails.
    pub async fn provider_catalog_view(&self) -> Result<ProviderCatalogService, ConfigError> {
        let cfg = self.resolved_config().await?;
        Ok(ProviderCatalogService::new_with_models(
            cfg,
            self.provider_catalog_models.as_ref().clone(),
        ))
    }
}
