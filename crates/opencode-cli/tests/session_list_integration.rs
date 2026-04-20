//! Integration coverage for `session list` via local router backend client.

use opencode_cli::bootstrap::bootstrap_backend_client;
use opencode_cli::commands::session_list;
use opencode_core::{
    dto::{ProjectRow, SessionRow},
    id::{ProjectId, SessionId},
};
use opencode_storage::{Storage, StorageImpl, connect};
use tempfile::TempDir;

#[tokio::test]
async fn session_list_text_mode_resolves_cwd_and_lists_session_ids() {
    let dir = TempDir::new().expect("tempdir should be created");
    let seeded = seed_project_with_sessions(dir.path(), "workspace").await;
    let client = bootstrap_backend_client(dir.path())
        .await
        .expect("backend client should bootstrap");

    let outcome = session_list::run(&client, dir.path()).await;

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stderr.is_empty());
    assert_eq!(
        outcome.stdout,
        format!(
            "id\ttitle\tdirectory\ttime_created\n{}\tFirst\t{}\t10\n{}\tSecond\t{}\t20\n",
            seeded.first.id,
            dir.path().display(),
            seeded.second.id,
            dir.path().display()
        )
    );
}

#[tokio::test]
async fn session_list_unresolved_project_uses_stderr_and_non_zero_exit() {
    let dir = TempDir::new().expect("tempdir should be created");
    seed_project_with_sessions(dir.path(), "workspace").await;
    let outside = TempDir::new().expect("tempdir should be created");
    let client = bootstrap_backend_client(dir.path())
        .await
        .expect("backend client should bootstrap");

    let outcome = session_list::run(&client, outside.path()).await;

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.stdout.is_empty());
    assert!(outcome.stderr.contains("could not resolve project for cwd"));
}

struct SeededSessions {
    first: SessionRow,
    second: SessionRow,
}

async fn seed_project_with_sessions(worktree: &std::path::Path, name: &str) -> SeededSessions {
    let db = worktree.join("opencode.db");
    let pool = connect(&db).await.expect("db should connect");
    let storage = StorageImpl::new(pool);
    let project_id = ProjectId::new();

    storage
        .upsert_project(ProjectRow {
            id: project_id,
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
        })
        .await
        .expect("project should seed");

    let second = sample_session(
        project_id,
        SessionId::new(),
        "session-b",
        "Second",
        worktree,
        20,
    );
    let first = sample_session(
        project_id,
        SessionId::new(),
        "session-a",
        "First",
        worktree,
        10,
    );
    storage
        .create_session(second.clone())
        .await
        .expect("second session should seed");
    storage
        .create_session(first.clone())
        .await
        .expect("first session should seed");

    SeededSessions { first, second }
}

fn sample_session(
    project_id: ProjectId,
    id: SessionId,
    slug: &str,
    title: &str,
    worktree: &std::path::Path,
    time_created: i64,
) -> SessionRow {
    SessionRow {
        id,
        project_id,
        workspace_id: None,
        parent_id: None,
        slug: slug.to_string(),
        directory: worktree.display().to_string(),
        title: title.to_string(),
        version: "0.0.0".to_string(),
        share_url: None,
        summary_additions: None,
        summary_deletions: None,
        summary_files: None,
        summary_diffs: None,
        revert: None,
        permission: None,
        time_created,
        time_updated: time_created,
        time_compacting: None,
        time_archived: None,
    }
}
