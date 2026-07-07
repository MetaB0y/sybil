use std::collections::HashMap;

use matching_engine::{
    ConditionDir, Fill, MarketGroup, MarketId, MmConstraint, MmId, MmSide, Nanos, Order,
    OrderDirection, PriceCondition, Qty,
};
use sha2::{Digest as _, Sha256};

use crate::block::hash_header;
use crate::state_schema;
use crate::types::{
    AccountReservationSnapshot, AccountSnapshot, BlockWitness, BridgeStateSnapshot,
    ChallengeSnapshot, MarketGroupSnapshot, MarketSnapshot, MarketStatusSnapshot,
    OracleSourceSnapshot, RejectionReason, ResolutionProposalSnapshot, ResolutionRecordSnapshot,
    RestingOrderSnapshot, StateSidecarSnapshot, SystemEventWitness, WithdrawalSnapshot,
    WitnessBlockHeader, WitnessOrder, WitnessRejection,
};
use crate::witness_schema;
use crate::{account_keys_digest, empty_account_keys_digest, AccountKeyDigestRecord};

#[test]
fn golden_vectors_pin_header_hash_and_snapshot_encoders() {
    let witness = byte_identity_witness();

    let state_leaves = state_schema::state_root_leaves(&witness.post_state, &witness.state_sidecar);
    let witness_bytes = witness_schema::canonical_witness_bytes(&witness);

    assert_eq!(state_leaves.len(), 11);
    assert_eq!(witness_bytes.len(), 3857);
    assert_eq!(
        hash_header(&witness.header),
        [
            237, 2, 52, 82, 23, 11, 241, 36, 196, 211, 229, 155, 159, 99, 198, 162, 76, 210, 68,
            96, 104, 0, 3, 235, 39, 53, 0, 15, 146, 163, 93, 242,
        ],
    );
    assert_eq!(
        digest_state_leaves(&state_leaves),
        [
            154, 146, 55, 187, 52, 65, 220, 208, 80, 111, 54, 63, 77, 138, 244, 103, 10, 41, 235,
            108, 210, 176, 0, 197, 39, 31, 143, 244, 142, 143, 13, 246,
        ],
    );
    assert_eq!(
        digest_bytes(&witness_bytes),
        [
            208, 47, 137, 188, 23, 4, 226, 81, 108, 122, 196, 17, 54, 40, 0, 71, 110, 232, 179,
            209, 159, 192, 2, 124, 233, 210, 246, 197, 140, 150, 16, 118,
        ],
    );
}

#[test]
fn golden_vectors_pin_account_keys_digest() {
    let raw_key = [
        0x02, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
        0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
        0x11, 0x11, 0x11,
    ];
    let webauthn_key = [
        0x03, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
        0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
        0x22, 0x22, 0x22,
    ];

    assert_eq!(
        empty_account_keys_digest(1001),
        [
            158, 95, 26, 40, 217, 135, 219, 244, 188, 77, 52, 176, 97, 248, 153, 190, 136, 74, 30,
            183, 235, 43, 231, 27, 12, 133, 47, 142, 109, 106, 12, 21,
        ],
    );
    assert_ne!(empty_account_keys_digest(1001), [0u8; 32]);

    assert_eq!(
        account_keys_digest(
            1001,
            [
                AccountKeyDigestRecord {
                    auth_scheme: 1,
                    pubkey_sec1: webauthn_key,
                },
                AccountKeyDigestRecord {
                    auth_scheme: 0,
                    pubkey_sec1: raw_key,
                },
            ],
        ),
        [
            180, 137, 49, 57, 103, 177, 109, 208, 143, 89, 122, 131, 248, 68, 234, 231, 149, 7, 66,
            31, 174, 246, 247, 190, 222, 155, 78, 108, 81, 196, 126, 3,
        ],
    );
}

fn byte_identity_witness() -> BlockWitness {
    let market_a = MarketId::new(3);
    let market_b = MarketId::new(9);

    let accepted_order = fixture_order(42, market_a, market_b, 610_000_000, Some(77));
    let rejected_order = fixture_order(7, market_b, market_a, 455_000_000, None);

    let previous_header = WitnessBlockHeader {
        height: 10,
        parent_hash: [1u8; 32],
        state_root: [2u8; 32],
        events_root: [3u8; 32],
        order_count: 4,
        fill_count: 2,
        timestamp_ms: 1_700_000_000_000,
    };

    let header = WitnessBlockHeader {
        height: 11,
        parent_hash: [4u8; 32],
        state_root: [5u8; 32],
        events_root: [6u8; 32],
        order_count: 2,
        fill_count: 1,
        timestamp_ms: 1_700_000_001_234,
    };

    let mut clearing_prices = HashMap::new();
    clearing_prices.insert(market_b, vec![Nanos(410_000_000), Nanos(590_000_000)]);
    clearing_prices.insert(market_a, vec![Nanos(610_000_000), Nanos(390_000_000)]);

    BlockWitness {
        header,
        previous_header: Some(previous_header),
        orders: vec![WitnessOrder {
            order: accepted_order.clone(),
            account_id: 1001,
            is_mm: false,
        }],
        rejections: vec![WitnessRejection {
            order: rejected_order,
            account_id: 1002,
            reason: RejectionReason::InsufficientBalance {
                required: 12_345,
                available: 6_789,
            },
        }],
        system_events: vec![SystemEventWitness::OrderCancelled {
            account_id: 1001,
            order_id: 41,
            market_ids: vec![market_b, market_a],
            side: OrderDirection::SellNo,
            remaining_quantity: 321,
        }],
        deposit_accumulator: crate::DepositAccumulatorWitness::default(),
        fills: vec![Fill {
            order_id: 42,
            fill_qty: Qty(250),
            fill_price: Nanos(600_000_000),
            account_id: 1001,
        }],
        clearing_prices,
        total_welfare: 12_345,
        minting_cost: -222,
        mm_constraints: vec![MmConstraint::new(MmId::new(12), Nanos(3_000_000_000))
            .with_order(42, MmSide::BuyYes)
            .with_order(7, MmSide::SellNo)],
        market_groups: vec![MarketGroup {
            name: "Weather basket".to_string(),
            markets: vec![market_b, market_a],
        }],
        pre_state: vec![account_snapshot(1002), account_snapshot(1001)],
        post_system_state: vec![account_snapshot(1001), account_snapshot(1002)],
        post_state: vec![account_snapshot(1002), account_snapshot(1001)],
        state_sidecar: state_sidecar(accepted_order),
        pre_state_sidecar: Default::default(),
        resolved_markets: vec![market_b, market_a],
    }
}

fn fixture_order(
    id: u64,
    primary: MarketId,
    secondary: MarketId,
    limit_price: u64,
    expires_at_block: Option<u64>,
) -> Order {
    let mut order = Order::new(id);
    order.markets[0] = primary;
    order.markets[1] = secondary;
    order.num_markets = 2;
    order.num_states = 4;
    order.payoffs[0] = 0;
    order.payoffs[1] = -1;
    order.payoffs[2] = 1;
    order.payoffs[3] = 0;
    order.limit_price = Nanos(limit_price);
    order.max_fill = Qty(500);
    order.condition = Some(PriceCondition {
        market: secondary,
        threshold: Nanos(500_000_000),
        direction: ConditionDir::Above,
    });
    order.expires_at_block = expires_at_block;
    order
}

fn account_snapshot(id: u64) -> AccountSnapshot {
    AccountSnapshot {
        id,
        balance: if id == 1001 { 9_000_000 } else { 7_000_000 },
        total_deposited: if id == 1001 { 10_000_000 } else { 8_000_000 },
        positions: vec![
            (MarketId::new(9), 1, 0),
            (MarketId::new(3), 0, 25),
            (MarketId::new(9), 0, -7),
        ],
        events_digest: [id as u8; 32],
        keys_digest: empty_account_keys_digest(id),
    }
}

fn state_sidecar(resting_order: Order) -> StateSidecarSnapshot {
    let proposal = ResolutionProposalSnapshot {
        id: 88,
        market_id: MarketId::new(3),
        payout_nanos: Nanos(700_000_000),
        source: OracleSourceSnapshot::DataFeed(55),
        proposed_at_ms: 1_700_000_000_100,
        reason: Some("feed quorum".to_string()),
    };
    let challenge = ChallengeSnapshot {
        id: 99,
        challenger: 1002,
        proposal_id: 88,
        bond_amount: Nanos(50_000),
        proposed_payout_nanos: Nanos(300_000_000),
        reason: "disputed source".to_string(),
        challenged_at_ms: 1_700_000_000_200,
    };

    StateSidecarSnapshot {
        bridge: BridgeStateSnapshot {
            deposit_cursor: 14,
            deposit_root: [8u8; 32],
            next_withdrawal_id: 4,
            withdrawals: vec![WithdrawalSnapshot {
                withdrawal_id: 3,
                account_id: 1001,
                recipient: [9u8; 20],
                token: [10u8; 20],
                amount_token_units: 123_000,
                amount_nanos: 456_000,
                expiry_height: 99,
                nullifier: [11u8; 32],
            }],
        },
        markets: vec![
            MarketSnapshot {
                market_id: MarketId::new(9),
                name: "Rain in London".to_string(),
                num_outcomes: 2,
                status: MarketStatusSnapshot::Resolved {
                    record: ResolutionRecordSnapshot {
                        market_id: MarketId::new(9),
                        payout_nanos: Nanos(1_000_000_000),
                        resolved_by: OracleSourceSnapshot::Admin,
                        resolved_at_ms: 1_700_000_000_300,
                        proposal: Some(proposal.clone()),
                        challenge: Some(challenge.clone()),
                    },
                },
                metadata_digest: [12u8; 32],
                resolution_template: "admin_immediate".to_string(),
            },
            MarketSnapshot {
                market_id: MarketId::new(3),
                name: "Wind over 20kt".to_string(),
                num_outcomes: 2,
                status: MarketStatusSnapshot::Challenged {
                    proposal,
                    challenge,
                },
                metadata_digest: [13u8; 32],
                resolution_template: "data_feed".to_string(),
            },
        ],
        market_groups: vec![MarketGroupSnapshot {
            group_id: 5,
            name: "Weather basket".to_string(),
            markets: vec![MarketId::new(9), MarketId::new(3)],
        }],
        resting_orders: vec![RestingOrderSnapshot {
            order: resting_order,
            account_id: 1001,
            created_at: 8,
            expires_at_block: 77,
            reserved_balance: 123_456,
            reserved_positions: vec![
                (MarketId::new(9), 1, 0),
                (MarketId::new(3), 0, 12),
                (MarketId::new(9), 0, -5),
            ],
        }],
        account_reservations: vec![AccountReservationSnapshot {
            account_id: 1001,
            reserved_balance: 123_456,
            reserved_positions: vec![
                (MarketId::new(9), 1, 0),
                (MarketId::new(3), 0, 12),
                (MarketId::new(9), 0, -5),
            ],
        }],
    }
}

fn digest_state_leaves(leaves: &[(Vec<u8>, Vec<u8>)]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for (key, value) in leaves {
        hasher.update((key.len() as u64).to_le_bytes());
        hasher.update(key);
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value);
    }
    hasher.finalize().into()
}

fn digest_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
    hasher.finalize().into()
}
