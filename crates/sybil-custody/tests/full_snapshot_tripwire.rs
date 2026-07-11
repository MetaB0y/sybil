use std::collections::HashMap;

use matching_engine::{MarketId, Nanos, Order, Qty};
use sybil_verifier::commitments::{state_schema, witness_schema};
use sybil_verifier::{
    AccountReservationSnapshot, AccountSnapshot, BlockWitness, BridgeStateSnapshot,
    DepositAccumulatorWitness, MarketGroupSnapshot, MarketSnapshot, MarketStatusSnapshot,
    RestingOrderSnapshot, StateSidecarSnapshot, WithdrawalSnapshot, WitnessBlockHeader,
};

/// SYB-80 section 7 guard rail. If the canonical DA witness ever becomes a
/// delta/openings witness, this test must go red until custody reconstruction
/// gains a separately retained full-snapshot artifact.
#[test]
fn canonical_da_payload_is_a_self_contained_full_state_snapshot() {
    let account = AccountSnapshot {
        id: 7,
        balance: 2_000_000_000,
        total_deposited: 2_000_000_000,
        positions: vec![(MarketId(3), 0, 1_000)],
        events_digest: [7; 32],
        keys_digest: sybil_verifier::empty_account_keys_digest(7),
    };
    let market = MarketSnapshot {
        market_id: MarketId(3),
        name: "Tripwire market".to_string(),
        num_outcomes: 2,
        status: MarketStatusSnapshot::Active,
        metadata_digest: [3; 32],
        resolution_template: "admin_immediate".to_string(),
        last_clearing_prices: vec![Nanos(500_000_000), Nanos(500_000_000)],
    };
    let mut order = Order::new(11);
    order.markets[0] = market.market_id;
    order.num_markets = 1;
    order.num_states = 2;
    order.payoffs[0] = 1;
    order.limit_price = Nanos(500_000_000);
    order.max_fill = Qty(1_000);
    let sidecar = StateSidecarSnapshot {
        bridge: BridgeStateSnapshot {
            next_withdrawal_id: 2,
            withdrawals: vec![WithdrawalSnapshot {
                withdrawal_id: 1,
                account_id: 7,
                recipient: [1; 20],
                token: [2; 20],
                amount_token_units: 1,
                amount_nanos: 1_000,
                expiry_height: 99,
                nullifier: [4; 32],
            }],
            ..BridgeStateSnapshot::default()
        },
        markets: vec![market],
        market_groups: vec![MarketGroupSnapshot {
            group_id: 5,
            name: "Tripwire group".to_string(),
            markets: vec![MarketId(3)],
        }],
        resting_orders: vec![RestingOrderSnapshot {
            order,
            account_id: 7,
            created_at: 1,
            expires_at_block: 10,
            reserved_balance: 500_000_000,
            reserved_positions: vec![],
        }],
        account_reservations: vec![AccountReservationSnapshot {
            account_id: 7,
            reserved_balance: 500_000_000,
            reserved_positions: vec![],
        }],
    };
    let state_root = sybil_verifier::block::compute_state_root_with_sidecar(
        std::slice::from_ref(&account),
        &sidecar,
    );
    let witness = BlockWitness {
        header: WitnessBlockHeader {
            height: 1,
            parent_hash: [0; 32],
            state_root,
            events_root: [0; 32],
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 1,
        },
        previous_header: None,
        genesis_hash: [9; 32],
        orders: vec![],
        rejections: vec![],
        system_events: vec![],
        deposit_accumulator: DepositAccumulatorWitness::default(),
        fills: vec![],
        clearing_prices: HashMap::new(),
        total_welfare: 0,
        minting_cost: 0,
        mm_constraints: vec![],
        market_groups: vec![],
        pre_state: vec![],
        post_system_state: vec![account.clone()],
        post_state: vec![account],
        account_keys: vec![],
        state_sidecar: sidecar,
        pre_state_sidecar: StateSidecarSnapshot::default(),
        resolved_markets: vec![],
    };

    let payload = witness_schema::canonical_witness_bytes(&witness);
    let decoded = witness_schema::decode_canonical_witness_bytes(&payload)
        .expect("canonical full snapshot decodes without external inputs");
    let leaves = state_schema::state_root_leaves(&decoded.post_state, &decoded.state_sidecar);
    for prefix in [
        b"acct/".as_slice(),
        b"acct_resv/".as_slice(),
        b"market/".as_slice(),
        b"market_group/".as_slice(),
        b"order/".as_slice(),
        b"withdrawal/".as_slice(),
        b"sys/".as_slice(),
    ] {
        assert!(
            leaves.iter().any(|(key, _)| key.starts_with(prefix)),
            "full snapshot lost leaf family {}",
            String::from_utf8_lossy(prefix)
        );
    }
    assert_eq!(
        sybil_verifier::block::compute_state_root_with_sidecar(
            &decoded.post_state,
            &decoded.state_sidecar,
        ),
        decoded.header.state_root
    );
}
