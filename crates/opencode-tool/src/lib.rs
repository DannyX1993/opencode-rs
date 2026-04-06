//! # opencode-tool
//!
//! [`Tool`] trait, permission system, DAG planner, parallel executor, and all
//! 22+ built-in tools.
//!
//! Phase 0 exposes the trait surface.
//! Full implementation arrives in Phase 3.

#![warn(missing_docs)]

pub mod types;
pub mod registry;

pub use types::{Tool, ToolCall, ToolError, ToolPolicy, ToolResult};
pub use registry::ToolRegistry;
