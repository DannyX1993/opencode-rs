//! # opencode-tool
//!
//! [`Tool`] trait, permission system, DAG planner, parallel executor, and all
//! 22+ built-in tools.
//!
//! Phase 0 exposes the trait surface.
//! Phase 3 adds built-in tools and the shared execution kernel.

#![warn(missing_docs)]

pub mod common;
pub mod tools;
pub mod types;
pub mod registry;

pub use common::Ctx;
pub use types::{Tool, ToolCall, ToolError, ToolPolicy, ToolResult};
pub use registry::ToolRegistry;
