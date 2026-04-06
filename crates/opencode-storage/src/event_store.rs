//! Append-only sync-event store skeleton.
//!
//! Full implementation lands in Phase 1 (P1-T08).

use opencode_core::error::StorageError;

/// Append-only event store backed by the `event` and `event_sequence` tables.
///
/// Phase 0 stub — methods return errors until Phase 1 implementation.
pub struct SyncEventStore;

impl SyncEventStore {
    /// Append an event for `aggregate_id`; returns the assigned sequence number.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Db`] until Phase 1 provides the real implementation.
    pub async fn append(
        &self,
        _aggregate_id: &str,
        _event_type: &str,
        _data: serde_json::Value,
    ) -> Result<i64, StorageError> {
        Err(StorageError::Db(
            "SyncEventStore not yet implemented".into(),
        ))
    }

    /// List all events for `aggregate_id` with sequence > `since`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Db`] until Phase 1 provides the real implementation.
    pub async fn list_since(
        &self,
        _aggregate_id: &str,
        _since: i64,
    ) -> Result<Vec<StoredEvent>, StorageError> {
        Err(StorageError::Db(
            "SyncEventStore not yet implemented".into(),
        ))
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
    use serde_json::json;

    #[tokio::test]
    async fn append_returns_not_implemented_error() {
        let store = SyncEventStore;
        let err = store
            .append("agg-1", "SessionCreated", json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }

    #[tokio::test]
    async fn list_since_returns_not_implemented_error() {
        let store = SyncEventStore;
        let err = store.list_since("agg-1", 0).await.unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
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
}
