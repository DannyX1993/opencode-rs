//! Auth resolver — resolves provider API keys from environment or config fallback.

use crate::error::ProviderError;

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
mod tests {
    use super::*;

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
}
