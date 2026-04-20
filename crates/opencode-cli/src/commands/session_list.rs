//! Handler for `session list` command.

use crate::backend_client::BackendClient;
use opencode_core::dto::{ProjectRow, SessionRow};
use std::path::{Path, PathBuf};

/// Scriptable command result payload for session listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionListOutcome {
    /// Table or empty-state payload for stdout.
    pub stdout: String,
    /// Actionable diagnostics for stderr.
    pub stderr: String,
    /// Process-compatible exit status.
    pub exit_code: i32,
}

impl SessionListOutcome {
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

/// Execute `session list` with cwd-aware project resolution.
pub async fn run(client: &dyn BackendClient, cwd: &Path) -> SessionListOutcome {
    let projects = match client.list_projects().await {
        Ok(projects) => projects,
        Err(error) => {
            return SessionListOutcome::failure(format!("error: {error}\n"), 1);
        }
    };

    let Some(project) = resolve_project_for_cwd(&projects, cwd) else {
        return SessionListOutcome::failure(
            format!(
                "error: could not resolve project for cwd '{}'; run in a known project directory\n",
                cwd.display()
            ),
            1,
        );
    };

    let sessions = match client.list_sessions(project.id).await {
        Ok(sessions) => sessions,
        Err(error) => {
            return SessionListOutcome::failure(format!("error: {error}\n"), 1);
        }
    };

    if sessions.is_empty() {
        return SessionListOutcome::success("No sessions found for current project.\n".to_string());
    }

    SessionListOutcome::success(format_table(&sorted_sessions(sessions)))
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

fn sorted_sessions(mut sessions: Vec<SessionRow>) -> Vec<SessionRow> {
    sessions.sort_by(|left, right| {
        left.time_created
            .cmp(&right.time_created)
            .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
    });
    sessions
}

fn format_table(sessions: &[SessionRow]) -> String {
    let mut output = String::from("id\ttitle\tdirectory\ttime_created\n");
    for session in sessions {
        output.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            session.id, session.title, session.directory, session.time_created
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::backend_client::BackendClient;
    use anyhow::anyhow;
    use async_trait::async_trait;
    use opencode_core::{
        dto::{ProjectRow, SessionDetachedPromptDto, SessionRow},
        id::ProjectId,
    };
    use opencode_provider::catalog::ProviderListDto;
    use opencode_session::types::SessionPrompt;
    use std::{collections::HashMap, path::PathBuf, str::FromStr};

    struct MockBackendClient {
        projects: Vec<ProjectRow>,
        sessions_by_project: HashMap<ProjectId, Vec<SessionRow>>,
    }

    #[async_trait]
    impl BackendClient for MockBackendClient {
        async fn list_projects(&self) -> anyhow::Result<Vec<ProjectRow>> {
            Ok(self.projects.clone())
        }

        async fn list_providers(&self) -> anyhow::Result<ProviderListDto> {
            Err(anyhow!("not used in session list tests"))
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
            _row: SessionRow,
        ) -> anyhow::Result<()> {
            Err(anyhow!("not used in session list tests"))
        }

        async fn prompt_detached(
            &self,
            _prompt: SessionPrompt,
        ) -> anyhow::Result<SessionDetachedPromptDto> {
            Err(anyhow!("not used in session list tests"))
        }
    }

    #[tokio::test]
    async fn resolves_project_from_cwd_and_lists_sessions_in_deterministic_order() {
        let project = sample_project("/tmp/workspace");
        let second = sample_session(
            project.id,
            "00000000-0000-0000-0000-000000000002",
            "session-beta",
            "B",
            20,
        );
        let first = sample_session(
            project.id,
            "00000000-0000-0000-0000-000000000001",
            "session-alpha",
            "A",
            10,
        );
        let client = MockBackendClient {
            projects: vec![project.clone()],
            sessions_by_project: HashMap::from([(project.id, vec![second, first])]),
        };

        let outcome = run(&client, PathBuf::from("/tmp/workspace/subdir").as_path()).await;

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stderr.is_empty());
        assert_eq!(
            outcome.stdout,
            concat!(
                "id\ttitle\tdirectory\ttime_created\n",
                "00000000-0000-0000-0000-000000000001\tA\t/tmp/workspace\t10\n",
                "00000000-0000-0000-0000-000000000002\tB\t/tmp/workspace\t20\n"
            )
        );
    }

    #[tokio::test]
    async fn empty_state_is_deterministic() {
        let project = sample_project("/tmp/workspace");
        let client = MockBackendClient {
            projects: vec![project.clone()],
            sessions_by_project: HashMap::from([(project.id, vec![])]),
        };

        let outcome = run(&client, PathBuf::from("/tmp/workspace").as_path()).await;

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stderr.is_empty());
        assert_eq!(outcome.stdout, "No sessions found for current project.\n");
    }

    #[tokio::test]
    async fn unresolved_project_maps_to_stderr_and_non_zero_exit() {
        let client = MockBackendClient {
            projects: vec![sample_project("/tmp/another-project")],
            sessions_by_project: HashMap::new(),
        };

        let outcome = run(&client, PathBuf::from("/tmp/workspace").as_path()).await;

        assert_eq!(outcome.exit_code, 1);
        assert!(outcome.stdout.is_empty());
        assert!(outcome.stderr.contains("could not resolve project for cwd"));
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
        id: &str,
        slug: &str,
        title: &str,
        time_created: i64,
    ) -> SessionRow {
        SessionRow {
            id: opencode_core::id::SessionId::from_str(id).expect("session id should parse"),
            project_id,
            workspace_id: None,
            parent_id: None,
            slug: slug.to_string(),
            directory: "/tmp/workspace".to_string(),
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
}
