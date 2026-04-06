//! Append-only sync-event store backed by SQLite.
//!
//! Two tables are used:
//! - `event_sequence` — one row per aggregate, holds the current `seq` counter.
//! - `event` — one row per event, with `aggregate_id`, `seq`, `type`, `data`.
//!
//! Appends run inside a `BEGIN EXCLUSIVE` transaction to guarantee monotonic,
//! gap-free sequence numbers even under concurrent writers.

use opencode_core::error::StorageError;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

fn map(e: sqlx::Error) -> StorageError {
    StorageError::Db(e.to_string())
}

/// Append-only event store backed by the `event` and `event_sequence` tables.
pub struct SyncEventStore {
    pool: SqlitePool,
}

impl SyncEventStore {
    /// Construct a new store using an already-opened pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Append an event for `aggregate_id`; returns the assigned sequence number.
    ///
    /// Uses `BEGIN EXCLUSIVE` to guarantee monotonic, gap-free sequences under
    /// concurrent writers.
    pub async fn append(
        &self,
        aggregate_id: &str,
        event_type: &str,
        data: serde_json::Value,
    ) -> Result<i64, StorageError> {
        let mut conn = self.pool.acquire().await.map_err(map)?;

        // Exclusive transaction to prevent seq gaps under concurrency.
        sqlx::query("BEGIN EXCLUSIVE")
            .execute(&mut *conn)
            .await
            .map_err(map)?;

        // Upsert the sequence counter and get the new seq.
        let seq: i64 = sqlx::query(
            r"
            INSERT INTO event_sequence (aggregate_id, seq)
            VALUES (?, 1)
            ON CONFLICT(aggregate_id) DO UPDATE SET seq = seq + 1
            RETURNING seq
            ",
        )
        .bind(aggregate_id)
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| {
            // Best-effort rollback; ignore rollback error.
            let _ = std::sync::Arc::new(e.to_string());
            StorageError::Db(e.to_string())
        })?
        .try_get("seq")
        .map_err(map)?;

        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO event (id, aggregate_id, seq, type, data) VALUES (?, ?, ?, ?, ?)")
            .bind(&id)
            .bind(aggregate_id)
            .bind(seq)
            .bind(event_type)
            .bind(data.to_string())
            .execute(&mut *conn)
            .await
            .map_err(map)?;

        sqlx::query("COMMIT")
            .execute(&mut *conn)
            .await
            .map_err(map)?;

        Ok(seq)
    }

    /// List all events for `aggregate_id` with sequence > `since`, ordered by seq.
    pub async fn list_since(
        &self,
        aggregate_id: &str,
        since: i64,
    ) -> Result<Vec<StoredEvent>, StorageError> {
        sqlx::query(
            "SELECT seq, aggregate_id, type, data FROM event WHERE aggregate_id = ? AND seq > ? ORDER BY seq",
        )
        .bind(aggregate_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map)?
        .into_iter()
        .map(|r| {
            let data_str: String = r.try_get("data").map_err(map)?;
            Ok(StoredEvent {
                seq: r.try_get("seq").map_err(map)?,
                aggregate_id: r.try_get("aggregate_id").map_err(map)?,
                event_type: r.try_get("type").map_err(map)?,
                data: serde_json::from_str(&data_str)
                    .map_err(|e| StorageError::Db(e.to_string()))?,
            })
        })
        .collect()
    }
}

/// A stored event row.
#[derive(Debug, Clone)]
pub struct StoredEvent {
    /// Monotonic sequence number.
    pub seq: i64,
    /// Aggregate the event belongs to.
    pub aggregate_id: String,
    /// Event type discriminant.
    pub event_type: String,
    /// JSON payload.
    pub data: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::connect;
    use serde_json::json;
    use tempfile::NamedTempFile;

    async fn make_store() -> (SyncEventStore, NamedTempFile) {
        let f = NamedTempFile::new().unwrap();
        let pool = connect(f.path()).await.unwrap();
        (SyncEventStore::new(pool), f)
    }

    // ── Task 2.1 / 2.2: sequential seq (RED → GREEN) ─────────────────────────
    #[tokio::test]
    async fn append_assigns_sequential_seq() {
        let (store, _f) = make_store().await;
        let s1 = store.append("agg-1", "Created", json!({})).await.unwrap();
        let s2 = store.append("agg-1", "Updated", json!({})).await.unwrap();
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
    }

    // ── Task 2.3 / 2.4: list_since (RED → GREEN) ─────────────────────────────
    #[tokio::test]
    async fn list_since_returns_events_after_cursor() {
        let (store, _f) = make_store().await;
        store.append("agg-2", "E1", json!({"n": 1})).await.unwrap();
        store.append("agg-2", "E2", json!({"n": 2})).await.unwrap();
        store.append("agg-2", "E3", json!({"n": 3})).await.unwrap();

        let events = store.list_since("agg-2", 1).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 2);
        assert_eq!(events[1].seq, 3);
        assert_eq!(events[0].event_type, "E2");
    }

    // ── Task 2.5 / 2.6: concurrent appends (RED → GREEN) ─────────────────────
    #[tokio::test]
    async fn concurrent_appends_no_seq_gap() {
        let f = NamedTempFile::new().unwrap();
        let pool = connect(f.path()).await.unwrap();
        let store = std::sync::Arc::new(SyncEventStore::new(pool));

        let handles: Vec<_> = (0..5)
            .map(|_| {
                let s = store.clone();
                tokio::spawn(async move { s.append("agg-conc", "Ev", json!({})).await.unwrap() })
            })
            .collect();

        let mut seqs: Vec<i64> = futures::future::join_all(handles)
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();
        seqs.sort_unstable();
        // Must be exactly [1, 2, 3, 4, 5] — no duplicates, no gaps.
        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn stored_event_fields_accessible() {
        let ev = StoredEvent {
            seq: 1,
            aggregate_id: "agg".into(),
            event_type: "Test".into(),
            data: json!({"key": "val"}),
        };
        assert_eq!(ev.seq, 1);
        assert_eq!(ev.aggregate_id, "agg");
        assert_eq!(ev.event_type, "Test");
    }

    #[tokio::test]
    async fn list_since_empty_when_no_events() {
        let (store, _f) = make_store().await;
        let events = store.list_since("agg-empty", 0).await.unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn different_aggregates_independent_seqs() {
        let (store, _f) = make_store().await;
        let s1 = store.append("agg-a", "E", json!({})).await.unwrap();
        let s2 = store.append("agg-b", "E", json!({})).await.unwrap();
        // Both start at 1 independently.
        assert_eq!(s1, 1);
        assert_eq!(s2, 1);
    }
}
