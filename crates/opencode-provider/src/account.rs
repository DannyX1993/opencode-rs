//! Storage-backed provider account service.

use crate::error::ProviderError;
use opencode_core::dto::{AccountInfoDto, ActiveAccountDto, OrganizationDto};
use opencode_core::id::AccountId;
use opencode_storage::Storage;
use std::{collections::HashMap, sync::Arc};

/// Provider account state response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountStateDto {
    /// All persisted accounts.
    pub accounts: Vec<AccountInfoDto>,
    /// Active account selection, when present.
    pub active: Option<ActiveAccountDto>,
}

/// Persist/update payload for provider accounts.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PersistAccountInput {
    /// Provider account id.
    pub id: AccountId,
    /// Account email.
    pub email: String,
    /// Provider URL.
    pub url: String,
    /// Access token.
    pub access_token: String,
    /// Refresh token.
    pub refresh_token: String,
    /// Token expiry in unix millis.
    #[serde(default)]
    pub token_expiry: Option<i64>,
    /// Active org to persist alongside active account state.
    #[serde(default)]
    pub active_org_id: Option<String>,
    /// Creation time in unix millis.
    pub time_created: i64,
    /// Update time in unix millis.
    pub time_updated: i64,
}

/// Storage-backed provider account service.
pub struct AccountService {
    storage: Arc<dyn Storage>,
    orgs: HashMap<AccountId, Vec<OrganizationDto>>,
}

impl AccountService {
    /// Create a new account service over shared storage.
    #[must_use]
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self {
            storage,
            orgs: HashMap::new(),
        }
    }

    /// Create a new account service with deterministic org fixtures.
    #[must_use]
    pub fn with_orgs(
        storage: Arc<dyn Storage>,
        orgs: HashMap<AccountId, Vec<OrganizationDto>>,
    ) -> Self {
        Self { storage, orgs }
    }

    /// Read persisted accounts plus active selection.
    pub async fn state(&self) -> Result<AccountStateDto, ProviderError> {
        let accounts = self
            .storage
            .list_accounts()
            .await
            .map_err(storage_error)?
            .into_iter()
            .map(|row| AccountInfoDto {
                id: row.id,
                email: row.email,
                url: row.url,
            })
            .collect();
        let active = self
            .storage
            .get_account_state()
            .await
            .map_err(storage_error)?
            .and_then(|row| {
                row.active_account_id.map(|account_id| ActiveAccountDto {
                    account_id,
                    active_org_id: row.active_org_id,
                })
            });

        Ok(AccountStateDto { accounts, active })
    }

    /// Persist an account and make it active.
    pub async fn persist(&self, input: PersistAccountInput) -> Result<(), ProviderError> {
        self.storage
            .upsert_account(opencode_core::dto::AccountRow {
                id: input.id,
                email: input.email,
                url: input.url,
                access_token: input.access_token,
                refresh_token: input.refresh_token,
                token_expiry: input.token_expiry,
                time_created: input.time_created,
                time_updated: input.time_updated,
            })
            .await
            .map_err(storage_error)?;
        self.storage
            .set_account_state(opencode_core::dto::AccountStateRow {
                id: 1,
                active_account_id: Some(input.id),
                active_org_id: input.active_org_id,
            })
            .await
            .map_err(storage_error)
    }

    /// Change active account and optional active org.
    pub async fn set_active(
        &self,
        account_id: AccountId,
        active_org_id: Option<String>,
    ) -> Result<(), ProviderError> {
        let Some(account) = self
            .storage
            .get_account(account_id)
            .await
            .map_err(storage_error)?
        else {
            return Err(ProviderError::Auth {
                provider: "account".into(),
                msg: format!("account {account_id} is not persisted"),
            });
        };

        if let Some(org_id) = active_org_id.as_ref() {
            let known = self
                .orgs
                .get(&account.id)
                .map(|entries| entries.iter().any(|org| &org.id == org_id))
                .unwrap_or(false)
                || self
                    .storage
                    .get_account_state()
                    .await
                    .map_err(storage_error)?
                    .is_some_and(|state| {
                        state.active_account_id == Some(account_id)
                            && state.active_org_id.as_deref() == Some(org_id)
                    });
            if !known {
                return Err(ProviderError::Auth {
                    provider: "account".into(),
                    msg: format!("organization {org_id} is not persisted for account {account_id}"),
                });
            }
        }

        self.storage
            .set_account_state(opencode_core::dto::AccountStateRow {
                id: 1,
                active_account_id: Some(account_id),
                active_org_id,
            })
            .await
            .map_err(storage_error)
    }

    /// Remove an account and rely on storage to clear invalid active state.
    pub async fn remove(&self, account_id: AccountId) -> Result<(), ProviderError> {
        self.storage
            .remove_account(account_id)
            .await
            .map_err(storage_error)
    }

    /// Refresh stored tokens for an existing account.
    pub async fn refresh_tokens(
        &self,
        account_id: AccountId,
        access_token: String,
        refresh_token: String,
        token_expiry: Option<i64>,
        time_updated: i64,
    ) -> Result<(), ProviderError> {
        self.storage
            .update_account_tokens(
                account_id,
                access_token,
                refresh_token,
                token_expiry,
                time_updated,
            )
            .await
            .map_err(storage_error)
    }
}

fn storage_error(err: opencode_core::error::StorageError) -> ProviderError {
    ProviderError::Http("account".into(), err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use opencode_core::{
        dto::{
            AccountRow, AccountStateRow, ControlAccountRow, MessageRow, MessageWithParts, PartRow,
            PermissionRow, ProjectRow, SessionRow, TodoRow,
        },
        error::StorageError,
        id::{ProjectId, SessionId},
    };
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct StubStorage {
        accounts: Mutex<Vec<AccountRow>>,
        state: Mutex<Option<AccountStateRow>>,
    }

    impl StubStorage {
        fn with_accounts(accounts: Vec<AccountRow>, state: Option<AccountStateRow>) -> Self {
            Self {
                accounts: Mutex::new(accounts),
                state: Mutex::new(state),
            }
        }
    }

    #[async_trait]
    impl Storage for StubStorage {
        async fn upsert_project(&self, _: ProjectRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn get_project(&self, _: ProjectId) -> Result<Option<ProjectRow>, StorageError> {
            Ok(None)
        }
        async fn list_projects(&self) -> Result<Vec<ProjectRow>, StorageError> {
            Ok(vec![])
        }
        async fn create_session(&self, _: SessionRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn get_session(&self, _: SessionId) -> Result<Option<SessionRow>, StorageError> {
            Ok(None)
        }
        async fn list_sessions(&self, _: ProjectId) -> Result<Vec<SessionRow>, StorageError> {
            Ok(vec![])
        }
        async fn update_session(&self, _: SessionRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn append_message(&self, _: MessageRow, _: Vec<PartRow>) -> Result<(), StorageError> {
            Ok(())
        }
        async fn append_part(&self, _: PartRow) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list_history(&self, _: SessionId) -> Result<Vec<MessageRow>, StorageError> {
            Ok(vec![])
        }
        async fn list_history_with_parts(
            &self,
            _: SessionId,
        ) -> Result<Vec<MessageWithParts>, StorageError> {
            Ok(vec![])
        }
        async fn save_todos(&self, _: SessionId, _: Vec<TodoRow>) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list_todos(&self, _: SessionId) -> Result<Vec<TodoRow>, StorageError> {
            Ok(vec![])
        }
        async fn get_permission(
            &self,
            _: ProjectId,
        ) -> Result<Option<PermissionRow>, StorageError> {
            Ok(None)
        }
        async fn set_permission(&self, _: PermissionRow) -> Result<(), StorageError> {
            Ok(())
        }

        async fn upsert_account(&self, row: AccountRow) -> Result<(), StorageError> {
            let mut accounts = self.accounts.lock().unwrap();
            if let Some(existing) = accounts.iter_mut().find(|item| item.id == row.id) {
                *existing = row;
            } else {
                accounts.push(row);
            }
            Ok(())
        }

        async fn list_accounts(&self) -> Result<Vec<AccountRow>, StorageError> {
            Ok(self.accounts.lock().unwrap().clone())
        }

        async fn get_account(&self, id: AccountId) -> Result<Option<AccountRow>, StorageError> {
            Ok(self
                .accounts
                .lock()
                .unwrap()
                .iter()
                .find(|item| item.id == id)
                .cloned())
        }

        async fn remove_account(&self, id: AccountId) -> Result<(), StorageError> {
            self.accounts.lock().unwrap().retain(|item| item.id != id);
            let mut state = self.state.lock().unwrap();
            if state.as_ref().and_then(|item| item.active_account_id) == Some(id) {
                *state = Some(AccountStateRow {
                    id: 1,
                    active_account_id: None,
                    active_org_id: None,
                });
            }
            Ok(())
        }

        async fn update_account_tokens(
            &self,
            id: AccountId,
            access_token: String,
            refresh_token: String,
            token_expiry: Option<i64>,
            time_updated: i64,
        ) -> Result<(), StorageError> {
            let mut accounts = self.accounts.lock().unwrap();
            let Some(row) = accounts.iter_mut().find(|item| item.id == id) else {
                return Err(StorageError::NotFound {
                    entity: "account",
                    id: id.to_string(),
                });
            };
            row.access_token = access_token;
            row.refresh_token = refresh_token;
            row.token_expiry = token_expiry;
            row.time_updated = time_updated;
            Ok(())
        }

        async fn get_account_state(&self) -> Result<Option<AccountStateRow>, StorageError> {
            Ok(self.state.lock().unwrap().clone())
        }

        async fn set_account_state(&self, row: AccountStateRow) -> Result<(), StorageError> {
            *self.state.lock().unwrap() = Some(AccountStateRow { id: 1, ..row });
            Ok(())
        }

        async fn get_control_account(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<ControlAccountRow>, StorageError> {
            Ok(None)
        }

        async fn get_active_control_account(
            &self,
        ) -> Result<Option<ControlAccountRow>, StorageError> {
            Ok(None)
        }

        async fn append_event(
            &self,
            _: &str,
            _: &str,
            _: serde_json::Value,
        ) -> Result<i64, StorageError> {
            Ok(0)
        }
    }

    fn account(id: AccountId, email: &str) -> AccountRow {
        AccountRow {
            id,
            email: email.into(),
            url: "https://provider.example.com".into(),
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            token_expiry: Some(100),
            time_created: 1,
            time_updated: 1,
        }
    }

    #[tokio::test]
    async fn account_service_reads_persisted_accounts_and_active_state() {
        let aid = AccountId::new();
        let storage = Arc::new(StubStorage::with_accounts(
            vec![account(aid, "user@example.com")],
            Some(AccountStateRow {
                id: 1,
                active_account_id: Some(aid),
                active_org_id: Some("org-a".into()),
            }),
        ));

        let service = AccountService::new(storage);
        let state = service.state().await.unwrap();

        assert_eq!(state.accounts.len(), 1);
        assert_eq!(state.accounts[0].email, "user@example.com");
        assert_eq!(
            state.active.unwrap().active_org_id.as_deref(),
            Some("org-a")
        );
    }

    #[tokio::test]
    async fn account_service_validates_active_switches_against_known_orgs() {
        let aid = AccountId::new();
        let storage = Arc::new(StubStorage::with_accounts(
            vec![account(aid, "user@example.com")],
            None,
        ));
        let service = AccountService::with_orgs(
            storage,
            HashMap::from([(
                aid,
                vec![OrganizationDto {
                    id: "org-a".into(),
                    name: "Org A".into(),
                }],
            )]),
        );

        service.set_active(aid, Some("org-a".into())).await.unwrap();

        let err = service
            .set_active(aid, Some("org-missing".into()))
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::Auth { .. }));
    }

    #[tokio::test]
    async fn account_service_removes_active_account_without_dangling_state() {
        let aid = AccountId::new();
        let storage = Arc::new(StubStorage::with_accounts(
            vec![account(aid, "user@example.com")],
            Some(AccountStateRow {
                id: 1,
                active_account_id: Some(aid),
                active_org_id: Some("org-a".into()),
            }),
        ));

        let service = AccountService::new(storage.clone());
        service.remove(aid).await.unwrap();

        let state = service.state().await.unwrap();
        assert!(state.accounts.is_empty());
        assert!(state.active.is_none());
    }

    #[tokio::test]
    async fn account_service_persists_and_refreshes_account_credentials() {
        let aid = AccountId::new();
        let storage = Arc::new(StubStorage::default());
        let service = AccountService::new(storage.clone());

        service
            .persist(PersistAccountInput {
                id: aid,
                email: "user@example.com".into(),
                url: "https://provider.example.com".into(),
                access_token: "access-1".into(),
                refresh_token: "refresh-1".into(),
                token_expiry: Some(10),
                active_org_id: Some("org-a".into()),
                time_created: 1,
                time_updated: 1,
            })
            .await
            .unwrap();

        service
            .refresh_tokens(aid, "access-2".into(), "refresh-2".into(), Some(20), 5)
            .await
            .unwrap();

        let row = storage.get_account(aid).await.unwrap().unwrap();
        assert_eq!(row.access_token, "access-2");
        assert_eq!(row.refresh_token, "refresh-2");
        assert_eq!(row.token_expiry, Some(20));
    }

    #[tokio::test]
    async fn account_service_allows_switching_back_to_persisted_active_org() {
        let aid = AccountId::new();
        let storage = Arc::new(StubStorage::default());
        let service = AccountService::new(storage);

        service
            .persist(PersistAccountInput {
                id: aid,
                email: "user@example.com".into(),
                url: "https://provider.example.com".into(),
                access_token: "access-1".into(),
                refresh_token: "refresh-1".into(),
                token_expiry: Some(10),
                active_org_id: Some("org-a".into()),
                time_created: 1,
                time_updated: 1,
            })
            .await
            .unwrap();

        service.set_active(aid, Some("org-a".into())).await.unwrap();
    }

    #[tokio::test]
    async fn account_service_removes_known_orgs_when_account_is_deleted() {
        let aid = AccountId::new();
        let storage = Arc::new(StubStorage::default());
        let service = AccountService::new(storage);

        service
            .persist(PersistAccountInput {
                id: aid,
                email: "user@example.com".into(),
                url: "https://provider.example.com".into(),
                access_token: "access-1".into(),
                refresh_token: "refresh-1".into(),
                token_expiry: Some(10),
                active_org_id: Some("org-a".into()),
                time_created: 1,
                time_updated: 1,
            })
            .await
            .unwrap();
        service.remove(aid).await.unwrap();

        let err = service
            .set_active(aid, Some("org-a".into()))
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::Auth { .. }));
    }
}
