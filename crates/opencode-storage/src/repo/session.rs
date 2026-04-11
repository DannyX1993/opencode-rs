//! Session repository.

use opencode_core::{
    dto::SessionRow,
    error::StorageError,
    id::{ProjectId, SessionId, WorkspaceId},
};
use sqlx::{Row, SqlitePool};

fn map(e: sqlx::Error) -> StorageError {
    StorageError::Db(e.to_string())
}

fn parse_opt_json(v: Option<String>) -> Result<Option<serde_json::Value>, StorageError> {
    v.as_deref()
        .map(|s| serde_json::from_str(s).map_err(|e| StorageError::Db(e.to_string())))
        .transpose()
}

fn from_row(r: sqlx::sqlite::SqliteRow) -> Result<SessionRow, StorageError> {
    let id_str: String = r.try_get("id").map_err(map)?;
    let pid_str: String = r.try_get("project_id").map_err(map)?;
    let workspace_str: Option<String> = r.try_get("workspace_id").map_err(map)?;
    let parent_str: Option<String> = r.try_get("parent_id").map_err(map)?;
    let summary_diffs: Option<String> = r.try_get("summary_diffs").map_err(map)?;
    let revert: Option<String> = r.try_get("revert").map_err(map)?;
    let permission: Option<String> = r.try_get("permission").map_err(map)?;
    Ok(SessionRow {
        id: id_str
            .parse()
            .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?,
        project_id: pid_str
            .parse()
            .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?,
        workspace_id: workspace_str
            .as_deref()
            .map(|s| {
                s.parse::<WorkspaceId>()
                    .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))
            })
            .transpose()?,
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
        summary_additions: r.try_get("summary_additions").map_err(map)?,
        summary_deletions: r.try_get("summary_deletions").map_err(map)?,
        summary_files: r.try_get("summary_files").map_err(map)?,
        summary_diffs: parse_opt_json(summary_diffs)?,
        revert: parse_opt_json(revert)?,
        permission: parse_opt_json(permission)?,
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
            (id, project_id, workspace_id, parent_id, slug, directory, title, version,
             share_url, summary_additions, summary_deletions, summary_files, summary_diffs,
             revert, permission, time_created, time_updated, time_compacting, time_archived)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ",
    )
    .bind(row.id.to_string())
    .bind(row.project_id.to_string())
    .bind(row.workspace_id.map(|i| i.to_string()))
    .bind(row.parent_id.map(|i| i.to_string()))
    .bind(&row.slug)
    .bind(&row.directory)
    .bind(&row.title)
    .bind(&row.version)
    .bind(&row.share_url)
    .bind(row.summary_additions)
    .bind(row.summary_deletions)
    .bind(row.summary_files)
    .bind(row.summary_diffs.as_ref().map(|v| v.to_string()))
    .bind(row.revert.as_ref().map(|v| v.to_string()))
    .bind(row.permission.as_ref().map(|v| v.to_string()))
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
            workspace_id     = ?,
            title           = ?,
            time_updated    = ?,
            share_url       = ?,
            summary_additions = ?,
            summary_deletions = ?,
            summary_files     = ?,
            summary_diffs     = ?,
            revert            = ?,
            permission      = ?,
            time_compacting = ?,
            time_archived   = ?
        WHERE id = ?
        ",
    )
    .bind(row.workspace_id.map(|i| i.to_string()))
    .bind(&row.title)
    .bind(row.time_updated)
    .bind(&row.share_url)
    .bind(row.summary_additions)
    .bind(row.summary_deletions)
    .bind(row.summary_files)
    .bind(row.summary_diffs.as_ref().map(|v| v.to_string()))
    .bind(row.revert.as_ref().map(|v| v.to_string()))
    .bind(row.permission.as_ref().map(|v| v.to_string()))
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
            workspace_id: None,
            parent_id: None,
            slug: "test-slug".into(),
            directory: "/tmp".into(),
            title: "Test Session".into(),
            version: "1".into(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
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

    #[tokio::test]
    async fn create_and_read_preserve_expanded_metadata() {
        let (pool, _f, pid) = setup().await;
        let mut r = row(pid);
        r.workspace_id = Some(WorkspaceId::new());
        r.share_url = Some("https://share.local/session".into());
        r.summary_additions = Some(12);
        r.summary_deletions = Some(4);
        r.summary_files = Some(3);
        r.summary_diffs = Some(serde_json::json!({"files": ["src/main.rs"]}));
        r.revert = Some(serde_json::json!({"message_id": "m-1", "part_id": "p-1"}));
        r.permission = Some(serde_json::json!({"mode": "write"}));
        r.time_compacting = Some(8_888);
        r.time_archived = Some(9_999);

        create(&pool, &r).await.unwrap();

        let got = get(&pool, r.id).await.unwrap().unwrap();
        assert_eq!(got.workspace_id, r.workspace_id);
        assert_eq!(got.share_url, r.share_url);
        assert_eq!(got.summary_additions, Some(12));
        assert_eq!(got.summary_deletions, Some(4));
        assert_eq!(got.summary_files, Some(3));
        assert_eq!(got.summary_diffs, r.summary_diffs);
        assert_eq!(got.revert, r.revert);
        assert_eq!(got.permission, r.permission);
        assert_eq!(got.time_compacting, Some(8_888));
        assert_eq!(got.time_archived, Some(9_999));

        let listed = list(&pool, pid).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].summary_diffs, r.summary_diffs);
        assert_eq!(listed[0].revert, r.revert);
    }
}
