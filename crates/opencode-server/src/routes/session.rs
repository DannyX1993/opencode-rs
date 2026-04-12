//! `/api/v1/projects/:pid/sessions` and `/api/v1/sessions/:sid` route handlers.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use opencode_core::{
    dto::{MessageWithParts, PartRow, SessionRow},
    id::{MessageId, ProjectId, SessionId, WorkspaceId},
};
use opencode_session::types::{SessionHandle, SessionPrompt};
use serde::{Deserialize, Serialize};

use crate::{error::HttpError, state::AppState};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionDto {
    id: SessionId,
    project_id: ProjectId,
    #[serde(default)]
    workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    parent_id: Option<SessionId>,
    slug: String,
    directory: String,
    title: String,
    version: String,
    #[serde(default)]
    share_url: Option<String>,
    #[serde(default)]
    summary_additions: Option<i64>,
    #[serde(default)]
    summary_deletions: Option<i64>,
    #[serde(default)]
    summary_files: Option<i64>,
    #[serde(default)]
    summary_diffs: Option<serde_json::Value>,
    #[serde(default)]
    revert: Option<serde_json::Value>,
    #[serde(default)]
    permission: Option<serde_json::Value>,
    time_created: i64,
    time_updated: i64,
    #[serde(default)]
    time_compacting: Option<i64>,
    #[serde(default)]
    time_archived: Option<i64>,
}

impl From<SessionRow> for SessionDto {
    fn from(row: SessionRow) -> Self {
        Self {
            id: row.id,
            project_id: row.project_id,
            workspace_id: row.workspace_id,
            parent_id: row.parent_id,
            slug: row.slug,
            directory: row.directory,
            title: row.title,
            version: row.version,
            share_url: row.share_url,
            summary_additions: row.summary_additions,
            summary_deletions: row.summary_deletions,
            summary_files: row.summary_files,
            summary_diffs: row.summary_diffs,
            revert: row.revert,
            permission: row.permission,
            time_created: row.time_created,
            time_updated: row.time_updated,
            time_compacting: row.time_compacting,
            time_archived: row.time_archived,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SessionCreateDto {
    id: SessionId,
    #[serde(default)]
    workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    parent_id: Option<SessionId>,
    slug: String,
    directory: String,
    title: String,
    version: String,
    #[serde(default)]
    share_url: Option<String>,
    #[serde(default)]
    summary_additions: Option<i64>,
    #[serde(default)]
    summary_deletions: Option<i64>,
    #[serde(default)]
    summary_files: Option<i64>,
    #[serde(default)]
    summary_diffs: Option<serde_json::Value>,
    #[serde(default)]
    revert: Option<serde_json::Value>,
    #[serde(default)]
    permission: Option<serde_json::Value>,
    time_created: i64,
    time_updated: i64,
    #[serde(default)]
    time_compacting: Option<i64>,
    #[serde(default)]
    time_archived: Option<i64>,
}

impl SessionCreateDto {
    fn into_row(self, project_id: ProjectId) -> SessionRow {
        SessionRow {
            id: self.id,
            project_id,
            workspace_id: self.workspace_id,
            parent_id: self.parent_id,
            slug: self.slug,
            directory: self.directory,
            title: self.title,
            version: self.version,
            share_url: self.share_url,
            summary_additions: self.summary_additions,
            summary_deletions: self.summary_deletions,
            summary_files: self.summary_files,
            summary_diffs: self.summary_diffs,
            revert: self.revert,
            permission: self.permission,
            time_created: self.time_created,
            time_updated: self.time_updated,
            time_compacting: self.time_compacting,
            time_archived: self.time_archived,
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

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SessionPatchDto {
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    workspace_id: PatchField<WorkspaceId>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    time_updated: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    share_url: PatchField<String>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    summary_additions: PatchField<i64>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    summary_deletions: PatchField<i64>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    summary_files: PatchField<i64>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    summary_diffs: PatchField<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    revert: PatchField<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    permission: PatchField<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    time_compacting: PatchField<i64>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    time_archived: PatchField<i64>,
}

impl SessionPatchDto {
    fn apply(self, row: &mut SessionRow) {
        match self.workspace_id {
            PatchField::Missing => {}
            PatchField::Null => row.workspace_id = None,
            PatchField::Value(value) => row.workspace_id = Some(value),
        }
        if let Some(value) = self.title {
            row.title = value;
        }
        if let Some(value) = self.time_updated {
            row.time_updated = value;
        }
        match self.share_url {
            PatchField::Missing => {}
            PatchField::Null => row.share_url = None,
            PatchField::Value(value) => row.share_url = Some(value),
        }
        match self.summary_additions {
            PatchField::Missing => {}
            PatchField::Null => row.summary_additions = None,
            PatchField::Value(value) => row.summary_additions = Some(value),
        }
        match self.summary_deletions {
            PatchField::Missing => {}
            PatchField::Null => row.summary_deletions = None,
            PatchField::Value(value) => row.summary_deletions = Some(value),
        }
        match self.summary_files {
            PatchField::Missing => {}
            PatchField::Null => row.summary_files = None,
            PatchField::Value(value) => row.summary_files = Some(value),
        }
        match self.summary_diffs {
            PatchField::Missing => {}
            PatchField::Null => row.summary_diffs = None,
            PatchField::Value(value) => row.summary_diffs = Some(value),
        }
        match self.revert {
            PatchField::Missing => {}
            PatchField::Null => row.revert = None,
            PatchField::Value(value) => row.revert = Some(value),
        }
        match self.permission {
            PatchField::Missing => {}
            PatchField::Null => row.permission = None,
            PatchField::Value(value) => row.permission = Some(value),
        }
        match self.time_compacting {
            PatchField::Missing => {}
            PatchField::Null => row.time_compacting = None,
            PatchField::Value(value) => row.time_compacting = Some(value),
        }
        match self.time_archived {
            PatchField::Missing => {}
            PatchField::Null => row.time_archived = None,
            PatchField::Value(value) => row.time_archived = Some(value),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SessionPromptDto {
    text: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    plan_mode: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SessionHandleDto {
    session_id: SessionId,
    #[serde(default)]
    assistant_message_id: Option<MessageId>,
    #[serde(default)]
    resolved_model: Option<String>,
}

impl From<SessionHandle> for SessionHandleDto {
    fn from(handle: SessionHandle) -> Self {
        Self {
            session_id: handle.session_id,
            assistant_message_id: handle.assistant_message_id,
            resolved_model: handle.resolved_model,
        }
    }
}

/// `POST /api/v1/projects/:pid/sessions` — create a new session.
pub(crate) async fn create(
    State(s): State<AppState>,
    Path(pid): Path<ProjectId>,
    Json(dto): Json<SessionCreateDto>,
) -> impl IntoResponse {
    let row = dto.into_row(pid);
    match s.storage.create_session(row).await {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => HttpError::from(e).into_response(),
    }
}

/// `GET /api/v1/projects/:pid/sessions` — list sessions for a project.
pub(crate) async fn list(
    State(s): State<AppState>,
    Path(pid): Path<ProjectId>,
) -> impl IntoResponse {
    match s.storage.list_sessions(pid).await {
        Ok(rows) => {
            let body = rows.into_iter().map(SessionDto::from).collect::<Vec<_>>();
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => HttpError::from(e).into_response(),
    }
}

/// `GET /api/v1/sessions/:sid` — fetch one session by id.
pub(crate) async fn get(
    State(s): State<AppState>,
    Path(sid): Path<SessionId>,
) -> impl IntoResponse {
    match s.storage.get_session(sid).await {
        Ok(Some(row)) => (StatusCode::OK, Json(SessionDto::from(row))).into_response(),
        Ok(None) => HttpError::not_found(format!("session {sid} not found")).into_response(),
        Err(e) => HttpError::from(e).into_response(),
    }
}

/// `PATCH /api/v1/sessions/:sid` — update a session.
pub(crate) async fn update(
    State(s): State<AppState>,
    Path(sid): Path<SessionId>,
    Json(patch): Json<SessionPatchDto>,
) -> impl IntoResponse {
    let mut row = match s.storage.get_session(sid).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            return HttpError::not_found(format!("session {sid} not found")).into_response();
        }
        Err(e) => return HttpError::from(e).into_response(),
    };
    patch.apply(&mut row);
    match s.storage.update_session(row).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => HttpError::from(e).into_response(),
    }
}

/// `GET /api/v1/sessions/:sid/messages` — list messages with parts for a session.
pub(crate) async fn list_messages(
    State(s): State<AppState>,
    Path(sid): Path<SessionId>,
) -> impl IntoResponse {
    match s.storage.list_history_with_parts(sid).await {
        Ok(rows) => (StatusCode::OK, Json(rows)).into_response(),
        Err(e) => HttpError::from(e).into_response(),
    }
}

/// `POST /api/v1/sessions/:sid/messages` — append a message (with parts) to a session.
pub(crate) async fn append_message(
    State(s): State<AppState>,
    Path(sid): Path<SessionId>,
    Json(payload): Json<MessageWithParts>,
) -> impl IntoResponse {
    let mut msg = payload.info;
    msg.session_id = sid;
    let parts: Vec<PartRow> = payload
        .parts
        .into_iter()
        .map(|mut p| {
            p.session_id = sid;
            p.message_id = msg.id;
            p
        })
        .collect();
    match s.storage.append_message(msg, parts).await {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => HttpError::from(e).into_response(),
    }
}

/// `POST /api/v1/sessions/:sid/prompt` — start a prompt turn for a session.
pub(crate) async fn prompt(
    State(s): State<AppState>,
    Path(sid): Path<SessionId>,
    Json(payload): Json<SessionPromptDto>,
) -> impl IntoResponse {
    match s
        .session
        .prompt(SessionPrompt {
            session_id: sid,
            text: payload.text,
            model: payload.model,
            plan_mode: payload.plan_mode,
        })
        .await
    {
        Ok(handle) => (StatusCode::ACCEPTED, Json(SessionHandleDto::from(handle))).into_response(),
        Err(err) => HttpError::from(err).into_response(),
    }
}

/// `POST /api/v1/sessions/:sid/cancel` — cancel the active prompt turn for a session.
pub(crate) async fn cancel(
    State(s): State<AppState>,
    Path(sid): Path<SessionId>,
) -> impl IntoResponse {
    match s.session.cancel(sid).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(err) => HttpError::from(err).into_response(),
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
        dto::{AccountRow, MessageRow, PermissionRow, ProjectRow, TodoRow},
        error::{SessionError, StorageError},
        id::{MessageId, PartId, ProjectId, SessionId},
    };
    use opencode_session::{
        engine::Session,
        types::{SessionHandle, SessionPrompt},
    };
    use opencode_storage::Storage;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    // ── Configurable stub with real in-memory session state ───────────────────

    #[derive(Default)]
    struct Stub {
        sessions: Mutex<Vec<SessionRow>>,
        messages: Mutex<Vec<MessageWithParts>>,
    }

    #[async_trait]
    impl Storage for Stub {
        async fn upsert_project(&self, _: ProjectRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn get_project(&self, _: ProjectId) -> Result<Option<ProjectRow>, StorageError> {
            Ok(None)
        }
        async fn list_projects(&self) -> Result<Vec<ProjectRow>, StorageError> {
            Ok(vec![])
        }
        async fn create_session(&self, row: SessionRow) -> Result<(), StorageError> {
            self.sessions.lock().unwrap().push(row);
            Ok(())
        }
        async fn get_session(&self, id: SessionId) -> Result<Option<SessionRow>, StorageError> {
            Ok(self
                .sessions
                .lock()
                .unwrap()
                .iter()
                .find(|s| s.id == id)
                .cloned())
        }
        async fn list_sessions(&self, pid: ProjectId) -> Result<Vec<SessionRow>, StorageError> {
            Ok(self
                .sessions
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.project_id == pid)
                .cloned()
                .collect())
        }
        async fn update_session(&self, row: SessionRow) -> Result<(), StorageError> {
            let mut v = self.sessions.lock().unwrap();
            if let Some(s) = v.iter_mut().find(|s| s.id == row.id) {
                *s = row;
            }
            Ok(())
        }
        async fn append_message(
            &self,
            msg: MessageRow,
            parts: Vec<PartRow>,
        ) -> Result<(), StorageError> {
            self.messages
                .lock()
                .unwrap()
                .push(MessageWithParts { info: msg, parts });
            Ok(())
        }
        async fn append_part(&self, part: PartRow) -> Result<(), StorageError> {
            let mut messages = self.messages.lock().unwrap();
            if let Some(message) = messages.iter_mut().find(|m| m.info.id == part.message_id) {
                message.parts.push(part);
            }
            Ok(())
        }
        async fn list_history(&self, _: SessionId) -> Result<Vec<MessageRow>, StorageError> {
            Ok(vec![])
        }
        async fn list_history_with_parts(
            &self,
            sid: SessionId,
        ) -> Result<Vec<MessageWithParts>, StorageError> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.info.session_id == sid)
                .cloned()
                .collect())
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
        async fn append_event(
            &self,
            _: &str,
            _: &str,
            _: serde_json::Value,
        ) -> Result<i64, StorageError> {
            Ok(0)
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum StubSessionMode {
        NotFound,
        Success,
        Busy,
        NoActiveRun,
    }

    struct StubSession {
        mode: StubSessionMode,
        prompts: Arc<Mutex<Vec<SessionPrompt>>>,
        cancels: Arc<Mutex<Vec<SessionId>>>,
    }

    impl StubSession {
        fn new(mode: StubSessionMode) -> Self {
            Self {
                mode,
                prompts: Arc::new(Mutex::new(Vec::new())),
                cancels: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl Session for StubSession {
        async fn prompt(&self, req: SessionPrompt) -> Result<SessionHandle, SessionError> {
            self.prompts.lock().unwrap().push(req.clone());
            match self.mode {
                StubSessionMode::NotFound => Err(SessionError::NotFound("stub".into())),
                StubSessionMode::Busy => Err(SessionError::Busy(req.session_id.to_string())),
                StubSessionMode::Success => Ok(SessionHandle::new(req.session_id)
                    .with_assistant_message_id(MessageId::new())
                    .with_resolved_model(req.model.unwrap_or_else(|| "stub/model".into()))),
                StubSessionMode::NoActiveRun => {
                    Err(SessionError::NoActiveRun(req.session_id.to_string()))
                }
            }
        }

        async fn cancel(&self, session_id: SessionId) -> Result<(), SessionError> {
            self.cancels.lock().unwrap().push(session_id);
            match self.mode {
                StubSessionMode::Success => Ok(()),
                StubSessionMode::NoActiveRun => {
                    Err(SessionError::NoActiveRun(session_id.to_string()))
                }
                StubSessionMode::NotFound => Err(SessionError::NotFound("stub".into())),
                StubSessionMode::Busy => Err(SessionError::Busy(session_id.to_string())),
            }
        }
    }

    fn app(stub: Stub) -> Router {
        app_with_session(stub, Arc::new(StubSession::new(StubSessionMode::NotFound)))
    }

    fn app_with_session(stub: Stub, session: Arc<dyn Session>) -> Router {
        let state = crate::state::AppState {
            config: Arc::new(Config::default()),
            bus: Arc::new(BroadcastBus::new(64)),
            storage: Arc::new(stub),
            session,
            registry: Arc::new(opencode_provider::ModelRegistry::new()),
            harness: false,
        };
        crate::router::build(state)
    }

    /// Build router while keeping a handle to the stub for post-request state inspection.
    fn app_arc(stub: Arc<Stub>) -> Router {
        app_arc_with_session(stub, Arc::new(StubSession::new(StubSessionMode::NotFound)))
    }

    fn app_arc_with_session(stub: Arc<Stub>, session: Arc<dyn Session>) -> Router {
        let state = crate::state::AppState {
            config: Arc::new(Config::default()),
            bus: Arc::new(BroadcastBus::new(64)),
            storage: stub,
            session,
            registry: Arc::new(opencode_provider::ModelRegistry::new()),
            harness: false,
        };
        crate::router::build(state)
    }

    fn sess(sid: SessionId, pid: ProjectId) -> SessionRow {
        SessionRow {
            id: sid,
            project_id: pid,
            workspace_id: None,
            parent_id: None,
            slug: "test-slug".into(),
            directory: "/tmp/s".into(),
            title: "Test Session".into(),
            version: "1".into(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            time_created: 1000,
            time_updated: 1001,
            time_compacting: None,
            time_archived: None,
        }
    }

    fn create_dto(row: SessionRow) -> SessionCreateDto {
        SessionCreateDto {
            id: row.id,
            workspace_id: row.workspace_id,
            parent_id: row.parent_id,
            slug: row.slug,
            directory: row.directory,
            title: row.title,
            version: row.version,
            share_url: row.share_url,
            summary_additions: row.summary_additions,
            summary_deletions: row.summary_deletions,
            summary_files: row.summary_files,
            summary_diffs: row.summary_diffs,
            revert: row.revert,
            permission: row.permission,
            time_created: row.time_created,
            time_updated: row.time_updated,
            time_compacting: row.time_compacting,
            time_archived: row.time_archived,
        }
    }

    fn msg_with_parts(mid: MessageId, sid: SessionId) -> MessageWithParts {
        MessageWithParts {
            info: MessageRow {
                id: mid,
                session_id: sid,
                time_created: 2000,
                time_updated: 2001,
                data: serde_json::json!({"role": "user"}),
            },
            parts: vec![PartRow {
                id: PartId::new(),
                message_id: mid,
                session_id: sid,
                time_created: 2000,
                time_updated: 2001,
                data: serde_json::json!({"type": "text", "text": "hello"}),
            }],
        }
    }

    // ── Task 6.1: POST /api/v1/projects/:pid/sessions → 201 ──────────────────

    #[tokio::test]
    async fn create_session_returns_201() {
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let body = serde_json::to_vec(&create_dto(sess(sid, pid))).unwrap();
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/projects/{pid}/sessions"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app(Stub::default()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // ── Task 6.1 triangulation: project_id is overridden from path ───────────

    #[tokio::test]
    async fn create_session_overrides_project_id_from_path() {
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let body = serde_json::to_vec(&create_dto(sess(sid, pid))).unwrap();
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/projects/{pid}/sessions"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let stub = Arc::new(Stub::default());
        let resp = app_arc(Arc::clone(&stub)).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        // verify the persisted session has the path pid, NOT other_pid
        let stored = stub.sessions.lock().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].project_id, pid, "project_id must come from path");
    }

    // ── Task 6.2: GET /api/v1/sessions/:sid → 200 with body ──────────────────

    #[tokio::test]
    async fn get_session_found_returns_200() {
        let stub = Stub::default();
        let pid = ProjectId::new();
        let sid = SessionId::new();
        stub.create_session(sess(sid, pid)).await.unwrap();

        let resp = app(stub)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/sessions/{sid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let row: SessionDto = serde_json::from_slice(&body).unwrap();
        assert_eq!(row.id, sid);
        assert_eq!(row.title, "Test Session");
    }

    // ── Task 6.2 triangulation: GET /api/v1/sessions/:sid not found → 404 ────

    #[tokio::test]
    async fn get_session_not_found_returns_404() {
        let sid = SessionId::new();
        let resp = app(Stub::default())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/sessions/{sid}"))
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

    // ── Task 6.3: PATCH /api/v1/sessions/:sid → 204 ──────────────────────────

    #[tokio::test]
    async fn update_session_returns_204() {
        let stub = Stub::default();
        let pid = ProjectId::new();
        let sid = SessionId::new();
        stub.create_session(sess(sid, pid)).await.unwrap();

        let body = serde_json::to_vec(&serde_json::json!({"title": "Updated Title"})).unwrap();
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(format!("/api/v1/sessions/{sid}"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app(stub).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    // ── Task 6.3 triangulation: update with different id overrides from path ──

    #[tokio::test]
    async fn update_session_updates_requested_id_only() {
        let stub = Arc::new(Stub::default());
        let pid = ProjectId::new();
        let sid = SessionId::new();
        stub.create_session(sess(sid, pid)).await.unwrap();
        let other_sid = SessionId::new();
        stub.create_session(sess(other_sid, pid)).await.unwrap();

        let body = serde_json::to_vec(&serde_json::json!({"title": "Overridden Title"})).unwrap();
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(format!("/api/v1/sessions/{sid}"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app_arc(Arc::clone(&stub)).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        // verify the persisted session was updated under sid (path), not wrong_sid
        let stored = stub.sessions.lock().unwrap();
        let found = stored
            .iter()
            .find(|s| s.id == sid)
            .expect("sid must still exist");
        assert_eq!(
            found.title, "Overridden Title",
            "title must reflect the update"
        );
        let untouched = stored
            .iter()
            .find(|s| s.id == other_sid)
            .expect("other_sid must still exist");
        assert_eq!(untouched.title, "Test Session");
    }

    #[tokio::test]
    async fn update_session_patch_null_clears_nullable_field() {
        let stub = Arc::new(Stub::default());
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mut row = sess(sid, pid);
        row.share_url = Some("https://share.local/s".into());
        stub.create_session(row).await.unwrap();

        let body = serde_json::to_vec(&serde_json::json!({"share_url": null})).unwrap();
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(format!("/api/v1/sessions/{sid}"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app_arc(Arc::clone(&stub)).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let stored = stub.sessions.lock().unwrap();
        let found = stored.iter().find(|s| s.id == sid).unwrap();
        assert_eq!(found.share_url, None);
    }

    #[tokio::test]
    async fn update_session_patch_omitted_keeps_nullable_field() {
        let stub = Arc::new(Stub::default());
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mut row = sess(sid, pid);
        row.share_url = Some("https://share.local/s".into());
        stub.create_session(row).await.unwrap();

        let body = serde_json::to_vec(&serde_json::json!({"title": "Only title"})).unwrap();
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(format!("/api/v1/sessions/{sid}"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app_arc(Arc::clone(&stub)).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let stored = stub.sessions.lock().unwrap();
        let found = stored.iter().find(|s| s.id == sid).unwrap();
        assert_eq!(found.title, "Only title");
        assert_eq!(found.share_url.as_deref(), Some("https://share.local/s"));
    }

    #[tokio::test]
    async fn update_session_patch_summary_replaces_only_summary_fields() {
        let stub = Arc::new(Stub::default());
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mut row = sess(sid, pid);
        row.summary_additions = Some(1);
        row.summary_deletions = Some(2);
        row.summary_files = Some(3);
        row.summary_diffs = Some(serde_json::json!({"before": true}));
        row.revert = Some(serde_json::json!({"token": "keep-me"}));
        row.permission = Some(serde_json::json!({"mode": "read"}));
        stub.create_session(row).await.unwrap();

        let body = serde_json::to_vec(&serde_json::json!({
            "summary_additions": 10,
            "summary_deletions": 20,
            "summary_files": 30,
            "summary_diffs": {"after": true}
        }))
        .unwrap();
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(format!("/api/v1/sessions/{sid}"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app_arc(Arc::clone(&stub)).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let stored = stub.sessions.lock().unwrap();
        let found = stored.iter().find(|s| s.id == sid).unwrap();
        assert_eq!(found.summary_additions, Some(10));
        assert_eq!(found.summary_deletions, Some(20));
        assert_eq!(found.summary_files, Some(30));
        assert_eq!(
            found.summary_diffs,
            Some(serde_json::json!({"after": true}))
        );
        assert_eq!(found.revert, Some(serde_json::json!({"token": "keep-me"})));
        assert_eq!(found.permission, Some(serde_json::json!({"mode": "read"})));
    }

    #[tokio::test]
    async fn update_session_patch_revert_replaces_independently_of_summary() {
        let stub = Arc::new(Stub::default());
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mut row = sess(sid, pid);
        row.summary_additions = Some(7);
        row.summary_diffs = Some(serde_json::json!({"summary": "stable"}));
        row.revert = Some(serde_json::json!({"token": "old"}));
        stub.create_session(row).await.unwrap();

        let body = serde_json::to_vec(&serde_json::json!({
            "revert": {"token": "new", "reason": "manual"}
        }))
        .unwrap();
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(format!("/api/v1/sessions/{sid}"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app_arc(Arc::clone(&stub)).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let stored = stub.sessions.lock().unwrap();
        let found = stored.iter().find(|s| s.id == sid).unwrap();
        assert_eq!(
            found.revert,
            Some(serde_json::json!({"token": "new", "reason": "manual"}))
        );
        assert_eq!(found.summary_additions, Some(7));
        assert_eq!(
            found.summary_diffs,
            Some(serde_json::json!({"summary": "stable"}))
        );
    }

    #[tokio::test]
    async fn update_session_patch_revert_null_clears_without_touching_summary() {
        let stub = Arc::new(Stub::default());
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mut row = sess(sid, pid);
        row.summary_files = Some(11);
        row.summary_diffs = Some(serde_json::json!({"summary": "keep"}));
        row.revert = Some(serde_json::json!({"token": "clear-me"}));
        stub.create_session(row).await.unwrap();

        let body = serde_json::to_vec(&serde_json::json!({"revert": null})).unwrap();
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(format!("/api/v1/sessions/{sid}"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app_arc(Arc::clone(&stub)).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let stored = stub.sessions.lock().unwrap();
        let found = stored.iter().find(|s| s.id == sid).unwrap();
        assert_eq!(found.revert, None);
        assert_eq!(found.summary_files, Some(11));
        assert_eq!(
            found.summary_diffs,
            Some(serde_json::json!({"summary": "keep"}))
        );
    }

    #[tokio::test]
    async fn update_session_patch_summary_diffs_null_clears_and_omitted_revert_keeps() {
        let stub = Arc::new(Stub::default());
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mut row = sess(sid, pid);
        row.summary_diffs = Some(serde_json::json!({"to": "clear"}));
        row.revert = Some(serde_json::json!({"token": "must-stay"}));
        stub.create_session(row).await.unwrap();

        let body = serde_json::to_vec(&serde_json::json!({"summary_diffs": null})).unwrap();
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(format!("/api/v1/sessions/{sid}"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app_arc(Arc::clone(&stub)).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let stored = stub.sessions.lock().unwrap();
        let found = stored.iter().find(|s| s.id == sid).unwrap();
        assert_eq!(found.summary_diffs, None);
        assert_eq!(
            found.revert,
            Some(serde_json::json!({"token": "must-stay"}))
        );
    }

    // ── Task 6.4: GET /api/v1/projects/:pid/sessions → list ──────────────────

    #[tokio::test]
    async fn list_sessions_empty() {
        let pid = ProjectId::new();
        let resp = app(Stub::default())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{pid}/sessions"))
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

    // ── Task 6.4 triangulation: list returns only sessions for given project ──

    #[tokio::test]
    async fn list_sessions_returns_project_sessions() {
        let stub = Stub::default();
        let pid = ProjectId::new();
        let other_pid = ProjectId::new();
        let sid1 = SessionId::new();
        let sid2 = SessionId::new();
        stub.create_session(sess(sid1, pid)).await.unwrap();
        stub.create_session(sess(sid2, other_pid)).await.unwrap();

        let resp = app(stub)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{pid}/sessions"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let rows: Vec<SessionDto> = serde_json::from_slice(&body).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, sid1);
        assert_eq!(rows[0].project_id, pid);
    }

    // ── Task 6.5: GET /api/v1/sessions/:sid/messages → empty list ────────────

    #[tokio::test]
    async fn list_messages_empty() {
        let sid = SessionId::new();
        let resp = app(Stub::default())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/sessions/{sid}/messages"))
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

    // ── Task 6.5 triangulation: list_messages returns seeded messages ─────────

    #[tokio::test]
    async fn list_messages_returns_rows() {
        let stub = Stub::default();
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mid = MessageId::new();
        stub.create_session(sess(sid, pid)).await.unwrap();

        let mwp = msg_with_parts(mid, sid);
        stub.append_message(mwp.info.clone(), mwp.parts.clone())
            .await
            .unwrap();

        let resp = app(stub)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/sessions/{sid}/messages"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let rows: Vec<MessageWithParts> = serde_json::from_slice(&body).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].info.id, mid);
        assert_eq!(rows[0].parts.len(), 1);
        assert_eq!(rows[0].parts[0].data["text"], "hello");
    }

    // ── Task 6.5 triangulation: POST /api/v1/sessions/:sid/messages → 201 ────

    #[tokio::test]
    async fn append_message_returns_201() {
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mid = MessageId::new();

        let stub = Stub::default();
        stub.create_session(sess(sid, pid)).await.unwrap();

        let mwp = msg_with_parts(mid, sid);
        let body = serde_json::to_vec(&mwp).unwrap();
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/sessions/{sid}/messages"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app(stub).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // ── Triangulation: message session_id overridden from path ───────────────

    #[tokio::test]
    async fn append_message_overrides_session_id_from_path() {
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let other_sid = SessionId::new();
        let mid = MessageId::new();

        let stub = Arc::new(Stub::default());
        stub.create_session(sess(sid, pid)).await.unwrap();
        stub.create_session(sess(other_sid, pid)).await.unwrap();

        // send with wrong session_id in body — path sid should win
        let mwp = msg_with_parts(mid, other_sid);
        let body = serde_json::to_vec(&mwp).unwrap();
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/sessions/{sid}/messages"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app_arc(Arc::clone(&stub)).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        // verify message was stored under sid (path), not other_sid
        let msgs = stub.messages.lock().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0].info.session_id, sid,
            "session_id must be overridden from path"
        );
        assert_ne!(msgs[0].info.session_id, other_sid);
        // verify parts also carry the overridden session_id
        assert!(
            msgs[0].parts.iter().all(|p| p.session_id == sid),
            "parts must carry overridden session_id"
        );
    }

    #[tokio::test]
    async fn prompt_route_returns_accepted_with_handle_metadata() {
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let stub = Stub::default();
        stub.create_session(sess(sid, pid)).await.unwrap();
        let session = Arc::new(StubSession::new(StubSessionMode::Success));

        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/sessions/{sid}/prompt"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "text": "hello from route",
                    "model": "stub/model",
                    "plan_mode": true
                }))
                .unwrap(),
            ))
            .unwrap();

        let resp = app_with_session(stub, session).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["session_id"], sid.to_string());
        assert_eq!(json["resolved_model"], "stub/model");
        assert!(json["assistant_message_id"].is_string());
    }

    #[tokio::test]
    async fn prompt_route_maps_busy_error_to_conflict() {
        let sid = SessionId::new();
        let session = Arc::new(StubSession::new(StubSessionMode::Busy));
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/sessions/{sid}/prompt"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({"text": "retry"})).unwrap(),
            ))
            .unwrap();

        let resp = app_with_session(Stub::default(), session)
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn cancel_route_returns_accepted_and_records_session_id() {
        let sid = SessionId::new();
        let session = Arc::new(StubSession::new(StubSessionMode::Success));
        let cancels = Arc::clone(&session.cancels);
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/sessions/{sid}/cancel"))
            .body(Body::empty())
            .unwrap();

        let resp = app_with_session(Stub::default(), session)
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        let calls = cancels.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], sid);
    }

    #[tokio::test]
    async fn cancel_route_maps_no_active_run_to_conflict() {
        let sid = SessionId::new();
        let session = Arc::new(StubSession::new(StubSessionMode::NoActiveRun));
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/sessions/{sid}/cancel"))
            .body(Body::empty())
            .unwrap();

        let resp = app_with_session(Stub::default(), session)
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }
}
