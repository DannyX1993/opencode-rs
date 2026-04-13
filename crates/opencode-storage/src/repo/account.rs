//! Account repository — OAuth account management.

use opencode_core::{
    dto::{AccountRow, AccountStateRow, ControlAccountRow},
    error::StorageError,
    id::AccountId,
};
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

fn from_state_row(r: sqlx::sqlite::SqliteRow) -> Result<AccountStateRow, StorageError> {
    let active_account_id = r
        .try_get::<Option<String>, _>("active_account_id")
        .map_err(map)?
        .map(|value| value.parse())
        .transpose()
        .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?;
    Ok(AccountStateRow {
        id: r.try_get("id").map_err(map)?,
        active_account_id,
        active_org_id: r.try_get("active_org_id").map_err(map)?,
    })
}

fn from_control_row(r: sqlx::sqlite::SqliteRow) -> Result<ControlAccountRow, StorageError> {
    Ok(ControlAccountRow {
        email: r.try_get("email").map_err(map)?,
        url: r.try_get("url").map_err(map)?,
        access_token: r.try_get("access_token").map_err(map)?,
        refresh_token: r.try_get("refresh_token").map_err(map)?,
        token_expiry: r.try_get("token_expiry").map_err(map)?,
        active: r.try_get("active").map_err(map)?,
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

/// Fetch an account by id.
pub async fn get(pool: &SqlitePool, id: AccountId) -> Result<Option<AccountRow>, StorageError> {
    sqlx::query("SELECT * FROM account WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await
        .map_err(map)?
        .map(from_row)
        .transpose()
}

/// Remove an account and clear singleton active state if necessary.
pub async fn remove(pool: &SqlitePool, id: AccountId) -> Result<(), StorageError> {
    sqlx::query("UPDATE account_state SET active_account_id = NULL, active_org_id = NULL WHERE active_account_id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(map)?;

    sqlx::query("DELETE FROM account WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(map)?;
    Ok(())
}

/// Update persisted tokens for an existing account.
pub async fn update_tokens(
    pool: &SqlitePool,
    id: AccountId,
    access_token: &str,
    refresh_token: &str,
    token_expiry: Option<i64>,
    time_updated: i64,
) -> Result<(), StorageError> {
    let result = sqlx::query(
        "UPDATE account
         SET access_token = ?, refresh_token = ?, token_expiry = ?, time_updated = ?
         WHERE id = ?",
    )
    .bind(access_token)
    .bind(refresh_token)
    .bind(token_expiry)
    .bind(time_updated)
    .bind(id.to_string())
    .execute(pool)
    .await
    .map_err(map)?;

    if result.rows_affected() == 0 {
        return Err(StorageError::NotFound {
            entity: "account",
            id: id.to_string(),
        });
    }

    Ok(())
}

/// Read the singleton account state row.
pub async fn get_state(pool: &SqlitePool) -> Result<Option<AccountStateRow>, StorageError> {
    sqlx::query("SELECT * FROM account_state WHERE id = 1")
        .fetch_optional(pool)
        .await
        .map_err(map)?
        .map(from_state_row)
        .transpose()
}

/// Persist the singleton account state row.
pub async fn set_state(pool: &SqlitePool, row: &AccountStateRow) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO account_state (id, active_account_id, active_org_id)
         VALUES (1, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           active_account_id = excluded.active_account_id,
           active_org_id = excluded.active_org_id",
    )
    .bind(row.active_account_id.map(|value| value.to_string()))
    .bind(&row.active_org_id)
    .execute(pool)
    .await
    .map_err(map)?;
    Ok(())
}

/// Read a legacy control-account row by composite key.
pub async fn get_control_account(
    pool: &SqlitePool,
    email: &str,
    url: &str,
) -> Result<Option<ControlAccountRow>, StorageError> {
    sqlx::query("SELECT * FROM control_account WHERE email = ? AND url = ?")
        .bind(email)
        .bind(url)
        .fetch_optional(pool)
        .await
        .map_err(map)?
        .map(from_control_row)
        .transpose()
}

/// Read the active legacy control-account row.
pub async fn get_active_control_account(
    pool: &SqlitePool,
) -> Result<Option<ControlAccountRow>, StorageError> {
    sqlx::query("SELECT * FROM control_account WHERE active = 1 ORDER BY time_updated DESC LIMIT 1")
        .fetch_optional(pool)
        .await
        .map_err(map)?
        .map(from_control_row)
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::connect;
    use opencode_core::{dto::AccountStateRow, id::AccountId};
    use sqlx::Executor;
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

    #[tokio::test]
    async fn get_returns_persisted_account() {
        let (pool, _f) = setup().await;
        let row = account("lookup@test.com");
        let id = row.id;
        upsert(&pool, &row).await.unwrap();

        let found = get(&pool, id).await.unwrap().unwrap();

        assert_eq!(found.id, id);
        assert_eq!(found.email, "lookup@test.com");
    }

    #[tokio::test]
    async fn get_returns_none_for_missing_account() {
        let (pool, _f) = setup().await;

        let found = get(&pool, AccountId::new()).await.unwrap();

        assert!(found.is_none());
    }

    #[tokio::test]
    async fn update_tokens_persists_latest_credentials() {
        let (pool, _f) = setup().await;
        let row = account("token@test.com");
        let id = row.id;
        upsert(&pool, &row).await.unwrap();

        update_tokens(&pool, id, "next-access", "next-refresh", Some(42), 9)
            .await
            .unwrap();

        let found = get(&pool, id).await.unwrap().unwrap();
        assert_eq!(found.access_token, "next-access");
        assert_eq!(found.refresh_token, "next-refresh");
        assert_eq!(found.token_expiry, Some(42));
        assert_eq!(found.time_updated, 9);
    }

    #[tokio::test]
    async fn update_tokens_errors_for_missing_account() {
        let (pool, _f) = setup().await;

        let err = update_tokens(
            &pool,
            AccountId::new(),
            "next-access",
            "next-refresh",
            None,
            9,
        )
        .await
        .unwrap_err();

        assert!(matches!(
            err,
            StorageError::NotFound {
                entity: "account",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn set_state_and_get_state_round_trip() {
        let (pool, _f) = setup().await;
        let row = account("active@test.com");
        upsert(&pool, &row).await.unwrap();

        set_state(
            &pool,
            &AccountStateRow {
                id: 99,
                active_account_id: Some(row.id),
                active_org_id: Some("org-1".into()),
            },
        )
        .await
        .unwrap();

        let state = get_state(&pool).await.unwrap().unwrap();
        assert_eq!(state.id, 1);
        assert_eq!(state.active_account_id, Some(row.id));
        assert_eq!(state.active_org_id.as_deref(), Some("org-1"));
    }

    #[tokio::test]
    async fn get_state_returns_none_when_singleton_missing() {
        let (pool, _f) = setup().await;

        let state = get_state(&pool).await.unwrap();

        assert!(state.is_none());
    }

    #[tokio::test]
    async fn remove_deletes_account_and_clears_active_state() {
        let (pool, _f) = setup().await;
        let row = account("remove@test.com");
        upsert(&pool, &row).await.unwrap();
        set_state(
            &pool,
            &AccountStateRow {
                id: 1,
                active_account_id: Some(row.id),
                active_org_id: Some("org-2".into()),
            },
        )
        .await
        .unwrap();

        remove(&pool, row.id).await.unwrap();

        assert!(get(&pool, row.id).await.unwrap().is_none());
        let state = get_state(&pool).await.unwrap().unwrap();
        assert_eq!(state.active_account_id, None);
        assert_eq!(state.active_org_id, None);
    }

    #[tokio::test]
    async fn get_control_account_returns_row_for_email_and_url() {
        let (pool, _f) = setup().await;
        pool.execute(
            sqlx::query(
                "INSERT INTO control_account (email, url, access_token, refresh_token, token_expiry, active, time_created, time_updated)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind("legacy@test.com")
            .bind("https://legacy.example.com")
            .bind("access")
            .bind("refresh")
            .bind(Option::<i64>::Some(77))
            .bind(true)
            .bind(1_i64)
            .bind(2_i64),
        )
        .await
        .unwrap();

        let found = get_control_account(&pool, "legacy@test.com", "https://legacy.example.com")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(found.email, "legacy@test.com");
        assert!(found.active);
    }

    #[tokio::test]
    async fn get_active_control_account_returns_active_row() {
        let (pool, _f) = setup().await;
        pool.execute(
            sqlx::query(
                "INSERT INTO control_account (email, url, access_token, refresh_token, token_expiry, active, time_created, time_updated)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?), (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind("inactive@test.com")
            .bind("https://inactive.example.com")
            .bind("inactive-access")
            .bind("inactive-refresh")
            .bind(Option::<i64>::None)
            .bind(false)
            .bind(1_i64)
            .bind(1_i64)
            .bind("active@test.com")
            .bind("https://active.example.com")
            .bind("active-access")
            .bind("active-refresh")
            .bind(Option::<i64>::Some(100))
            .bind(true)
            .bind(2_i64)
            .bind(3_i64),
        )
        .await
        .unwrap();

        let found = get_active_control_account(&pool).await.unwrap().unwrap();

        assert_eq!(found.email, "active@test.com");
        assert_eq!(found.url, "https://active.example.com");
    }
}
