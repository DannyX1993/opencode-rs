//! `/api/v1/config/providers` route handlers.

use axum::{Json, extract::State, response::IntoResponse};

use crate::{routes::provider::map_provider_error, state::AppState};

/// `GET /api/v1/config/providers` — list connected config providers and defaults.
pub async fn providers(State(state): State<AppState>) -> impl IntoResponse {
    match state.provider_catalog.config_providers() {
        Ok(body) => Json(body).into_response(),
        Err(err) => map_provider_error(err).into_response(),
    }
}
