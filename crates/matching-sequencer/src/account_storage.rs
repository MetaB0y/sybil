use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use crate::account::{Account, AccountId, AccountStore};
use crate::qmdb_accounts::QmdbAccounts;
use crate::store::StoreError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountSnapshotSlot {
    A,
    B,
}

impl AccountSnapshotSlot {
    pub const fn encode(self) -> u64 {
        match self {
            Self::A => 0,
            Self::B => 1,
        }
    }

    pub fn decode(raw: u64) -> Result<Self, StoreError> {
        match raw {
            0 => Ok(Self::A),
            1 => Ok(Self::B),
            other => Err(StoreError::CorruptLayout(format!(
                "invalid account snapshot slot value: {other}"
            ))),
        }
    }

    pub const fn inactive(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

pub struct CommittedAccountState<'a> {
    pub accounts: &'a AccountStore,
    pub height: u64,
    pub next_account_id: u64,
    pub slot: AccountSnapshotSlot,
}

pub struct RecoveryAccountState {
    pub height: u64,
    pub next_account_id: u64,
    pub slot: AccountSnapshotSlot,
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
                .persist(state.slot, state.accounts, state.height, state.next_account_id)
                .await
        })
    }

    fn recover<'a>(
        &'a self,
        state: RecoveryAccountState,
    ) -> Pin<Box<dyn Future<Output = Result<HashMap<AccountId, Account>, StoreError>> + Send + 'a>>
    {
        Box::pin(async move {
            let qmdb_accounts = self.qmdb_accounts.load(state.slot).await?;

            if qmdb_accounts.height == Some(state.height)
                && qmdb_accounts.next_account_id.unwrap_or(state.next_account_id)
                    == state.next_account_id
            {
                return Ok(qmdb_accounts.accounts);
            }

            Err(StoreError::CorruptLayout(format!(
                "qmdb account snapshot metadata mismatch for slot {:?}: expected height={} next_account_id={}, got height={:?} next_account_id={:?}",
                state.slot,
                state.height,
                state.next_account_id,
                qmdb_accounts.height,
                qmdb_accounts.next_account_id
            )))
        })
    }
}
