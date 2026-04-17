//! [`Storage`] trait and real [`StorageImpl`] backed by SQLite.

use async_trait::async_trait;
use opencode_core::{
    dto::{
        AccountRow, AccountStateRow, ControlAccountRow, MessageRow, MessageWithParts, PartRow,
        PermissionRow, ProjectFoundationRow, ProjectRow, SessionRow, TodoRow,
    },
    error::StorageError,
    id::{AccountId, ProjectId, SessionId},
};
use sqlx::SqlitePool;

use crate::{
    event_store::SyncEventStore,
    repo::{account, message, permission, project, project_repository_state, session, todo},
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
    /// Fetch additive project-foundation companion state.
    ///
    /// Callers may lazily backfill by probing and writing foundation state when
    /// this returns `None` for an existing project.
    async fn get_project_foundation(
        &self,
        _project_id: ProjectId,
    ) -> Result<Option<ProjectFoundationRow>, StorageError> {
        Ok(None)
    }
    /// Upsert additive project-foundation companion state.
    ///
    /// This keeps `project` CRUD additive: repository/worktree metadata evolves
    /// in a companion table without mutating the legacy project schema.
    async fn upsert_project_foundation(
        &self,
        _row: ProjectFoundationRow,
    ) -> Result<(), StorageError> {
        Ok(())
    }

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
    /// Append a single part row to an existing message.
    async fn append_part(&self, part: PartRow) -> Result<(), StorageError>;
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
    /// Fetch one account by id.
    async fn get_account(&self, id: AccountId) -> Result<Option<AccountRow>, StorageError>;
    /// Remove an account.
    async fn remove_account(&self, id: AccountId) -> Result<(), StorageError>;
    /// Update persisted account tokens.
    async fn update_account_tokens(
        &self,
        id: AccountId,
        access_token: String,
        refresh_token: String,
        token_expiry: Option<i64>,
        time_updated: i64,
    ) -> Result<(), StorageError>;
    /// Read singleton active account state.
    async fn get_account_state(&self) -> Result<Option<AccountStateRow>, StorageError>;
    /// Persist singleton active account state.
    async fn set_account_state(&self, row: AccountStateRow) -> Result<(), StorageError>;
    /// Lookup a legacy control-account row by email and url.
    async fn get_control_account(
        &self,
        email: &str,
        url: &str,
    ) -> Result<Option<ControlAccountRow>, StorageError>;
    /// Lookup the active legacy control-account row.
    async fn get_active_control_account(&self) -> Result<Option<ControlAccountRow>, StorageError>;

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
    async fn get_project_foundation(
        &self,
        project_id: ProjectId,
    ) -> Result<Option<ProjectFoundationRow>, StorageError> {
        project_repository_state::get(&self.pool, project_id).await
    }
    async fn upsert_project_foundation(
        &self,
        row: ProjectFoundationRow,
    ) -> Result<(), StorageError> {
        project_repository_state::upsert(&self.pool, &row).await
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
    async fn append_part(&self, part: PartRow) -> Result<(), StorageError> {
        message::append_part(&self.pool, &part).await
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
    async fn get_account(&self, id: AccountId) -> Result<Option<AccountRow>, StorageError> {
        account::get(&self.pool, id).await
    }
    async fn remove_account(&self, id: AccountId) -> Result<(), StorageError> {
        account::remove(&self.pool, id).await
    }
    async fn update_account_tokens(
        &self,
        id: AccountId,
        access_token: String,
        refresh_token: String,
        token_expiry: Option<i64>,
        time_updated: i64,
    ) -> Result<(), StorageError> {
        account::update_tokens(
            &self.pool,
            id,
            &access_token,
            &refresh_token,
            token_expiry,
            time_updated,
        )
        .await
    }
    async fn get_account_state(&self) -> Result<Option<AccountStateRow>, StorageError> {
        account::get_state(&self.pool).await
    }
    async fn set_account_state(&self, row: AccountStateRow) -> Result<(), StorageError> {
        account::set_state(&self.pool, &row).await
    }
    async fn get_control_account(
        &self,
        email: &str,
        url: &str,
    ) -> Result<Option<ControlAccountRow>, StorageError> {
        account::get_control_account(&self.pool, email, url).await
    }
    async fn get_active_control_account(&self) -> Result<Option<ControlAccountRow>, StorageError> {
        account::get_active_control_account(&self.pool).await
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
    use opencode_core::{
        dto::AccountStateRow,
        id::{AccountId, MessageId, PartId, ProjectId, SessionId},
        project::{RepositoryState, WorktreeState},
    };
    use serde_json::json;
    use sqlx::Executor;
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

    #[tokio::test]
    async fn storage_impl_delegates_account_lookup_token_updates_and_state() {
        let (s, _f) = make_storage().await;
        let aid = AccountId::new();

        s.upsert_account(acct(aid)).await.unwrap();

        let found = s.get_account(aid).await.unwrap().unwrap();
        assert_eq!(found.id, aid);

        s.update_account_tokens(aid, "next-token".into(), "next-refresh".into(), Some(9), 5)
            .await
            .unwrap();
        let updated = s.get_account(aid).await.unwrap().unwrap();
        assert_eq!(updated.access_token, "next-token");
        assert_eq!(updated.refresh_token, "next-refresh");

        s.set_account_state(AccountStateRow {
            id: 7,
            active_account_id: Some(aid),
            active_org_id: Some("org-live".into()),
        })
        .await
        .unwrap();
        let state = s.get_account_state().await.unwrap().unwrap();
        assert_eq!(state.id, 1);
        assert_eq!(state.active_account_id, Some(aid));
        assert_eq!(state.active_org_id.as_deref(), Some("org-live"));
    }

    #[tokio::test]
    async fn storage_impl_remove_account_clears_singleton_state() {
        let (s, _f) = make_storage().await;
        let aid = AccountId::new();

        s.upsert_account(acct(aid)).await.unwrap();
        s.set_account_state(AccountStateRow {
            id: 1,
            active_account_id: Some(aid),
            active_org_id: Some("org-stale".into()),
        })
        .await
        .unwrap();

        s.remove_account(aid).await.unwrap();

        assert!(s.get_account(aid).await.unwrap().is_none());
        let state = s.get_account_state().await.unwrap().unwrap();
        assert_eq!(state.active_account_id, None);
        assert_eq!(state.active_org_id, None);
    }

    #[tokio::test]
    async fn storage_impl_reads_legacy_control_accounts() {
        let (s, _f) = make_storage().await;

        s.pool
            .execute(
                sqlx::query(
                    "INSERT INTO control_account (email, url, access_token, refresh_token, token_expiry, active, time_created, time_updated)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?), (?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind("legacy@test.com")
                .bind("https://legacy.example.com")
                .bind("legacy-access")
                .bind("legacy-refresh")
                .bind(Option::<i64>::Some(1))
                .bind(false)
                .bind(1_i64)
                .bind(1_i64)
                .bind("active@test.com")
                .bind("https://active.example.com")
                .bind("active-access")
                .bind("active-refresh")
                .bind(Option::<i64>::Some(2))
                .bind(true)
                .bind(2_i64)
                .bind(3_i64),
            )
            .await
            .unwrap();

        let exact = s
            .get_control_account("legacy@test.com", "https://legacy.example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(exact.email, "legacy@test.com");

        let active = s.get_active_control_account().await.unwrap().unwrap();
        assert_eq!(active.email, "active@test.com");
        assert!(active.active);
    }

    #[tokio::test]
    async fn storage_impl_project_foundation_round_trips_partial_fields() {
        let (s, _f) = make_storage().await;
        let pid = ProjectId::new();
        s.upsert_project(proj(pid)).await.unwrap();

        s.upsert_project_foundation(ProjectFoundationRow {
            project_id: pid,
            canonical_worktree: Some("/tmp".into()),
            repository_root: None,
            vcs_kind: None,
            worktree_state: WorktreeState {
                branch: Some("main".into()),
                head_oid: None,
                is_dirty: Some(false),
            },
            repository_state: RepositoryState::default(),
            sync_basis: None,
            time_created: 10,
            time_updated: 11,
        })
        .await
        .unwrap();

        let got = s.get_project_foundation(pid).await.unwrap().unwrap();
        assert_eq!(got.project_id, pid);
        assert_eq!(got.canonical_worktree.as_deref(), Some("/tmp"));
        assert_eq!(got.vcs_kind, None);
        assert_eq!(got.worktree_state.branch.as_deref(), Some("main"));
        assert_eq!(got.worktree_state.head_oid, None);
    }
}
