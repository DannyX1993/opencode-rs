//! Provider-specific error type.

use thiserror::Error;

/// Errors produced by the provider layer.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProviderError {
    /// API authentication failed.
    #[error("authentication failed for provider {provider}: {msg}")]
    Auth {
        /// Provider identifier.
        provider: String,
        /// Detail.
        msg: String,
    },

    /// Provider rate-limited this request.
    #[error("rate limited by {provider} (retry after {retry_after:?}s)")]
    RateLimit {
        /// Provider identifier.
        provider: String,
        /// Suggested retry delay in seconds.
        retry_after: Option<u64>,
    },

    /// The model's context window was exceeded.
    #[error("context length exceeded for model {model}")]
    ContextLength {
        /// Model identifier.
        model: String,
    },

    /// Network or HTTP error.
    #[error("http error: {0}: {1}")]
    Http(
        /// Provider.
        String,
        /// Message.
        String,
    ),

    /// Stream parsing error.
    #[error("stream parse error: {0}")]
    Stream(String),

    /// Circuit breaker is open.
    #[error("circuit breaker open for {provider}/{model}")]
    CircuitOpen {
        /// Provider.
        provider: String,
        /// Model.
        model: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let e = ProviderError::Auth {
            provider: "anthropic".into(),
            msg: "bad key".into(),
        };
        assert!(e.to_string().contains("anthropic"));
        assert!(e.to_string().contains("bad key"));
    }

    #[test]
    fn rate_limit_display() {
        let e = ProviderError::RateLimit {
            provider: "openai".into(),
            retry_after: Some(30),
        };
        assert!(e.to_string().contains("openai"));
    }

    #[test]
    fn context_length_display() {
        let e = ProviderError::ContextLength {
            model: "gpt-4".into(),
        };
        assert!(e.to_string().contains("gpt-4"));
    }

    #[test]
    fn http_display() {
        let e = ProviderError::Http("anthropic".into(), "500".into());
        assert!(e.to_string().contains("500"));
    }

    #[test]
    fn stream_display() {
        let e = ProviderError::Stream("unexpected eof".into());
        assert!(e.to_string().contains("unexpected eof"));
    }

    #[test]
    fn circuit_open_display() {
        let e = ProviderError::CircuitOpen {
            provider: "p".into(),
            model: "m".into(),
        };
        assert!(e.to_string().contains("p/m"));
    }
}
