//! # opencode-provider
//!
//! [`LanguageModel`] trait, model registry, streaming normaliser, retry/circuit
//! breaker, and concrete provider implementations (Anthropic, OpenAI, Google, …).
//!
//! Phase 0 exposes the trait surface only.
//! Full implementations arrive in Phase 2.

#![warn(missing_docs)]

pub mod error;
pub mod types;
pub mod registry;

pub use types::{LanguageModel, ModelEvent, ModelInfo, ModelRequest};
pub use error::ProviderError;
pub use registry::ModelRegistry;
