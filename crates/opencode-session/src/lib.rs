//! # opencode-session
//!
//! Agent loop orchestration: maps user prompts to LLM streaming, tool execution,
//! persistence, and event fan-out.
//!
//! Current status: bounded runtime-core is implemented, including prompt
//! lifecycle entry, permission/question interactive runtimes, blocked status
//! projection, cancellation, per-session run-state, and text-delta streaming.
//! Full parity work is intentionally deferred to follow-up slices.

#![warn(missing_docs)]

pub mod engine;
pub mod permission_runtime;
pub mod question_runtime;
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

    #[test]
    fn exposes_permission_and_question_runtime_modules() {
        let _: Option<crate::permission_runtime::InMemoryPermissionRuntime> = None;
        let _: Option<crate::question_runtime::InMemoryQuestionRuntime> = None;
    }
}
