//! Repository for additive `project_repository_state` companion table.

use opencode_core::{dto::ProjectFoundationRow, error::StorageError, id::ProjectId};
use sqlx::{Row, SqlitePool};

fn map(e: sqlx::Error) -> StorageError {
    StorageError::Db(e.to_string())
}

fn parse_json_or_default<T>(value: Option<String>) -> Result<T, StorageError>
where
    T: Default + serde::de::DeserializeOwned,
{
    // Missing JSON columns are valid for additive/backfill flows; callers get
    // the semantic default rather than a hard failure.
    match value {
        Some(raw) => serde_json::from_str(&raw).map_err(|e| StorageError::Serde(e.to_string())),
        None => Ok(T::default()),
    }
}

fn parse_json_optional<T>(value: Option<String>) -> Result<Option<T>, StorageError>
where
    T: serde::de::DeserializeOwned,
{
    match value {
        Some(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|e| StorageError::Serde(e.to_string())),
        None => Ok(None),
    }
}

fn from_row(r: sqlx::sqlite::SqliteRow) -> Result<ProjectFoundationRow, StorageError> {
    let pid: String = r.try_get("project_id").map_err(map)?;
    Ok(ProjectFoundationRow {
        project_id: pid
            .parse::<ProjectId>()
            .map_err(|e| StorageError::Db(e.to_string()))?,
        canonical_worktree: r.try_get("canonical_worktree").map_err(map)?,
        repository_root: r.try_get("repository_root").map_err(map)?,
        vcs_kind: r.try_get("vcs_kind").map_err(map)?,
        worktree_state: parse_json_or_default(r.try_get("worktree_state").map_err(map)?)?,
        repository_state: parse_json_or_default(r.try_get("repository_state").map_err(map)?)?,
        sync_basis: parse_json_optional(r.try_get("sync_basis").map_err(map)?)?,
        time_created: r.try_get("time_created").map_err(map)?,
        time_updated: r.try_get("time_updated").map_err(map)?,
    })
}

/// Insert or update project foundation state.
pub async fn upsert(pool: &SqlitePool, row: &ProjectFoundationRow) -> Result<(), StorageError> {
    sqlx::query(
        r"
        INSERT INTO project_repository_state (
            project_id,
            canonical_worktree,
            repository_root,
            vcs_kind,
            worktree_state,
            repository_state,
            sync_basis,
            time_created,
            time_updated
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(project_id) DO UPDATE SET
            canonical_worktree = excluded.canonical_worktree,
            repository_root = excluded.repository_root,
            vcs_kind = excluded.vcs_kind,
            worktree_state = excluded.worktree_state,
            repository_state = excluded.repository_state,
            sync_basis = excluded.sync_basis,
            time_updated = excluded.time_updated
        ",
    )
    .bind(row.project_id.to_string())
    .bind(&row.canonical_worktree)
    .bind(&row.repository_root)
    .bind(&row.vcs_kind)
    .bind(
        serde_json::to_string(&row.worktree_state)
            .map_err(|e| StorageError::Serde(e.to_string()))?,
    )
    .bind(
        serde_json::to_string(&row.repository_state)
            .map_err(|e| StorageError::Serde(e.to_string()))?,
    )
    .bind(
        row.sync_basis
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| StorageError::Serde(e.to_string()))?,
    )
    .bind(row.time_created)
    .bind(row.time_updated)
    .execute(pool)
    .await
    .map_err(map)?;

    Ok(())
}

/// Fetch project foundation state by project id.
pub async fn get(
    pool: &SqlitePool,
    project_id: ProjectId,
) -> Result<Option<ProjectFoundationRow>, StorageError> {
    sqlx::query("SELECT * FROM project_repository_state WHERE project_id = ?")
        .bind(project_id.to_string())
        .fetch_optional(pool)
        .await
        .map_err(map)?
        .map(from_row)
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pool::connect, repo::project};
    use opencode_core::dto::ProjectRow;
    use opencode_core::project::{RepositoryState, WorktreeState};
    use tempfile::NamedTempFile;

    async fn setup() -> (SqlitePool, NamedTempFile, ProjectId) {
        let f = NamedTempFile::new().unwrap();
        let pool = connect(f.path()).await.unwrap();
        let pid = ProjectId::new();
        project::upsert(
            &pool,
            &ProjectRow {
                id: pid,
                worktree: "/tmp".into(),
                vcs: None,
                name: None,
                icon_url: None,
                icon_color: None,
                time_created: 1,
                time_updated: 1,
                time_initialized: None,
                sandboxes: serde_json::json!([]),
                commands: None,
            },
        )
        .await
        .unwrap();
        (pool, f, pid)
    }

    #[tokio::test]
    async fn upsert_and_get_round_trip_partial_fields() {
        let (pool, _f, pid) = setup().await;

        upsert(
            &pool,
            &ProjectFoundationRow {
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
            },
        )
        .await
        .unwrap();

        let got = get(&pool, pid).await.unwrap().unwrap();
        assert_eq!(got.project_id, pid);
        assert_eq!(got.canonical_worktree.as_deref(), Some("/tmp"));
        assert_eq!(got.repository_root, None);
        assert_eq!(got.vcs_kind, None);
        assert_eq!(got.worktree_state.branch.as_deref(), Some("main"));
        assert_eq!(got.worktree_state.head_oid, None);
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let (pool, _f, _pid) = setup().await;
        let got = get(&pool, ProjectId::new()).await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn connect_upgrades_from_0001_to_project_repository_state_table() {
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}?mode=rwc", f.path().display());
        let raw_pool = SqlitePool::connect(&url).await.unwrap();
        for statement in include_str!("../../migrations/0001_initial.sql").split(';') {
            let stmt = statement.trim();
            if stmt.is_empty() {
                continue;
            }
            sqlx::query(stmt).execute(&raw_pool).await.unwrap();
        }
        raw_pool.close().await;

        let migrated_pool = connect(f.path()).await.unwrap();
        let table_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = 'project_repository_state'",
        )
        .fetch_one(&migrated_pool)
        .await
        .unwrap();
        assert_eq!(table_count, 1);
        migrated_pool.close().await;
    }
}
