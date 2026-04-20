//! Handler for `serve` command.

use crate::bootstrap::bootstrap_app_state;
use anyhow::Result;
use axum::Router;
use opencode_core::config_service::ServerBindOverrides;
use std::{future::Future, net::SocketAddr, path::Path};

/// Scriptable command result payload for `serve`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeOutcome {
    /// Startup confirmation emitted on stdout.
    pub stdout: String,
    /// Actionable startup diagnostics emitted on stderr.
    pub stderr: String,
    /// Process-compatible exit status.
    pub exit_code: i32,
}

impl ServeOutcome {
    fn success(stdout: String) -> Self {
        Self {
            stdout,
            stderr: String::new(),
            exit_code: 0,
        }
    }

    fn failure(stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            exit_code,
        }
    }
}

/// Execute `serve` using configured bind host/port and runtime bootstrap.
pub async fn run(cwd: &Path, host: Option<String>, port: Option<u16>) -> ServeOutcome {
    run_with_runner(cwd, host, port, |router, addr| async move {
        opencode_server::serve(router, addr)
            .await
            .map_err(anyhow::Error::from)
    })
    .await
}

async fn run_with_runner<F, Fut>(
    cwd: &Path,
    host: Option<String>,
    port: Option<u16>,
    runner: F,
) -> ServeOutcome
where
    F: FnOnce(Router, SocketAddr) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let state = match bootstrap_app_state(cwd).await {
        Ok(state) => state,
        Err(error) => return ServeOutcome::failure(format!("error: {error}\n"), 1),
    };

    let bind = match state
        .config_service
        .resolve_bind(ServerBindOverrides { host, port })
        .await
    {
        Ok(bind) => bind,
        Err(error) => return ServeOutcome::failure(format!("error: {error}\n"), 1),
    };
    let addr: SocketAddr = match format!("{}:{}", bind.0, bind.1).parse() {
        Ok(addr) => addr,
        Err(error) => return ServeOutcome::failure(format!("error: {error}\n"), 1),
    };
    let startup = format!("serve listening on {addr}\n");
    let router = opencode_server::build(state);

    match runner(router, addr).await {
        Ok(()) => ServeOutcome::success(startup),
        Err(error) => ServeOutcome::failure(format!("error: {error}\n"), 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use tempfile::TempDir;

    #[tokio::test]
    async fn serve_startup_confirmation_is_printed_to_stdout() {
        let dir = TempDir::new().expect("tempdir should be created");

        let outcome = run_with_runner(
            dir.path(),
            Some("127.0.0.1".to_string()),
            Some(4141),
            |_router, _addr| async { Ok(()) },
        )
        .await;

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stderr.is_empty());
        assert_eq!(outcome.stdout, "serve listening on 127.0.0.1:4141\n");
    }

    #[tokio::test]
    async fn serve_bind_failure_writes_diagnostic_to_stderr() {
        let dir = TempDir::new().expect("tempdir should be created");
        let bind_error = std::io::Error::new(std::io::ErrorKind::AddrInUse, "address in use");

        let outcome = run_with_runner(
            dir.path(),
            Some("127.0.0.1".to_string()),
            Some(4141),
            |_router, _addr: SocketAddr| async move { Err(anyhow::Error::from(bind_error)) },
        )
        .await;

        assert_eq!(outcome.exit_code, 1);
        assert!(outcome.stdout.is_empty());
        assert!(outcome.stderr.contains("address in use"));
    }
}
