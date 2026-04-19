//! # opencode-server
//!
//! Axum HTTP server for the Rust workspace.
//!
//! Currently includes:
//! - liveness route (`/health`)
//! - project/session/message REST routes under `/api/v1`
//! - runtime session entrypoints (`/api/v1/sessions/:sid/prompt|cancel`)
//! - workspace control-plane middleware for selector-based local/forward routing
//! - env-gated manual provider stream harness (`/api/v1/provider/stream`)
//!
//! API and behavior are intentionally evolving while runtime parity work is in
//! progress.

#![warn(missing_docs)]

pub mod control_plane;
pub mod error;
pub mod router;
pub mod routes;
pub mod state;

pub use router::{build, serve};
pub use state::AppState;
