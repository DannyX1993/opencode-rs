//! `/api/v1/config*` route handlers.

use axum::{Json, extract::State, response::IntoResponse};
use opencode_core::{config::Config, config_service::ConfigScope};
use serde::Serialize;

use crate::{error::HttpError, routes::provider::map_provider_error, state::AppState};

#[derive(Debug, Serialize)]
struct ScopedConfigResponse {
    /// Scope marker so clients can distinguish local/global payload origin.
    scope: &'static str,
    /// Persisted payload for this scope (not fully resolved layered runtime config).
    config: Config,
}

impl ScopedConfigResponse {
    fn local(config: Config) -> Self {
        Self {
            scope: "local",
            config,
        }
    }

    fn global(config: Config) -> Self {
        Self {
            scope: "global",
            config,
        }
    }
}

/// `GET /api/v1/config` — read persisted local config payload.
///
/// This returns the raw local file view, not the fully resolved layered config.
pub async fn get_local(State(state): State<AppState>) -> impl IntoResponse {
    match state.config_service.read_scope(ConfigScope::Local).await {
        Ok(config) => Json(ScopedConfigResponse::local(config)).into_response(),
        Err(err) => HttpError::internal(err.to_string()).into_response(),
    }
}

/// `GET /api/v1/global/config` — read persisted global config payload.
///
/// This returns the raw global file view, not the fully resolved layered config.
pub async fn get_global(State(state): State<AppState>) -> impl IntoResponse {
    match state.config_service.read_scope(ConfigScope::Global).await {
        Ok(config) => Json(ScopedConfigResponse::global(config)).into_response(),
        Err(err) => HttpError::internal(err.to_string()).into_response(),
    }
}

/// `PATCH /api/v1/config` — merge/persist local config payload.
///
/// Successful writes invalidate the resolved config cache in `ConfigService`.
pub async fn patch_local(
    State(state): State<AppState>,
    Json(payload): Json<Config>,
) -> impl IntoResponse {
    match state
        .config_service
        .update_scope(ConfigScope::Local, &payload)
        .await
    {
        Ok(config) => Json(ScopedConfigResponse::local(config)).into_response(),
        Err(err) => HttpError::bad_request(err.to_string()).into_response(),
    }
}

/// `PATCH /api/v1/global/config` — merge/persist global config payload.
///
/// Successful writes invalidate the resolved config cache in `ConfigService`.
pub async fn patch_global(
    State(state): State<AppState>,
    Json(payload): Json<Config>,
) -> impl IntoResponse {
    match state
        .config_service
        .update_scope(ConfigScope::Global, &payload)
        .await
    {
        Ok(config) => Json(ScopedConfigResponse::global(config)).into_response(),
        Err(err) => HttpError::bad_request(err.to_string()).into_response(),
    }
}

/// `GET /api/v1/config/providers` — list connected config providers and defaults.
pub async fn providers(State(state): State<AppState>) -> impl IntoResponse {
    let catalog = match state.provider_catalog_view().await {
        Ok(catalog) => catalog,
        Err(err) => return HttpError::internal(err.to_string()).into_response(),
    };

    match catalog.config_providers() {
        Ok(body) => Json(body).into_response(),
        Err(err) => map_provider_error(err).into_response(),
    }
}
