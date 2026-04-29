//! Layer 3: Block header integrity verification.
//!
//! Checks state root, parent hash chaining, height, and counts.

use sha2::{Digest as _, Sha256};

use crate::types::{
    AccountSnapshot, BlockWitness, BridgeStateSnapshot, WithdrawalSnapshot, WitnessBlockHeader,
};
use crate::violations::{VerificationResult, VerificationStats, Violation, ViolationKind};

/// Verify block header integrity.
pub fn verify_block(witness: &BlockWitness) -> VerificationResult {
    let mut violations = Vec::new();
    let stats = VerificationStats::default();

    // 1. State root: recompute from post-state and bridge sidecar
    let computed_root = compute_state_root_with_bridge(&witness.post_state, &witness.bridge_state);
    if computed_root != witness.header.state_root {
        violations.push(Violation {
            kind: ViolationKind::StateRootMismatch,
            details: format!(
                "Computed state root {:?} != header state root {:?}",
                hex(&computed_root),
                hex(&witness.header.state_root),
            ),
        });
    }

    // 2. Parent hash
    match &witness.previous_header {
        Some(prev) => {
            let computed_parent = hash_header(prev);
            if computed_parent != witness.header.parent_hash {
                violations.push(Violation {
                    kind: ViolationKind::ParentHashMismatch,
                    details: format!(
                        "Computed parent hash {:?} != header parent hash {:?}",
                        hex(&computed_parent),
                        hex(&witness.header.parent_hash),
                    ),
                });
            }

            // 3. Height consecutive
            if witness.header.height != prev.height + 1 {
                violations.push(Violation {
                    kind: ViolationKind::HeightNotConsecutive,
                    details: format!(
                        "Height {} != previous {} + 1",
                        witness.header.height, prev.height
                    ),
                });
            }
        }
        None => {
            // Genesis block: parent hash must be zeros, height must be 1
            if witness.header.parent_hash != [0u8; 32] {
                violations.push(Violation {
                    kind: ViolationKind::GenesisParentHashNonZero,
                    details: format!(
                        "Genesis block has non-zero parent hash: {:?}",
                        hex(&witness.header.parent_hash),
                    ),
                });
            }
            if witness.header.height != 1 {
                violations.push(Violation {
                    kind: ViolationKind::HeightNotConsecutive,
                    details: format!("Genesis block height {} != 1", witness.header.height),
                });
            }
        }
    }

    // 4. Counts match
    let expected_order_count = witness.orders.len() + witness.rejections.len();
    if witness.header.order_count != expected_order_count as u32 {
        violations.push(Violation {
            kind: ViolationKind::OrderCountMismatch,
            details: format!(
                "header.order_count {} != orders ({}) + rejections ({})",
                witness.header.order_count,
                witness.orders.len(),
                witness.rejections.len(),
            ),
        });
    }

    if witness.header.fill_count != witness.fills.len() as u32 {
        violations.push(Violation {
            kind: ViolationKind::FillCountMismatch,
            details: format!(
                "header.fill_count {} != fills.len() {}",
                witness.header.fill_count,
                witness.fills.len(),
            ),
        });
    }

    VerificationResult {
        valid: violations.is_empty(),
        violations,
        stats,
    }
}

/// Compute the current deterministic v2 state root from account snapshots and
/// an empty bridge sidecar.
///
/// Use [`compute_state_root_with_bridge`] when verifying real blocks.
pub fn compute_state_root(accounts: &[AccountSnapshot]) -> [u8; 32] {
    compute_state_root_with_bridge(accounts, &BridgeStateSnapshot::default())
}

/// Compute a deterministic v1 account-only state root from account snapshots.
///
/// Must produce the exact same hash as `matching-sequencer`'s
/// historical `compute_state_root_v1`. Canonical encoding: sorted by account
/// id, each account encodes balance, total_deposited, then sorted
/// `(market, outcome) -> qty`.
pub fn compute_account_state_root_v1(accounts: &[AccountSnapshot]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();

    // Accounts should already be sorted by id, but sort to be safe
    let mut sorted: Vec<&AccountSnapshot> = accounts.iter().collect();
    sorted.sort_by_key(|a| a.id);

    for account in sorted {
        // AccountId
        hasher.update(&account.id.to_le_bytes());
        // Balance
        hasher.update(&account.balance.to_le_bytes());
        // Total deposited
        hasher.update(&account.total_deposited.to_le_bytes());

        // Positions should already be sorted, but sort again to ensure
        // canonical hashing even if a caller constructs a snapshot manually.
        let mut positions = account.positions.clone();
        positions.sort_by_key(|&(market, outcome, _)| (market.0, outcome));
        for (market, outcome, qty) in positions {
            hasher.update(&market.0.to_le_bytes());
            hasher.update(&[outcome]);
            hasher.update(&qty.to_le_bytes());
        }

        hasher.update(&account.events_digest);
    }

    *hasher.finalize().as_bytes()
}

/// Compute the current v2 typed state root.
///
/// This commits account leaves plus the bridge leaves required for normal
/// withdrawals. The encoding is deliberately key/value-shaped so the same
/// leaves can be persisted in qmdb and later swapped to a native qmdb proof
/// root without changing leaf domains.
pub fn compute_state_root_with_bridge(
    accounts: &[AccountSnapshot],
    bridge: &BridgeStateSnapshot,
) -> [u8; 32] {
    let leaves = state_root_v2_leaves(accounts, bridge);
    state_root_v2_from_leaves(&leaves)
}

pub fn state_root_v2_leaves(
    accounts: &[AccountSnapshot],
    bridge: &BridgeStateSnapshot,
) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut leaves = Vec::new();

    let mut sorted_accounts: Vec<&AccountSnapshot> = accounts.iter().collect();
    sorted_accounts.sort_by_key(|account| account.id);
    for account in sorted_accounts {
        leaves.push((account_leaf_key(account.id), account_leaf_value(account)));
    }

    leaves.push((
        b"sys/deposit_cursor".to_vec(),
        sys_u64_leaf_value(b"deposit_cursor", bridge.deposit_cursor),
    ));
    leaves.push((
        b"sys/deposit_root".to_vec(),
        sys_bytes32_leaf_value(b"deposit_root", &bridge.deposit_root),
    ));
    leaves.push((
        b"sys/next_withdrawal_id".to_vec(),
        sys_u64_leaf_value(b"next_withdrawal_id", bridge.next_withdrawal_id),
    ));

    let mut withdrawals: Vec<&WithdrawalSnapshot> = bridge.withdrawals.iter().collect();
    withdrawals.sort_by_key(|withdrawal| withdrawal.withdrawal_id);
    for withdrawal in withdrawals {
        leaves.push((
            withdrawal_leaf_key(withdrawal.withdrawal_id),
            withdrawal_leaf_value(withdrawal),
        ));
    }

    leaves.sort_by(|(left, _), (right, _)| left.cmp(right));
    leaves
}

pub fn state_root_v2_from_leaves(leaves: &[(Vec<u8>, Vec<u8>)]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"sybil/state-root/v2");
    hasher.update((leaves.len() as u64).to_le_bytes());
    for (key, value) in leaves {
        hasher.update((key.len() as u32).to_le_bytes());
        hasher.update(key);
        hasher.update((value.len() as u32).to_le_bytes());
        hasher.update(value);
    }
    hasher.finalize().into()
}

pub fn account_leaf_key(account_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(13);
    key.extend_from_slice(b"acct/");
    key.extend_from_slice(&account_id.to_be_bytes());
    key
}

pub fn withdrawal_leaf_key(withdrawal_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(19);
    key.extend_from_slice(b"withdrawal/");
    key.extend_from_slice(&withdrawal_id.to_be_bytes());
    key
}

fn account_leaf_value(account: &AccountSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/state/acct/v1");
    value.extend_from_slice(&account.id.to_le_bytes());
    value.extend_from_slice(&account.balance.to_le_bytes());
    value.extend_from_slice(&account.total_deposited.to_le_bytes());

    let mut positions = account.positions.clone();
    positions.sort_by_key(|&(market, outcome, _)| (market.0, outcome));
    positions.retain(|(_, _, qty)| *qty != 0);
    value.extend_from_slice(&(positions.len() as u64).to_le_bytes());
    for (market, outcome, qty) in positions {
        value.extend_from_slice(&market.0.to_le_bytes());
        value.push(outcome);
        value.extend_from_slice(&qty.to_le_bytes());
    }

    value.extend_from_slice(&account.events_digest);
    value
}

fn sys_u64_leaf_value(name: &[u8], raw: u64) -> Vec<u8> {
    let mut value = Vec::with_capacity(19 + 1 + name.len() + 8);
    value.extend_from_slice(b"sybil/state/sys/v1");
    value.push(name.len() as u8);
    value.extend_from_slice(name);
    value.extend_from_slice(&raw.to_le_bytes());
    value
}

fn sys_bytes32_leaf_value(name: &[u8], raw: &[u8; 32]) -> Vec<u8> {
    let mut value = Vec::with_capacity(19 + 1 + name.len() + 32);
    value.extend_from_slice(b"sybil/state/sys/v1");
    value.push(name.len() as u8);
    value.extend_from_slice(name);
    value.extend_from_slice(raw);
    value
}

fn withdrawal_leaf_value(withdrawal: &WithdrawalSnapshot) -> Vec<u8> {
    let mut value = Vec::with_capacity(25 + 8 + 8 + 20 + 20 + 8 + 8 + 8 + 32);
    value.extend_from_slice(b"sybil/state/withdrawal/v1");
    value.extend_from_slice(&withdrawal.withdrawal_id.to_le_bytes());
    value.extend_from_slice(&withdrawal.account_id.to_le_bytes());
    value.extend_from_slice(&withdrawal.recipient);
    value.extend_from_slice(&withdrawal.token);
    value.extend_from_slice(&withdrawal.amount_token_units.to_le_bytes());
    value.extend_from_slice(&withdrawal.amount_nanos.to_le_bytes());
    value.extend_from_slice(&withdrawal.expiry_height.to_le_bytes());
    value.extend_from_slice(&withdrawal.nullifier);
    value
}

/// Compute blake3 hash of a block header for chaining.
///
/// Must match `matching-sequencer`'s `hash_header`.
pub fn hash_header(header: &WitnessBlockHeader) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&header.height.to_le_bytes());
    hasher.update(&header.parent_hash);
    hasher.update(&header.state_root);
    hasher.update(&header.order_count.to_le_bytes());
    hasher.update(&header.fill_count.to_le_bytes());
    hasher.update(&header.timestamp_ms.to_le_bytes());
    *hasher.finalize().as_bytes()
}

/// Format a hash as hex (first 8 bytes).
fn hex(bytes: &[u8; 32]) -> String {
    bytes
        .iter()
        .take(8)
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
        + "..."
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WitnessBlockHeader;
    use matching_engine::MarketId;
    use proptest::prelude::*;
    use std::collections::HashMap;

    fn genesis_header(state_root: [u8; 32]) -> WitnessBlockHeader {
        WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root,
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 1000,
        }
    }

    #[test]
    fn test_state_root_deterministic() {
        let accounts = vec![
            AccountSnapshot {
                id: 0,
                balance: 100,
                total_deposited: 100,
                positions: vec![(MarketId::new(0), 0, 10)],
                events_digest: [0u8; 32],
            },
            AccountSnapshot {
                id: 1,
                balance: 200,
                total_deposited: 200,
                positions: vec![(MarketId::new(0), 1, 5)],
                events_digest: [0u8; 32],
            },
        ];

        let root1 = compute_state_root(&accounts);
        let root2 = compute_state_root(&accounts);
        assert_eq!(root1, root2);
    }

    #[test]
    fn test_state_root_changes_on_mutation() {
        let accounts1 = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let accounts2 = vec![AccountSnapshot {
            id: 0,
            balance: 200,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        assert_ne!(
            compute_state_root(&accounts1),
            compute_state_root(&accounts2)
        );
    }

    #[test]
    fn test_state_root_changes_on_total_deposited_only() {
        let accounts1 = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let accounts2 = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 150,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        assert_ne!(
            compute_state_root(&accounts1),
            compute_state_root(&accounts2)
        );
    }

    #[test]
    fn test_state_root_changes_on_bridge_cursor() {
        let accounts = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let mut bridge = BridgeStateSnapshot::default();
        let before = compute_state_root_with_bridge(&accounts, &bridge);

        bridge.deposit_cursor = 1;
        let after = compute_state_root_with_bridge(&accounts, &bridge);

        assert_ne!(before, after);
    }

    #[test]
    fn test_state_root_changes_on_withdrawal_leaf() {
        let accounts = vec![AccountSnapshot {
            id: 7,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let bridge = BridgeStateSnapshot {
            deposit_cursor: 1,
            deposit_root: [1u8; 32],
            next_withdrawal_id: 3,
            withdrawals: vec![WithdrawalSnapshot {
                withdrawal_id: 2,
                account_id: 7,
                recipient: [2u8; 20],
                token: [3u8; 20],
                amount_token_units: 1_000,
                amount_nanos: 2_000,
                expiry_height: 99,
                nullifier: [4u8; 32],
            }],
        };
        let mut changed = bridge.clone();
        changed.withdrawals[0].amount_nanos += 1;

        assert_ne!(
            compute_state_root_with_bridge(&accounts, &bridge),
            compute_state_root_with_bridge(&accounts, &changed)
        );
    }

    #[test]
    fn test_state_root_bridge_withdrawals_are_order_independent() {
        let accounts = vec![AccountSnapshot {
            id: 7,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let first = WithdrawalSnapshot {
            withdrawal_id: 2,
            account_id: 7,
            recipient: [2u8; 20],
            token: [3u8; 20],
            amount_token_units: 1_000,
            amount_nanos: 2_000,
            expiry_height: 99,
            nullifier: [4u8; 32],
        };
        let second = WithdrawalSnapshot {
            withdrawal_id: 1,
            account_id: 8,
            recipient: [5u8; 20],
            token: [6u8; 20],
            amount_token_units: 3_000,
            amount_nanos: 4_000,
            expiry_height: 100,
            nullifier: [7u8; 32],
        };
        let bridge_a = BridgeStateSnapshot {
            deposit_cursor: 1,
            deposit_root: [1u8; 32],
            next_withdrawal_id: 3,
            withdrawals: vec![first.clone(), second.clone()],
        };
        let bridge_b = BridgeStateSnapshot {
            withdrawals: vec![second, first],
            ..bridge_a.clone()
        };

        assert_eq!(
            compute_state_root_with_bridge(&accounts, &bridge_a),
            compute_state_root_with_bridge(&accounts, &bridge_b)
        );
    }

    #[test]
    fn test_valid_genesis_block() {
        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let state_root = compute_state_root(&post_state);

        let witness = BlockWitness {
            header: genesis_header(state_root),
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state,
            bridge_state: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_block(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_state_root_mismatch() {
        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        let witness = BlockWitness {
            header: genesis_header([0xff; 32]), // wrong root
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state,
            bridge_state: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_block(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::StateRootMismatch));
    }

    #[test]
    fn test_parent_hash_chain() {
        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let state_root = compute_state_root(&post_state);

        let prev_header = genesis_header(state_root);
        let parent_hash = hash_header(&prev_header);

        let header = WitnessBlockHeader {
            height: 2,
            parent_hash,
            state_root,
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 2000,
        };

        let witness = BlockWitness {
            header,
            previous_header: Some(prev_header),
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state,
            bridge_state: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_block(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_height_not_consecutive() {
        let post_state = vec![];
        let state_root = compute_state_root(&post_state);

        let prev_header = genesis_header(state_root);
        let parent_hash = hash_header(&prev_header);

        let header = WitnessBlockHeader {
            height: 5, // Should be 2
            parent_hash,
            state_root,
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 2000,
        };

        let witness = BlockWitness {
            header,
            previous_header: Some(prev_header),
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state,
            bridge_state: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_block(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::HeightNotConsecutive));
    }

    #[test]
    fn test_hash_header_deterministic() {
        let header = WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [1u8; 32],
            order_count: 5,
            fill_count: 3,
            timestamp_ms: 1000,
        };
        assert_eq!(hash_header(&header), hash_header(&header));
    }

    fn position_set_strategy() -> impl Strategy<Value = Vec<(u8, u8, i16)>> {
        prop::collection::btree_map(
            (0u8..6, 0u8..2),
            (-20i16..=20i16).prop_filter("qty must be non-zero", |qty| *qty != 0),
            0..12,
        )
        .prop_map(|map| {
            map.into_iter()
                .map(|((market, outcome), qty)| (market, outcome, qty))
                .collect::<Vec<_>>()
        })
    }

    proptest! {
        #[test]
        fn prop_state_root_invariant_to_position_order(
            balance in -1_000i64..=1_000,
            total_deposited in 0i64..=2_000,
            events_digest in prop::array::uniform32(any::<u8>()),
            positions in position_set_strategy(),
        ) {
            let mut reversed_positions = positions.clone();
            reversed_positions.reverse();

            let account_a = AccountSnapshot {
                id: 7,
                balance,
                total_deposited,
                positions: positions
                    .iter()
                    .map(|(market, outcome, qty)| (MarketId::new(*market as u32), *outcome, *qty as i64))
                    .collect(),
                events_digest,
            };
            let account_b = AccountSnapshot {
                id: 7,
                balance,
                total_deposited,
                positions: reversed_positions
                    .iter()
                    .map(|(market, outcome, qty)| (MarketId::new(*market as u32), *outcome, *qty as i64))
                    .collect(),
                events_digest,
            };

            prop_assert_eq!(
                compute_state_root(&[account_a]),
                compute_state_root(&[account_b]),
            );
        }

        #[test]
        fn prop_state_root_changes_when_balance_changes(
            balance in -1_000i64..=1_000,
            total_deposited in 0i64..=2_000,
            positions in position_set_strategy(),
            events_digest in prop::array::uniform32(any::<u8>()),
        ) {
            let positions: Vec<_> = positions
                .iter()
                .map(|(market, outcome, qty)| (MarketId::new(*market as u32), *outcome, *qty as i64))
                .collect();

            let before = AccountSnapshot {
                id: 0,
                balance,
                total_deposited,
                positions: positions.clone(),
                events_digest,
            };
            let after = AccountSnapshot {
                id: 0,
                balance: balance.saturating_add(1),
                total_deposited,
                positions,
                events_digest,
            };

            prop_assert_ne!(compute_state_root(&[before]), compute_state_root(&[after]));
        }

        #[test]
        fn prop_state_root_changes_when_total_deposited_changes(
            balance in -1_000i64..=1_000,
            total_deposited in 0i64..=2_000,
            positions in position_set_strategy(),
            events_digest in prop::array::uniform32(any::<u8>()),
        ) {
            let positions: Vec<_> = positions
                .iter()
                .map(|(market, outcome, qty)| (MarketId::new(*market as u32), *outcome, *qty as i64))
                .collect();

            let before = AccountSnapshot {
                id: 0,
                balance,
                total_deposited,
                positions: positions.clone(),
                events_digest,
            };
            let after = AccountSnapshot {
                id: 0,
                balance,
                total_deposited: total_deposited.saturating_add(1),
                positions,
                events_digest,
            };

            prop_assert_ne!(compute_state_root(&[before]), compute_state_root(&[after]));
        }

        #[test]
        fn prop_state_root_changes_when_events_digest_changes(
            balance in -1_000i64..=1_000,
            total_deposited in 0i64..=2_000,
            positions in position_set_strategy(),
            seed in any::<u8>(),
        ) {
            let positions: Vec<_> = positions
                .iter()
                .map(|(market, outcome, qty)| (MarketId::new(*market as u32), *outcome, *qty as i64))
                .collect();

            let before_digest = [seed; 32];
            let mut after_digest = before_digest;
            after_digest[0] = after_digest[0].wrapping_add(1);

            let before = AccountSnapshot {
                id: 0,
                balance,
                total_deposited,
                positions: positions.clone(),
                events_digest: before_digest,
            };
            let after = AccountSnapshot {
                id: 0,
                balance,
                total_deposited,
                positions,
                events_digest: after_digest,
            };

            prop_assert_ne!(compute_state_root(&[before]), compute_state_root(&[after]));
        }
    }
}
