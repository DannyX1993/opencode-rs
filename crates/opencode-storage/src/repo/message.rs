//! Message and Part repositories.

use opencode_core::{
    dto::{MessageRow, MessageWithParts, PartRow},
    error::StorageError,
    id::{MessageId, SessionId},
};
use sqlx::{Row, SqlitePool};

fn map(e: sqlx::Error) -> StorageError {
    StorageError::Db(e.to_string())
}

// ── Message ────────────────────────────────────────────────────────────────────

fn msg_from_row(r: sqlx::sqlite::SqliteRow) -> Result<MessageRow, StorageError> {
    let id_str: String = r.try_get("id").map_err(map)?;
    let sid_str: String = r.try_get("session_id").map_err(map)?;
    let data_str: String = r.try_get("data").map_err(map)?;
    Ok(MessageRow {
        id: id_str
            .parse()
            .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?,
        session_id: sid_str
            .parse()
            .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?,
        time_created: r.try_get("time_created").map_err(map)?,
        time_updated: r.try_get("time_updated").map_err(map)?,
        data: serde_json::from_str(&data_str).map_err(|e| StorageError::Db(e.to_string()))?,
    })
}

/// Append a message row.
pub async fn append(pool: &SqlitePool, msg: &MessageRow) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(msg.id.to_string())
    .bind(msg.session_id.to_string())
    .bind(msg.time_created)
    .bind(msg.time_updated)
    .bind(msg.data.to_string())
    .execute(pool)
    .await
    .map_err(map)?;
    Ok(())
}

/// List all messages for a session, ordered by `(time_created, id)`.
pub async fn list(
    pool: &SqlitePool,
    session_id: SessionId,
) -> Result<Vec<MessageRow>, StorageError> {
    sqlx::query("SELECT * FROM message WHERE session_id = ? ORDER BY time_created, id")
        .bind(session_id.to_string())
        .fetch_all(pool)
        .await
        .map_err(map)?
        .into_iter()
        .map(msg_from_row)
        .collect()
}

// ── Part ───────────────────────────────────────────────────────────────────────

fn part_from_row(r: sqlx::sqlite::SqliteRow) -> Result<PartRow, StorageError> {
    let id_str: String = r.try_get("id").map_err(map)?;
    let mid_str: String = r.try_get("message_id").map_err(map)?;
    let sid_str: String = r.try_get("session_id").map_err(map)?;
    let data_str: String = r.try_get("data").map_err(map)?;
    Ok(PartRow {
        id: id_str
            .parse()
            .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?,
        message_id: mid_str
            .parse()
            .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?,
        session_id: sid_str
            .parse()
            .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?,
        time_created: r.try_get("time_created").map_err(map)?,
        time_updated: r.try_get("time_updated").map_err(map)?,
        data: serde_json::from_str(&data_str).map_err(|e| StorageError::Db(e.to_string()))?,
    })
}

/// Append a part row.
pub async fn append_part(pool: &SqlitePool, part: &PartRow) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(part.id.to_string())
    .bind(part.message_id.to_string())
    .bind(part.session_id.to_string())
    .bind(part.time_created)
    .bind(part.time_updated)
    .bind(part.data.to_string())
    .execute(pool)
    .await
    .map_err(map)?;
    Ok(())
}

/// List all parts for a message, ordered by `id`.
pub async fn list_parts(
    pool: &SqlitePool,
    message_id: MessageId,
) -> Result<Vec<PartRow>, StorageError> {
    sqlx::query("SELECT * FROM part WHERE message_id = ? ORDER BY id")
        .bind(message_id.to_string())
        .fetch_all(pool)
        .await
        .map_err(map)?
        .into_iter()
        .map(part_from_row)
        .collect()
}

/// List all messages for a session with their parts, ordered by `(time_created, id)`.
///
/// This is the honest contract for history queries: each [`MessageWithParts`] bundles
/// the message row together with its associated parts so callers never need a
/// second round-trip to fetch parts separately.
pub async fn list_with_parts(
    pool: &SqlitePool,
    session_id: SessionId,
) -> Result<Vec<MessageWithParts>, StorageError> {
    let msgs = list(pool, session_id).await?;
    let mut result = Vec::with_capacity(msgs.len());
    for msg in msgs {
        let parts = list_parts(pool, msg.id).await?;
        result.push(MessageWithParts { info: msg, parts });
    }
    Ok(result)
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
        id::{PartId, ProjectId},
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
                workspace_id: None,
                parent_id: None,
                slug: "s".into(),
                directory: "/tmp".into(),
                title: "T".into(),
                version: "1".into(),
                share_url: None,
                summary_additions: None,
                summary_deletions: None,
                summary_files: None,
                summary_diffs: None,
                revert: None,
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

    #[tokio::test]
    async fn messages_ordered() {
        let (pool, _f, sid) = setup().await;
        for i in 0..3_i64 {
            append(
                &pool,
                &MessageRow {
                    id: MessageId::new(),
                    session_id: sid,
                    time_created: 1_000 + i,
                    time_updated: 1_000 + i,
                    data: serde_json::json!({"role": "user", "idx": i}),
                },
            )
            .await
            .unwrap();
        }
        let msgs = list(&pool, sid).await.unwrap();
        assert_eq!(msgs.len(), 3);
        assert!(msgs[0].time_created <= msgs[1].time_created);
    }

    #[tokio::test]
    async fn parts_round_trip() {
        let (pool, _f, sid) = setup().await;
        let mid = MessageId::new();
        append(
            &pool,
            &MessageRow {
                id: mid,
                session_id: sid,
                time_created: 1,
                time_updated: 1,
                data: serde_json::json!({}),
            },
        )
        .await
        .unwrap();
        append_part(
            &pool,
            &PartRow {
                id: PartId::new(),
                message_id: mid,
                session_id: sid,
                time_created: 1,
                time_updated: 1,
                data: serde_json::json!({"type": "text", "text": "hello"}),
            },
        )
        .await
        .unwrap();
        let parts = list_parts(&pool, mid).await.unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].data["text"], "hello");
    }

    #[tokio::test]
    async fn list_empty_returns_empty() {
        let (pool, _f, sid) = setup().await;
        let msgs = list(&pool, sid).await.unwrap();
        assert!(msgs.is_empty());
    }

    // ── Task 3.2 / 3.3: list_with_parts (RED → GREEN) ────────────────────────

    #[tokio::test]
    async fn list_with_parts_returns_message_with_its_parts() {
        let (pool, _f, sid) = setup().await;
        let mid = MessageId::new();

        // insert message + 2 parts
        append(
            &pool,
            &MessageRow {
                id: mid,
                session_id: sid,
                time_created: 10,
                time_updated: 10,
                data: serde_json::json!({"role": "user"}),
            },
        )
        .await
        .unwrap();
        for i in 0..2_i64 {
            append_part(
                &pool,
                &PartRow {
                    id: opencode_core::id::PartId::new(),
                    message_id: mid,
                    session_id: sid,
                    time_created: 10 + i,
                    time_updated: 10 + i,
                    data: serde_json::json!({"type": "text", "idx": i}),
                },
            )
            .await
            .unwrap();
        }

        let history = list_with_parts(&pool, sid).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].info.id, mid);
        assert_eq!(history[0].parts.len(), 2);
    }

    #[tokio::test]
    async fn list_with_parts_empty_parts_when_no_parts() {
        let (pool, _f, sid) = setup().await;
        let mid = MessageId::new();

        append(
            &pool,
            &MessageRow {
                id: mid,
                session_id: sid,
                time_created: 5,
                time_updated: 5,
                data: serde_json::json!({}),
            },
        )
        .await
        .unwrap();

        let history = list_with_parts(&pool, sid).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].info.id, mid);
        // message exists but has zero parts
        assert!(history[0].parts.is_empty());
    }

    #[tokio::test]
    async fn list_with_parts_empty_session_returns_empty() {
        let (pool, _f, sid) = setup().await;
        let history = list_with_parts(&pool, sid).await.unwrap();
        assert!(history.is_empty());
    }
}
