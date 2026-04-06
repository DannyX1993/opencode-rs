//! Project repository — CRUD operations against the `project` table.

use opencode_core::{dto::ProjectRow, error::StorageError, id::ProjectId};
use sqlx::{Row, SqlitePool};

fn map(e: sqlx::Error) -> StorageError {
    StorageError::Db(e.to_string())
}

fn from_row(r: sqlx::sqlite::SqliteRow) -> Result<ProjectRow, StorageError> {
    let id_str: String = r.try_get("id").map_err(map)?;
    let sandboxes_str: String = r.try_get("sandboxes").map_err(map)?;
    let commands_str: Option<String> = r.try_get("commands").map_err(map)?;
    Ok(ProjectRow {
        id: id_str
            .parse::<ProjectId>()
            .map_err(|e| StorageError::Db(e.to_string()))?,
        worktree: r.try_get("worktree").map_err(map)?,
        vcs: r.try_get("vcs").map_err(map)?,
        name: r.try_get("name").map_err(map)?,
        icon_url: r.try_get("icon_url").map_err(map)?,
        icon_color: r.try_get("icon_color").map_err(map)?,
        time_created: r.try_get("time_created").map_err(map)?,
        time_updated: r.try_get("time_updated").map_err(map)?,
        time_initialized: r.try_get("time_initialized").map_err(map)?,
        sandboxes: serde_json::from_str(&sandboxes_str).unwrap_or(serde_json::json!([])),
        commands: commands_str
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
    })
}

/// Insert or replace a project row.
pub async fn upsert(pool: &SqlitePool, row: &ProjectRow) -> Result<(), StorageError> {
    sqlx::query(
        r"
        INSERT INTO project
            (id, worktree, vcs, name, icon_url, icon_color,
             time_created, time_updated, time_initialized, sandboxes, commands)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            worktree         = excluded.worktree,
            vcs              = excluded.vcs,
            name             = excluded.name,
            icon_url         = excluded.icon_url,
            icon_color       = excluded.icon_color,
            time_updated     = excluded.time_updated,
            time_initialized = excluded.time_initialized,
            sandboxes        = excluded.sandboxes,
            commands         = excluded.commands
        ",
    )
    .bind(row.id.to_string())
    .bind(&row.worktree)
    .bind(&row.vcs)
    .bind(&row.name)
    .bind(&row.icon_url)
    .bind(&row.icon_color)
    .bind(row.time_created)
    .bind(row.time_updated)
    .bind(row.time_initialized)
    .bind(row.sandboxes.to_string())
    .bind(row.commands.as_ref().map(|v| v.to_string()))
    .execute(pool)
    .await
    .map_err(map)?;
    Ok(())
}

/// Fetch a project by ID; returns `None` if not found.
pub async fn get(pool: &SqlitePool, id: ProjectId) -> Result<Option<ProjectRow>, StorageError> {
    sqlx::query("SELECT * FROM project WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await
        .map_err(map)?
        .map(from_row)
        .transpose()
}

/// List all projects.
pub async fn list(pool: &SqlitePool) -> Result<Vec<ProjectRow>, StorageError> {
    sqlx::query("SELECT * FROM project ORDER BY time_created")
        .fetch_all(pool)
        .await
        .map_err(map)?
        .into_iter()
        .map(from_row)
        .collect()
}

/// Delete a project by ID.
pub async fn delete(pool: &SqlitePool, id: ProjectId) -> Result<(), StorageError> {
    sqlx::query("DELETE FROM project WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(map)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::connect;
    use tempfile::NamedTempFile;

    async fn make_pool() -> (SqlitePool, NamedTempFile) {
        let f = NamedTempFile::new().unwrap();
        let p = connect(f.path()).await.unwrap();
        (p, f)
    }

    fn row() -> ProjectRow {
        ProjectRow {
            id: ProjectId::new(),
            worktree: "/tmp/test".into(),
            vcs: Some("git".into()),
            name: Some("Test".into()),
            icon_url: None,
            icon_color: None,
            time_created: 1_000,
            time_updated: 2_000,
            time_initialized: None,
            sandboxes: serde_json::json!([]),
            commands: None,
        }
    }

    #[tokio::test]
    async fn crud_round_trip() {
        let (pool, _f) = make_pool().await;
        let r = row();
        upsert(&pool, &r).await.unwrap();

        let got = get(&pool, r.id).await.unwrap().unwrap();
        assert_eq!(got.id, r.id);
        assert_eq!(got.worktree, r.worktree);

        let all = list(&pool).await.unwrap();
        assert_eq!(all.len(), 1);

        delete(&pool, r.id).await.unwrap();
        assert!(get(&pool, r.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn upsert_updates_name() {
        let (pool, _f) = make_pool().await;
        let mut r = row();
        upsert(&pool, &r).await.unwrap();
        r.name = Some("Updated".into());
        r.time_updated = 3_000;
        upsert(&pool, &r).await.unwrap();
        let got = get(&pool, r.id).await.unwrap().unwrap();
        assert_eq!(got.name.as_deref(), Some("Updated"));
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let (pool, _f) = make_pool().await;
        let result = get(&pool, ProjectId::new()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn list_empty_returns_empty() {
        let (pool, _f) = make_pool().await;
        let all = list(&pool).await.unwrap();
        assert!(all.is_empty());
    }
}
