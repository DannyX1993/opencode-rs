//! Shared server application state injected into all route handlers.

use opencode_bus::BroadcastBus;
use opencode_core::config::Config;
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
}
