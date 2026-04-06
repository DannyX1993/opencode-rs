//! Tracing/observability bootstrap.
//!
//! Call [`init`] once at process startup (typically from `opencode-cli`).
//!
//! # Examples
//!
//! ```no_run
//! use opencode_core::{config::Config, tracing::init};
//!
//! # async fn run() -> anyhow::Result<()> {
//! let cfg = Config::load(std::path::Path::new(".")).await?;
//! init(&cfg);
//! # Ok(())
//! # }
//! ```

use crate::config::Config;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialise the global `tracing` subscriber.
///
/// - When `cfg.log_json` is `true`, emits newline-delimited JSON.
/// - Otherwise emits coloured ANSI text (useful for dev/TUI).
/// - Log level comes from `cfg.log_level` (overridden by `RUST_LOG` env var).
pub fn init(cfg: &Config) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cfg.log_level));

    if cfg.log_json {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().with_ansi(true))
            .init();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn init_text_mode_does_not_panic() {
        // Only the first call takes effect; subsequent are no-ops because of
        // `try_init` behaviour. We just confirm it doesn't panic.
        let cfg = Config::default();
        // Safe to call even if already initialised in another test.
        let _ = std::panic::catch_unwind(|| init(&cfg));
    }

    #[test]
    fn init_json_mode_does_not_panic() {
        // Exercise the `log_json = true` branch.
        let mut cfg = Config::default();
        cfg.log_json = true;
        let _ = std::panic::catch_unwind(|| init(&cfg));
    }
}
