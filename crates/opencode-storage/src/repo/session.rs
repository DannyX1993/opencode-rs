//! Session repository.

use opencode_core::{
    dto::SessionRow,
    error::StorageError,
    id::{ProjectId, SessionId},
};
use sqlx::{Row, SqlitePool};

fn map(e: sqlx::Error) -> StorageError {
    StorageError::Db(e.to_string())
}

fn from_row(r: sqlx::sqlite::SqliteRow) -> Result<SessionRow, StorageError> {
    let id_str: String = r.try_get("id").map_err(map)?;
    let pid_str: String = r.try_get("project_id").map_err(map)?;
    let parent_str: Option<String> = r.try_get("parent_id").map_err(map)?;
    Ok(SessionRow {
        id: id_str
            .parse()
            .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?,
        project_id: pid_str
            .parse()
            .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?,
        parent_id: parent_str
            .as_deref()
            .map(|s| {
                s.parse()
                    .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))
            })
            .transpose()?,
        slug: r.try_get("slug").map_err(map)?,
        directory: r.try_get("directory").map_err(map)?,
        title: r.try_get("title").map_err(map)?,
        version: r.try_get("version").map_err(map)?,
        share_url: r.try_get("share_url").map_err(map)?,
        permission: r.try_get("permission").map_err(map)?,
        time_created: r.try_get("time_created").map_err(map)?,
        time_updated: r.try_get("time_updated").map_err(map)?,
        time_compacting: r.try_get("time_compacting").map_err(map)?,
        time_archived: r.try_get("time_archived").map_err(map)?,
    })
}

/// Insert a new session row.
pub async fn create(pool: &SqlitePool, row: &SessionRow) -> Result<(), StorageError> {
    sqlx::query(
        r"
        INSERT INTO session
            (id, project_id, parent_id, slug, directory, title, version,
             share_url, permission, time_created, time_updated, time_compacting, time_archived)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ",
    )
    .bind(row.id.to_string())
    .bind(row.project_id.to_string())
    .bind(row.parent_id.map(|i| i.to_string()))
    .bind(&row.slug)
    .bind(&row.directory)
    .bind(&row.title)
    .bind(&row.version)
    .bind(&row.share_url)
    .bind(&row.permission)
    .bind(row.time_created)
    .bind(row.time_updated)
    .bind(row.time_compacting)
    .bind(row.time_archived)
    .execute(pool)
    .await
    .map_err(map)?;
    Ok(())
}

/// Fetch a session by ID.
pub async fn get(pool: &SqlitePool, id: SessionId) -> Result<Option<SessionRow>, StorageError> {
    sqlx::query("SELECT * FROM session WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await
        .map_err(map)?
        .map(from_row)
        .transpose()
}

/// List sessions for a project, ordered by `time_created` ascending.
pub async fn list(
    pool: &SqlitePool,
    project_id: ProjectId,
) -> Result<Vec<SessionRow>, StorageError> {
    sqlx::query("SELECT * FROM session WHERE project_id = ? ORDER BY time_created")
        .bind(project_id.to_string())
        .fetch_all(pool)
        .await
        .map_err(map)?
        .into_iter()
        .map(from_row)
        .collect()
}

/// Update mutable session fields.
pub async fn update(pool: &SqlitePool, row: &SessionRow) -> Result<(), StorageError> {
    sqlx::query(
        r"
        UPDATE session SET
            title           = ?,
            time_updated    = ?,
            share_url       = ?,
            permission      = ?,
            time_compacting = ?,
            time_archived   = ?
        WHERE id = ?
        ",
    )
    .bind(&row.title)
    .bind(row.time_updated)
    .bind(&row.share_url)
    .bind(&row.permission)
    .bind(row.time_compacting)
    .bind(row.time_archived)
    .bind(row.id.to_string())
    .execute(pool)
    .await
    .map_err(map)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pool::connect, repo::project};
    use opencode_core::dto::ProjectRow;
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

    fn row(pid: ProjectId) -> SessionRow {
        SessionRow {
            id: SessionId::new(),
            project_id: pid,
            parent_id: None,
            slug: "test-slug".into(),
            directory: "/tmp".into(),
            title: "Test Session".into(),
            version: "1".into(),
            share_url: None,
            permission: None,
            time_created: 1_000,
            time_updated: 2_000,
            time_compacting: None,
            time_archived: None,
        }
    }

    #[tokio::test]
    async fn crud_round_trip() {
        let (pool, _f, pid) = setup().await;
        let r = row(pid);
        create(&pool, &r).await.unwrap();

        let got = get(&pool, r.id).await.unwrap().unwrap();
        assert_eq!(got.id, r.id);
        assert_eq!(got.title, "Test Session");

        let all = list(&pool, pid).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, r.id);
    }

    #[tokio::test]
    async fn list_ordered_by_time_created() {
        let (pool, _f, pid) = setup().await;
        for i in 0..3_i64 {
            let mut r = row(pid);
            r.id = SessionId::new();
            r.slug = format!("slug-{i}");
            r.time_created = 1_000 + i;
            create(&pool, &r).await.unwrap();
        }
        let all = list(&pool, pid).await.unwrap();
        assert_eq!(all.len(), 3);
        assert!(all[0].time_created <= all[1].time_created);
        assert!(all[1].time_created <= all[2].time_created);
    }

    #[tokio::test]
    async fn update_title() {
        let (pool, _f, pid) = setup().await;
        let mut r = row(pid);
        create(&pool, &r).await.unwrap();
        r.title = "Updated Title".into();
        r.time_updated = 9_999;
        update(&pool, &r).await.unwrap();
        let got = get(&pool, r.id).await.unwrap().unwrap();
        assert_eq!(got.title, "Updated Title");
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let (pool, _f, _pid) = setup().await;
        let result = get(&pool, SessionId::new()).await.unwrap();
        assert!(result.is_none());
    }
}
