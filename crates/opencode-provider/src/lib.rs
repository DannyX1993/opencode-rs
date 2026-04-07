//! # opencode-provider
//!
//! [`LanguageModel`] trait, model registry, streaming normaliser, retry/circuit
//! breaker, and concrete provider implementations (Anthropic, OpenAI, Google, …).
//!
//! Phase 0 exposes the trait surface only.
//! Full implementations arrive in Phase 2.

#![warn(missing_docs)]

pub mod anthropic;
pub mod auth;
pub mod catalog;
pub mod error;
pub mod google;
pub mod openai;
pub mod registry;
pub mod sse;
pub mod types;

pub use anthropic::AnthropicProvider;
pub use auth::{AuthResolver, EnvAuthResolver};
pub use catalog::CatalogCache;
pub use error::ProviderError;
pub use google::GoogleProvider;
pub use openai::OpenAiProvider;
pub use registry::ModelRegistry;
pub use sse::{SseDecoder, SseEvent, parse_events};
pub use types::{LanguageModel, ModelEvent, ModelInfo, ModelRequest};
