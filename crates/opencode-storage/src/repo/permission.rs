//! Permission repository — one row per project.

use opencode_core::{dto::PermissionRow, error::StorageError, id::ProjectId};
use sqlx::{Row, SqlitePool};

fn map(e: sqlx::Error) -> StorageError {
    StorageError::Db(e.to_string())
}

fn from_row(r: sqlx::sqlite::SqliteRow) -> Result<PermissionRow, StorageError> {
    let pid: String = r.try_get("project_id").map_err(map)?;
    let data_str: String = r.try_get("data").map_err(map)?;
    Ok(PermissionRow {
        project_id: pid
            .parse()
            .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?,
        time_created: r.try_get("time_created").map_err(map)?,
        time_updated: r.try_get("time_updated").map_err(map)?,
        data: serde_json::from_str(&data_str).map_err(|e| StorageError::Db(e.to_string()))?,
    })
}

/// Fetch the permission row for `project_id`.
pub async fn get(
    pool: &SqlitePool,
    project_id: ProjectId,
) -> Result<Option<PermissionRow>, StorageError> {
    sqlx::query("SELECT * FROM permission WHERE project_id = ?")
        .bind(project_id.to_string())
        .fetch_optional(pool)
        .await
        .map_err(map)?
        .map(from_row)
        .transpose()
}

/// Insert or update the permission row for a project.
pub async fn set(pool: &SqlitePool, row: &PermissionRow) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO permission (project_id, time_created, time_updated, data) VALUES (?, ?, ?, ?)
         ON CONFLICT(project_id) DO UPDATE SET time_updated = excluded.time_updated, data = excluded.data",
    )
    .bind(row.project_id.to_string())
    .bind(row.time_created)
    .bind(row.time_updated)
    .bind(row.data.to_string())
    .execute(pool)
    .await
    .map_err(map)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pool::connect, repo::project};
    use opencode_core::{dto::ProjectRow, id::ProjectId};
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
    async fn set_and_get() {
        let (pool, _f, pid) = setup().await;
        let row = PermissionRow {
            project_id: pid,
            time_created: 1,
            time_updated: 1,
            data: serde_json::json!({"allow": ["read"]}),
        };
        set(&pool, &row).await.unwrap();
        let result = get(&pool, pid).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().data["allow"][0], "read");
    }

    #[tokio::test]
    async fn set_upserts_on_conflict() {
        let (pool, _f, pid) = setup().await;
        set(
            &pool,
            &PermissionRow {
                project_id: pid,
                time_created: 1,
                time_updated: 1,
                data: serde_json::json!({"allow": ["read"]}),
            },
        )
        .await
        .unwrap();
        set(
            &pool,
            &PermissionRow {
                project_id: pid,
                time_created: 1,
                time_updated: 2,
                data: serde_json::json!({"allow": ["read", "write"]}),
            },
        )
        .await
        .unwrap();
        let result = get(&pool, pid).await.unwrap().unwrap();
        assert_eq!(result.data["allow"].as_array().unwrap().len(), 2);
        assert_eq!(result.time_updated, 2);
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let (pool, _f, _pid) = setup().await;
        let missing = ProjectId::new();
        let result = get(&pool, missing).await.unwrap();
        assert!(result.is_none());
    }
}
