use std::collections::HashMap;

use matching_engine::{Fill, MarketId, Nanos};
use matching_solver::PipelineResult;
use sybil_verifier::BlockWitness;

use crate::account::AccountStore;
use crate::canonical_state::CanonicalState;
use crate::error::Rejection;
use crate::system_event::SystemEvent;

/// Named result of [`BlockSequencer::produce_block`].
pub struct BlockProduction {
    pub block: Block,
    pub pipeline: PipelineResult,
    pub witness: BlockWitness,
}

/// Header of a sequencer block.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BlockHeader {
    pub height: u64,
    /// blake3(previous header bytes), zeros for genesis.
    pub parent_hash: [u8; 32],
    /// blake3(canonical account state).
    pub state_root: [u8; 32],
    pub order_count: u32,
    pub fill_count: u32,
    pub timestamp_ms: u64,
}

/// A sequencer block produced each tick.
#[derive(Clone)]
pub struct Block {
    pub header: BlockHeader,
    pub order_ids: Vec<u64>,
    pub system_events: Vec<SystemEvent>,
    pub fills: Vec<Fill>,
    pub clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    pub rejections: Vec<Rejection>,
    pub total_welfare: i64,
    pub total_volume: u64,
    pub orders_filled: usize,
}

/// Compute a deterministic state root from accounts.
///
/// Canonical encoding: sorted by AccountId, each account encodes
/// balance, total_deposited, then sorted (MarketId, outcome) -> position pairs.
/// All integers are little-endian i64/u64.
///
/// NOTE: This is a flat hash over all accounts — O(n) per block. For the validium
/// proof pipeline, replace with an authenticated data structure (Merkle tree / MMR)
/// so we can produce per-account inclusion proofs and incremental state roots.
/// Candidate: commonware-storage qmdb (LayerZero research + Commonware productionization).
/// See: https://commonware.xyz/blogs/qmdb
pub fn compute_state_root(accounts: &AccountStore) -> [u8; 32] {
    CanonicalState::from_accounts(accounts).state_root()
}

/// Compute blake3 hash of a block header for chaining.
pub fn hash_header(header: &BlockHeader) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&header.height.to_le_bytes());
    hasher.update(&header.parent_hash);
    hasher.update(&header.state_root);
    hasher.update(&header.order_count.to_le_bytes());
    hasher.update(&header.fill_count.to_le_bytes());
    hasher.update(&header.timestamp_ms.to_le_bytes());
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use crate::canonical_state::CanonicalState;
    use matching_engine::MarketId;
    use proptest::prelude::*;
    use sybil_verifier::AccountSnapshot;

    #[test]
    fn test_state_root_deterministic() {
        let mut accounts = AccountStore::new();
        let a0 = accounts.create_account(100);
        let a1 = accounts.create_account(200);

        let m0 = MarketId::new(0);
        accounts.get_mut(a0).unwrap().positions.insert((m0, 0), 10);
        accounts.get_mut(a1).unwrap().positions.insert((m0, 1), 5);

        let root1 = compute_state_root(&accounts);
        let root2 = compute_state_root(&accounts);
        assert_eq!(root1, root2);
    }

    #[test]
    fn test_state_root_changes_on_mutation() {
        let mut accounts = AccountStore::new();
        let a0 = accounts.create_account(100);

        let root1 = compute_state_root(&accounts);

        accounts.get_mut(a0).unwrap().balance = 200;
        let root2 = compute_state_root(&accounts);

        assert_ne!(root1, root2);
    }

    #[test]
    fn test_state_root_changes_on_events_digest_only() {
        let mut accounts = AccountStore::new();
        let a0 = accounts.create_account(100);

        let root1 = compute_state_root(&accounts);
        accounts.get_mut(a0).unwrap().events_digest = [7u8; 32];
        let root2 = compute_state_root(&accounts);

        assert_ne!(root1, root2);
    }

    #[test]
    fn test_state_root_changes_on_total_deposited_only() {
        let mut accounts = AccountStore::new();
        let a0 = accounts.create_account(100);

        let root1 = compute_state_root(&accounts);
        accounts.get_mut(a0).unwrap().total_deposited = 150;
        let root2 = compute_state_root(&accounts);

        assert_ne!(root1, root2);
    }

    #[test]
    fn test_state_root_empty_accounts() {
        let accounts = AccountStore::new();
        let root = compute_state_root(&accounts);
        // Empty hash should be deterministic
        assert_eq!(root, compute_state_root(&accounts));
    }

    #[test]
    fn test_state_root_position_order_independence() {
        // Adding positions in different order should produce same root
        let m0 = MarketId::new(0);
        let m1 = MarketId::new(1);

        let mut accounts1 = AccountStore::new();
        let a = accounts1.create_account(100);
        let acc = accounts1.get_mut(a).unwrap();
        acc.positions.insert((m0, 0), 10);
        acc.positions.insert((m1, 0), 20);

        let mut accounts2 = AccountStore::new();
        let b = accounts2.create_account(100);
        let acc = accounts2.get_mut(b).unwrap();
        acc.positions.insert((m1, 0), 20);
        acc.positions.insert((m0, 0), 10);

        assert_eq!(
            compute_state_root(&accounts1),
            compute_state_root(&accounts2)
        );
    }

    #[test]
    fn test_state_root_ignores_zero_quantity_positions() {
        let m0 = MarketId::new(0);

        let mut accounts = AccountStore::new();
        let a0 = accounts.create_account(100);
        accounts.get_mut(a0).unwrap().positions.insert((m0, 0), 0);

        let mut snapshot: Vec<_> = accounts
            .iter()
            .map(|(&id, account)| {
                let mut positions: Vec<_> = account
                    .positions
                    .iter()
                    .filter(|(_, &qty)| qty != 0)
                    .map(|(&(market, outcome), &qty)| (market, outcome, qty))
                    .collect();
                positions.sort_by_key(|&(market, outcome, _)| (market.0, outcome));
                AccountSnapshot {
                    id: id.0,
                    balance: account.balance,
                    total_deposited: account.total_deposited,
                    positions,
                    events_digest: account.events_digest,
                }
            })
            .collect();
        snapshot.sort_by_key(|account| account.id);

        assert_eq!(
            compute_state_root(&accounts),
            sybil_verifier::block::compute_state_root(&snapshot)
        );
    }

    #[test]
    fn test_hash_header_deterministic() {
        let header = BlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [1u8; 32],
            order_count: 5,
            fill_count: 3,
            timestamp_ms: 1000,
        };
        assert_eq!(hash_header(&header), hash_header(&header));
    }

    #[test]
    fn test_hash_header_changes_on_field_change() {
        let h1 = BlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [1u8; 32],
            order_count: 5,
            fill_count: 3,
            timestamp_ms: 1000,
        };
        let h2 = BlockHeader {
            height: 2,
            ..h1.clone()
        };
        assert_ne!(hash_header(&h1), hash_header(&h2));
    }

    proptest! {
        #[test]
        fn prop_sequencer_and_verifier_state_roots_agree(
            balances in prop::collection::vec(-1_000i64..=1_000, 0..6),
            digests in prop::collection::vec(prop::array::uniform32(any::<u8>()), 0..6),
        ) {
            let len = balances.len().min(digests.len());
            let mut accounts = AccountStore::new();

            for index in 0..len {
                let account_id = accounts.create_account(balances[index]);
                let account = accounts.get_mut(account_id).unwrap();
                account.total_deposited = balances[index].saturating_add(index as i64);
                account.events_digest = digests[index];

                if index % 2 == 0 {
                    account.positions.insert((MarketId::new(index as u32), 0), index as i64 + 1);
                } else {
                    account.positions.insert((MarketId::new(index as u32), 1), -((index as i64) + 1));
                }
            }

            let snapshots = CanonicalState::from_accounts(&accounts).into_snapshots();
            prop_assert_eq!(
                compute_state_root(&accounts),
                sybil_verifier::block::compute_state_root(&snapshots),
            );
        }
    }
}
