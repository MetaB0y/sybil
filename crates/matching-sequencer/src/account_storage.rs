use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use crate::account::{Account, AccountId, AccountStore};
use crate::canonical_state::CanonicalState;
use crate::qmdb_accounts::QmdbAccounts;
use crate::qmdb_state::QmdbState;
pub use crate::qmdb_state::{
    QmdbStateExclusionProofParts, QmdbStateKeyValueProofParts, QmdbStateLeafExclusionProof,
    QmdbStateLeafProof, QmdbStateOperationProofParts, QmdbStateRangeProofParts, QmdbStateRoot,
    QMDB_STATE_MAX_KEY_BYTES,
};
use crate::store::StoreError;

type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StoreError>> + Send + 'a>>;
type RecoveredAccounts = HashMap<AccountId, Account>;

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
    pub state_sidecar: &'a sybil_verifier::StateSidecarSnapshot,
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
    fn persist<'a>(&'a self, state: CommittedAccountState<'a>) -> StoreFuture<'a, ()>;

    fn recover<'a>(&'a self, state: RecoveryAccountState) -> StoreFuture<'a, RecoveredAccounts>;

    fn qmdb_state_root<'a>(&'a self, slot: AccountSnapshotSlot) -> StoreFuture<'a, QmdbStateRoot>;

    fn qmdb_state_leaves<'a>(
        &'a self,
        slot: AccountSnapshotSlot,
    ) -> StoreFuture<'a, Vec<(Vec<u8>, Vec<u8>)>>;

    fn qmdb_state_leaf_proof<'a>(
        &'a self,
        slot: AccountSnapshotSlot,
        leaf_key: &'a [u8],
    ) -> StoreFuture<'a, Option<QmdbStateLeafProof>>;

    fn qmdb_state_leaf_exclusion_proof<'a>(
        &'a self,
        slot: AccountSnapshotSlot,
        leaf_key: &'a [u8],
    ) -> StoreFuture<'a, Option<QmdbStateLeafExclusionProof>>;
}

pub struct FencedAccountStorage {
    qmdb_accounts: QmdbAccounts,
    qmdb_state_a: QmdbState,
    qmdb_state_b: QmdbState,
}

impl FencedAccountStorage {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            qmdb_accounts: QmdbAccounts::open(path)?,
            qmdb_state_a: QmdbState::open(&path.join("state-a"), AccountSnapshotSlot::A)?,
            qmdb_state_b: QmdbState::open(&path.join("state-b"), AccountSnapshotSlot::B)?,
        })
    }

    fn state_slot(&self, slot: AccountSnapshotSlot) -> &QmdbState {
        match slot {
            AccountSnapshotSlot::A => &self.qmdb_state_a,
            AccountSnapshotSlot::B => &self.qmdb_state_b,
        }
    }
}

impl AccountStateStore for FencedAccountStorage {
    fn persist<'a>(&'a self, state: CommittedAccountState<'a>) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            self.qmdb_accounts
                .persist(
                    state.slot,
                    state.accounts,
                    state.height,
                    state.next_account_id,
                )
                .await?;

            let canonical = CanonicalState::from_accounts(state.accounts);
            let leaves = sybil_verifier::block::state_root_leaves(
                canonical.as_snapshots(),
                state.state_sidecar,
            );
            self.state_slot(state.slot).persist(leaves).await
        })
    }

    fn recover<'a>(&'a self, state: RecoveryAccountState) -> StoreFuture<'a, RecoveredAccounts> {
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

    fn qmdb_state_root<'a>(&'a self, slot: AccountSnapshotSlot) -> StoreFuture<'a, QmdbStateRoot> {
        Box::pin(async move { self.state_slot(slot).root().await })
    }

    fn qmdb_state_leaves<'a>(
        &'a self,
        slot: AccountSnapshotSlot,
    ) -> StoreFuture<'a, Vec<(Vec<u8>, Vec<u8>)>> {
        Box::pin(async move { self.state_slot(slot).leaves().await })
    }

    fn qmdb_state_leaf_proof<'a>(
        &'a self,
        slot: AccountSnapshotSlot,
        leaf_key: &'a [u8],
    ) -> StoreFuture<'a, Option<QmdbStateLeafProof>> {
        let leaf_key = leaf_key.to_vec();
        Box::pin(async move { self.state_slot(slot).leaf_proof(&leaf_key).await })
    }

    fn qmdb_state_leaf_exclusion_proof<'a>(
        &'a self,
        slot: AccountSnapshotSlot,
        leaf_key: &'a [u8],
    ) -> StoreFuture<'a, Option<QmdbStateLeafExclusionProof>> {
        let leaf_key = leaf_key.to_vec();
        Box::pin(async move { self.state_slot(slot).leaf_exclusion_proof(&leaf_key).await })
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
        let state_sidecar = sybil_verifier::StateSidecarSnapshot::default();

        storage
            .persist(CommittedAccountState {
                accounts: &accounts_a,
                state_sidecar: &state_sidecar,
                height: 1,
                next_account_id: accounts_a.next_id(),
                slot: AccountSnapshotSlot::A,
            })
            .await
            .unwrap();
        storage
            .persist(CommittedAccountState {
                accounts: &accounts_b,
                state_sidecar: &state_sidecar,
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
        let state_sidecar = sybil_verifier::StateSidecarSnapshot::default();

        storage
            .persist(CommittedAccountState {
                accounts: &accounts,
                state_sidecar: &state_sidecar,
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

    #[tokio::test]
    async fn test_state_qmdb_matches_verifier_state_root() {
        let path = temp_dir("fenced-account-storage-typed-leaves");
        let storage = FencedAccountStorage::open(&path).unwrap();
        let accounts = sample_accounts(100);
        let state_sidecar = sybil_verifier::StateSidecarSnapshot::default();
        let slot = AccountSnapshotSlot::A;

        storage
            .persist(CommittedAccountState {
                accounts: &accounts,
                state_sidecar: &state_sidecar,
                height: 1,
                next_account_id: accounts.next_id(),
                slot,
            })
            .await
            .unwrap();

        let canonical = crate::canonical_state::CanonicalState::from_accounts(&accounts);
        let expected =
            sybil_verifier::block::state_root_leaves(canonical.as_snapshots(), &state_sidecar);
        let actual = storage.qmdb_state_leaves(slot).await.unwrap();
        assert_eq!(actual, expected);

        let state_root = storage.qmdb_state_root(slot).await.unwrap();
        assert_eq!(state_root.slot, slot);
        assert_eq!(
            state_root.root,
            sybil_verifier::block::state_root_from_leaves(&expected)
        );

        let (leaf_key, leaf_value) = expected
            .iter()
            .find(|(key, _)| key.starts_with(b"acct/"))
            .expect("expected account leaf");
        let proof = storage
            .qmdb_state_leaf_proof(slot, leaf_key)
            .await
            .unwrap()
            .expect("typed leaf should exist");
        assert_eq!(proof.root, state_root.root);
        assert_eq!(proof.slot, slot);
        assert_eq!(&proof.leaf_key, leaf_key);
        assert_eq!(&proof.leaf_value, leaf_value);
        assert!(proof.verify());

        let missing_key = b"acct/missing".to_vec();
        assert!(storage
            .qmdb_state_leaf_proof(slot, &missing_key)
            .await
            .unwrap()
            .is_none());
        let exclusion = storage
            .qmdb_state_leaf_exclusion_proof(slot, &missing_key)
            .await
            .unwrap()
            .expect("missing typed leaf should have exclusion proof");
        assert_eq!(exclusion.root, state_root.root);
        assert_eq!(exclusion.slot, slot);
        assert!(exclusion.verify());
        assert!(storage
            .qmdb_state_leaf_exclusion_proof(slot, leaf_key)
            .await
            .unwrap()
            .is_none());
    }
}
