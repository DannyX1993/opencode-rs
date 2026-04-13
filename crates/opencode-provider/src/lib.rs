//! # opencode-provider
//!
//! [`LanguageModel`] trait, model registry, streaming normaliser, retry/circuit
//! breaker, and concrete provider implementations (Anthropic, OpenAI, Google, …).
//!
//! Phase 0 exposes the trait surface only.
//! Full implementations arrive in Phase 2.

#![warn(missing_docs)]

pub mod account;
pub mod anthropic;
pub mod auth;
pub mod catalog;
pub mod error;
pub mod google;
pub mod openai;
pub mod registry;
pub mod sse;
pub mod types;

pub use account::{AccountService, AccountStateDto, PersistAccountInput};
pub use anthropic::AnthropicProvider;
pub use auth::{
    AuthAuthorizationDto, AuthMethodDto, AuthMethodKind, AuthPromptDto, AuthResolver,
    AuthorizeInput, CallbackInput, EnvAuthResolver, ProviderAuthService,
};
pub use catalog::{
    CatalogCache, ConfigProvidersDto, ProviderCatalogService, ProviderInfoDto, ProviderListDto,
};
pub use error::ProviderError;
pub use google::GoogleProvider;
pub use openai::OpenAiProvider;
pub use registry::ModelRegistry;
pub use sse::{SseDecoder, SseEvent, parse_events};
pub use types::{LanguageModel, ModelEvent, ModelInfo, ModelRequest};
