//! Shared server application state injected into all route handlers.

use std::{sync::Arc, time::Duration};

use opencode_bus::BroadcastBus;
use opencode_core::config::Config;
use opencode_provider::{
    AccountService, ModelRegistry, ProviderAuthService, ProviderCatalogService,
};
use opencode_session::engine::Session;
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
    /// Loaded configuration.
    pub config: Arc<Config>,
    /// In-process event bus.
    pub bus: Arc<BroadcastBus>,
    /// SSE heartbeat driver for `/api/v1/event`.
    pub event_heartbeat: EventHeartbeat,
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
