//! Integration contract tests for local backend client seam.

use opencode_cli::backend_client::BackendClient;
use opencode_cli::bootstrap::bootstrap_backend_client;
use opencode_core::id::SessionId;
use opencode_session::types::SessionPrompt;
use tempfile::TempDir;

#[tokio::test]
async fn bootstrap_backend_client_lists_providers_from_local_router() {
    let dir = TempDir::new().expect("tempdir should be created");
    let client = bootstrap_backend_client(dir.path())
        .await
        .expect("backend client should bootstrap");

    let providers = client
        .list_providers()
        .await
        .expect("provider list should be returned");

    assert!(
        providers.all.iter().any(|provider| provider.id == "openai"),
        "expected builtin providers to be visible"
    );
}

#[tokio::test]
async fn backend_client_maps_non_success_prompt_to_error() {
    let dir = TempDir::new().expect("tempdir should be created");
    let client = bootstrap_backend_client(dir.path())
        .await
        .expect("backend client should bootstrap");

    let result = client
        .prompt_detached(SessionPrompt {
            session_id: SessionId::new(),
            text: "hello".to_string(),
            model: None,
            plan_mode: false,
        })
        .await;

    assert!(result.is_err(), "missing session should return an error");
}
