//! Layer 3: Block header integrity verification.
//!
//! Checks state root, parent hash chaining, height, and counts.

use crate::types::{AccountSnapshot, BlockWitness, WitnessBlockHeader};
use crate::violations::{VerificationResult, VerificationStats, Violation, ViolationKind};

/// Verify block header integrity.
pub fn verify_block(witness: &BlockWitness) -> VerificationResult {
    let mut violations = Vec::new();
    let stats = VerificationStats::default();

    // 1. State root: recompute from post-state
    let computed_root = compute_state_root(&witness.post_state);
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

/// Compute a deterministic state root from account snapshots.
///
/// Must produce the exact same hash as `matching-sequencer`'s
/// `compute_state_root`. Canonical encoding: sorted by account id,
/// each account encodes balance, total_deposited, then sorted
/// (market, outcome) -> qty.
pub fn compute_state_root(accounts: &[AccountSnapshot]) -> [u8; 32] {
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

        // Positions (already sorted by (market, outcome) in AccountSnapshot)
        for &(market, outcome, qty) in &account.positions {
            hasher.update(&market.0.to_le_bytes());
            hasher.update(&[outcome]);
            hasher.update(&qty.to_le_bytes());
        }

        hasher.update(&account.events_digest);
    }

    *hasher.finalize().as_bytes()
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
}
