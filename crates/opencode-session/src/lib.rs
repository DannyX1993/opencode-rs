//! # opencode-session
//!
//! Agent loop orchestration: maps user prompts to LLM streaming, tool execution,
//! persistence, and event fan-out.
//!
//! Phase 0 exposes the public trait surface and stub types.
//! Full implementation arrives in Phase 4.

#![warn(missing_docs)]

pub mod types;
pub mod engine;
