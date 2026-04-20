//! Integration coverage for `providers list` via local router backend client.

use opencode_cli::bootstrap::bootstrap_backend_client;
use opencode_cli::commands::providers_list;
use tempfile::TempDir;

#[tokio::test]
async fn providers_list_text_mode_writes_only_stdout_with_success_exit() {
    let dir = TempDir::new().expect("tempdir should be created");
    let client = bootstrap_backend_client(dir.path())
        .await
        .expect("backend client should bootstrap");

    let outcome = providers_list::run(&client, "text").await;

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stderr.is_empty());
    assert!(
        outcome
            .stdout
            .starts_with("id\tname\tdefault_model\tconnected\n")
    );
}

#[tokio::test]
async fn providers_list_invalid_output_uses_stderr_and_non_zero_exit() {
    let dir = TempDir::new().expect("tempdir should be created");
    let client = bootstrap_backend_client(dir.path())
        .await
        .expect("backend client should bootstrap");

    let outcome = providers_list::run(&client, "yaml").await;

    assert_eq!(outcome.exit_code, 2);
    assert!(outcome.stdout.is_empty());
    assert!(outcome.stderr.contains("invalid --output value"));
}
