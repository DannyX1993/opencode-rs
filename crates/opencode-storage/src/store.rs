//! [`Storage`] trait and real [`StorageImpl`] backed by SQLite.

use async_trait::async_trait;
use opencode_core::{
    dto::{
        AccountRow, MessageRow, MessageWithParts, PartRow, PermissionRow, ProjectRow, SessionRow,
        TodoRow,
    },
    error::StorageError,
    id::{ProjectId, SessionId},
};
use sqlx::SqlitePool;

use crate::{
    event_store::SyncEventStore,
    repo::{account, message, permission, project, session, todo},
};

/// Unified storage facade used by session and server layers.
///
/// All methods are `async` and return typed domain errors.
/// Concrete implementations live behind `Arc<dyn Storage>`.
#[async_trait]
pub trait Storage: Send + Sync {
    // ── Projects ─────────────────────────────────────────────────────────────
    /// Insert or update a project row.
    async fn upsert_project(&self, row: ProjectRow) -> Result<(), StorageError>;
    /// Fetch a project by id.
    async fn get_project(&self, id: ProjectId) -> Result<Option<ProjectRow>, StorageError>;
    /// List all projects.
    async fn list_projects(&self) -> Result<Vec<ProjectRow>, StorageError>;

    // ── Sessions ─────────────────────────────────────────────────────────────
    /// Create a new session row.
    async fn create_session(&self, row: SessionRow) -> Result<(), StorageError>;
    /// Fetch a session by id.
    async fn get_session(&self, id: SessionId) -> Result<Option<SessionRow>, StorageError>;
    /// List sessions for a project, ordered by `time_created` ascending.
    async fn list_sessions(&self, project_id: ProjectId) -> Result<Vec<SessionRow>, StorageError>;
    /// Update mutable session fields.
    async fn update_session(&self, row: SessionRow) -> Result<(), StorageError>;

    // ── Messages ─────────────────────────────────────────────────────────────
    /// Append a message and its initial parts to a session.
    async fn append_message(
        &self,
        msg: MessageRow,
        parts: Vec<PartRow>,
    ) -> Result<(), StorageError>;
    /// Return all messages for a session, ordered by creation time.
    async fn list_history(&self, session_id: SessionId) -> Result<Vec<MessageRow>, StorageError>;
    /// Return all messages for a session, each bundled with its parts.
    ///
    /// This is the honest message-history contract: parts are always attached.
    async fn list_history_with_parts(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<MessageWithParts>, StorageError>;

    // ── Todos ─────────────────────────────────────────────────────────────────
    /// Replace all todos for a session atomically.
    async fn save_todos(
        &self,
        session_id: SessionId,
        rows: Vec<TodoRow>,
    ) -> Result<(), StorageError>;
    /// List todos for a session in position order.
    async fn list_todos(&self, session_id: SessionId) -> Result<Vec<TodoRow>, StorageError>;

    // ── Permissions ──────────────────────────────────────────────────────────
    /// Fetch the permission blob for a project.
    async fn get_permission(
        &self,
        project_id: ProjectId,
    ) -> Result<Option<PermissionRow>, StorageError>;
    /// Persist the permission blob for a project.
    async fn set_permission(&self, row: PermissionRow) -> Result<(), StorageError>;

    // ── Accounts ─────────────────────────────────────────────────────────────
    /// Insert or update an account.
    async fn upsert_account(&self, row: AccountRow) -> Result<(), StorageError>;
    /// List all accounts.
    async fn list_accounts(&self) -> Result<Vec<AccountRow>, StorageError>;

    // ── Events ───────────────────────────────────────────────────────────────
    /// Append a raw sync event; returns the assigned sequence number.
    async fn append_event(
        &self,
        aggregate_id: &str,
        event_type: &str,
        data: serde_json::Value,
    ) -> Result<i64, StorageError>;
}

/// SQLite-backed implementation of [`Storage`].
///
/// Delegates every method to the corresponding `repo::*` function.
/// `events` is kept as a field so it shares the same pool/connection.
pub struct StorageImpl {
    pool: SqlitePool,
    events: SyncEventStore,
}

impl StorageImpl {
    /// Construct a new `StorageImpl` using an already-opened pool.
    pub fn new(pool: SqlitePool) -> Self {
        let events = SyncEventStore::new(pool.clone());
        Self { pool, events }
    }
}

#[async_trait]
impl Storage for StorageImpl {
    async fn upsert_project(&self, row: ProjectRow) -> Result<(), StorageError> {
        project::upsert(&self.pool, &row).await
    }
    async fn get_project(&self, id: ProjectId) -> Result<Option<ProjectRow>, StorageError> {
        project::get(&self.pool, id).await
    }
    async fn list_projects(&self) -> Result<Vec<ProjectRow>, StorageError> {
        project::list(&self.pool).await
    }

    async fn create_session(&self, row: SessionRow) -> Result<(), StorageError> {
        session::create(&self.pool, &row).await
    }
    async fn get_session(&self, id: SessionId) -> Result<Option<SessionRow>, StorageError> {
        session::get(&self.pool, id).await
    }
    async fn list_sessions(&self, project_id: ProjectId) -> Result<Vec<SessionRow>, StorageError> {
        session::list(&self.pool, project_id).await
    }
    async fn update_session(&self, row: SessionRow) -> Result<(), StorageError> {
        session::update(&self.pool, &row).await
    }

    async fn append_message(
        &self,
        msg: MessageRow,
        parts: Vec<PartRow>,
    ) -> Result<(), StorageError> {
        message::append(&self.pool, &msg).await?;
        for part in &parts {
            message::append_part(&self.pool, part).await?;
        }
        Ok(())
    }
    async fn list_history(&self, session_id: SessionId) -> Result<Vec<MessageRow>, StorageError> {
        message::list(&self.pool, session_id).await
    }
    async fn list_history_with_parts(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<MessageWithParts>, StorageError> {
        message::list_with_parts(&self.pool, session_id).await
    }

    async fn save_todos(
        &self,
        session_id: SessionId,
        rows: Vec<TodoRow>,
    ) -> Result<(), StorageError> {
        todo::save(&self.pool, session_id, &rows).await
    }
    async fn list_todos(&self, session_id: SessionId) -> Result<Vec<TodoRow>, StorageError> {
        todo::list(&self.pool, session_id).await
    }

    async fn get_permission(
        &self,
        project_id: ProjectId,
    ) -> Result<Option<PermissionRow>, StorageError> {
        permission::get(&self.pool, project_id).await
    }
    async fn set_permission(&self, row: PermissionRow) -> Result<(), StorageError> {
        permission::set(&self.pool, &row).await
    }

    async fn upsert_account(&self, row: AccountRow) -> Result<(), StorageError> {
        account::upsert(&self.pool, &row).await
    }
    async fn list_accounts(&self) -> Result<Vec<AccountRow>, StorageError> {
        account::list(&self.pool).await
    }

    async fn append_event(
        &self,
        aggregate_id: &str,
        event_type: &str,
        data: serde_json::Value,
    ) -> Result<i64, StorageError> {
        self.events.append(aggregate_id, event_type, data).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::connect;
    use opencode_core::id::{AccountId, MessageId, PartId, ProjectId, SessionId};
    use serde_json::json;
    use tempfile::NamedTempFile;

    async fn make_storage() -> (StorageImpl, NamedTempFile) {
        let f = NamedTempFile::new().unwrap();
        let pool = connect(f.path()).await.unwrap();
        (StorageImpl::new(pool), f)
    }

    fn proj(pid: ProjectId) -> ProjectRow {
        ProjectRow {
            id: pid,
            worktree: "/tmp".into(),
            vcs: None,
            name: None,
            icon_url: None,
            icon_color: None,
            time_created: 0,
            time_updated: 0,
            time_initialized: None,
            sandboxes: json!([]),
            commands: None,
        }
    }

    fn sess(sid: SessionId, pid: ProjectId) -> SessionRow {
        SessionRow {
            id: sid,
            project_id: pid,
            workspace_id: None,
            parent_id: None,
            slug: "s".into(),
            directory: "/tmp".into(),
            title: "t".into(),
            version: "1".into(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            time_created: 0,
            time_updated: 0,
            time_compacting: None,
            time_archived: None,
        }
    }

    fn msg(mid: MessageId, sid: SessionId) -> MessageRow {
        MessageRow {
            id: mid,
            session_id: sid,
            time_created: 0,
            time_updated: 0,
            data: json!({}),
        }
    }

    fn perm(pid: ProjectId) -> PermissionRow {
        PermissionRow {
            project_id: pid,
            time_created: 0,
            time_updated: 0,
            data: json!({}),
        }
    }

    fn acct(aid: AccountId) -> AccountRow {
        AccountRow {
            id: aid,
            email: "a@b.com".into(),
            url: "https://x.com".into(),
            access_token: "tok".into(),
            refresh_token: "ref".into(),
            token_expiry: None,
            time_created: 0,
            time_updated: 0,
        }
    }

    // ── Task 1.1 / 1.2: project upsert + list (RED → GREEN) ──────────────────
    #[tokio::test]
    async fn storage_impl_delegates_project_upsert_and_list() {
        let (s, _f) = make_storage().await;
        let pid = ProjectId::new();
        s.upsert_project(proj(pid)).await.unwrap();

        let got = s.get_project(pid).await.unwrap().unwrap();
        assert_eq!(got.id, pid);

        let all = s.list_projects().await.unwrap();
        assert_eq!(all.len(), 1);
    }

    // ── Task 1.3 / 1.4: session CRUD (RED → GREEN) ───────────────────────────
    #[tokio::test]
    async fn storage_impl_delegates_session_crud() {
        let (s, _f) = make_storage().await;
        let pid = ProjectId::new();
        let sid = SessionId::new();

        s.upsert_project(proj(pid)).await.unwrap();
        s.create_session(sess(sid, pid)).await.unwrap();

        let got = s.get_session(sid).await.unwrap().unwrap();
        assert_eq!(got.id, sid);
        assert_eq!(got.project_id, pid);

        let all = s.list_sessions(pid).await.unwrap();
        assert_eq!(all.len(), 1);

        let mut updated = sess(sid, pid);
        updated.title = "updated".into();
        updated.time_updated = 9_999;
        s.update_session(updated).await.unwrap();
        let got2 = s.get_session(sid).await.unwrap().unwrap();
        assert_eq!(got2.title, "updated");
    }

    // ── Task 1.5 / 1.6: message append + list_history (RED → GREEN) ──────────
    #[tokio::test]
    async fn storage_impl_delegates_message_append_and_list() {
        let (s, _f) = make_storage().await;
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mid = MessageId::new();

        s.upsert_project(proj(pid)).await.unwrap();
        s.create_session(sess(sid, pid)).await.unwrap();

        let part = PartRow {
            id: PartId::new(),
            message_id: mid,
            session_id: sid,
            time_created: 1,
            time_updated: 1,
            data: json!({"type": "text", "text": "hello"}),
        };
        s.append_message(msg(mid, sid), vec![part]).await.unwrap();

        let history = s.list_history(sid).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, mid);
    }

    // ── Task 3.4 / 3.5: list_history_with_parts (RED → GREEN) ────────────────
    #[tokio::test]
    async fn storage_impl_list_history_with_parts_bundles_parts() {
        let (s, _f) = make_storage().await;
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mid = MessageId::new();

        s.upsert_project(proj(pid)).await.unwrap();
        s.create_session(sess(sid, pid)).await.unwrap();

        let part = PartRow {
            id: PartId::new(),
            message_id: mid,
            session_id: sid,
            time_created: 1,
            time_updated: 1,
            data: json!({"type": "text", "text": "world"}),
        };
        s.append_message(msg(mid, sid), vec![part]).await.unwrap();

        let history = s.list_history_with_parts(sid).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].info.id, mid);
        assert_eq!(history[0].parts.len(), 1);
        assert_eq!(history[0].parts[0].data["text"], "world");
    }

    #[tokio::test]
    async fn storage_impl_list_history_with_parts_empty_parts_message() {
        let (s, _f) = make_storage().await;
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mid = MessageId::new();

        s.upsert_project(proj(pid)).await.unwrap();
        s.create_session(sess(sid, pid)).await.unwrap();

        // append a message with zero parts
        s.append_message(msg(mid, sid), vec![]).await.unwrap();

        let history = s.list_history_with_parts(sid).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].info.id, mid);
        assert!(history[0].parts.is_empty());
    }

    // ── Task 1.7 / 1.8: todos (RED → GREEN) ──────────────────────────────────
    #[tokio::test]
    async fn storage_impl_delegates_todos() {
        let (s, _f) = make_storage().await;
        let pid = ProjectId::new();
        let sid = SessionId::new();

        s.upsert_project(proj(pid)).await.unwrap();
        s.create_session(sess(sid, pid)).await.unwrap();

        let rows = vec![
            TodoRow {
                session_id: sid,
                content: "task 1".into(),
                status: "pending".into(),
                priority: "high".into(),
                position: 0,
                time_created: 1,
                time_updated: 1,
            },
            TodoRow {
                session_id: sid,
                content: "task 2".into(),
                status: "done".into(),
                priority: "low".into(),
                position: 1,
                time_created: 1,
                time_updated: 1,
            },
        ];
        s.save_todos(sid, rows).await.unwrap();

        let todos = s.list_todos(sid).await.unwrap();
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].content, "task 1");
        assert_eq!(todos[1].content, "task 2");
    }

    // ── Task 2.7: StorageImpl::append_event delegates to SyncEventStore ────────
    #[tokio::test]
    async fn storage_impl_append_event_delegates_to_event_store() {
        let (s, _f) = make_storage().await;
        let seq1 = s.append_event("agg-x", "Created", json!({})).await.unwrap();
        let seq2 = s.append_event("agg-x", "Updated", json!({})).await.unwrap();
        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);
    }

    // ── Task 1.9 / 1.10: permissions + accounts (RED → GREEN) ────────────────
    #[tokio::test]
    async fn storage_impl_delegates_permissions_and_accounts() {
        let (s, _f) = make_storage().await;
        let pid = ProjectId::new();
        let aid = AccountId::new();

        s.upsert_project(proj(pid)).await.unwrap();

        // permissions
        assert!(s.get_permission(pid).await.unwrap().is_none());
        s.set_permission(perm(pid)).await.unwrap();
        let got = s.get_permission(pid).await.unwrap();
        assert!(got.is_some());

        // accounts
        s.upsert_account(acct(aid)).await.unwrap();
        let all = s.list_accounts().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, aid);
    }
}
