//! # opencode-server
//!
//! Axum HTTP server providing the OpenAPI 3.1.1-compatible REST API,
//! SSE event stream, and WebSocket upgrade used by all opencode clients.
//!
//! Phase 0 wires up the router skeleton and health endpoint.
//! Full route groups are added in Phase 6.

#![warn(missing_docs)]

pub mod router;
pub mod state;
pub mod error;

pub use router::{build, serve};
pub use state::AppState;
