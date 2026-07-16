//! Layer 3: Block header integrity verification.
//!
//! Checks state root, events root, parent hash chaining, height, and counts.

use std::num::{NonZeroU16, NonZeroU64, NonZeroUsize};
use std::sync::{OnceLock, mpsc};
use std::thread;

use commonware_codec::RangeCfg;
use commonware_cryptography::Sha256 as QmdbSha256;
use commonware_parallel::Sequential;
use commonware_runtime::buffer::paged::CacheRef;
use commonware_runtime::{Runner as _, deterministic};
use commonware_storage::journal::contiguous::variable::Config as VConfig;
use commonware_storage::merkle::mmr::Family as MmrFamily;
use commonware_storage::merkle::mmr::full::Config as MmrConfig;
use commonware_storage::qmdb::current::VariableConfig;
use commonware_storage::qmdb::current::ordered::variable::Db as OrderedVariableDb;
use commonware_storage::translator::OneCap;

pub use crate::commitments::hash_header;
use crate::event_commitment::compute_events_root;
pub use crate::state_schema::{
    account_leaf_key, account_reservation_leaf_key, market_group_leaf_key, market_leaf_key,
    market_metadata_digest, resting_order_leaf_key, state_root_leaves, withdrawal_leaf_key,
};
use crate::types::{AccountSnapshot, BlockWitness, BridgeStateSnapshot, StateSidecarSnapshot};
use crate::violations::{VerificationResult, VerificationStats, Violation, ViolationKind};

const QMDB_CHUNK_SIZE: usize = 32;
const PAGE_SIZE: u16 = 4096;
const PAGE_CACHE_PAGES: usize = 128;
const ITEMS_PER_BLOB: u64 = 1024;
const WRITE_BUFFER_BYTES: usize = 64 * 1024;
const MAX_KEY_BYTES: usize = 64;
const MAX_VALUE_BYTES: usize = 1 << 20;

type StateRootDb = OrderedVariableDb<
    MmrFamily,
    deterministic::Context,
    Vec<u8>,
    Vec<u8>,
    QmdbSha256,
    OneCap,
    QMDB_CHUNK_SIZE,
    Sequential,
>;

struct StateRootRequest {
    leaves: Vec<(Vec<u8>, Vec<u8>)>,
    respond_to: mpsc::SyncSender<[u8; 32]>,
}

struct StateRootWorker {
    sender: mpsc::Sender<StateRootRequest>,
}

static STATE_ROOT_WORKER: OnceLock<StateRootWorker> = OnceLock::new();

/// Verify block header integrity.
pub fn verify_block(witness: &BlockWitness) -> VerificationResult {
    let mut violations = Vec::new();
    let stats = VerificationStats::default();

    // 1. State root: recompute from post-state and non-account sidecar
    let computed_root =
        compute_state_root_with_sidecar(&witness.post_state, &witness.state_sidecar);
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

    // 2. Events root: recompute from canonical per-block event bytes.
    let computed_events_root = compute_events_root(witness);
    if computed_events_root != witness.header.events_root {
        violations.push(Violation {
            kind: ViolationKind::EventRootMismatch,
            details: format!(
                "Computed events root {:?} != header events root {:?}",
                hex(&computed_events_root),
                hex(&witness.header.events_root),
            ),
        });
    }

    // 3. Parent hash
    match &witness.previous_header {
        Some(prev) => {
            let computed_pre_root =
                compute_state_root_with_sidecar(&witness.pre_state, &witness.pre_state_sidecar);
            if computed_pre_root != prev.state_root {
                violations.push(Violation {
                    kind: ViolationKind::PreStateRootMismatch,
                    details: format!(
                        "Computed pre-state root {:?} != previous header state root {:?}",
                        hex(&computed_pre_root),
                        hex(&prev.state_root),
                    ),
                });
            }

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

            // 4. Height consecutive
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

    // 5. Counts match
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

/// Compute the deterministic state root from account snapshots and an empty
/// sidecar.
///
/// Use [`compute_state_root_with_sidecar`] when verifying real blocks.
pub fn compute_state_root(accounts: &[AccountSnapshot]) -> [u8; 32] {
    compute_state_root_with_sidecar(accounts, &StateSidecarSnapshot::default())
}

/// Compute the typed state root with bridge leaves.
///
/// This convenience wrapper commits account leaves plus bridge leaves, with an
/// otherwise empty sidecar. Use [`compute_state_root_with_sidecar`] for real
/// blocks so order and reservation leaves are included.
pub fn compute_state_root_with_bridge(
    accounts: &[AccountSnapshot],
    bridge: &BridgeStateSnapshot,
) -> [u8; 32] {
    let sidecar = StateSidecarSnapshot {
        bridge: bridge.clone(),
        ..StateSidecarSnapshot::default()
    };
    compute_state_root_with_sidecar(accounts, &sidecar)
}

pub fn compute_state_root_with_sidecar(
    accounts: &[AccountSnapshot],
    sidecar: &StateSidecarSnapshot,
) -> [u8; 32] {
    let leaves = state_root_leaves(accounts, sidecar);
    state_root_from_leaves(&leaves)
}

pub fn state_root_from_leaves(leaves: &[(Vec<u8>, Vec<u8>)]) -> [u8; 32] {
    let mut leaves = leaves.to_vec();
    leaves.sort_by(|(left, _), (right, _)| left.cmp(right));

    let (respond_to, response) = mpsc::sync_channel(1);
    state_root_worker()
        .sender
        .send(StateRootRequest { leaves, respond_to })
        .expect("state root worker should be available");
    response.recv().expect("state root worker should respond")
}

fn state_root_worker() -> &'static StateRootWorker {
    STATE_ROOT_WORKER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<StateRootRequest>();
        thread::Builder::new()
            .name("sybil-state-root-qmdb".to_string())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    let root = state_root_from_sorted_leaves(request.leaves);
                    let _ = request.respond_to.send(root);
                }
            })
            .expect("state root qmdb thread should spawn");

        StateRootWorker { sender }
    })
}

fn state_root_from_sorted_leaves(leaves: Vec<(Vec<u8>, Vec<u8>)>) -> [u8; 32] {
    deterministic::Runner::default().start(|context| async move {
        let mut db = open_state_root_db(context)
            .await
            .expect("state root qmdb should initialize");
        if !leaves.is_empty() {
            let mut batch = db.new_batch();
            for (key, value) in leaves {
                assert!(
                    key.len() <= MAX_KEY_BYTES,
                    "state root key exceeds qmdb key limit"
                );
                assert!(
                    value.len() <= MAX_VALUE_BYTES,
                    "state root value exceeds qmdb value limit"
                );
                batch = batch.write(key, Some(value));
            }
            let merkleized = batch
                .merkleize(&db, None)
                .await
                .expect("state root qmdb batch should merkleize");
            db.apply_batch(merkleized)
                .await
                .expect("state root qmdb batch should apply");
        }
        db.root().0
    })
}

async fn open_state_root_db(context: deterministic::Context) -> Result<StateRootDb, String> {
    let page_cache = CacheRef::from_pooler(
        &context,
        NonZeroU16::new(PAGE_SIZE).unwrap(),
        NonZeroUsize::new(PAGE_CACHE_PAGES).unwrap(),
    );
    let config = VariableConfig {
        merkle_config: MmrConfig {
            journal_partition: "state-root-mmr-journal".to_string(),
            items_per_blob: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
            metadata_partition: "state-root-mmr-metadata".to_string(),
            strategy: Sequential,
            page_cache: page_cache.clone(),
        },
        journal_config: VConfig {
            partition: "state-root-log".to_string(),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
            compression: None,
            codec_config: (
                (RangeCfg::from(0..=MAX_KEY_BYTES), ()),
                (RangeCfg::from(0..=MAX_VALUE_BYTES), ()),
            ),
            items_per_section: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
            page_cache,
        },
        grafted_metadata_partition: "state-root-grafted-mmr-metadata".to_string(),
        translator: OneCap,
    };

    StateRootDb::init(context, config)
        .await
        .map_err(|error| format!("failed to initialize state root qmdb: {error}"))
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
    use crate::event_commitment::empty_events_root;
    use crate::types::{
        AccountReservationSnapshot, MarketGroupSnapshot, MarketSnapshot, MarketStatusSnapshot,
        OracleSourceSnapshot, ResolutionRecordSnapshot, RestingOrderSnapshot, WithdrawalSnapshot,
        WitnessBlockHeader,
    };
    use matching_engine::{MarketId, Nanos, Qty};
    use proptest::prelude::*;
    use std::collections::HashMap;

    fn genesis_header(state_root: [u8; 32]) -> WitnessBlockHeader {
        WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root,
            events_root: empty_events_root(),
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
                keys_digest: crate::empty_account_keys_digest(0),
                last_trading_nonce: 0,
            },
            AccountSnapshot {
                id: 1,
                balance: 200,
                total_deposited: 200,
                positions: vec![(MarketId::new(0), 1, 5)],
                events_digest: [0u8; 32],
                keys_digest: crate::empty_account_keys_digest(1),
                last_trading_nonce: 0,
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
            keys_digest: crate::empty_account_keys_digest(0),
            last_trading_nonce: 0,
        }];
        let accounts2 = vec![AccountSnapshot {
            id: 0,
            balance: 200,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
            last_trading_nonce: 0,
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
            keys_digest: crate::empty_account_keys_digest(0),
            last_trading_nonce: 0,
        }];
        let accounts2 = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 150,
            positions: vec![],
            events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
            last_trading_nonce: 0,
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
            keys_digest: crate::empty_account_keys_digest(0),
            last_trading_nonce: 0,
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
            keys_digest: crate::empty_account_keys_digest(7),
            last_trading_nonce: 0,
        }];
        let bridge = BridgeStateSnapshot {
            deposit_cursor: 1,
            deposit_root: [1u8; 32],
            observed_l1_height: 0,
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
            quarantine: vec![],
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
            keys_digest: crate::empty_account_keys_digest(7),
            last_trading_nonce: 0,
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
            observed_l1_height: 0,
            next_withdrawal_id: 3,
            withdrawals: vec![first.clone(), second.clone()],
            quarantine: vec![],
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
    fn test_state_root_changes_on_resting_order_leaf() {
        let accounts = vec![AccountSnapshot {
            id: 7,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(7),
            last_trading_nonce: 0,
        }];
        let mut order = matching_engine::Order::new(42);
        order.markets[0] = MarketId::new(1);
        order.num_markets = 1;
        order.num_states = 2;
        order.payoffs[0] = 1;
        order.limit_price = Nanos(500_000_000);
        order.max_fill = Qty(3);

        let sidecar = StateSidecarSnapshot {
            resting_orders: vec![RestingOrderSnapshot {
                order,
                account_id: 7,
                created_at: 3,
                expires_at_block: 10,
                reserved_balance: 1_500_000_000,
                reserved_positions: vec![],
            }],
            account_reservations: vec![AccountReservationSnapshot {
                account_id: 7,
                reserved_balance: 1_500_000_000,
                reserved_positions: vec![],
            }],
            ..StateSidecarSnapshot::default()
        };

        assert_ne!(
            compute_state_root(&accounts),
            compute_state_root_with_sidecar(&accounts, &sidecar)
        );
    }

    #[test]
    fn test_state_root_order_book_leaves_are_order_independent() {
        let accounts = vec![];
        let mut first_order = matching_engine::Order::new(2);
        first_order.limit_price = Nanos(500_000_000);
        first_order.max_fill = Qty(2);
        let mut second_order = matching_engine::Order::new(1);
        second_order.limit_price = Nanos(600_000_000);
        second_order.max_fill = Qty(1);
        let first = RestingOrderSnapshot {
            order: first_order,
            account_id: 7,
            created_at: 3,
            expires_at_block: 10,
            reserved_balance: 1_000_000_000,
            reserved_positions: vec![],
        };
        let second = RestingOrderSnapshot {
            order: second_order,
            account_id: 8,
            created_at: 4,
            expires_at_block: 11,
            reserved_balance: 600_000_000,
            reserved_positions: vec![],
        };
        let reservation_a = AccountReservationSnapshot {
            account_id: 8,
            reserved_balance: 600_000_000,
            reserved_positions: vec![],
        };
        let reservation_b = AccountReservationSnapshot {
            account_id: 7,
            reserved_balance: 1_000_000_000,
            reserved_positions: vec![],
        };
        let sidecar_a = StateSidecarSnapshot {
            resting_orders: vec![first.clone(), second.clone()],
            account_reservations: vec![reservation_a.clone(), reservation_b.clone()],
            ..StateSidecarSnapshot::default()
        };
        let sidecar_b = StateSidecarSnapshot {
            resting_orders: vec![second, first],
            account_reservations: vec![reservation_b, reservation_a],
            ..StateSidecarSnapshot::default()
        };

        assert_eq!(
            compute_state_root_with_sidecar(&accounts, &sidecar_a),
            compute_state_root_with_sidecar(&accounts, &sidecar_b)
        );
    }

    #[test]
    fn test_state_root_changes_on_market_leaf() {
        let accounts = vec![];
        let market = MarketSnapshot {
            market_id: MarketId::new(1),
            name: "Will it rain?".to_string(),
            num_outcomes: 2,
            status: MarketStatusSnapshot::Active,
            metadata_digest: [1u8; 32],
            resolution_template: "admin_immediate".to_string(),
            last_clearing_prices: vec![],
        };
        let mut resolved = market.clone();
        resolved.status = MarketStatusSnapshot::Resolved {
            record: ResolutionRecordSnapshot {
                payout_nanos: Nanos(1_000_000_000),
                resolved_by: OracleSourceSnapshot::Admin,
                resolved_at_ms: 42,
            },
        };

        let before = StateSidecarSnapshot {
            markets: vec![market],
            ..StateSidecarSnapshot::default()
        };
        let after = StateSidecarSnapshot {
            markets: vec![resolved],
            ..StateSidecarSnapshot::default()
        };

        assert_ne!(
            compute_state_root_with_sidecar(&accounts, &before),
            compute_state_root_with_sidecar(&accounts, &after)
        );

        let mut repriced = before.clone();
        repriced.markets[0].last_clearing_prices = vec![Nanos(600_000_000), Nanos(400_000_000)];
        assert_ne!(
            compute_state_root_with_sidecar(&accounts, &before),
            compute_state_root_with_sidecar(&accounts, &repriced)
        );
    }

    #[test]
    fn test_state_root_market_leaves_are_order_independent() {
        let accounts = vec![];
        let first_market = MarketSnapshot {
            market_id: MarketId::new(2),
            name: "B".to_string(),
            num_outcomes: 2,
            status: MarketStatusSnapshot::Active,
            metadata_digest: [2u8; 32],
            resolution_template: "admin_immediate".to_string(),
            last_clearing_prices: vec![],
        };
        let second_market = MarketSnapshot {
            market_id: MarketId::new(1),
            name: "A".to_string(),
            num_outcomes: 2,
            status: MarketStatusSnapshot::Active,
            metadata_digest: [1u8; 32],
            resolution_template: "admin_immediate".to_string(),
            last_clearing_prices: vec![],
        };
        let first_group = MarketGroupSnapshot {
            group_id: 1,
            name: "Group B".to_string(),
            markets: vec![MarketId::new(2), MarketId::new(1)],
        };
        let second_group = MarketGroupSnapshot {
            group_id: 0,
            name: "Group A".to_string(),
            markets: vec![MarketId::new(3), MarketId::new(1)],
        };
        let sidecar_a = StateSidecarSnapshot {
            markets: vec![first_market.clone(), second_market.clone()],
            market_groups: vec![first_group.clone(), second_group.clone()],
            ..StateSidecarSnapshot::default()
        };
        let sidecar_b = StateSidecarSnapshot {
            markets: vec![second_market, first_market],
            market_groups: vec![second_group, first_group],
            ..StateSidecarSnapshot::default()
        };

        assert_eq!(
            compute_state_root_with_sidecar(&accounts, &sidecar_a),
            compute_state_root_with_sidecar(&accounts, &sidecar_b)
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
            keys_digest: crate::empty_account_keys_digest(0),
            last_trading_nonce: 0,
        }];
        let state_root = compute_state_root(&post_state);

        let witness = BlockWitness {
            header: genesis_header(state_root),
            previous_header: None,
            genesis_hash: [0u8; 32],
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: crate::DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: post_state.clone(),
            post_system_state: vec![],
            post_state,
            account_keys: vec![],
            state_sidecar: Default::default(),

            pre_state_sidecar: Default::default(),

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
            keys_digest: crate::empty_account_keys_digest(0),
            last_trading_nonce: 0,
        }];

        let witness = BlockWitness {
            header: genesis_header([0xff; 32]), // wrong root
            previous_header: None,
            genesis_hash: [0u8; 32],
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: crate::DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: post_state.clone(),
            post_system_state: vec![],
            post_state,
            account_keys: vec![],
            state_sidecar: Default::default(),

            pre_state_sidecar: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_block(&witness);
        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::StateRootMismatch)
        );
    }

    #[test]
    fn test_events_root_mismatch() {
        let post_state = vec![];
        let state_root = compute_state_root(&post_state);
        let witness = BlockWitness {
            header: WitnessBlockHeader {
                height: 1,
                parent_hash: [0u8; 32],
                state_root,
                events_root: [0xff; 32],
                order_count: 0,
                fill_count: 0,
                timestamp_ms: 1000,
            },
            previous_header: None,
            genesis_hash: [0u8; 32],
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: crate::DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state,
            account_keys: vec![],
            state_sidecar: Default::default(),
            pre_state_sidecar: Default::default(),
            resolved_markets: vec![],
        };

        let result = verify_block(&witness);
        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::EventRootMismatch)
        );
    }

    #[test]
    fn test_parent_hash_chain() {
        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
            last_trading_nonce: 0,
        }];
        let state_root = compute_state_root(&post_state);

        let prev_header = genesis_header(state_root);
        let parent_hash = hash_header(&prev_header);

        let header = WitnessBlockHeader {
            height: 2,
            parent_hash,
            state_root,
            events_root: empty_events_root(),
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 2000,
        };

        let witness = BlockWitness {
            header,
            previous_header: Some(prev_header),
            genesis_hash: [0u8; 32],
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: crate::DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: post_state.clone(),
            post_system_state: vec![],
            post_state,
            account_keys: vec![],
            state_sidecar: Default::default(),

            pre_state_sidecar: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_block(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_pre_state_sidecar_authenticated_against_previous_header() {
        let pre_state = vec![AccountSnapshot {
            id: 9,
            balance: 500,
            total_deposited: 500,
            positions: vec![],
            events_digest: [9u8; 32],
            keys_digest: crate::empty_account_keys_digest(9),
            last_trading_nonce: 0,
        }];
        let pre_state_sidecar = StateSidecarSnapshot {
            markets: vec![MarketSnapshot {
                market_id: MarketId::new(7),
                name: "Authenticated".to_string(),
                num_outcomes: 2,
                status: MarketStatusSnapshot::Active,
                metadata_digest: [7u8; 32],
                resolution_template: "admin".to_string(),
                last_clearing_prices: vec![],
            }],
            ..StateSidecarSnapshot::default()
        };
        let post_state = pre_state.clone();
        let state_sidecar = pre_state_sidecar.clone();
        let prev_header = genesis_header(compute_state_root_with_sidecar(
            &pre_state,
            &pre_state_sidecar,
        ));
        let header = WitnessBlockHeader {
            height: 2,
            parent_hash: hash_header(&prev_header),
            state_root: compute_state_root_with_sidecar(&post_state, &state_sidecar),
            events_root: empty_events_root(),
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 2000,
        };
        let mut witness = BlockWitness {
            header,
            previous_header: Some(prev_header),
            genesis_hash: [0u8; 32],
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: crate::DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state,
            post_system_state: post_state.clone(),
            post_state,
            account_keys: vec![],
            state_sidecar,
            pre_state_sidecar,
            resolved_markets: vec![],
        };

        let result = verify_block(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);

        witness.pre_state_sidecar.markets.clear();
        let result = verify_block(&witness);
        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::PreStateRootMismatch)
        );
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
            events_root: empty_events_root(),
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 2000,
        };

        let witness = BlockWitness {
            header,
            previous_header: Some(prev_header),
            genesis_hash: [0u8; 32],
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: crate::DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state,
            account_keys: vec![],
            state_sidecar: Default::default(),

            pre_state_sidecar: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_block(&witness);
        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::HeightNotConsecutive)
        );
    }

    #[test]
    fn test_hash_header_deterministic() {
        let header = WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [1u8; 32],
            events_root: [2u8; 32],
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
                keys_digest: crate::empty_account_keys_digest(7),
                last_trading_nonce: 0,
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
                keys_digest: crate::empty_account_keys_digest(7),
                last_trading_nonce: 0,
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
                keys_digest: crate::empty_account_keys_digest(0),
                last_trading_nonce: 0,
            };
            let after = AccountSnapshot {
                id: 0,
                balance: balance.saturating_add(1),
                total_deposited,
                positions,
                events_digest,
                keys_digest: crate::empty_account_keys_digest(0),
                last_trading_nonce: 0,
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
                keys_digest: crate::empty_account_keys_digest(0),
                last_trading_nonce: 0,
            };
            let after = AccountSnapshot {
                id: 0,
                balance,
                total_deposited: total_deposited.saturating_add(1),
                positions,
                events_digest,
                keys_digest: crate::empty_account_keys_digest(0),
                last_trading_nonce: 0,
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
                keys_digest: crate::empty_account_keys_digest(0),
                last_trading_nonce: 0,
            };
            let after = AccountSnapshot {
                id: 0,
                balance,
                total_deposited,
                positions,
                events_digest: after_digest,
                keys_digest: crate::empty_account_keys_digest(0),
                last_trading_nonce: 0,
            };

            prop_assert_ne!(compute_state_root(&[before]), compute_state_root(&[after]));
        }

        #[test]
        fn prop_state_root_changes_when_keys_digest_changes(
            balance in -1_000i64..=1_000,
            total_deposited in 0i64..=2_000,
            positions in position_set_strategy(),
            events_digest in prop::array::uniform32(any::<u8>()),
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
                events_digest,
                keys_digest: before_digest,
                last_trading_nonce: 0,
            };
            let after = AccountSnapshot {
                id: 0,
                balance,
                total_deposited,
                positions,
                events_digest,
                keys_digest: after_digest,
                last_trading_nonce: 0,
            };

            prop_assert_ne!(compute_state_root(&[before]), compute_state_root(&[after]));
        }
    }
}
