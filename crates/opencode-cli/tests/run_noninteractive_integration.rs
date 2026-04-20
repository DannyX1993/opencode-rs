//! Integration coverage for non-interactive `run` via local router backend client.

use opencode_cli::{
    backend_client::BackendClient, bootstrap::bootstrap_backend_client, commands::run,
};
use opencode_core::{
    dto::ProjectRow,
    id::{ProjectId, SessionId},
};
use opencode_storage::{Storage, StorageImpl, connect};
use std::str::FromStr;
use tempfile::TempDir;

#[tokio::test]
async fn run_noninteractive_ensures_session_then_submits_detached_prompt() {
    let dir = TempDir::new().expect("tempdir should be created");
    let project = seed_project(dir.path(), "workspace").await;
    let client = bootstrap_backend_client(dir.path())
        .await
        .expect("backend client should bootstrap");

    let outcome = run::run(
        &client,
        dir.path(),
        "summarize this repository",
        "json",
        std::time::Duration::from_secs(2),
    )
    .await;

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stderr.is_empty());

    let payload: serde_json::Value =
        serde_json::from_str(&outcome.stdout).expect("json output should parse");
    assert_eq!(payload["detached"], true);
    let accepted = SessionId::from_str(payload["session_id"].as_str().unwrap_or_default())
        .expect("session id should parse");

    let sessions = client
        .list_sessions(project.id)
        .await
        .expect("sessions should list");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, accepted);
}

async fn seed_project(worktree: &std::path::Path, name: &str) -> ProjectRow {
    let db = worktree.join("opencode.db");
    let pool = connect(&db).await.expect("db should connect");
    let storage = StorageImpl::new(pool);
    let row = ProjectRow {
        id: ProjectId::new(),
        worktree: worktree.display().to_string(),
        vcs: Some("git".to_string()),
        name: Some(name.to_string()),
        icon_url: None,
        icon_color: None,
        time_created: 1,
        time_updated: 1,
        time_initialized: Some(1),
        sandboxes: serde_json::json!({}),
        commands: None,
    };
    storage
        .upsert_project(row.clone())
        .await
        .expect("project should seed");
    row
}
