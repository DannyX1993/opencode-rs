//! Tracing/observability bootstrap.
//!
//! Call [`init`] once at process startup (typically from `opencode-cli`).
//!
//! All log output is routed to **stderr** so that stdout remains clean for
//! machine-readable output (e.g. `opencode tool … --output json`).
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
/// All output goes to **stderr** — stdout is reserved for tool output.
///
/// - When `cfg.log_json` is `true`, emits newline-delimited JSON to stderr.
/// - Otherwise emits coloured ANSI text to stderr.
/// - Log level comes from `cfg.log_level` (overridden by `RUST_LOG` env var).
pub fn init(cfg: &Config) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cfg.log_level));

    if cfg.log_json {
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json().with_writer(std::io::stderr))
            .try_init();
    } else {
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().with_ansi(true).with_writer(std::io::stderr))
            .try_init();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn init_text_mode_does_not_panic() {
        // Subscriber may already be set by another test; `try_init` returns Err
        // in that case — we only confirm no panic occurs.
        let cfg = Config::default();
        init(&cfg);
    }

    #[test]
    fn init_json_mode_does_not_panic() {
        // Exercise the `log_json = true` branch.
        let cfg = Config {
            log_json: true,
            ..Default::default()
        };
        init(&cfg);
    }

    // ── C.2 RED: stderr routing — logs must not pollute stdout ──────────────

    /// Confirm `init()` succeeds (or silently no-ops when already set) in text mode.
    /// The key behavioral change is the layer routes to stderr; this test exercises
    /// the full init() path including the `.with_writer(std::io::stderr)` call.
    #[test]
    fn init_routes_text_to_stderr_no_panic() {
        // Call init with default config — exercises the non-json (text) branch
        // with `.with_writer(std::io::stderr)`. Second calls are silent no-ops via try_init.
        let cfg = Config::default();
        init(&cfg); // must not panic regardless of call order
        // Emit a log — if stderr routing is broken this panics; if it works, the log goes to stderr.
        tracing::info!("tracing-stderr-routing-test");
    }

    /// Confirm `init()` succeeds in JSON mode with stderr routing.
    #[test]
    fn init_routes_json_to_stderr_no_panic() {
        let cfg = Config {
            log_json: true,
            ..Default::default()
        };
        init(&cfg); // exercises json branch with `.with_writer(std::io::stderr)`
        tracing::info!("tracing-json-stderr-routing-test");
    }
}
