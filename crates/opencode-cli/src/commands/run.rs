//! Handler for non-interactive `run` / `prompt` flow.

use crate::backend_client::BackendClient;
use opencode_core::{
    dto::{ProjectRow, SessionDetachedPromptDto, SessionRow},
    id::{ProjectId, SessionId},
};
use opencode_session::types::SessionPrompt;
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Scriptable command result payload for non-interactive run/prompt commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// User-facing payload emitted on stdout.
    pub stdout: String,
    /// Actionable diagnostics emitted on stderr.
    pub stderr: String,
    /// Process-compatible exit status.
    pub exit_code: i32,
}

#[derive(Debug, Serialize)]
struct PromptAcceptedJson {
    session_id: SessionId,
    assistant_message_id: Option<opencode_core::id::MessageId>,
    resolved_model: Option<String>,
    detached: bool,
}

impl RunOutcome {
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

/// Execute a non-interactive run/prompt request.
///
/// Minimal behavior decision: this slice is detached-acceptance only and does
/// not poll for full completion payloads.
pub async fn run(
    client: &dyn BackendClient,
    cwd: &Path,
    text: &str,
    output: &str,
    timeout: Duration,
) -> RunOutcome {
    if text.trim().is_empty() {
        return RunOutcome::failure("error: prompt text must not be empty\n", 2);
    }
    if output != "text" && output != "json" {
        return RunOutcome::failure(
            format!("error: invalid --output value '{output}': expected 'text' or 'json'\n"),
            2,
        );
    }

    let timeout_ms = timeout.as_millis();
    match tokio::time::timeout(timeout, execute(client, cwd, text)).await {
        Ok(Ok(accepted)) => {
            if output == "json" {
                let payload = PromptAcceptedJson {
                    session_id: accepted.session_id,
                    assistant_message_id: accepted.assistant_message_id,
                    resolved_model: accepted.resolved_model,
                    detached: true,
                };
                return match serde_json::to_string_pretty(&payload) {
                    Ok(mut body) => {
                        body.push('\n');
                        RunOutcome::success(body)
                    }
                    Err(err) => RunOutcome::failure(
                        format!("error: failed to encode JSON output: {err}\n"),
                        1,
                    ),
                };
            }

            let assistant = accepted
                .assistant_message_id
                .map_or_else(String::new, |id| id.to_string());
            let model = accepted.resolved_model.unwrap_or_default();
            RunOutcome::success(format!(
                "session_id\tassistant_message_id\tresolved_model\n{}\t{}\t{}\n",
                accepted.session_id, assistant, model
            ))
        }
        Ok(Err(error)) => RunOutcome::failure(format!("error: {error}\n"), 1),
        Err(_) => RunOutcome::failure(
            format!("error: request timed out after {timeout_ms}ms\n"),
            1,
        ),
    }
}

async fn execute(
    client: &dyn BackendClient,
    cwd: &Path,
    text: &str,
) -> anyhow::Result<SessionDetachedPromptDto> {
    let projects = client.list_projects().await?;
    let project = resolve_project_for_cwd(&projects, cwd).ok_or_else(|| {
        anyhow::anyhow!(
            "could not resolve project for cwd '{}'; run in a known project directory",
            cwd.display()
        )
    })?;

    let sessions = client.list_sessions(project.id).await?;
    let session_id = if let Some(existing) = sessions.into_iter().max_by(|left, right| {
        left.time_created
            .cmp(&right.time_created)
            .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
    }) {
        existing.id
    } else {
        let row = new_session_row(project.id, cwd);
        let session_id = row.id;
        client.create_session(project.id, row).await?;
        session_id
    };

    client
        .prompt_detached(SessionPrompt {
            session_id,
            text: text.to_string(),
            model: None,
            plan_mode: false,
        })
        .await
}

fn resolve_project_for_cwd<'a>(projects: &'a [ProjectRow], cwd: &Path) -> Option<&'a ProjectRow> {
    let cwd = canonicalize_lossy(cwd);
    projects
        .iter()
        .filter_map(|project| {
            let worktree = canonicalize_lossy(Path::new(&project.worktree));
            if cwd.starts_with(&worktree) {
                Some((worktree.components().count(), project))
            } else {
                None
            }
        })
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, project)| project)
}

fn canonicalize_lossy(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}

fn new_session_row(project_id: ProjectId, cwd: &Path) -> SessionRow {
    let id = SessionId::new();
    let now = now_millis();
    SessionRow {
        id,
        project_id,
        workspace_id: None,
        parent_id: None,
        slug: format!("cli-{}", id),
        directory: cwd.display().to_string(),
        title: "CLI prompt".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        share_url: None,
        summary_additions: None,
        summary_deletions: None,
        summary_files: None,
        summary_diffs: None,
        revert: None,
        permission: None,
        time_created: now,
        time_updated: now,
        time_compacting: None,
        time_archived: None,
    }
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::backend_client::BackendClient;
    use anyhow::anyhow;
    use async_trait::async_trait;
    use opencode_core::{
        dto::{ProjectRow, SessionDetachedPromptDto, SessionRow},
        id::{ProjectId, SessionId},
    };
    use opencode_provider::catalog::ProviderListDto;
    use opencode_session::types::SessionPrompt;
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{Arc, Mutex},
        time::Duration,
    };

    struct MockBackendClient {
        projects: Vec<ProjectRow>,
        sessions_by_project: HashMap<ProjectId, Vec<SessionRow>>,
        accepted: SessionDetachedPromptDto,
        fail_prompt: Option<String>,
        create_calls: Arc<Mutex<Vec<SessionRow>>>,
        prompt_delay: Option<Duration>,
    }

    #[async_trait]
    impl BackendClient for MockBackendClient {
        async fn list_projects(&self) -> anyhow::Result<Vec<ProjectRow>> {
            Ok(self.projects.clone())
        }

        async fn list_providers(&self) -> anyhow::Result<ProviderListDto> {
            Err(anyhow!("not used in run tests"))
        }

        async fn list_sessions(&self, project_id: ProjectId) -> anyhow::Result<Vec<SessionRow>> {
            Ok(self
                .sessions_by_project
                .get(&project_id)
                .cloned()
                .unwrap_or_default())
        }

        async fn create_session(
            &self,
            _project_id: ProjectId,
            row: SessionRow,
        ) -> anyhow::Result<()> {
            self.create_calls
                .lock()
                .expect("lock create_calls")
                .push(row);
            Ok(())
        }

        async fn prompt_detached(
            &self,
            _prompt: SessionPrompt,
        ) -> anyhow::Result<SessionDetachedPromptDto> {
            if let Some(delay) = self.prompt_delay {
                tokio::time::sleep(delay).await;
            }
            if let Some(message) = &self.fail_prompt {
                return Err(anyhow!(message.clone()));
            }
            Ok(self.accepted.clone())
        }
    }

    #[tokio::test]
    async fn one_shot_success_payload_is_emitted_on_stdout_with_zero_exit() {
        let project = sample_project("/tmp/workspace");
        let accepted = SessionDetachedPromptDto {
            session_id: SessionId::new(),
            assistant_message_id: None,
            resolved_model: Some("gpt-5".to_string()),
        };
        let client = MockBackendClient {
            projects: vec![project.clone()],
            sessions_by_project: HashMap::from([(
                project.id,
                vec![sample_session(
                    project.id,
                    SessionId::new(),
                    "session-alpha",
                    10,
                )],
            )]),
            accepted: accepted.clone(),
            fail_prompt: None,
            create_calls: Arc::new(Mutex::new(Vec::new())),
            prompt_delay: None,
        };

        let outcome = run(
            &client,
            PathBuf::from("/tmp/workspace").as_path(),
            "summarize this",
            "json",
            Duration::from_secs(2),
        )
        .await;

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stderr.is_empty());
        let payload: serde_json::Value =
            serde_json::from_str(&outcome.stdout).expect("json output should parse");
        assert_eq!(payload["session_id"], accepted.session_id.to_string());
        assert_eq!(payload["resolved_model"], "gpt-5");
        assert_eq!(payload["detached"], true);
    }

    #[tokio::test]
    async fn backend_failure_maps_to_stderr_and_non_zero_exit() {
        let project = sample_project("/tmp/workspace");
        let client = MockBackendClient {
            projects: vec![project.clone()],
            sessions_by_project: HashMap::from([(
                project.id,
                vec![sample_session(
                    project.id,
                    SessionId::new(),
                    "session-alpha",
                    10,
                )],
            )]),
            accepted: SessionDetachedPromptDto {
                session_id: SessionId::new(),
                assistant_message_id: None,
                resolved_model: None,
            },
            fail_prompt: Some("backend unavailable".to_string()),
            create_calls: Arc::new(Mutex::new(Vec::new())),
            prompt_delay: None,
        };

        let outcome = run(
            &client,
            PathBuf::from("/tmp/workspace").as_path(),
            "summarize this",
            "text",
            Duration::from_secs(2),
        )
        .await;

        assert_eq!(outcome.exit_code, 1);
        assert!(outcome.stdout.is_empty());
        assert_eq!(outcome.stderr, "error: backend unavailable\n");
    }

    #[tokio::test]
    async fn timeout_maps_to_stderr_non_zero_exit() {
        let project = sample_project("/tmp/workspace");
        let client = MockBackendClient {
            projects: vec![project.clone()],
            sessions_by_project: HashMap::from([(
                project.id,
                vec![sample_session(
                    project.id,
                    SessionId::new(),
                    "session-alpha",
                    10,
                )],
            )]),
            accepted: SessionDetachedPromptDto {
                session_id: SessionId::new(),
                assistant_message_id: None,
                resolved_model: Some("gpt-5".to_string()),
            },
            fail_prompt: None,
            create_calls: Arc::new(Mutex::new(Vec::new())),
            prompt_delay: Some(Duration::from_millis(60)),
        };

        let outcome = run(
            &client,
            PathBuf::from("/tmp/workspace").as_path(),
            "summarize this",
            "text",
            Duration::from_millis(5),
        )
        .await;

        assert_eq!(outcome.exit_code, 1);
        assert!(outcome.stdout.is_empty());
        assert!(outcome.stderr.contains("request timed out"));
    }

    #[tokio::test]
    async fn ensure_session_creates_when_none_exist_before_detached_prompt() {
        let project = sample_project("/tmp/workspace");
        let create_calls = Arc::new(Mutex::new(Vec::new()));
        let client = MockBackendClient {
            projects: vec![project.clone()],
            sessions_by_project: HashMap::from([(project.id, vec![])]),
            accepted: SessionDetachedPromptDto {
                session_id: SessionId::new(),
                assistant_message_id: None,
                resolved_model: Some("gpt-5".to_string()),
            },
            fail_prompt: None,
            create_calls: Arc::clone(&create_calls),
            prompt_delay: None,
        };

        let outcome = run(
            &client,
            PathBuf::from("/tmp/workspace").as_path(),
            "summarize this",
            "text",
            Duration::from_secs(2),
        )
        .await;

        assert_eq!(outcome.exit_code, 0);
        let created = create_calls.lock().expect("lock create calls");
        assert_eq!(
            created.len(),
            1,
            "expected session ensure to create exactly once"
        );
    }

    fn sample_project(worktree: &str) -> ProjectRow {
        ProjectRow {
            id: ProjectId::new(),
            worktree: worktree.to_string(),
            vcs: Some("git".to_string()),
            name: Some("workspace".to_string()),
            icon_url: None,
            icon_color: None,
            time_created: 1,
            time_updated: 1,
            time_initialized: Some(1),
            sandboxes: serde_json::json!({}),
            commands: None,
        }
    }

    fn sample_session(
        project_id: ProjectId,
        id: SessionId,
        slug: &str,
        time_created: i64,
    ) -> SessionRow {
        SessionRow {
            id,
            project_id,
            workspace_id: None,
            parent_id: None,
            slug: slug.to_string(),
            directory: "/tmp/workspace".to_string(),
            title: "Workspace".to_string(),
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
}
