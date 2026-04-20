//! Integration coverage for `serve` command behavior.

use opencode_cli::commands::serve;
use std::net::TcpListener;
use tempfile::TempDir;
use tokio::time::{Duration, Instant};

#[tokio::test]
async fn serve_listens_on_requested_port_and_exposes_health() {
    let dir = TempDir::new().expect("tempdir should be created");
    let listener = TcpListener::bind("127.0.0.1:0").expect("ephemeral bind should succeed");
    let port = listener
        .local_addr()
        .expect("listener local addr should resolve")
        .port();
    drop(listener);

    let path = dir.path().to_path_buf();
    let handle =
        tokio::spawn(
            async move { serve::run(&path, Some("127.0.0.1".to_string()), Some(port)).await },
        );

    wait_for_health(port).await;

    let response = reqwest::get(format!("http://127.0.0.1:{port}/health"))
        .await
        .expect("health request should succeed");
    assert_eq!(response.status().as_u16(), 200);

    handle.abort();
}

#[tokio::test]
async fn serve_occupied_port_returns_stderr_and_non_zero_exit() {
    let dir = TempDir::new().expect("tempdir should be created");
    let listener = TcpListener::bind("127.0.0.1:0").expect("ephemeral bind should succeed");
    let port = listener
        .local_addr()
        .expect("listener local addr should resolve")
        .port();

    let outcome = serve::run(dir.path(), Some("127.0.0.1".to_string()), Some(port)).await;

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.stdout.is_empty());
    assert!(
        outcome.stderr.contains("in use") || outcome.stderr.contains("already"),
        "expected bind failure diagnostic, got: {}",
        outcome.stderr
    );

    drop(listener);
}

async fn wait_for_health(port: u16) {
    let client = reqwest::Client::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    let health_url = format!("http://127.0.0.1:{port}/health");

    loop {
        match client.get(&health_url).send().await {
            Ok(response) if response.status().is_success() => return,
            _ if Instant::now() >= deadline => {
                panic!("serve command did not become ready at {health_url}");
            }
            _ => tokio::time::sleep(Duration::from_millis(25)).await,
        }
    }
}
