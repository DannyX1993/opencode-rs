//! # opencode-session
//!
//! Agent loop orchestration: maps user prompts to LLM streaming, tool execution,
//! persistence, and event fan-out.
//!
//! Current status: runtime-core is implemented (prompt lifecycle entry,
//! cancellation, per-session run-state, and text-delta stream projection).
//! Full parity work (tool-use execution and richer orchestration flows) is
//! intentionally deferred to follow-up slices.

#![warn(missing_docs)]

pub mod engine;
pub mod run_state;
pub mod runtime;
pub mod types;

#[cfg(test)]
mod tests {
    #[test]
    fn exposes_runtime_support_modules() {
        let _ = crate::run_state::RunState::default();
        let _ = crate::runtime::RuntimeEventSink;
    }

    #[test]
    fn can_reference_provider_types_from_session_crate() {
        let _ = opencode_provider::ModelRegistry::new();
    }
}
