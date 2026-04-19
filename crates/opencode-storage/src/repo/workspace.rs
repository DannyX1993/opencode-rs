//! Workspace repository.

use opencode_core::{
    dto::WorkspaceRow,
    error::StorageError,
    id::{ProjectId, WorkspaceId},
};
use sqlx::{Row, SqlitePool};

fn map(e: sqlx::Error) -> StorageError {
    StorageError::Db(e.to_string())
}

pub(crate) fn encode_extra(
    extra: &Option<serde_json::Value>,
) -> Result<Option<String>, StorageError> {
    extra
        .as_ref()
        .map(|value| serde_json::to_string(value).map_err(|e| StorageError::Serde(e.to_string())))
        .transpose()
}

pub(crate) fn decode_extra(
    extra: Option<String>,
) -> Result<Option<serde_json::Value>, StorageError> {
    extra
        .as_deref()
        .map(|value| serde_json::from_str(value).map_err(|e| StorageError::Serde(e.to_string())))
        .transpose()
}

fn from_row(r: sqlx::sqlite::SqliteRow) -> Result<WorkspaceRow, StorageError> {
    let id: String = r.try_get("id").map_err(map)?;
    let project_id: String = r.try_get("project_id").map_err(map)?;
    let extra: Option<String> = r.try_get("extra").map_err(map)?;
    Ok(WorkspaceRow {
        id: id
            .parse::<WorkspaceId>()
            .map_err(|e| StorageError::Db(e.to_string()))?,
        r#type: r.try_get("type").map_err(map)?,
        branch: r.try_get("branch").map_err(map)?,
        name: r.try_get("name").map_err(map)?,
        directory: r.try_get("directory").map_err(map)?,
        extra: decode_extra(extra)?,
        project_id: project_id
            .parse::<ProjectId>()
            .map_err(|e| StorageError::Db(e.to_string()))?,
    })
}

/// Insert or update a workspace row.
pub async fn upsert(pool: &SqlitePool, row: &WorkspaceRow) -> Result<(), StorageError> {
    sqlx::query(
        r"
        INSERT INTO workspace (id, type, branch, name, directory, extra, project_id)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            type       = excluded.type,
            branch     = excluded.branch,
            name       = excluded.name,
            directory  = excluded.directory,
            extra      = excluded.extra,
            project_id = excluded.project_id
        ",
    )
    .bind(row.id.to_string())
    .bind(&row.r#type)
    .bind(&row.branch)
    .bind(&row.name)
    .bind(&row.directory)
    .bind(encode_extra(&row.extra)?)
    .bind(row.project_id.to_string())
    .execute(pool)
    .await
    .map_err(map)?;
    Ok(())
}

/// Fetch a workspace by ID; returns `None` if not found.
pub async fn get(pool: &SqlitePool, id: WorkspaceId) -> Result<Option<WorkspaceRow>, StorageError> {
    sqlx::query("SELECT * FROM workspace WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await
        .map_err(map)?
        .map(from_row)
        .transpose()
}

/// List all workspaces.
pub async fn list(pool: &SqlitePool) -> Result<Vec<WorkspaceRow>, StorageError> {
    sqlx::query("SELECT * FROM workspace ORDER BY id")
        .fetch_all(pool)
        .await
        .map_err(map)?
        .into_iter()
        .map(from_row)
        .collect()
}

/// Delete a workspace by ID.
pub async fn delete(pool: &SqlitePool, id: WorkspaceId) -> Result<(), StorageError> {
    sqlx::query("DELETE FROM workspace WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(map)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{decode_extra, delete, encode_extra, get, list, upsert};
    use crate::{pool::connect, repo::project};
    use opencode_core::{
        dto::{ProjectRow, WorkspaceRow},
        error::StorageError,
        id::{ProjectId, WorkspaceId},
    };
    use tempfile::NamedTempFile;

    async fn setup() -> (sqlx::SqlitePool, NamedTempFile, ProjectId) {
        let f = NamedTempFile::new().expect("temp sqlite file");
        let pool = connect(f.path()).await.expect("open sqlite pool");
        let project_id = ProjectId::new();
        project::upsert(
            &pool,
            &ProjectRow {
                id: project_id,
                worktree: "/tmp".into(),
                vcs: Some("git".into()),
                name: Some("control-plane".into()),
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
        .expect("insert project row");
        (pool, f, project_id)
    }

    fn workspace_row(project_id: ProjectId) -> WorkspaceRow {
        WorkspaceRow {
            id: WorkspaceId::new(),
            r#type: "remote".into(),
            branch: Some("main".into()),
            name: Some("alpha".into()),
            directory: Some("/tmp/alpha".into()),
            extra: Some(
                serde_json::json!({"instance": "control-plane-a", "base_url": "https://a.example"}),
            ),
            project_id,
        }
    }

    #[tokio::test]
    async fn crud_round_trip() {
        let (pool, _tmp, project_id) = setup().await;
        let row = workspace_row(project_id);

        upsert(&pool, &row).await.expect("insert workspace");

        let fetched = get(&pool, row.id)
            .await
            .expect("read workspace")
            .expect("workspace exists");
        assert_eq!(fetched.id, row.id);
        assert_eq!(fetched.r#type, "remote");
        assert_eq!(fetched.extra, row.extra);

        let all = list(&pool).await.expect("list workspaces");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, row.id);

        delete(&pool, row.id).await.expect("delete workspace");
        assert!(
            get(&pool, row.id)
                .await
                .expect("read after delete")
                .is_none()
        );
    }

    #[tokio::test]
    async fn upsert_updates_mutable_fields() {
        let (pool, _tmp, project_id) = setup().await;
        let mut row = workspace_row(project_id);
        upsert(&pool, &row).await.expect("insert workspace");

        row.name = Some("alpha-updated".into());
        row.branch = Some("release".into());
        row.extra = Some(serde_json::json!({"instance": "control-plane-b"}));
        upsert(&pool, &row).await.expect("upsert workspace update");

        let fetched = get(&pool, row.id)
            .await
            .expect("read workspace")
            .expect("workspace exists");
        assert_eq!(fetched.name.as_deref(), Some("alpha-updated"));
        assert_eq!(fetched.branch.as_deref(), Some("release"));
        assert_eq!(fetched.extra, row.extra);
    }

    #[test]
    fn encode_decode_extra_handles_none_and_json() {
        assert_eq!(encode_extra(&None).expect("encode none"), None);
        assert_eq!(decode_extra(None).expect("decode none"), None);

        let payload = Some(serde_json::json!({"k": "v", "n": 7}));
        let encoded = encode_extra(&payload).expect("encode payload");
        let decoded = decode_extra(encoded).expect("decode payload");
        assert_eq!(decoded, payload);
    }

    #[test]
    fn decode_extra_rejects_invalid_json() {
        let err = decode_extra(Some("not-json".into())).expect_err("invalid json should fail");
        assert!(matches!(err, StorageError::Serde(_)));
    }
}
