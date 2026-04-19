//! `/api/v1/projects` CRUD route handlers.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use opencode_core::{
    dto::{ProjectFoundationRow, ProjectRow},
    id::ProjectId,
    project::{
        ProjectFoundationRecord, ProjectProbeError, RepositoryProbe, RepositoryState, SyncBasis,
        WorktreeState,
    },
};
use opencode_storage::Storage;
use std::{path::Path as StdPath, time::SystemTime};
use tokio::process::Command;

use crate::{error::HttpError, state::AppState};

/// `GET /api/v1/projects` — list all projects.
pub async fn list(State(s): State<AppState>) -> impl IntoResponse {
    match s.storage.list_projects().await {
        Ok(rows) => (StatusCode::OK, Json(rows)).into_response(),
        Err(e) => HttpError::from(e).into_response(),
    }
}

/// `PUT /api/v1/projects/:id` — upsert (create or update) a project.
pub async fn upsert(
    State(s): State<AppState>,
    Path(id): Path<ProjectId>,
    Json(mut row): Json<ProjectRow>,
) -> impl IntoResponse {
    row.id = id;
    let foundation_row = row.clone();
    match s.storage.upsert_project(row).await {
        Ok(()) => {
            if let Err(e) =
                persist_project_foundation_for_row(s.storage.as_ref(), &foundation_row).await
            {
                return HttpError::from(e).into_response();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => HttpError::from(e).into_response(),
    }
}

async fn persist_project_foundation_for_row(
    storage: &dyn Storage,
    row: &ProjectRow,
) -> Result<(), opencode_core::error::StorageError> {
    // Foundation persistence is intentionally additive and best-effort wrt probe
    // availability. If probing fails (missing git binary, non-git path, or
    // command errors), we still persist a canonical worktree + unknown
    // repository fields to keep the row usable for future lazy enrichment.
    let probe = GitCliRepositoryProbe;
    let now = now_unix_ms();
    let time_created = storage
        .get_project_foundation(row.id)
        .await?
        .map(|existing| existing.time_created)
        .unwrap_or(now);

    let record = probe
        .inspect(row.id, StdPath::new(&row.worktree))
        .await
        .unwrap_or_else(|_| unknown_foundation_record(row.id, StdPath::new(&row.worktree)));

    storage
        .upsert_project_foundation(ProjectFoundationRow {
            project_id: row.id,
            canonical_worktree: record.canonical_worktree,
            repository_root: record.repository_root,
            vcs_kind: record.vcs_kind,
            worktree_state: record.worktree_state,
            repository_state: record.repository_state,
            sync_basis: record.sync_basis,
            time_created,
            time_updated: now,
        })
        .await
}

fn unknown_foundation_record(project_id: ProjectId, worktree: &StdPath) -> ProjectFoundationRecord {
    ProjectFoundationRecord {
        project_id,
        canonical_worktree: canonicalize_to_string(worktree),
        repository_root: None,
        vcs_kind: None,
        worktree_state: WorktreeState::default(),
        repository_state: RepositoryState::default(),
        sync_basis: None,
    }
}

fn canonicalize_to_string(path: &StdPath) -> Option<String> {
    std::fs::canonicalize(path)
        .ok()
        .and_then(|value| value.into_os_string().into_string().ok())
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64
}

#[derive(Debug, Default)]
struct GitCliRepositoryProbe;

#[async_trait::async_trait]
impl RepositoryProbe for GitCliRepositoryProbe {
    async fn inspect(
        &self,
        project_id: ProjectId,
        worktree: &StdPath,
    ) -> Result<ProjectFoundationRecord, ProjectProbeError> {
        let canonical_worktree = canonicalize_to_string(worktree);
        let repository_root = run_git_trimmed(worktree, &["rev-parse", "--show-toplevel"])
            .await?
            .filter(|value| !value.is_empty());

        let Some(repository_root) = repository_root else {
            return Ok(ProjectFoundationRecord {
                project_id,
                canonical_worktree,
                repository_root: None,
                vcs_kind: None,
                worktree_state: WorktreeState::default(),
                repository_state: RepositoryState::default(),
                sync_basis: None,
            });
        };

        let worktree_branch = run_git_trimmed(worktree, &["branch", "--show-current"])
            .await?
            .filter(|value| !value.is_empty());
        let worktree_head = run_git_trimmed(worktree, &["rev-parse", "HEAD"])
            .await?
            .filter(|value| !value.is_empty());
        let is_dirty = run_git_trimmed(worktree, &["status", "--porcelain"])
            .await?
            .map(|porcelain| !porcelain.is_empty());
        let default_branch = run_git_trimmed(
            worktree,
            &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        )
        .await?
        .filter(|value| !value.is_empty())
        .and_then(|value| value.rsplit('/').next().map(|segment| segment.to_string()));

        let sync_basis = if worktree_head.is_some() || is_dirty.is_some() {
            Some(SyncBasis {
                head_oid: worktree_head.clone(),
                base_oid: None,
                is_dirty,
            })
        } else {
            None
        };

        Ok(ProjectFoundationRecord {
            project_id,
            canonical_worktree,
            repository_root: Some(repository_root),
            vcs_kind: Some("git".to_string()),
            worktree_state: WorktreeState {
                branch: worktree_branch,
                head_oid: worktree_head.clone(),
                is_dirty,
            },
            repository_state: RepositoryState {
                default_branch,
                head_oid: worktree_head,
            },
            sync_basis,
        })
    }
}

async fn run_git_trimmed(
    worktree: &StdPath,
    args: &[&str],
) -> Result<Option<String>, ProjectProbeError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(args)
        .output()
        .await
        .map_err(|e| ProjectProbeError::Probe(format!("git {:?} failed: {e}", args)))?;

    // Non-zero status is treated as unknown metadata instead of a hard error.
    // This keeps non-git directories and partial repositories representable.
    if !output.status.success() {
        return Ok(None);
    }

    let value = String::from_utf8(output.stdout)
        .map_err(|e| ProjectProbeError::Probe(format!("git {:?} stdout utf8: {e}", args)))?;
    let trimmed = value.trim().to_string();
    Ok(Some(trimmed))
}

/// `GET /api/v1/projects/:id` — fetch one project by id.
pub async fn get(State(s): State<AppState>, Path(id): Path<ProjectId>) -> impl IntoResponse {
    match s.storage.get_project(id).await {
        Ok(Some(row)) => (StatusCode::OK, Json(row)).into_response(),
        Ok(None) => HttpError::not_found(format!("project {id} not found")).into_response(),
        Err(e) => HttpError::from(e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{
        Router,
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use opencode_bus::BroadcastBus;
    use opencode_core::{
        config::Config,
        dto::{
            AccountRow, AccountStateRow, ControlAccountRow, MessageRow, MessageWithParts, PartRow,
            PermissionRow, ProjectFoundationRow, SessionRow, TodoRow,
        },
        error::{SessionError, StorageError},
        id::{AccountId, ProjectId, SessionId},
    };
    use opencode_provider::{AccountService, ProviderAuthService};
    use opencode_session::{
        engine::Session,
        types::{DetachedPromptAccepted, SessionHandle, SessionPrompt, SessionRuntimeStatus},
    };
    use opencode_storage::Storage;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    // ── Configurable stub storage ─────────────────────────────────────────────

    #[derive(Default)]
    struct Stub {
        fail: bool,
        projects: Mutex<Vec<ProjectRow>>,
        foundations: Mutex<Vec<ProjectFoundationRow>>,
    }

    impl Stub {
        fn failing() -> Self {
            Self {
                fail: true,
                projects: Mutex::new(vec![]),
                foundations: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl Storage for Stub {
        async fn upsert_project(&self, row: ProjectRow) -> Result<(), StorageError> {
            if self.fail {
                return Err(StorageError::Db("db down".into()));
            }
            let mut v = self.projects.lock().unwrap();
            v.retain(|p| p.id != row.id);
            v.push(row);
            Ok(())
        }
        async fn get_project(&self, id: ProjectId) -> Result<Option<ProjectRow>, StorageError> {
            if self.fail {
                return Err(StorageError::Db("db down".into()));
            }
            Ok(self
                .projects
                .lock()
                .unwrap()
                .iter()
                .find(|p| p.id == id)
                .cloned())
        }
        async fn list_projects(&self) -> Result<Vec<ProjectRow>, StorageError> {
            if self.fail {
                return Err(StorageError::Db("db down".into()));
            }
            Ok(self.projects.lock().unwrap().clone())
        }
        async fn get_project_foundation(
            &self,
            project_id: ProjectId,
        ) -> Result<Option<ProjectFoundationRow>, StorageError> {
            Ok(self
                .foundations
                .lock()
                .unwrap()
                .iter()
                .find(|row| row.project_id == project_id)
                .cloned())
        }
        async fn upsert_project_foundation(
            &self,
            row: ProjectFoundationRow,
        ) -> Result<(), StorageError> {
            let mut foundations = self.foundations.lock().unwrap();
            foundations.retain(|existing| existing.project_id != row.project_id);
            foundations.push(row);
            Ok(())
        }
        async fn create_session(&self, _: SessionRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn get_session(&self, _: SessionId) -> Result<Option<SessionRow>, StorageError> {
            Ok(None)
        }
        async fn list_sessions(&self, _: ProjectId) -> Result<Vec<SessionRow>, StorageError> {
            Ok(vec![])
        }
        async fn update_session(&self, _: SessionRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn append_message(&self, _: MessageRow, _: Vec<PartRow>) -> Result<(), StorageError> {
            Ok(())
        }
        async fn append_part(&self, _: PartRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list_history(&self, _: SessionId) -> Result<Vec<MessageRow>, StorageError> {
            Ok(vec![])
        }
        async fn list_history_with_parts(
            &self,
            _: SessionId,
        ) -> Result<Vec<MessageWithParts>, StorageError> {
            Ok(vec![])
        }
        async fn save_todos(&self, _: SessionId, _: Vec<TodoRow>) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list_todos(&self, _: SessionId) -> Result<Vec<TodoRow>, StorageError> {
            Ok(vec![])
        }
        async fn get_permission(
            &self,
            _: ProjectId,
        ) -> Result<Option<PermissionRow>, StorageError> {
            Ok(None)
        }
        async fn set_permission(&self, _: PermissionRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn upsert_account(&self, _: AccountRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list_accounts(&self) -> Result<Vec<AccountRow>, StorageError> {
            Ok(vec![])
        }
        async fn get_account(&self, _: AccountId) -> Result<Option<AccountRow>, StorageError> {
            Ok(None)
        }
        async fn remove_account(&self, _: AccountId) -> Result<(), StorageError> {
            Ok(())
        }
        async fn update_account_tokens(
            &self,
            _: AccountId,
            _: String,
            _: String,
            _: Option<i64>,
            _: i64,
        ) -> Result<(), StorageError> {
            Ok(())
        }
        async fn get_account_state(&self) -> Result<Option<AccountStateRow>, StorageError> {
            Ok(None)
        }
        async fn set_account_state(&self, _: AccountStateRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn get_control_account(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<ControlAccountRow>, StorageError> {
            Ok(None)
        }
        async fn get_active_control_account(
            &self,
        ) -> Result<Option<ControlAccountRow>, StorageError> {
            Ok(None)
        }
        async fn append_event(
            &self,
            _: &str,
            _: &str,
            _: serde_json::Value,
        ) -> Result<i64, StorageError> {
            Ok(0)
        }
    }

    struct StubSession;

    #[async_trait]
    impl Session for StubSession {
        async fn prompt(&self, _: SessionPrompt) -> Result<SessionHandle, SessionError> {
            Err(SessionError::NotFound("stub".into()))
        }
        async fn cancel(&self, _: SessionId) -> Result<(), SessionError> {
            Err(SessionError::NotFound("stub".into()))
        }
        async fn prompt_detached(
            &self,
            _: SessionPrompt,
        ) -> Result<DetachedPromptAccepted, SessionError> {
            Err(SessionError::NotFound("stub".into()))
        }
        async fn status(&self, _: SessionId) -> Result<SessionRuntimeStatus, SessionError> {
            Err(SessionError::NotFound("stub".into()))
        }
        async fn list_statuses(
            &self,
        ) -> Result<std::collections::HashMap<SessionId, SessionRuntimeStatus>, SessionError>
        {
            Ok(std::collections::HashMap::new())
        }
    }

    fn app_with_storage(storage: Arc<dyn Storage>) -> Router {
        let cfg = Config::default();
        let bus = Arc::new(BroadcastBus::new(64));
        let state = AppState {
            config_service: Arc::new(
                opencode_core::config_service::ConfigService::with_cached_resolved(
                    std::env::temp_dir(),
                    None,
                    cfg,
                ),
            ),
            bus: Arc::clone(&bus),
            event_heartbeat: crate::state::EventHeartbeat::default(),
            storage: Arc::clone(&storage),
            session: Arc::new(StubSession),
            permission_runtime: Arc::new(
                opencode_session::permission_runtime::InMemoryPermissionRuntime::new(
                    Arc::clone(&storage),
                    Arc::clone(&bus),
                ),
            ),
            question_runtime: Arc::new(
                opencode_session::question_runtime::InMemoryQuestionRuntime::new(Arc::clone(&bus)),
            ),
            registry: Arc::new(opencode_provider::ModelRegistry::new()),
            provider_catalog_models: Arc::new(Vec::new()),
            provider_auth: Arc::new(ProviderAuthService::new()),
            provider_accounts: Arc::new(AccountService::new(storage)),
            control_plane: crate::state::ControlPlaneConfig::default(),
            control_plane_proxy: Arc::new(crate::control_plane::proxy::HttpProxyService::new(
                reqwest::Client::new(),
                crate::state::ProxyPolicy::default(),
            )),
            harness: false,
        };
        crate::router::build(state)
    }

    fn app(stub: Stub) -> Router {
        let storage: Arc<dyn Storage> = Arc::new(stub);
        app_with_storage(storage)
    }

    fn proj(id: ProjectId) -> ProjectRow {
        ProjectRow {
            id,
            worktree: "/tmp/p".into(),
            vcs: None,
            name: Some("Test".into()),
            icon_url: None,
            icon_color: None,
            time_created: 1000,
            time_updated: 1001,
            time_initialized: None,
            sandboxes: serde_json::Value::Null,
            commands: None,
        }
    }

    // ── Task 5.1: GET /api/v1/projects returns empty list ─────────────────────

    #[tokio::test]
    async fn list_projects_empty() {
        let resp = app(Stub::default())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(val.as_array().unwrap().is_empty());
    }

    // ── Task 5.2: GET /api/v1/projects returns seeded rows ───────────────────

    #[tokio::test]
    async fn list_projects_returns_rows() {
        let stub = Stub::default();
        let pid = ProjectId::new();
        stub.upsert_project(proj(pid)).await.unwrap();

        let resp = app(stub)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let rows: Vec<ProjectRow> = serde_json::from_slice(&body).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, pid);
        assert_eq!(rows[0].worktree, "/tmp/p");
    }

    // ── Task 5.3: PUT /api/v1/projects/:id returns 204 ───────────────────────

    #[tokio::test]
    async fn upsert_project_returns_204() {
        let pid = ProjectId::new();
        let body = serde_json::to_vec(&proj(pid)).unwrap();
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/v1/projects/{pid}"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app(Stub::default()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn upsert_project_path_id_overrides_payload_id() {
        let stub = Arc::new(Stub::default());
        let path_id = ProjectId::new();
        let payload_id = ProjectId::new();
        let body = serde_json::to_vec(&proj(payload_id)).unwrap();
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/v1/projects/{path_id}"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let storage: Arc<dyn Storage> = stub.clone();
        let resp = app_with_storage(storage).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let projects = stub.projects.lock().unwrap();
        assert_eq!(projects.len(), 1);
        let row = &projects[0];
        assert_eq!(row.id, path_id);
        assert_ne!(row.id, payload_id);
    }

    // ── Task 5.4: GET /api/v1/projects/:id not found returns 404 ─────────────

    #[tokio::test]
    async fn get_project_not_found_returns_404() {
        let pid = ProjectId::new();
        let resp = app(Stub::default())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{pid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(val["error"].as_str().unwrap().contains("not found"));
    }

    // ── Task 5.5: GET /api/v1/projects/:id found returns 200 + body ──────────

    #[tokio::test]
    async fn get_project_found_returns_200() {
        let stub = Stub::default();
        let pid = ProjectId::new();
        stub.upsert_project(proj(pid)).await.unwrap();

        let resp = app(stub)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{pid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let row: ProjectRow = serde_json::from_slice(&body).unwrap();
        assert_eq!(row.id, pid);
        assert_eq!(row.name.as_deref(), Some("Test"));
    }

    // ── Error-path coverage: storage failures map to 500 ─────────────────────

    #[tokio::test]
    async fn list_projects_storage_error_returns_500() {
        let resp = app(Stub::failing())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn upsert_project_storage_error_returns_500() {
        let pid = ProjectId::new();
        let body = serde_json::to_vec(&proj(pid)).unwrap();
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/v1/projects/{pid}"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app(Stub::failing()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn get_project_storage_error_returns_500() {
        let pid = ProjectId::new();
        let resp = app(Stub::failing())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{pid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn stub_session_trait_alignment_behaviors_are_deterministic() {
        let session = StubSession;
        let session_id = SessionId::new();

        let prompt_error = session
            .prompt(SessionPrompt {
                session_id,
                text: "hello".into(),
                model: None,
                plan_mode: false,
            })
            .await
            .expect_err("stub prompt should stay unavailable");
        assert!(matches!(prompt_error, SessionError::NotFound(_)));

        let detached_error = session
            .prompt_detached(SessionPrompt {
                session_id,
                text: "hello".into(),
                model: Some("model".into()),
                plan_mode: true,
            })
            .await
            .expect_err("stub detached prompt should stay unavailable");
        assert!(matches!(detached_error, SessionError::NotFound(_)));

        let cancel_error = session
            .cancel(session_id)
            .await
            .expect_err("stub cancel should stay unavailable");
        assert!(matches!(cancel_error, SessionError::NotFound(_)));

        let status_error = session
            .status(session_id)
            .await
            .expect_err("stub status should stay unavailable");
        assert!(matches!(status_error, SessionError::NotFound(_)));

        let statuses = session
            .list_statuses()
            .await
            .expect("stub status list should stay empty");
        assert!(statuses.is_empty());
    }

    #[tokio::test]
    async fn upsert_project_persists_foundation_row_for_non_git_worktree() {
        let stub = Arc::new(Stub::default());
        let pid = ProjectId::new();
        let worktree = temp_test_dir("project-route-foundation-non-git");

        let mut project = proj(pid);
        project.worktree = worktree.to_string_lossy().into_owned();

        let body = serde_json::to_vec(&project).unwrap();
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/v1/projects/{pid}"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let storage: Arc<dyn Storage> = stub.clone();
        let resp = app_with_storage(storage).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let foundations = stub.foundations.lock().unwrap();
        assert_eq!(foundations.len(), 1);
        let foundation = foundations.first().unwrap();
        assert_eq!(foundation.project_id, pid);
        assert_eq!(foundation.vcs_kind, None);
        assert_eq!(foundation.repository_root, None);
    }

    #[tokio::test]
    async fn upsert_project_keeps_projects_response_shape_unchanged() {
        let stub = Arc::new(Stub::default());
        let pid = ProjectId::new();
        let worktree = temp_test_dir("project-route-foundation-response-shape");

        let mut project = proj(pid);
        project.worktree = worktree.to_string_lossy().into_owned();

        let upsert = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/v1/projects/{pid}"))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&project).unwrap()))
            .unwrap();

        let storage: Arc<dyn Storage> = stub.clone();
        let app = app_with_storage(storage);
        let upsert_resp = app.clone().oneshot(upsert).await.unwrap();
        assert_eq!(upsert_resp.status(), StatusCode::NO_CONTENT);

        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{pid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(get_resp.into_body(), 8192)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed.get("project_id").is_none());
        assert!(parsed.get("canonical_worktree").is_none());
        assert_eq!(parsed["id"], serde_json::json!(pid));
        assert_eq!(parsed["worktree"], serde_json::json!(project.worktree));
    }

    #[tokio::test]
    async fn upsert_project_persists_git_probe_details() {
        let stub = Arc::new(Stub::default());
        let pid = ProjectId::new();
        let repo_dir = temp_test_dir("project-route-foundation-git");
        init_git_repo_with_commit(&repo_dir);

        let mut project = proj(pid);
        project.worktree = repo_dir.to_string_lossy().into_owned();

        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/v1/projects/{pid}"))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&project).unwrap()))
            .unwrap();

        let storage: Arc<dyn Storage> = stub.clone();
        let resp = app_with_storage(storage).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let foundations = stub.foundations.lock().unwrap();
        assert_eq!(foundations.len(), 1);
        let foundation = foundations.first().unwrap();

        assert_eq!(foundation.project_id, pid);
        assert_eq!(foundation.vcs_kind.as_deref(), Some("git"));
        assert!(foundation.repository_root.is_some());
        assert_eq!(
            foundation.worktree_state.branch.as_deref(),
            Some("probe-branch")
        );
        assert!(foundation.worktree_state.head_oid.is_some());
        assert_eq!(foundation.worktree_state.is_dirty, Some(false));
    }

    fn temp_test_dir(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("opencode-server-{label}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn init_git_repo_with_commit(dir: &std::path::Path) {
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .status()
                .unwrap();
            assert!(status.success(), "git command failed: git {:?}", args);
        };

        run(&["init"]);
        run(&["checkout", "-b", "probe-branch"]);
        run(&["config", "user.email", "tests@opencode.local"]);
        run(&["config", "user.name", "opencode tests"]);
        std::fs::write(dir.join("README.md"), "probe\n").unwrap();
        run(&["add", "README.md"]);
        run(&["commit", "-m", "init"]);
    }
}
