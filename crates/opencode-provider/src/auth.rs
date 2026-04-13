//! Auth resolver — resolves provider API keys from environment or config fallback.

use crate::error::ProviderError;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Mutex};

/// Supported auth method kinds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethodKind {
    /// API key entry.
    Api,
    /// OAuth or device authorization.
    Oauth,
}

/// Prompt metadata for auth input collection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthPromptDto {
    /// Prompt input type.
    pub kind: String,
    /// Input field key.
    pub key: String,
    /// Human-readable message.
    pub message: String,
    /// Optional placeholder text.
    #[serde(default)]
    pub placeholder: Option<String>,
}

/// Public auth method descriptor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthMethodDto {
    /// Auth method kind.
    pub kind: AuthMethodKind,
    /// User-facing label.
    pub label: String,
    /// Optional prompts required before authorizing.
    #[serde(default)]
    pub prompts: Vec<AuthPromptDto>,
}

/// Input payload for starting auth.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthorizeInput {
    /// Selected auth-method index.
    pub method: usize,
    /// Prompt responses keyed by prompt id.
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
}

/// Output payload for auth authorization handoff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthAuthorizationDto {
    /// URL the client should open.
    pub url: String,
    /// Authorization completion style.
    pub method: String,
    /// Instructions shown to the user.
    pub instructions: String,
}

/// Input payload for auth callback completion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallbackInput {
    /// Selected auth-method index.
    pub method: usize,
    /// Optional callback code.
    #[serde(default)]
    pub code: Option<String>,
}

#[derive(Debug, Clone)]
struct PendingFlow {
    method: usize,
    code: Option<String>,
}

/// Built-in provider auth discovery and in-process OAuth handshake manager.
pub struct ProviderAuthService {
    pending: Mutex<BTreeMap<String, PendingFlow>>,
}

impl ProviderAuthService {
    /// Create a new auth service.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(BTreeMap::new()),
        }
    }

    /// Return supported auth methods for built-in providers.
    pub fn methods(&self) -> BTreeMap<String, Vec<AuthMethodDto>> {
        [
            (
                "anthropic".to_string(),
                vec![AuthMethodDto {
                    kind: AuthMethodKind::Api,
                    label: "API key".into(),
                    prompts: vec![AuthPromptDto {
                        kind: "text".into(),
                        key: "api_key".into(),
                        message: "Enter your Anthropic API key".into(),
                        placeholder: Some("sk-ant-...".into()),
                    }],
                }],
            ),
            (
                "openai".to_string(),
                vec![
                    AuthMethodDto {
                        kind: AuthMethodKind::Api,
                        label: "API key".into(),
                        prompts: vec![AuthPromptDto {
                            kind: "text".into(),
                            key: "api_key".into(),
                            message: "Enter your OpenAI API key".into(),
                            placeholder: Some("sk-...".into()),
                        }],
                    },
                    AuthMethodDto {
                        kind: AuthMethodKind::Oauth,
                        label: "Device login".into(),
                        prompts: vec![],
                    },
                ],
            ),
            (
                "google".to_string(),
                vec![AuthMethodDto {
                    kind: AuthMethodKind::Api,
                    label: "API key".into(),
                    prompts: vec![AuthPromptDto {
                        kind: "text".into(),
                        key: "api_key".into(),
                        message: "Enter your Google API key".into(),
                        placeholder: Some("AIza...".into()),
                    }],
                }],
            ),
        ]
        .into_iter()
        .collect()
    }

    /// Start an auth flow for a supported provider.
    pub fn authorize(
        &self,
        provider: &str,
        input: AuthorizeInput,
    ) -> Result<Option<AuthAuthorizationDto>, ProviderError> {
        let methods = self.methods();
        let Some(method) = methods
            .get(provider)
            .and_then(|entries| entries.get(input.method))
        else {
            return Ok(None);
        };

        if method.kind != AuthMethodKind::Oauth {
            return Ok(None);
        }

        let url = format!(
            "https://auth.opencode.dev/{provider}/authorize?method={}",
            input.method
        );
        self.pending
            .lock()
            .map_err(|_| ProviderError::Stream("oauth pending state poisoned".into()))?
            .insert(
                provider.to_string(),
                PendingFlow {
                    method: input.method,
                    code: input.inputs.get("code").cloned(),
                },
            );

        Ok(Some(AuthAuthorizationDto {
            url,
            method: "code".into(),
            instructions: format!("Open the {provider} authorization URL and complete login."),
        }))
    }

    /// Complete a previously-started auth callback.
    pub fn callback(&self, provider: &str, input: CallbackInput) -> Result<(), ProviderError> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| ProviderError::Stream("oauth pending state poisoned".into()))?;
        let Some(flow) = pending.get(provider) else {
            return Err(ProviderError::Auth {
                provider: provider.into(),
                msg: "no pending oauth flow".into(),
            });
        };

        if flow.method != input.method {
            return Err(ProviderError::Auth {
                provider: provider.into(),
                msg: "oauth callback method mismatch".into(),
            });
        }

        if input.code.as_deref().or(flow.code.as_deref()).is_none() {
            return Err(ProviderError::Auth {
                provider: provider.into(),
                msg: "oauth callback code missing".into(),
            });
        }

        pending.remove(provider);
        Ok(())
    }
}

impl Default for ProviderAuthService {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve an API key for a provider.
pub trait AuthResolver: Send + Sync {
    /// Return the API key for this provider.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::Auth`] when no key can be found.
    fn resolve(&self) -> Result<String, ProviderError>;
}

/// Resolves a key from an environment variable, falling back to a static config string.
pub struct EnvAuthResolver {
    /// Provider slug used in error messages.
    provider: String,
    /// Env-var name to check (e.g. `"ANTHROPIC_API_KEY"`).
    env_key: String,
    /// Fallback value from config (e.g. loaded from `~/.opencode/config.jsonc`).
    config_key: Option<String>,
}

impl EnvAuthResolver {
    /// Create a new resolver.
    pub fn new(
        provider: impl Into<String>,
        env_key: impl Into<String>,
        config_key: Option<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            env_key: env_key.into(),
            config_key,
        }
    }
}

impl AuthResolver for EnvAuthResolver {
    fn resolve(&self) -> Result<String, ProviderError> {
        if let Ok(val) = std::env::var(&self.env_key) {
            if !val.is_empty() {
                return Ok(val);
            }
        }
        if let Some(ref key) = self.config_key {
            if !key.is_empty() {
                return Ok(key.clone());
            }
        }
        Err(ProviderError::Auth {
            provider: self.provider.clone(),
            msg: format!(
                "no API key found — set {} or configure it in ~/.opencode/config.jsonc",
                self.env_key
            ),
        })
    }
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // RED 1: env key present → returns key
    #[test]
    fn env_key_present_returns_key() {
        // SAFETY: single-threaded test, unique key name
        unsafe { std::env::set_var("TEST_OPENCODE_AUTH_KEY_A", "sk-from-env-abc") };
        let r = EnvAuthResolver::new("testprovider", "TEST_OPENCODE_AUTH_KEY_A", None);
        let key = r.resolve().unwrap();
        assert_eq!(key, "sk-from-env-abc");
    }

    // RED 2: env key absent, config present → returns config key
    #[test]
    fn env_absent_falls_back_to_config() {
        // SAFETY: single-threaded test, unique key name
        unsafe { std::env::remove_var("TEST_OPENCODE_AUTH_KEY_B") };
        let r = EnvAuthResolver::new(
            "testprovider",
            "TEST_OPENCODE_AUTH_KEY_B",
            Some("sk-from-config-xyz".into()),
        );
        let key = r.resolve().unwrap();
        assert_eq!(key, "sk-from-config-xyz");
    }

    // RED 3: both absent → ProviderError::Auth
    #[test]
    fn both_absent_returns_auth_error() {
        // SAFETY: single-threaded test, unique key name
        unsafe { std::env::remove_var("TEST_OPENCODE_AUTH_KEY_C") };
        let r = EnvAuthResolver::new("testprovider", "TEST_OPENCODE_AUTH_KEY_C", None);
        let err = r.resolve().unwrap_err();
        assert!(matches!(err, ProviderError::Auth { .. }));
        assert!(err.to_string().contains("testprovider"));
    }

    // TRIANGULATE: env empty string is treated as absent
    #[test]
    fn empty_env_value_falls_back_to_config() {
        // SAFETY: single-threaded test, unique key name
        unsafe { std::env::set_var("TEST_OPENCODE_AUTH_KEY_D", "") };
        let r = EnvAuthResolver::new(
            "tp",
            "TEST_OPENCODE_AUTH_KEY_D",
            Some("fallback-key".into()),
        );
        let key = r.resolve().unwrap();
        assert_eq!(key, "fallback-key");
    }

    // TRIANGULATE: empty config string also treated as absent
    #[test]
    fn empty_config_value_returns_auth_error() {
        // SAFETY: single-threaded test, unique key name
        unsafe { std::env::remove_var("TEST_OPENCODE_AUTH_KEY_E") };
        let r = EnvAuthResolver::new("tp", "TEST_OPENCODE_AUTH_KEY_E", Some("".into()));
        let err = r.resolve().unwrap_err();
        assert!(matches!(err, ProviderError::Auth { .. }));
    }

    #[test]
    fn provider_auth_methods_include_supported_builtins_only() {
        let auth = ProviderAuthService::new();
        let methods = auth.methods();

        assert_eq!(methods.len(), 3);
        assert_eq!(methods["openai"].len(), 2);
        assert_eq!(methods["anthropic"][0].kind, AuthMethodKind::Api);
        assert!(!methods.contains_key("bedrock"));
    }

    #[test]
    fn authorize_omits_unsupported_provider_and_non_oauth_method() {
        let auth = ProviderAuthService::new();

        assert_eq!(
            auth.authorize(
                "missing",
                AuthorizeInput {
                    method: 0,
                    inputs: BTreeMap::new(),
                },
            )
            .unwrap(),
            None
        );

        assert_eq!(
            auth.authorize(
                "anthropic",
                AuthorizeInput {
                    method: 0,
                    inputs: BTreeMap::new(),
                },
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn authorize_and_callback_round_trip_pending_oauth_flow() {
        let auth = ProviderAuthService::new();
        let authorization = auth
            .authorize(
                "openai",
                AuthorizeInput {
                    method: 1,
                    inputs: BTreeMap::new(),
                },
            )
            .unwrap()
            .unwrap();

        assert!(authorization.url.contains("openai"));
        assert_eq!(authorization.method, "code");

        auth.callback(
            "openai",
            CallbackInput {
                method: 1,
                code: Some("auth-code".into()),
            },
        )
        .unwrap();
    }

    #[test]
    fn callback_requires_pending_flow_and_code() {
        let auth = ProviderAuthService::new();
        let missing = auth
            .callback(
                "openai",
                CallbackInput {
                    method: 1,
                    code: Some("code".into()),
                },
            )
            .unwrap_err();
        assert!(matches!(missing, ProviderError::Auth { .. }));

        auth.authorize(
            "openai",
            AuthorizeInput {
                method: 1,
                inputs: BTreeMap::new(),
            },
        )
        .unwrap();

        let missing_code = auth
            .callback(
                "openai",
                CallbackInput {
                    method: 1,
                    code: None,
                },
            )
            .unwrap_err();
        assert!(matches!(missing_code, ProviderError::Auth { .. }));
    }
}
