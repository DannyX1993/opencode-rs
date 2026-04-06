//! SQLite connection pool with WAL mode enabled.

use opencode_core::error::StorageError;
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::path::Path;

/// Open (or create) a SQLite database at `path` with WAL mode.
///
/// Runs all pending migrations before returning.
///
/// # Errors
///
/// Returns [`StorageError::Db`] if the pool cannot be created or migrations fail.
pub async fn connect(path: &Path) -> Result<SqlitePool, StorageError> {
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .map_err(|e| StorageError::Db(e.to_string()))?;

    // Enable WAL mode for better concurrent read/write performance.
    sqlx::query("PRAGMA journal_mode=WAL")
        .execute(&pool)
        .await
        .map_err(|e| StorageError::Db(e.to_string()))?;

    sqlx::query("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .map_err(|e| StorageError::Db(e.to_string()))?;

    // Run embedded migrations.
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| StorageError::Db(e.to_string()))?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn connect_creates_db_with_wal() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("test.db");
        let pool = connect(&db).await.expect("connect should succeed");
        // Verify WAL mode
        let row: (String,) = sqlx::query_as("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.0, "wal");
        pool.close().await;
    }

    #[tokio::test]
    async fn connect_invalid_path_returns_error() {
        let result = connect(std::path::Path::new("/no/such/dir/test.db")).await;
        assert!(result.is_err());
    }
}
