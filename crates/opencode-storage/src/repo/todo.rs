//! Todo repository — session-scoped list, replaced atomically.

use opencode_core::{dto::TodoRow, error::StorageError, id::SessionId};
use sqlx::{Row, SqlitePool};

fn map(e: sqlx::Error) -> StorageError {
    StorageError::Db(e.to_string())
}

fn from_row(r: sqlx::sqlite::SqliteRow) -> Result<TodoRow, StorageError> {
    let sid: String = r.try_get("session_id").map_err(map)?;
    Ok(TodoRow {
        session_id: sid
            .parse()
            .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?,
        content: r.try_get("content").map_err(map)?,
        status: r.try_get("status").map_err(map)?,
        priority: r.try_get("priority").map_err(map)?,
        position: r.try_get("position").map_err(map)?,
        time_created: r.try_get("time_created").map_err(map)?,
        time_updated: r.try_get("time_updated").map_err(map)?,
    })
}

/// Replace all todos for `session_id` atomically.
pub async fn save(
    pool: &SqlitePool,
    session_id: SessionId,
    rows: &[TodoRow],
) -> Result<(), StorageError> {
    let mut tx = pool.begin().await.map_err(map)?;
    sqlx::query("DELETE FROM todo WHERE session_id = ?")
        .bind(session_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(map)?;
    for row in rows {
        sqlx::query(
            "INSERT INTO todo (session_id, content, status, priority, position, time_created, time_updated) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(row.session_id.to_string())
        .bind(&row.content)
        .bind(&row.status)
        .bind(&row.priority)
        .bind(row.position)
        .bind(row.time_created)
        .bind(row.time_updated)
        .execute(&mut *tx)
        .await
        .map_err(map)?;
    }
    tx.commit().await.map_err(map)?;
    Ok(())
}

/// List todos for `session_id` ordered by `position`.
pub async fn list(pool: &SqlitePool, session_id: SessionId) -> Result<Vec<TodoRow>, StorageError> {
    sqlx::query("SELECT * FROM todo WHERE session_id = ? ORDER BY position")
        .bind(session_id.to_string())
        .fetch_all(pool)
        .await
        .map_err(map)?
        .into_iter()
        .map(from_row)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        pool::connect,
        repo::{project, session},
    };
    use opencode_core::{
        dto::{ProjectRow, SessionRow},
        id::{ProjectId, SessionId},
    };
    use tempfile::NamedTempFile;

    async fn setup() -> (SqlitePool, NamedTempFile, SessionId) {
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
        let sid = SessionId::new();
        session::create(
            &pool,
            &SessionRow {
                id: sid,
                project_id: pid,
                parent_id: None,
                slug: "s".into(),
                directory: "/tmp".into(),
                title: "T".into(),
                version: "1".into(),
                share_url: None,
                permission: None,
                time_created: 1,
                time_updated: 1,
                time_compacting: None,
                time_archived: None,
            },
        )
        .await
        .unwrap();
        (pool, f, sid)
    }

    fn todo(sid: SessionId, pos: i64, status: &str) -> TodoRow {
        TodoRow {
            session_id: sid,
            content: format!("task {pos}"),
            status: status.into(),
            priority: "medium".into(),
            position: pos,
            time_created: 1,
            time_updated: 1,
        }
    }

    #[tokio::test]
    async fn save_and_list() {
        let (pool, _f, sid) = setup().await;
        let todos = vec![todo(sid, 0, "pending"), todo(sid, 1, "done")];
        save(&pool, sid, &todos).await.unwrap();
        let result = list(&pool, sid).await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].position, 0);
        assert_eq!(result[1].position, 1);
    }

    #[tokio::test]
    async fn save_replaces_existing() {
        let (pool, _f, sid) = setup().await;
        save(
            &pool,
            sid,
            &[todo(sid, 0, "pending"), todo(sid, 1, "pending")],
        )
        .await
        .unwrap();
        save(&pool, sid, &[todo(sid, 0, "done")]).await.unwrap();
        let result = list(&pool, sid).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, "done");
    }

    #[tokio::test]
    async fn list_empty_returns_empty() {
        let (pool, _f, sid) = setup().await;
        let result = list(&pool, sid).await.unwrap();
        assert!(result.is_empty());
    }
}
