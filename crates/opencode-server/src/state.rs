//! Shared server application state injected into all route handlers.

use opencode_bus::BroadcastBus;
use opencode_core::config::Config;
use opencode_provider::{
    AccountService, ModelRegistry, ProviderAuthService, ProviderCatalogService,
};
use opencode_session::engine::Session;
use opencode_storage::Storage;
use std::sync::Arc;

/// Shared state cloned into every Axum request handler.
#[derive(Clone)]
pub struct AppState {
    /// Loaded configuration.
    pub config: Arc<Config>,
    /// In-process event bus.
    pub bus: Arc<BroadcastBus>,
    /// Persistent storage facade.
    pub storage: Arc<dyn Storage>,
    /// Session engine.
    pub session: Arc<dyn Session>,
    /// LLM provider registry (may be empty when harness is disabled).
    pub registry: Arc<ModelRegistry>,
    /// Provider catalog service for public provider/config metadata.
    pub provider_catalog: Arc<ProviderCatalogService>,
    /// Provider auth discovery + oauth orchestration service.
    pub provider_auth: Arc<ProviderAuthService>,
    /// Provider account persistence + active-state service.
    pub provider_accounts: Arc<AccountService>,
    /// When `true`, the manual provider harness route is active.
    /// Set by reading `OPENCODE_MANUAL_HARNESS=1` at startup.
    pub harness: bool,
}
