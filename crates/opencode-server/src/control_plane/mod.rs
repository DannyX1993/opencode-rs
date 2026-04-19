//! Workspace control-plane middleware and routing decision service.

pub mod error;
pub mod observability;
pub mod policy;
pub mod proxy;
pub mod resolver;

use std::sync::Arc;

use axum::{
    body::Body,
    extract::State,
    http::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use opencode_core::id::WorkspaceId;
use opencode_storage::Storage;
use tracing::{debug, warn};

use crate::{
    routes::workspace::{WorkspaceTargetMetadata, parse_workspace_target},
    state::{AppState, ControlPlaneConfig},
};

use self::{
    error::ControlPlaneError,
    policy::{PolicyAction, RoutePolicy},
    proxy::is_websocket_upgrade,
    resolver::resolve_selector,
};

/// Source used for selector resolution precedence diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectorSource {
    /// Selector came from `?workspace=` query.
    Query,
    /// Selector came from `x-opencode-workspace` header.
    Header,
}

impl SelectorSource {
    /// Static representation for logs and forwarded context headers.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Header => "header",
        }
    }
}

/// Selector token resolved from the incoming request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSelector {
    /// Raw selector payload (workspace id string).
    pub raw: String,
    /// Source used for precedence and observability.
    pub source: SelectorSource,
}

/// Remote destination metadata required for forwarding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTarget {
    /// Selected workspace id.
    pub workspace_id: WorkspaceId,
    /// Destination instance identity.
    pub instance_id: String,
    /// Destination control-plane base URL.
    pub base_url: String,
}

/// Local-vs-forward routing outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingDecision {
    /// Handle request in this process.
    Local,
    /// Forward request to the resolved remote target.
    Forward(RemoteTarget),
}

/// Stateless control-plane routing service.
#[derive(Clone)]
pub struct ControlPlaneService {
    storage: Arc<dyn Storage>,
    config: ControlPlaneConfig,
}

impl ControlPlaneService {
    /// Construct a routing service from storage + runtime config.
    #[must_use]
    pub fn new(storage: Arc<dyn Storage>, config: ControlPlaneConfig) -> Self {
        Self { storage, config }
    }

    /// Resolve workspace selector into a local/forward routing decision.
    pub async fn decide(
        &self,
        selector: &WorkspaceSelector,
    ) -> Result<RoutingDecision, ControlPlaneError> {
        if self.config.force_local_only {
            debug!(selector = %selector.raw, "control-plane local-only override active");
            return Ok(RoutingDecision::Local);
        }

        let workspace_id = selector.raw.parse::<WorkspaceId>().map_err(|_| {
            ControlPlaneError::SelectorMalformed {
                message: "workspace selector must be a valid workspace id".into(),
            }
        })?;

        let workspace = self
            .storage
            .get_workspace(workspace_id)
            .await
            .map_err(|err| ControlPlaneError::Internal {
                message: format!("workspace lookup failed: {err}"),
            })?
            .ok_or_else(|| ControlPlaneError::WorkspaceNotFound {
                selector: selector.raw.clone(),
            })?;

        let target =
            parse_workspace_target(&workspace.r#type, workspace.extra.as_ref()).map_err(|err| {
                ControlPlaneError::WorkspaceMetadataInvalid {
                    selector: selector.raw.clone(),
                    message: err.to_string(),
                }
            })?;

        let decision = match target {
            WorkspaceTargetMetadata::Other => RoutingDecision::Local,
            WorkspaceTargetMetadata::Remote(remote)
                if remote.instance == self.config.instance_id =>
            {
                RoutingDecision::Local
            }
            WorkspaceTargetMetadata::Remote(remote) => RoutingDecision::Forward(RemoteTarget {
                workspace_id,
                instance_id: remote.instance,
                base_url: remote.base_url,
            }),
        };

        Ok(decision)
    }
}

/// Middleware that applies selector resolution + routing decision before handlers.
///
/// Non-eligible paths pass through unchanged. Eligible paths without selector
/// also pass through, preserving local behavior parity.
pub async fn middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let policy = RoutePolicy;
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    if policy.classify(&method, &path) == PolicyAction::LocalOnly {
        return next.run(request).await;
    }

    let selector = match resolve_selector(request.uri(), request.headers()) {
        Ok(Some(selector)) => selector,
        Ok(None) => return next.run(request).await,
        Err(err) => return err.into_response(),
    };

    let service = ControlPlaneService::new(Arc::clone(&state.storage), state.control_plane.clone());
    let decision = match service.decide(&selector).await {
        Ok(decision) => decision,
        Err(err) => return err.into_response(),
    };

    match decision {
        RoutingDecision::Local => {
            observability::global_metrics().record_local_decision(&selector, &method, &path);
            next.run(request).await
        }
        RoutingDecision::Forward(target) => {
            let metrics = observability::global_metrics();
            metrics.record_forward_decision(&selector, &method, &path, &target);
            // Parity is intentionally deferred for this slice: once we decide to
            // forward, websocket upgrades return a deterministic 501 instead of
            // attempting a partial HTTP fallback that could hide behavior drift.
            if is_websocket_upgrade(request.headers()) {
                return ControlPlaneError::WebSocketForwardingDeferred.into_response();
            }

            match state
                .control_plane_proxy
                .forward(request, &target, &selector, metrics)
                .await
            {
                Ok(response) => response,
                Err(err) => {
                    warn!(error = %err, "control-plane forwarding failed");
                    err.into_response()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use opencode_core::{
        dto::{ProjectRow, WorkspaceRow},
        id::{ProjectId, WorkspaceId},
    };
    use opencode_storage::{Storage, StorageImpl, connect};

    async fn storage_with_workspace(instance: &str) -> (Arc<dyn Storage>, WorkspaceId) {
        let db_path = std::env::temp_dir().join(format!(
            "opencode-control-plane-test-{}.db",
            WorkspaceId::new()
        ));
        let pool = connect(&db_path).await.unwrap();
        let storage: Arc<dyn Storage> = Arc::new(StorageImpl::new(pool));
        let project_id = ProjectId::new();
        storage
            .upsert_project(ProjectRow {
                id: project_id,
                worktree: "/tmp/project".into(),
                vcs: Some("git".into()),
                name: Some("project".into()),
                icon_url: None,
                icon_color: None,
                time_created: 1,
                time_updated: 1,
                time_initialized: None,
                sandboxes: serde_json::json!([]),
                commands: None,
            })
            .await
            .unwrap();

        let workspace_id = WorkspaceId::new();
        storage
            .upsert_workspace(WorkspaceRow {
                id: workspace_id,
                r#type: "remote".into(),
                branch: None,
                name: Some("alpha".into()),
                directory: None,
                extra: Some(
                    serde_json::json!({"instance": instance, "base_url": "https://remote.example"}),
                ),
                project_id,
            })
            .await
            .unwrap();
        (storage, workspace_id)
    }

    #[tokio::test]
    async fn decide_returns_local_for_same_instance_workspace() {
        let (storage, workspace_id) = storage_with_workspace("cp-local").await;
        let service = ControlPlaneService::new(
            storage,
            ControlPlaneConfig::new(
                "cp-local".into(),
                false,
                crate::state::ProxyPolicy::default(),
            ),
        );
        let selector = WorkspaceSelector {
            raw: workspace_id.to_string(),
            source: SelectorSource::Query,
        };

        assert_eq!(
            service.decide(&selector).await.unwrap(),
            RoutingDecision::Local
        );
    }

    #[tokio::test]
    async fn decide_returns_forward_for_remote_instance_workspace() {
        let (storage, workspace_id) = storage_with_workspace("cp-remote").await;
        let service = ControlPlaneService::new(
            storage,
            ControlPlaneConfig::new(
                "cp-local".into(),
                false,
                crate::state::ProxyPolicy::default(),
            ),
        );
        let selector = WorkspaceSelector {
            raw: workspace_id.to_string(),
            source: SelectorSource::Header,
        };

        let decision = service.decide(&selector).await.unwrap();
        match decision {
            RoutingDecision::Forward(target) => {
                assert_eq!(target.workspace_id, workspace_id);
                assert_eq!(target.instance_id, "cp-remote");
            }
            RoutingDecision::Local => panic!("remote workspace should forward"),
        }
    }

    #[tokio::test]
    async fn decide_honors_force_local_override() {
        let (storage, workspace_id) = storage_with_workspace("cp-remote").await;
        let service = ControlPlaneService::new(
            storage,
            ControlPlaneConfig::new(
                "cp-local".into(),
                true,
                crate::state::ProxyPolicy::default(),
            ),
        );
        let selector = WorkspaceSelector {
            raw: workspace_id.to_string(),
            source: SelectorSource::Header,
        };

        assert_eq!(
            service.decide(&selector).await.unwrap(),
            RoutingDecision::Local
        );
    }
}
