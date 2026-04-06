//! [`Storage`] trait and blanket implementation.

use async_trait::async_trait;
use opencode_core::{
    dto::{AccountRow, MessageRow, PartRow, PermissionRow, ProjectRow, SessionRow, TodoRow},
    error::StorageError,
    id::{ProjectId, SessionId},
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
    /// Return all messages (with parts) for a session, ordered by creation time.
    async fn list_history(&self, session_id: SessionId) -> Result<Vec<MessageRow>, StorageError>;

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

/// Placeholder implementation that will be fully fleshed out in Phase 1.
///
/// Methods return `Err(StorageError::Db("not yet implemented".into()))` so
/// that the workspace compiles in Phase 0 without requiring a live database.
pub struct StorageImpl;

#[async_trait]
impl Storage for StorageImpl {
    async fn upsert_project(&self, _row: ProjectRow) -> Result<(), StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn get_project(&self, _id: ProjectId) -> Result<Option<ProjectRow>, StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn list_projects(&self) -> Result<Vec<ProjectRow>, StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn create_session(&self, _row: SessionRow) -> Result<(), StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn get_session(&self, _id: SessionId) -> Result<Option<SessionRow>, StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn list_sessions(&self, _project_id: ProjectId) -> Result<Vec<SessionRow>, StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn update_session(&self, _row: SessionRow) -> Result<(), StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn append_message(
        &self,
        _msg: MessageRow,
        _parts: Vec<PartRow>,
    ) -> Result<(), StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn list_history(&self, _session_id: SessionId) -> Result<Vec<MessageRow>, StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn save_todos(
        &self,
        _session_id: SessionId,
        _rows: Vec<TodoRow>,
    ) -> Result<(), StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn list_todos(&self, _session_id: SessionId) -> Result<Vec<TodoRow>, StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn get_permission(
        &self,
        _project_id: ProjectId,
    ) -> Result<Option<PermissionRow>, StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn set_permission(&self, _row: PermissionRow) -> Result<(), StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn upsert_account(&self, _row: AccountRow) -> Result<(), StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn list_accounts(&self) -> Result<Vec<AccountRow>, StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
    async fn append_event(
        &self,
        _aggregate_id: &str,
        _event_type: &str,
        _data: serde_json::Value,
    ) -> Result<i64, StorageError> {
        Err(StorageError::Db("not yet implemented".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencode_core::id::{AccountId, MessageId, ProjectId, SessionId};
    use serde_json::json;

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
            parent_id: None,
            slug: "s".into(),
            directory: "/tmp".into(),
            title: "t".into(),
            version: "1".into(),
            share_url: None,
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

    #[tokio::test]
    async fn storage_impl_all_methods_return_error() {
        let s = StorageImpl;
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mid = MessageId::new();
        let aid = AccountId::new();

        assert!(s.upsert_project(proj(pid)).await.is_err());
        assert!(s.get_project(pid).await.is_err());
        assert!(s.list_projects().await.is_err());
        assert!(s.create_session(sess(sid, pid)).await.is_err());
        assert!(s.get_session(sid).await.is_err());
        assert!(s.list_sessions(pid).await.is_err());
        assert!(s.update_session(sess(sid, pid)).await.is_err());
        assert!(s.append_message(msg(mid, sid), vec![]).await.is_err());
        assert!(s.list_history(sid).await.is_err());
        assert!(s.save_todos(sid, vec![]).await.is_err());
        assert!(s.list_todos(sid).await.is_err());
        assert!(s.get_permission(pid).await.is_err());
        assert!(s.set_permission(perm(pid)).await.is_err());
        assert!(s.upsert_account(acct(aid)).await.is_err());
        assert!(s.list_accounts().await.is_err());
        assert!(s.append_event("agg", "T", json!({})).await.is_err());
    }
}
