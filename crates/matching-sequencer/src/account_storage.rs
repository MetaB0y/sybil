use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use crate::account::{Account, AccountId, AccountStore};
use crate::qmdb_accounts::QmdbAccounts;
use crate::store::StoreError;

/// Logical slot for a committed qmdb account snapshot.
///
/// We keep two slots and flip the redb fence between them. Only the slot named
/// by redb is authoritative during recovery.
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

/// Account snapshot metadata written before a redb fence flip.
pub struct CommittedAccountState<'a> {
    pub accounts: &'a AccountStore,
    pub height: u64,
    pub next_account_id: u64,
    pub slot: AccountSnapshotSlot,
}

/// Account snapshot metadata that recovery expects to find in the fenced slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryAccountState {
    pub height: u64,
    pub next_account_id: u64,
    pub slot: AccountSnapshotSlot,
}

/// Boundary for the authoritative account snapshot store.
///
/// Implementations must treat `recover()` as fence-driven: the caller chooses
/// the committed snapshot via external metadata, and the implementation must not
/// silently "pick the latest" state on its own.
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

pub struct FencedAccountStorage {
    qmdb_accounts: QmdbAccounts,
}

impl FencedAccountStorage {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            qmdb_accounts: QmdbAccounts::open(path)?,
        })
    }
}

impl AccountStateStore for FencedAccountStorage {
    fn persist<'a>(
        &'a self,
        state: CommittedAccountState<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<(), StoreError>> + Send + 'a>> {
        Box::pin(async move {
            self.qmdb_accounts
                .persist(
                    state.slot,
                    state.accounts,
                    state.height,
                    state.next_account_id,
                )
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
                && qmdb_accounts
                    .next_account_id
                    .unwrap_or(state.next_account_id)
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(prefix: &str) -> PathBuf {
        let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("sybil-{prefix}-{}-{unique}", std::process::id()))
    }

    fn sample_accounts(balance: i64) -> AccountStore {
        let mut accounts = AccountStore::new();
        let account_id = accounts.create_account(balance);
        accounts.get_mut(account_id).unwrap().events_digest = [balance as u8; 32];
        accounts
    }

    #[tokio::test]
    async fn test_recover_uses_fenced_slot_only() {
        let path = temp_dir("fenced-account-storage");
        let storage = FencedAccountStorage::open(&path).unwrap();

        let accounts_a = sample_accounts(100);
        let accounts_b = sample_accounts(200);

        storage
            .persist(CommittedAccountState {
                accounts: &accounts_a,
                height: 1,
                next_account_id: accounts_a.next_id(),
                slot: AccountSnapshotSlot::A,
            })
            .await
            .unwrap();
        storage
            .persist(CommittedAccountState {
                accounts: &accounts_b,
                height: 2,
                next_account_id: accounts_b.next_id(),
                slot: AccountSnapshotSlot::B,
            })
            .await
            .unwrap();

        let recovered = storage
            .recover(RecoveryAccountState {
                height: 1,
                next_account_id: accounts_a.next_id(),
                slot: AccountSnapshotSlot::A,
            })
            .await
            .unwrap();

        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered.get(&AccountId(0)).unwrap().balance, 100);
    }

    #[tokio::test]
    async fn test_recover_rejects_slot_metadata_mismatch() {
        let path = temp_dir("fenced-account-storage-mismatch");
        let storage = FencedAccountStorage::open(&path).unwrap();
        let accounts = sample_accounts(100);

        storage
            .persist(CommittedAccountState {
                accounts: &accounts,
                height: 1,
                next_account_id: accounts.next_id(),
                slot: AccountSnapshotSlot::A,
            })
            .await
            .unwrap();

        let error = match storage
            .recover(RecoveryAccountState {
                height: 2,
                next_account_id: accounts.next_id(),
                slot: AccountSnapshotSlot::A,
            })
            .await
        {
            Ok(_) => panic!("expected slot metadata mismatch to be rejected"),
            Err(error) => error,
        };

        assert!(matches!(error, StoreError::CorruptLayout(_)));
    }
}
