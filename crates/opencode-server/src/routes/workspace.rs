//! Workspace request DTO and metadata validation parsers.
#![allow(dead_code)]

use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, Uri},
    response::IntoResponse,
};
use opencode_core::{
    dto::WorkspaceRow,
    error::StorageError,
    id::{ProjectId, WorkspaceId},
};
use serde::{Deserialize, Serialize};

use crate::{error::HttpError, state::AppState};

/// Workspace payload accepted by create/update handlers.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WorkspaceWriteDto {
    /// Type identifier (for example: `worktree`, `remote`).
    pub r#type: String,
    /// Optional git branch.
    #[serde(default)]
    pub branch: Option<String>,
    /// Optional display name.
    #[serde(default)]
    pub name: Option<String>,
    /// Optional directory path.
    #[serde(default)]
    pub directory: Option<String>,
    /// Optional metadata blob validated by `parse_workspace_target`.
    #[serde(default)]
    pub extra: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WorkspaceCreateDto {
    #[serde(default)]
    id: Option<WorkspaceId>,
    r#type: String,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    directory: Option<String>,
    #[serde(default)]
    extra: Option<serde_json::Value>,
    project_id: ProjectId,
}

impl WorkspaceCreateDto {
    fn into_row(self) -> WorkspaceRow {
        WorkspaceRow {
            id: self.id.unwrap_or_default(),
            r#type: self.r#type,
            branch: self.branch,
            name: self.name,
            directory: self.directory,
            extra: self.extra,
            project_id: self.project_id,
        }
    }
}

#[derive(Debug, Clone, Default)]
enum PatchField<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}

fn deserialize_patch_field<'de, D, T>(deserializer: D) -> Result<PatchField<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    match Option::<T>::deserialize(deserializer)? {
        Some(value) => Ok(PatchField::Value(value)),
        None => Ok(PatchField::Null),
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct WorkspacePatchDto {
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    branch: PatchField<String>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    name: PatchField<String>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    directory: PatchField<String>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    extra: PatchField<serde_json::Value>,
}

impl WorkspacePatchDto {
    fn apply(self, row: &mut WorkspaceRow) {
        if let Some(value) = self.r#type {
            row.r#type = value;
        }
        match self.branch {
            PatchField::Missing => {}
            PatchField::Null => row.branch = None,
            PatchField::Value(value) => row.branch = Some(value),
        }
        match self.name {
            PatchField::Missing => {}
            PatchField::Null => row.name = None,
            PatchField::Value(value) => row.name = Some(value),
        }
        match self.directory {
            PatchField::Missing => {}
            PatchField::Null => row.directory = None,
            PatchField::Value(value) => row.directory = Some(value),
        }
        match self.extra {
            PatchField::Missing => {}
            PatchField::Null => row.extra = None,
            PatchField::Value(value) => row.extra = Some(value),
        }
    }
}

/// `GET /api/v1/workspaces` — list all workspaces.
pub(crate) async fn list(State(s): State<AppState>) -> impl IntoResponse {
    match s.storage.list_workspaces().await {
        Ok(rows) => (StatusCode::OK, Json(rows)).into_response(),
        Err(err) => map_workspace_storage_error(err).into_response(),
    }
}

/// `POST /api/v1/workspaces` — create a workspace row.
pub(crate) async fn create(
    State(s): State<AppState>,
    Json(dto): Json<WorkspaceCreateDto>,
) -> impl IntoResponse {
    if let Err(err) = parse_workspace_target(&dto.r#type, dto.extra.as_ref()) {
        return HttpError::bad_request(err.to_string()).into_response();
    }

    let row = dto.into_row();
    match s.storage.upsert_workspace(row.clone()).await {
        Ok(()) => (StatusCode::OK, Json(row)).into_response(),
        Err(err) => map_workspace_storage_error(err).into_response(),
    }
}

/// `GET /api/v1/workspaces/:id` — fetch a workspace by id.
pub(crate) async fn get(
    State(s): State<AppState>,
    Path(id): Path<WorkspaceId>,
) -> impl IntoResponse {
    match s.storage.get_workspace(id).await {
        Ok(Some(row)) => (StatusCode::OK, Json(row)).into_response(),
        Ok(None) => HttpError::not_found(format!("workspace {id} not found")).into_response(),
        Err(err) => map_workspace_storage_error(err).into_response(),
    }
}

/// `PATCH /api/v1/workspaces/:id` — patch mutable workspace fields.
pub(crate) async fn patch(
    State(s): State<AppState>,
    Path(id): Path<WorkspaceId>,
    Json(dto): Json<WorkspacePatchDto>,
) -> impl IntoResponse {
    let mut row = match s.storage.get_workspace(id).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            return HttpError::not_found(format!("workspace {id} not found")).into_response();
        }
        Err(err) => return map_workspace_storage_error(err).into_response(),
    };

    dto.apply(&mut row);
    if let Err(err) = parse_workspace_target(&row.r#type, row.extra.as_ref()) {
        return HttpError::bad_request(err.to_string()).into_response();
    }

    match s.storage.upsert_workspace(row.clone()).await {
        Ok(()) => (StatusCode::OK, Json(row)).into_response(),
        Err(err) => map_workspace_storage_error(err).into_response(),
    }
}

/// `DELETE /api/v1/workspaces/:id` — delete and return prior row.
pub(crate) async fn delete(
    State(s): State<AppState>,
    Path(id): Path<WorkspaceId>,
) -> impl IntoResponse {
    let row = match s.storage.get_workspace(id).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            return HttpError::not_found(format!("workspace {id} not found")).into_response();
        }
        Err(err) => return map_workspace_storage_error(err).into_response(),
    };

    match s.storage.delete_workspace(id).await {
        Ok(()) => (StatusCode::OK, Json(row)).into_response(),
        Err(err) => map_workspace_storage_error(err).into_response(),
    }
}

fn map_workspace_storage_error(err: StorageError) -> HttpError {
    match err {
        StorageError::NotFound { entity, id } => {
            HttpError::not_found(format!("not found: {entity} {id}"))
        }
        StorageError::Db(msg) | StorageError::Serde(msg) => HttpError::internal(msg),
        _ => HttpError::internal(err.to_string()),
    }
}

/// Parsed workspace target metadata used by route-level validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkspaceTargetMetadata {
    /// Remote target with explicit instance and base URL.
    Remote(RemoteWorkspaceTarget),
    /// Non-remote workspace types with no required target fields.
    Other,
}

/// Validated remote workspace target metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteWorkspaceTarget {
    /// Destination instance identifier.
    pub instance: String,
    /// Destination control-plane base URL.
    pub base_url: String,
}

/// Validation failures for workspace metadata payload parsing.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub(crate) enum WorkspaceValidationError {
    #[error("remote workspace extra metadata is required")]
    MissingExtra,
    #[error("remote workspace extra metadata must be a JSON object")]
    ExtraNotObject,
    #[error("remote workspace metadata field '{field}' is required")]
    MissingField { field: &'static str },
    #[error("remote workspace metadata field '{field}' must be a non-empty string")]
    InvalidField { field: &'static str },
    #[error("remote workspace base_url must be an absolute http(s) URL")]
    InvalidBaseUrl,
}

/// Parse route payload metadata into validated workspace target semantics.
///
/// Control-plane routing depends on this parser to reject malformed `remote`
/// targets early at write-time, so selector-based forwarding does not fail
/// later with ambiguous proxy errors.
pub(crate) fn parse_workspace_target(
    workspace_type: &str,
    extra: Option<&serde_json::Value>,
) -> Result<WorkspaceTargetMetadata, WorkspaceValidationError> {
    if workspace_type != "remote" {
        return Ok(WorkspaceTargetMetadata::Other);
    }

    let Some(extra) = extra else {
        return Err(WorkspaceValidationError::MissingExtra);
    };
    let Some(obj) = extra.as_object() else {
        return Err(WorkspaceValidationError::ExtraNotObject);
    };

    let instance = read_required_string(obj, "instance")?;
    let base_url = read_required_string(obj, "base_url")?;
    validate_base_url(&base_url)?;

    Ok(WorkspaceTargetMetadata::Remote(RemoteWorkspaceTarget {
        instance,
        base_url,
    }))
}

fn read_required_string(
    obj: &serde_json::Map<String, serde_json::Value>,
    field: &'static str,
) -> Result<String, WorkspaceValidationError> {
    let Some(value) = obj.get(field) else {
        return Err(WorkspaceValidationError::MissingField { field });
    };
    let Some(value) = value.as_str() else {
        return Err(WorkspaceValidationError::InvalidField { field });
    };
    if value.trim().is_empty() {
        return Err(WorkspaceValidationError::InvalidField { field });
    }
    Ok(value.to_string())
}

fn validate_base_url(raw: &str) -> Result<(), WorkspaceValidationError> {
    let Ok(uri) = raw.parse::<Uri>() else {
        return Err(WorkspaceValidationError::InvalidBaseUrl);
    };
    let Some(scheme) = uri.scheme_str() else {
        return Err(WorkspaceValidationError::InvalidBaseUrl);
    };
    if !(scheme == "http" || scheme == "https") {
        return Err(WorkspaceValidationError::InvalidBaseUrl);
    }
    if uri.authority().is_none() {
        return Err(WorkspaceValidationError::InvalidBaseUrl);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{
        Router,
        body::Body,
        http::{Method, Request, StatusCode},
        routing::get as get_route,
    };
    use opencode_bus::BroadcastBus;
    use opencode_core::{
        config::Config,
        dto::{
            AccountRow, AccountStateRow, ControlAccountRow, MessageRow, MessageWithParts, PartRow,
            PermissionRow, ProjectRow, SessionRow, TodoRow, WorkspaceRow,
        },
        error::{SessionError, StorageError},
        id::{AccountId, ProjectId, SessionId, WorkspaceId},
    };
    use opencode_provider::{AccountService, ModelRegistry, ProviderAuthService};
    use opencode_session::{
        engine::Session,
        types::{DetachedPromptAccepted, SessionHandle, SessionPrompt, SessionRuntimeStatus},
    };
    use opencode_storage::Storage;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    #[test]
    fn remote_workspace_rejects_missing_extra_payload() {
        let err = parse_workspace_target("remote", None).expect_err("missing payload must fail");
        assert!(err.to_string().contains("extra metadata is required"));
    }

    #[test]
    fn remote_workspace_accepts_valid_target_payload() {
        let parsed = parse_workspace_target(
            "remote",
            Some(&json!({"instance": "control-plane-a", "base_url": "https://cp-a.example"})),
        )
        .expect("valid payload should parse");

        match parsed {
            WorkspaceTargetMetadata::Remote(remote) => {
                assert_eq!(remote.instance, "control-plane-a");
                assert_eq!(remote.base_url, "https://cp-a.example");
            }
            WorkspaceTargetMetadata::Other => {
                panic!("remote payload should parse as remote target")
            }
        }
    }

    #[test]
    fn remote_workspace_rejects_missing_instance_field() {
        let err =
            parse_workspace_target("remote", Some(&json!({"base_url": "https://cp-a.example"})))
                .expect_err("missing instance must fail");
        assert_eq!(
            err,
            WorkspaceValidationError::MissingField { field: "instance" }
        );
    }

    #[test]
    fn remote_workspace_rejects_non_http_base_url() {
        let err = parse_workspace_target(
            "remote",
            Some(&json!({"instance": "control-plane-a", "base_url": "ftp://cp-a.example"})),
        )
        .expect_err("unsupported scheme must fail");
        assert_eq!(err, WorkspaceValidationError::InvalidBaseUrl);
    }

    #[test]
    fn non_remote_workspace_skips_remote_target_validation() {
        let parsed = parse_workspace_target("worktree", None).expect("non-remote should pass");
        assert_eq!(parsed, WorkspaceTargetMetadata::Other);
    }

    #[derive(Default)]
    struct StubStorage {
        workspaces: Mutex<Vec<WorkspaceRow>>,
        list_error: Option<StubListError>,
    }

    #[derive(Debug, Clone)]
    enum StubListError {
        Db(String),
        Serde(String),
    }

    impl StubStorage {
        fn with_list_error(msg: impl Into<String>) -> Self {
            Self {
                workspaces: Mutex::new(Vec::new()),
                list_error: Some(StubListError::Db(msg.into())),
            }
        }

        fn with_list_serde_error(msg: impl Into<String>) -> Self {
            Self {
                workspaces: Mutex::new(Vec::new()),
                list_error: Some(StubListError::Serde(msg.into())),
            }
        }
    }

    #[async_trait]
    impl Storage for StubStorage {
        async fn upsert_project(&self, _: ProjectRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn get_project(&self, _: ProjectId) -> Result<Option<ProjectRow>, StorageError> {
            Ok(None)
        }
        async fn list_projects(&self) -> Result<Vec<ProjectRow>, StorageError> {
            Ok(vec![])
        }
        async fn upsert_workspace(&self, row: WorkspaceRow) -> Result<(), StorageError> {
            let mut items = self.workspaces.lock().expect("workspace store lock");
            items.retain(|item| item.id != row.id);
            items.push(row);
            Ok(())
        }
        async fn get_workspace(
            &self,
            id: WorkspaceId,
        ) -> Result<Option<WorkspaceRow>, StorageError> {
            Ok(self
                .workspaces
                .lock()
                .expect("workspace store lock")
                .iter()
                .find(|item| item.id == id)
                .cloned())
        }
        async fn list_workspaces(&self) -> Result<Vec<WorkspaceRow>, StorageError> {
            if let Some(err) = self.list_error.clone() {
                return Err(match err {
                    StubListError::Db(msg) => StorageError::Db(msg),
                    StubListError::Serde(msg) => StorageError::Serde(msg),
                });
            }
            Ok(self
                .workspaces
                .lock()
                .expect("workspace store lock")
                .clone())
        }
        async fn delete_workspace(&self, id: WorkspaceId) -> Result<(), StorageError> {
            self.workspaces
                .lock()
                .expect("workspace store lock")
                .retain(|item| item.id != id);
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

    fn app() -> Router {
        app_with_storage(StubStorage::default())
    }

    fn app_with_storage(storage: StubStorage) -> Router {
        let storage: Arc<dyn Storage> = Arc::new(storage);
        app_with_dyn_storage(storage)
    }

    fn app_with_dyn_storage(storage: Arc<dyn Storage>) -> Router {
        let bus = Arc::new(BroadcastBus::new(64));
        let state = crate::state::AppState {
            config_service: Arc::new(
                opencode_core::config_service::ConfigService::with_cached_resolved(
                    std::env::temp_dir(),
                    None,
                    Config::default(),
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
            registry: Arc::new(ModelRegistry::new()),
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

        Router::new()
            .route("/api/v1/workspaces", get_route(list).post(create))
            .route(
                "/api/v1/workspaces/{id}",
                get_route(get).patch(patch).delete(delete),
            )
            .with_state(state)
    }

    #[tokio::test]
    async fn workspace_crud_round_trip_through_handlers() {
        let app = app();
        let id = WorkspaceId::new();
        let project_id = ProjectId::new();

        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "id": id,
                            "type": "remote",
                            "project_id": project_id,
                            "extra": {
                                "instance": "cp-a",
                                "base_url": "https://cp-a.example"
                            }
                        }))
                        .expect("serialize create payload"),
                    ))
                    .expect("build create request"),
            )
            .await
            .expect("create request should complete");
        assert_eq!(created.status(), StatusCode::OK);

        let listed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces")
                    .body(Body::empty())
                    .expect("build list request"),
            )
            .await
            .expect("list request should complete");
        assert_eq!(listed.status(), StatusCode::OK);
        let listed_body = axum::body::to_bytes(listed.into_body(), 8192)
            .await
            .expect("list body bytes");
        let listed_value: serde_json::Value =
            serde_json::from_slice(&listed_body).expect("parse list response JSON");
        assert_eq!(listed_value.as_array().map(std::vec::Vec::len), Some(1));

        let fetched = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/{id}"))
                    .body(Body::empty())
                    .expect("build get request"),
            )
            .await
            .expect("get request should complete");
        assert_eq!(fetched.status(), StatusCode::OK);

        let patched = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/api/v1/workspaces/{id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({ "name": "Alpha" }))
                            .expect("serialize patch payload"),
                    ))
                    .expect("build patch request"),
            )
            .await
            .expect("patch request should complete");
        assert_eq!(patched.status(), StatusCode::OK);
        let patched_body = axum::body::to_bytes(patched.into_body(), 8192)
            .await
            .expect("patch body bytes");
        let patched_value: serde_json::Value =
            serde_json::from_slice(&patched_body).expect("parse patch response JSON");
        assert_eq!(patched_value["name"], "Alpha");

        let removed = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/v1/workspaces/{id}"))
                    .body(Body::empty())
                    .expect("build delete request"),
            )
            .await
            .expect("delete request should complete");
        assert_eq!(removed.status(), StatusCode::OK);

        let missing = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/{id}"))
                    .body(Body::empty())
                    .expect("build get missing request"),
            )
            .await
            .expect("missing request should complete");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn create_remote_workspace_rejects_invalid_metadata_without_writing() {
        let app = app();
        let project_id = ProjectId::new();

        let create = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "type": "remote",
                            "project_id": project_id
                        }))
                        .expect("serialize invalid create payload"),
                    ))
                    .expect("build invalid create request"),
            )
            .await
            .expect("invalid create request should complete");
        assert_eq!(create.status(), StatusCode::BAD_REQUEST);

        let listed = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces")
                    .body(Body::empty())
                    .expect("build list request"),
            )
            .await
            .expect("list request should complete");
        assert_eq!(listed.status(), StatusCode::OK);
        let body = axum::body::to_bytes(listed.into_body(), 8192)
            .await
            .expect("list body bytes");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("parse list JSON");
        assert_eq!(value.as_array().map(std::vec::Vec::len), Some(0));
    }

    #[tokio::test]
    async fn create_remote_workspace_validation_failure_returns_bad_request_error_body() {
        let app = app();
        let project_id = ProjectId::new();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "type": "remote",
                            "project_id": project_id
                        }))
                        .expect("serialize invalid create payload"),
                    ))
                    .expect("build invalid create request"),
            )
            .await
            .expect("invalid create request should complete");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .expect("response body bytes");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("parse response JSON");
        assert_eq!(
            value["error"],
            "remote workspace extra metadata is required"
        );
    }

    #[tokio::test]
    async fn get_missing_workspace_returns_not_found_error_body() {
        let app = app();
        let missing_id = WorkspaceId::new();

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/{missing_id}"))
                    .body(Body::empty())
                    .expect("build get request"),
            )
            .await
            .expect("get request should complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .expect("response body bytes");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("parse response JSON");
        assert_eq!(value["error"], format!("workspace {missing_id} not found"));
    }

    #[tokio::test]
    async fn list_storage_failure_returns_internal_error_body() {
        let app = app_with_storage(StubStorage::with_list_error("workspace list unavailable"));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces")
                    .body(Body::empty())
                    .expect("build list request"),
            )
            .await
            .expect("list request should complete");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .expect("response body bytes");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("parse response JSON");
        assert_eq!(value["error"], "workspace list unavailable");
    }

    #[tokio::test]
    async fn list_storage_serde_failure_returns_internal_error_body() {
        let app = app_with_storage(StubStorage::with_list_serde_error(
            "workspace metadata decode failed",
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces")
                    .body(Body::empty())
                    .expect("build list request"),
            )
            .await
            .expect("list request should complete");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .expect("response body bytes");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("parse response JSON");
        assert_eq!(value["error"], "workspace metadata decode failed");
    }
}
