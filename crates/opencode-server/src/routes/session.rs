//! `/api/v1/projects/:pid/sessions` and `/api/v1/sessions/:sid` route handlers.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use opencode_core::{
    dto::{MessageWithParts, PartRow, SessionRow},
    id::{ProjectId, SessionId},
};

use crate::{error::HttpError, state::AppState};

/// `POST /api/v1/projects/:pid/sessions` — create a new session.
pub async fn create(
    State(s): State<AppState>,
    Path(pid): Path<ProjectId>,
    Json(mut row): Json<SessionRow>,
) -> impl IntoResponse {
    row.project_id = pid;
    match s.storage.create_session(row).await {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => HttpError::from(e).into_response(),
    }
}

/// `GET /api/v1/projects/:pid/sessions` — list sessions for a project.
pub async fn list(State(s): State<AppState>, Path(pid): Path<ProjectId>) -> impl IntoResponse {
    match s.storage.list_sessions(pid).await {
        Ok(rows) => (StatusCode::OK, Json(rows)).into_response(),
        Err(e) => HttpError::from(e).into_response(),
    }
}

/// `GET /api/v1/sessions/:sid` — fetch one session by id.
pub async fn get(State(s): State<AppState>, Path(sid): Path<SessionId>) -> impl IntoResponse {
    match s.storage.get_session(sid).await {
        Ok(Some(row)) => (StatusCode::OK, Json(row)).into_response(),
        Ok(None) => HttpError::not_found(format!("session {sid} not found")).into_response(),
        Err(e) => HttpError::from(e).into_response(),
    }
}

/// `PATCH /api/v1/sessions/:sid` — update a session.
pub async fn update(
    State(s): State<AppState>,
    Path(sid): Path<SessionId>,
    Json(mut row): Json<SessionRow>,
) -> impl IntoResponse {
    row.id = sid;
    match s.storage.update_session(row).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => HttpError::from(e).into_response(),
    }
}

/// `GET /api/v1/sessions/:sid/messages` — list messages with parts for a session.
pub async fn list_messages(
    State(s): State<AppState>,
    Path(sid): Path<SessionId>,
) -> impl IntoResponse {
    match s.storage.list_history_with_parts(sid).await {
        Ok(rows) => (StatusCode::OK, Json(rows)).into_response(),
        Err(e) => HttpError::from(e).into_response(),
    }
}

/// `POST /api/v1/sessions/:sid/messages` — append a message (with parts) to a session.
pub async fn append_message(
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

    struct StubSession;

    #[async_trait]
    impl Session for StubSession {
        async fn prompt(&self, _: SessionPrompt) -> Result<SessionHandle, SessionError> {
            Err(SessionError::NotFound("stub".into()))
        }
        async fn cancel(&self, _: SessionId) -> Result<(), SessionError> {
            Err(SessionError::NotFound("stub".into()))
        }
    }

    fn app(stub: Stub) -> Router {
        let state = crate::state::AppState {
            config: Arc::new(Config::default()),
            bus: Arc::new(BroadcastBus::new(64)),
            storage: Arc::new(stub),
            session: Arc::new(StubSession),
        };
        crate::router::build(state)
    }

    /// Build router while keeping a handle to the stub for post-request state inspection.
    fn app_arc(stub: Arc<Stub>) -> Router {
        let state = crate::state::AppState {
            config: Arc::new(Config::default()),
            bus: Arc::new(BroadcastBus::new(64)),
            storage: stub,
            session: Arc::new(StubSession),
        };
        crate::router::build(state)
    }

    fn sess(sid: SessionId, pid: ProjectId) -> SessionRow {
        SessionRow {
            id: sid,
            project_id: pid,
            parent_id: None,
            slug: "test-slug".into(),
            directory: "/tmp/s".into(),
            title: "Test Session".into(),
            version: "1".into(),
            share_url: None,
            permission: None,
            time_created: 1000,
            time_updated: 1001,
            time_compacting: None,
            time_archived: None,
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
        let body = serde_json::to_vec(&sess(sid, pid)).unwrap();
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
        let other_pid = ProjectId::new();
        let sid = SessionId::new();
        // send with wrong project_id in body
        let body = serde_json::to_vec(&sess(sid, other_pid)).unwrap();
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
        assert_eq!(
            stored[0].project_id, pid,
            "project_id must be overridden from path"
        );
        assert_ne!(stored[0].project_id, other_pid);
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
        let row: SessionRow = serde_json::from_slice(&body).unwrap();
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

        let mut updated = sess(sid, pid);
        updated.title = "Updated Title".into();
        let body = serde_json::to_vec(&updated).unwrap();
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
    async fn update_session_overrides_id_from_path() {
        let stub = Arc::new(Stub::default());
        let pid = ProjectId::new();
        let sid = SessionId::new();
        stub.create_session(sess(sid, pid)).await.unwrap();

        // body has a different id — path wins
        let wrong_sid = SessionId::new();
        let mut s = sess(wrong_sid, pid);
        s.title = "Overridden Title".into();
        let body = serde_json::to_vec(&s).unwrap();
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
        assert!(
            !stored.iter().any(|s| s.id == wrong_sid),
            "wrong_sid must not be stored"
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
        let rows: Vec<SessionRow> = serde_json::from_slice(&body).unwrap();
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
}
