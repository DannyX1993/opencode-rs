//! Account repository — OAuth account management.

use opencode_core::{dto::AccountRow, error::StorageError};
use sqlx::{Row, SqlitePool};

fn map(e: sqlx::Error) -> StorageError {
    StorageError::Db(e.to_string())
}

fn from_row(r: sqlx::sqlite::SqliteRow) -> Result<AccountRow, StorageError> {
    let id_str: String = r.try_get("id").map_err(map)?;
    Ok(AccountRow {
        id: id_str
            .parse()
            .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?,
        email: r.try_get("email").map_err(map)?,
        url: r.try_get("url").map_err(map)?,
        access_token: r.try_get("access_token").map_err(map)?,
        refresh_token: r.try_get("refresh_token").map_err(map)?,
        token_expiry: r.try_get("token_expiry").map_err(map)?,
        time_created: r.try_get("time_created").map_err(map)?,
        time_updated: r.try_get("time_updated").map_err(map)?,
    })
}

/// Insert or update an account row.
pub async fn upsert(pool: &SqlitePool, row: &AccountRow) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO account (id, email, url, access_token, refresh_token, token_expiry, time_created, time_updated)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           email = excluded.email,
           url = excluded.url,
           access_token = excluded.access_token,
           refresh_token = excluded.refresh_token,
           token_expiry = excluded.token_expiry,
           time_updated = excluded.time_updated",
    )
    .bind(row.id.to_string())
    .bind(&row.email)
    .bind(&row.url)
    .bind(&row.access_token)
    .bind(&row.refresh_token)
    .bind(row.token_expiry)
    .bind(row.time_created)
    .bind(row.time_updated)
    .execute(pool)
    .await
    .map_err(map)?;
    Ok(())
}

/// List all accounts.
pub async fn list(pool: &SqlitePool) -> Result<Vec<AccountRow>, StorageError> {
    sqlx::query("SELECT * FROM account ORDER BY time_created")
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
    use crate::pool::connect;
    use opencode_core::id::AccountId;
    use tempfile::NamedTempFile;

    async fn setup() -> (SqlitePool, NamedTempFile) {
        let f = NamedTempFile::new().unwrap();
        let pool = connect(f.path()).await.unwrap();
        (pool, f)
    }

    fn account(email: &str) -> AccountRow {
        AccountRow {
            id: AccountId::new(),
            email: email.into(),
            url: "https://example.com".into(),
            access_token: "tok".into(),
            refresh_token: "ref".into(),
            token_expiry: None,
            time_created: 1,
            time_updated: 1,
        }
    }

    #[tokio::test]
    async fn upsert_and_list() {
        let (pool, _f) = setup().await;
        upsert(&pool, &account("a@test.com")).await.unwrap();
        upsert(&pool, &account("b@test.com")).await.unwrap();
        let result = list(&pool).await.unwrap();
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn upsert_updates_on_conflict() {
        let (pool, _f) = setup().await;
        let mut acc = account("c@test.com");
        upsert(&pool, &acc).await.unwrap();
        acc.access_token = "new_tok".into();
        acc.time_updated = 2;
        upsert(&pool, &acc).await.unwrap();
        let result = list(&pool).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].access_token, "new_tok");
    }

    #[tokio::test]
    async fn list_empty_returns_empty() {
        let (pool, _f) = setup().await;
        let result = list(&pool).await.unwrap();
        assert!(result.is_empty());
    }
}
