//! Shared server application state injected into all route handlers.

use std::{sync::Arc, time::Duration};

use crate::control_plane::proxy::HttpProxyService;
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

/// Control-plane proxy retry/timeout policy bounds used during request forwarding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyPolicy {
    /// End-to-end timeout applied per upstream attempt.
    pub timeout: Duration,
    /// Maximum retry attempts after an initial failed try.
    pub max_retries: u32,
    /// Backoff delay between retry attempts.
    pub backoff: Duration,
}

impl ProxyPolicy {
    const MIN_TIMEOUT: Duration = Duration::from_millis(100);
    const MAX_TIMEOUT: Duration = Duration::from_secs(120);
    const MAX_RETRIES: u32 = 5;
    const MIN_BACKOFF: Duration = Duration::from_millis(10);
    const MAX_BACKOFF: Duration = Duration::from_secs(5);

    /// Clamp caller-provided timeout/retry settings to safe runtime bounds.
    #[must_use]
    pub fn bounded(timeout: Duration, max_retries: u32, backoff: Duration) -> Self {
        Self {
            timeout: timeout.clamp(Self::MIN_TIMEOUT, Self::MAX_TIMEOUT),
            max_retries: max_retries.min(Self::MAX_RETRIES),
            backoff: backoff.clamp(Self::MIN_BACKOFF, Self::MAX_BACKOFF),
        }
    }
}

impl Default for ProxyPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            max_retries: 2,
            backoff: Duration::from_millis(200),
        }
    }
}

/// Startup-loaded control-plane settings shared across routing/proxy services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPlaneConfig {
    /// Unique identity for this running instance.
    pub instance_id: String,
    /// Emergency rollback switch that forces all requests to stay local.
    pub force_local_only: bool,
    /// Retry + timeout policy for remote forwarding.
    pub proxy: ProxyPolicy,
}

impl ControlPlaneConfig {
    /// Build config while normalizing an empty instance id to a safe default.
    #[must_use]
    pub fn new(instance_id: String, force_local_only: bool, proxy: ProxyPolicy) -> Self {
        let instance_id = if instance_id.trim().is_empty() {
            "local".to_string()
        } else {
            instance_id
        };

        Self {
            instance_id,
            force_local_only,
            proxy,
        }
    }
}

impl Default for ControlPlaneConfig {
    fn default() -> Self {
        Self {
            instance_id: "local".to_string(),
            force_local_only: false,
            proxy: ProxyPolicy::default(),
        }
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
    /// Control-plane runtime configuration shared by middleware/proxy layers.
    pub control_plane: ControlPlaneConfig,
    /// Shared proxy transport service used for remote forwarding.
    pub control_plane_proxy: Arc<HttpProxyService>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_plane_defaults_are_safe_for_local_only_runtime() {
        let cfg = ControlPlaneConfig::default();
        assert_eq!(cfg.instance_id, "local");
        assert!(!cfg.force_local_only);
        assert_eq!(cfg.proxy.timeout, Duration::from_secs(5));
        assert_eq!(cfg.proxy.max_retries, 2);
        assert_eq!(cfg.proxy.backoff, Duration::from_millis(200));
    }

    #[test]
    fn control_plane_proxy_policy_clamps_extreme_values() {
        let cfg = ControlPlaneConfig::new(
            "instance-a".into(),
            true,
            ProxyPolicy::bounded(Duration::from_millis(10), 99, Duration::from_millis(1)),
        );

        assert_eq!(cfg.instance_id, "instance-a");
        assert!(cfg.force_local_only);
        assert_eq!(cfg.proxy.timeout, Duration::from_millis(100));
        assert_eq!(cfg.proxy.max_retries, 5);
        assert_eq!(cfg.proxy.backoff, Duration::from_millis(10));
    }
}
