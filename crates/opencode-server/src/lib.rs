//! # opencode-server
//!
//! Axum HTTP server providing the OpenAPI 3.1.1-compatible REST API,
//! SSE event stream, and WebSocket upgrade used by all opencode clients.
//!
//! Phase 0 wires up the router skeleton and health endpoint.
//! Phase 5 adds project CRUD routes under `/api/v1`.

#![warn(missing_docs)]

pub mod error;
pub mod router;
pub mod routes;
pub mod state;

pub use router::{build, serve};
pub use state::AppState;
