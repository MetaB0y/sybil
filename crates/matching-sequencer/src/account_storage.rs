use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use crate::account::{Account, AccountId, AccountStore};
use crate::qmdb_accounts::QmdbAccounts;
use crate::store::StoreError;

pub struct CommittedAccountState<'a> {
    pub accounts: &'a AccountStore,
    pub height: u64,
    pub next_account_id: u64,
}

pub struct RecoveryAccountState {
    pub fallback_accounts: HashMap<AccountId, Account>,
    pub height: u64,
    pub next_account_id: u64,
}

pub trait AccountStateStore: Send + Sync {
    fn persist<'a>(
        &'a self,
        state: CommittedAccountState<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<(), StoreError>> + Send + 'a>>;

    fn recover<'a>(
        &'a self,
        state: RecoveryAccountState,
    ) -> Pin<Box<dyn Future<Output = Result<HashMap<AccountId, Account>, StoreError>> + Send + 'a>>;
}

pub struct TransitionAccountStorage {
    qmdb_accounts: QmdbAccounts,
}

impl TransitionAccountStorage {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            qmdb_accounts: QmdbAccounts::open(path)?,
        })
    }
}

impl AccountStateStore for TransitionAccountStorage {
    fn persist<'a>(
        &'a self,
        state: CommittedAccountState<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<(), StoreError>> + Send + 'a>> {
        Box::pin(async move {
            self.qmdb_accounts
                .persist(state.accounts, state.height, state.next_account_id)
                .await
        })
    }

    fn recover<'a>(
        &'a self,
        state: RecoveryAccountState,
    ) -> Pin<Box<dyn Future<Output = Result<HashMap<AccountId, Account>, StoreError>> + Send + 'a>>
    {
        Box::pin(async move {
            let qmdb_accounts = self.qmdb_accounts.load().await?;
            if qmdb_accounts.accounts.is_empty() {
                if !state.fallback_accounts.is_empty() {
                    tracing::warn!("qmdb account snapshot missing, falling back to redb accounts");
                }
                return Ok(state.fallback_accounts);
            }

            if qmdb_accounts.height == Some(state.height)
                && qmdb_accounts.next_account_id.unwrap_or(state.next_account_id)
                    == state.next_account_id
            {
                return Ok(qmdb_accounts.accounts);
            }

            tracing::warn!(
                redb_height = state.height,
                qmdb_height = ?qmdb_accounts.height,
                redb_next_account_id = state.next_account_id,
                qmdb_next_account_id = ?qmdb_accounts.next_account_id,
                "qmdb account snapshot did not match redb metadata, falling back to redb accounts"
            );
            Ok(state.fallback_accounts)
        })
    }
}
