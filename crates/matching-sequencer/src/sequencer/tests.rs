use super::testutil::*;
use super::*;
use crate::account::AccountStore;
use crate::crypto::sign_attestation;
use crate::error::RejectionReason;
use crate::market_info::ResolutionConfig;
use crate::order_book::RestingOrder;
use crate::store::{AcknowledgedWrite, SequencedAcknowledgedWrite};
use crate::validation::{validate_order, validate_order_with_reservation};
use matching_engine::{
    MarketId, MarketSet, MmId, NANOS_PER_DOLLAR, Nanos, Order, Problem, Qty, notional_nanos,
    outcome_buy, outcome_sell, shares_to_qty,
};
use matching_scenarios::{ScenarioConfig, generate_scenario};
use proptest::prelude::*;
use sybil_oracle::{ResolutionAttestation, ResolutionPolicy, ResolutionTemplate, TemplateId};

fn setup() -> (MarketSet, MarketId) {
    let mut markets = MarketSet::new();
    let m0 = markets.add_binary("Test");
    (markets, m0)
}

fn make_sequencer(balance: i64) -> (BlockSequencer, AccountId) {
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(balance);
    let markets = MarketSet::new();
    // Most kernel tests exercise settlement/replay semantics with deliberately
    // tiny fixture quantities. Admission-floor behavior has dedicated tests
    // below, so keep that product policy out of unrelated kernel assertions.
    let config = SequencerConfig {
        min_resting_order_notional_nanos: 0,
        ..SequencerConfig::default()
    };
    (
        BlockSequencer::with_default_solver(accounts, markets, vec![], config),
        aid,
    )
}

fn fresh_public_key() -> crate::crypto::PublicKey {
    let signing_key =
        <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
            &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
        );
    crate::crypto::PublicKey(*signing_key.verifying_key())
}

#[test]
fn api_key_churn_is_lifetime_bounded_and_account_stays_well_below_qmdb_limit() {
    let (mut seq, aid) = make_sequencer(0);
    for index in 0..crate::account::MAX_API_KEYS_PER_ACCOUNT {
        let mut hash = [0u8; 32];
        hash[..8].copy_from_slice(&(index as u64).to_le_bytes());
        let id = seq
            .create_api_key(
                aid,
                hash,
                Some("x".repeat(crate::account::MAX_API_KEY_LABEL_BYTES)),
                index as u64,
            )
            .unwrap();
        seq.revoke_api_key(aid, id, 10_000 + index as u64).unwrap();
    }

    let account = seq.accounts.get(aid).unwrap();
    assert_eq!(
        account.api_keys.len(),
        crate::account::MAX_API_KEYS_PER_ACCOUNT
    );
    assert!(account.api_keys.iter().all(|key| !key.is_active()));
    let serialized = rmp_serde::to_vec(account).unwrap();
    assert!(serialized.len() <= crate::account::MAX_SERIALIZED_ACCOUNT_BYTES);
    assert!(serialized.len() < (1 << 20));

    let error = seq
        .create_api_key(aid, [0xff; 32], Some("after-churn".to_string()), 99_999)
        .unwrap_err();
    assert!(matches!(error, SequencerError::ApiKeyLimit { .. }));
}

#[test]
fn api_key_label_and_aggregate_account_byte_budgets_are_preflighted() {
    let (mut seq, aid) = make_sequencer(0);
    let too_long = "x".repeat(crate::account::MAX_API_KEY_LABEL_BYTES + 1);
    assert!(matches!(
        seq.can_create_api_key(aid, [1; 32], Some(&too_long), 1),
        Err(SequencerError::ApiKeyLabelTooLong { .. })
    ));
    assert!(seq.accounts.get(aid).unwrap().api_keys.is_empty());

    seq.accounts.get_mut(aid).unwrap().profile.avatar_seed =
        Some("x".repeat(crate::account::MAX_SERIALIZED_ACCOUNT_BYTES));
    assert!(matches!(
        seq.can_create_api_key(aid, [2; 32], None, 2),
        Err(SequencerError::AccountStorageBudgetExceeded { .. })
    ));
    assert!(seq.accounts.get(aid).unwrap().api_keys.is_empty());
}

fn q(shares: u64) -> u64 {
    shares_to_qty(shares).0
}

fn qi(shares: u64) -> i64 {
    q(shares) as i64
}

fn fund_scenario_account(accounts: &mut AccountStore, markets: &MarketSet) -> AccountId {
    let account_id = accounts.create_account(1_000_000_000_000_000);
    let account = accounts.get_mut(account_id).expect("account exists");
    for market in markets.iter() {
        account.positions.insert((market.id, 0), 1_000_000_000);
        account.positions.insert((market.id, 1), 1_000_000_000);
    }
    account_id
}

fn sequencer_from_scenario_problem(
    problem: Problem,
    direct_admits: usize,
) -> (BlockSequencer, Vec<OrderSubmission>, Vec<AccountId>) {
    let config = SequencerConfig {
        order_ttl_blocks: 2,
        debug_verify_full: true,
        min_resting_order_notional_nanos: 0,
        ..SequencerConfig::default()
    };
    let mut accounts = AccountStore::new();
    let mut account_ids = Vec::new();
    let mut submissions = Vec::new();
    let mut mm_order_ids = HashSet::new();
    for constraint in &problem.mm_constraints {
        mm_order_ids.extend(constraint.order_ids.iter().copied());
    }
    let orders_by_id: HashMap<u64, Order> = problem
        .orders
        .iter()
        .map(|order| (order.id, order.clone()))
        .collect();

    for constraint in &problem.mm_constraints {
        let account_id = fund_scenario_account(&mut accounts, &problem.markets);
        account_ids.push(account_id);
        let orders = constraint
            .order_ids
            .iter()
            .filter_map(|order_id| orders_by_id.get(order_id).cloned())
            .collect();
        submissions.push(OrderSubmission {
            account_id,
            orders,
            mm_constraint: Some(constraint.clone()),
        });
    }

    for order in problem.orders {
        if mm_order_ids.contains(&order.id) {
            continue;
        }
        let account_id = fund_scenario_account(&mut accounts, &problem.markets);
        account_ids.push(account_id);
        submissions.push(OrderSubmission {
            account_id,
            orders: vec![order],
            mm_constraint: None,
        });
    }

    let mut sequencer = BlockSequencer::with_default_solver(
        accounts,
        problem.markets,
        problem.market_groups,
        config,
    );

    let mut remaining = Vec::new();
    let mut direct_count = 0usize;
    for submission in submissions {
        if direct_count < direct_admits && submission.mm_constraint.is_none() {
            match sequencer.try_admit_direct(submission, 10 + direct_count as u64) {
                AdmitOutcome::Admitted { .. } => {
                    direct_count += 1;
                }
                other => panic!("scenario direct admit should succeed, got {other:?}"),
            }
        } else {
            remaining.push(submission);
        }
    }

    (sequencer, remaining, account_ids)
}

fn restored_analytics_from(seq: &BlockSequencer) -> crate::store::AnalyticsRestoredState {
    crate::store::AnalyticsRestoredState {
        last_clearing_prices: seq.analytics().last_clearing_prices().clone(),
        market_volumes: seq.analytics().market_volumes().clone(),
        trader_tracker: Default::default(),
        rolling_volume: Default::default(),
        rolling_price_anchors: Default::default(),
        liquidity_tracker: Default::default(),
        order_stats_tracker: Default::default(),
        welfare_tracker: Default::default(),
        first_deposit_ms: HashMap::new(),
        fill_total_counts: HashMap::new(),
        cost_basis_tracker: Default::default(),
        next_product_event_seq: 0,
    }
}

fn restored_state_with_resting_orders(
    seq: &BlockSequencer,
    markets: MarketSet,
    resting_orders: Vec<RestingOrder>,
) -> RestoredState {
    RestoredState {
        accounts: seq.accounts.clone(),
        markets,
        market_groups: seq.market_groups().to_vec(),
        market_statuses: HashMap::new(),
        market_metadata: HashMap::new(),
        height: seq.height(),
        last_header: seq.last_header().cloned(),
        genesis_hash: seq.genesis_hash().unwrap_or([0; 32]),
        next_order_id: seq.next_order_id(),
        pubkey_registry: seq.pubkey_registry().clone(),
        resting_orders,
        data_feeds: Vec::new(),
        resolution_templates: Vec::new(),
        acknowledged_writes: Vec::new(),
        bridge_state: seq.bridge_state().clone(),
        analytics: restored_analytics_from(seq),
    }
}

fn sequenced_acknowledged_writes(
    writes: Vec<AcknowledgedWrite>,
) -> Vec<SequencedAcknowledgedWrite> {
    writes
        .into_iter()
        .enumerate()
        .map(|(sequence, write)| SequencedAcknowledgedWrite {
            sequence: sequence as u64,
            write,
        })
        .collect()
}

fn total_balance(seq: &BlockSequencer) -> i64 {
    seq.accounts
        .iter()
        .map(|(_, account)| account.balance)
        .sum()
}

type PositionState = (MarketId, u8, i64);
type AccountState = (u64, i64, Vec<PositionState>);

fn account_state_for_assertions(accounts: &AccountStore) -> Vec<AccountState> {
    let mut rows: Vec<_> = accounts
        .iter()
        .map(|(account_id, account)| {
            let mut positions: Vec<_> = account
                .positions
                .iter()
                .map(|(&(market, outcome), &qty)| (market, outcome, qty))
                .collect();
            positions.sort_by_key(|&(market, outcome, _)| (market.0, outcome));
            (account_id.0, account.balance, positions)
        })
        .collect();
    rows.sort_by_key(|(account_id, _, _)| *account_id);
    rows
}

fn sequencer_with_single_market(balance: i64) -> (BlockSequencer, AccountId, MarketId) {
    let (markets, market) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(balance);
    (
        BlockSequencer::with_default_solver(accounts, markets, vec![], SequencerConfig::default()),
        aid,
        market,
    )
}

fn expect_invariant_failure(result: Result<BlockProduction, SequencerError>) -> SequencerError {
    match result {
        Err(err @ SequencerError::BlockInvariantFailure { .. }) => err,
        Err(other) => panic!("expected block invariant failure, got {other}"),
        Ok(_) => panic!("expected block invariant failure, got committed block"),
    }
}

fn raw_single_market_order(
    market: MarketId,
    payoffs: [i8; 2],
    limit_price: u64,
    max_fill: u64,
) -> Order {
    let mut order = Order::new(0);
    order.markets[0] = market;
    order.num_markets = 1;
    order.num_states = 2;
    order.payoffs[0] = payoffs[0];
    order.payoffs[1] = payoffs[1];
    order.limit_price = Nanos(limit_price);
    order.max_fill = Qty(max_fill);
    order
}

#[test]
fn commit_rejects_prepared_block_when_verify_full_invalid_and_retains_pre_state() {
    let (mut seq, _aid, _market) = sequencer_with_single_market(1_000);
    let pre_height = seq.height();
    let pre_accounts = account_state_for_assertions(&seq.accounts);
    let pre_header = seq.last_header.clone();

    let mut prepared = seq.prepare_block(vec![], 1_000).unwrap();
    prepared.production.witness.header.state_root[0] ^= 0xff;

    let err = expect_invariant_failure(seq.commit_prepared_block(prepared));
    assert!(matches!(
        err,
        SequencerError::BlockInvariantFailure { failures, .. }
            if failures
                .iter()
                .any(|failure| matches!(failure, BlockInvariantFailure::FullVerificationFailed { .. }))
    ));
    assert_eq!(seq.height(), pre_height);
    assert_eq!(account_state_for_assertions(&seq.accounts), pre_accounts);
    assert_eq!(seq.last_header, pre_header);
}

#[test]
fn commit_rejects_prepared_block_with_position_imbalance_and_retains_pre_state() {
    let (mut seq, aid, market) = sequencer_with_single_market(1_000);
    let pre_height = seq.height();
    let pre_accounts = account_state_for_assertions(&seq.accounts);
    let pre_header = seq.last_header.clone();

    let mut prepared = seq.prepare_block(vec![], 1_000).unwrap();
    prepared
        .next_sequencer
        .accounts
        .get_mut(aid)
        .unwrap()
        .positions
        .insert((market, 0), 1);

    let err = expect_invariant_failure(seq.commit_prepared_block(prepared));
    assert!(matches!(
        err,
        SequencerError::BlockInvariantFailure { failures, .. }
            if failures.iter().any(|failure| {
                matches!(
                    failure,
                    BlockInvariantFailure::PositionImbalance { market_id, .. }
                        if *market_id == market
                )
            })
    ));
    assert_eq!(seq.height(), pre_height);
    assert_eq!(account_state_for_assertions(&seq.accounts), pre_accounts);
    assert_eq!(seq.last_header, pre_header);
}

#[test]
fn commit_rejects_prepared_block_with_negative_non_mint_balance_and_retains_pre_state() {
    let (mut seq, aid, _market) = sequencer_with_single_market(1_000);
    let pre_height = seq.height();
    let pre_accounts = account_state_for_assertions(&seq.accounts);
    let pre_header = seq.last_header.clone();

    let mut prepared = seq.prepare_block(vec![], 1_000).unwrap();
    prepared
        .next_sequencer
        .accounts
        .get_mut(aid)
        .unwrap()
        .balance = -1;

    let err = expect_invariant_failure(seq.commit_prepared_block(prepared));
    assert!(matches!(
        err,
        SequencerError::BlockInvariantFailure { failures, .. }
            if failures.iter().any(|failure| {
                matches!(
                    failure,
                    BlockInvariantFailure::NegativeBalance { account_id, balance }
                        if *account_id == aid && *balance == -1
                )
            })
    ));
    assert_eq!(seq.height(), pre_height);
    assert_eq!(account_state_for_assertions(&seq.accounts), pre_accounts);
    assert_eq!(seq.last_header, pre_header);
}

fn eth_address(byte: u8) -> [u8; 20] {
    [byte; 20]
}

fn l1_deposit(account_id: AccountId, deposit_id: u64, amount_token_units: u64) -> L1Deposit {
    let mut deposit = L1Deposit {
        deposit_id,
        account_id: Some(account_id),
        chain_id: 1,
        vault_address: eth_address(0x10),
        token_address: eth_address(0x20),
        sender: eth_address(0x30),
        sybil_account_key: account_key(account_id),
        amount_token_units,
        deposit_root: [0u8; 32],
    };
    deposit.deposit_root = crate::bridge::deposit_log_root(&[deposit.clone()]);
    deposit
}

fn next_l1_deposit(
    seq: &BlockSequencer,
    account_id: AccountId,
    amount_token_units: u64,
) -> L1Deposit {
    let mut deposit = L1Deposit {
        deposit_id: seq.bridge_state().deposit_cursor + 1,
        account_id: Some(account_id),
        chain_id: 1,
        vault_address: eth_address(0x10),
        token_address: eth_address(0x20),
        sender: eth_address(0x30),
        sybil_account_key: account_key(account_id),
        amount_token_units,
        deposit_root: [0u8; 32],
    };
    let mut frontier = seq.bridge_state().deposit_frontier;
    deposit.deposit_root = crate::bridge::append_deposit_frontier(
        &mut frontier,
        seq.bridge_state().deposit_cursor,
        &deposit,
    )
    .expect("test deposit fits in frontier");
    deposit
}

#[test]
fn bridge_deposit_and_withdrawal_emit_block_sidecar() {
    let (mut seq, aid) = make_sequencer(0);

    let DepositDisposition::Credited(account) =
        seq.ingest_l1_deposit(l1_deposit(aid, 1, 10_000)).unwrap()
    else {
        panic!("resolved deposit must be credited");
    };
    assert_eq!(account.balance, 10_000_000);

    let withdrawal = seq
        .request_bridge_withdrawal(BridgeWithdrawalRequest {
            account_id: aid,
            chain_id: 1,
            vault_address: eth_address(0x10),
            recipient: eth_address(0x40),
            token_address: eth_address(0x20),
            amount_token_units: 4_000,
            expiry_height: 10,
        })
        .unwrap();
    assert_eq!(withdrawal.amount_nanos, 4_000_000);

    let block = seq.produce_block(vec![], 1_000).block;
    assert_eq!(block.bridge.deposit_count, 1);
    assert_eq!(
        block.bridge.deposit_root,
        l1_deposit(aid, 1, 10_000).deposit_root
    );
    assert_eq!(block.bridge.consumed_deposits.len(), 1);
    assert_eq!(block.bridge.withdrawal_leaves, vec![withdrawal]);
    assert_eq!(seq.accounts.get(aid).unwrap().balance, 6_000_000);
}

#[test]
fn bridge_withdrawal_l1_event_replay_is_idempotent() {
    let (mut seq, aid) = make_sequencer(10_000_000);
    let withdrawal = seq
        .request_bridge_withdrawal(BridgeWithdrawalRequest {
            account_id: aid,
            chain_id: 1,
            vault_address: eth_address(0x10),
            recipient: eth_address(0x40),
            token_address: eth_address(0x20),
            amount_token_units: 4_000,
            expiry_height: 10,
        })
        .unwrap();
    let balance_after_request = seq.accounts.get(aid).unwrap().balance;
    let total_balance_after_request = seq.accounts.total_balance();
    let event = BridgeWithdrawalL1Event {
        nullifier: withdrawal.nullifier,
        status: L1WithdrawalStatus::Queued,
        event_at_unix: 1_700_000_000,
        executable_at_unix: Some(1_700_086_400),
        tx_hash: Some([0xAB; 32]),
        l1_block_height: 5,
    };

    let first_leaf = seq
        .apply_bridge_withdrawal_l1_event(event.clone())
        .unwrap()
        .unwrap();
    let balance_after_first_apply = seq.accounts.get(aid).unwrap().balance;
    let second_leaf = seq
        .apply_bridge_withdrawal_l1_event(event)
        .unwrap()
        .unwrap();

    assert_eq!(first_leaf.l1_status, L1WithdrawalStatus::Queued);
    assert_eq!(first_leaf.l1_requested_at_unix, Some(1_700_000_000));
    assert_eq!(first_leaf.l1_executable_at_unix, Some(1_700_086_400));
    assert_eq!(first_leaf.l1_tx_hash, Some([0xAB; 32]));
    assert_eq!(second_leaf, first_leaf);
    assert_eq!(balance_after_first_apply, balance_after_request);
    assert_eq!(
        seq.accounts.get(aid).unwrap().balance,
        balance_after_request
    );
    assert_eq!(seq.accounts.total_balance(), total_balance_after_request);
}

fn test_withdrawal_request(account_id: AccountId, expiry_height: u64) -> BridgeWithdrawalRequest {
    BridgeWithdrawalRequest {
        account_id,
        chain_id: 1,
        vault_address: eth_address(0x10),
        recipient: eth_address(0x40),
        token_address: eth_address(0x20),
        amount_token_units: 4_000,
        expiry_height,
    }
}

fn test_withdrawal_event(
    withdrawal: &WithdrawalLeaf,
    status: L1WithdrawalStatus,
    l1_block_height: u64,
) -> BridgeWithdrawalL1Event {
    BridgeWithdrawalL1Event {
        nullifier: withdrawal.nullifier,
        status,
        event_at_unix: 1_700_000_000 + l1_block_height,
        executable_at_unix: None,
        tx_hash: Some([status as u8; 32]),
        l1_block_height,
    }
}

#[test]
fn cancelled_withdrawal_refunds_exactly_once_and_prunes_with_valid_witness() {
    let (mut seq, aid) = make_sequencer(10_000_000);
    let withdrawal = seq
        .request_bridge_withdrawal(test_withdrawal_request(aid, 10))
        .unwrap();
    seq.produce_block(vec![], 1_000);
    assert_eq!(seq.accounts.get(aid).unwrap().balance, 6_000_000);

    let event = test_withdrawal_event(&withdrawal, L1WithdrawalStatus::Cancelled, 5);
    let first = seq
        .apply_bridge_withdrawal_l1_event(event.clone())
        .unwrap()
        .unwrap();
    let second = seq
        .apply_bridge_withdrawal_l1_event(event)
        .unwrap()
        .unwrap();
    assert_eq!(first.l1_status, L1WithdrawalStatus::Refunded);
    assert_eq!(second, first);
    assert_eq!(seq.accounts.get(aid).unwrap().balance, 10_000_000);

    let refunded_block = seq.produce_block(vec![], 2_000);
    assert!(
        !seq.bridge_state()
            .withdrawals
            .contains_key(&withdrawal.withdrawal_id)
    );
    assert!(
        seq.apply_bridge_withdrawal_l1_event(test_withdrawal_event(
            &withdrawal,
            L1WithdrawalStatus::Cancelled,
            5,
        ))
        .unwrap()
        .is_none()
    );
    assert_eq!(seq.accounts.get(aid).unwrap().balance, 10_000_000);
    let verification = sybil_verifier::verify_full(&refunded_block.witness, false);
    assert!(verification.valid, "{:?}", verification.violations);
    let mut omitted_credit = refunded_block.witness.clone();
    omitted_credit
        .post_system_state
        .iter_mut()
        .find(|account| account.id == aid.0)
        .unwrap()
        .balance -= withdrawal.amount_nanos as i64;
    assert!(!sybil_verifier::verify_system(&omitted_credit).valid);
}

#[test]
fn expiry_and_cancel_observations_are_commutative_for_refund() {
    for cancel_first in [false, true] {
        let (mut seq, aid) = make_sequencer(10_000_000);
        let withdrawal = seq
            .request_bridge_withdrawal(test_withdrawal_request(aid, 10))
            .unwrap();
        seq.produce_block(vec![], 1_000);
        let cancelled = test_withdrawal_event(&withdrawal, L1WithdrawalStatus::Cancelled, 5);

        if cancel_first {
            seq.apply_bridge_withdrawal_l1_event(cancelled.clone())
                .unwrap();
            seq.observe_bridge_l1_height(11).unwrap();
        } else {
            seq.observe_bridge_l1_height(11).unwrap();
            seq.apply_bridge_withdrawal_l1_event(cancelled).unwrap();
        }

        assert_eq!(seq.accounts.get(aid).unwrap().balance, 10_000_000);
        assert_eq!(
            seq.bridge_withdrawal(withdrawal.withdrawal_id)
                .unwrap()
                .l1_status,
            L1WithdrawalStatus::Refunded
        );
        let block = seq.produce_block(vec![], 2_000);
        assert!(seq.bridge_state().withdrawals.is_empty());
        let verification = sybil_verifier::verify_full(&block.witness, false);
        assert!(verification.valid, "{:?}", verification.violations);
    }
}

#[test]
fn unrelated_confirmed_l1_event_still_advances_expiry_clock() {
    let (mut seq, aid) = make_sequencer(10_000_000);
    let withdrawal = seq
        .request_bridge_withdrawal(test_withdrawal_request(aid, 10))
        .unwrap();
    seq.produce_block(vec![], 1_000);

    let unrelated = BridgeWithdrawalL1Event {
        nullifier: [0xEE; 32],
        status: L1WithdrawalStatus::Queued,
        event_at_unix: 1_700_000_011,
        executable_at_unix: None,
        tx_hash: Some([0xAA; 32]),
        l1_block_height: 11,
    };
    assert!(
        seq.apply_bridge_withdrawal_l1_event(unrelated)
            .unwrap()
            .is_none()
    );
    assert_eq!(seq.bridge_state().observed_l1_height, 11);
    assert_eq!(seq.accounts.get(aid).unwrap().balance, 10_000_000);
    assert_eq!(
        seq.bridge_withdrawal(withdrawal.withdrawal_id)
            .unwrap()
            .l1_status,
        L1WithdrawalStatus::Refunded
    );

    let block = seq.produce_block(vec![], 2_000);
    assert!(seq.bridge_state().withdrawals.is_empty());
    let verification = sybil_verifier::verify_full(&block.witness, false);
    assert!(verification.valid, "{:?}", verification.violations);
}

#[test]
fn finalized_withdrawal_never_refunds_and_is_pruned() {
    let (mut seq, aid) = make_sequencer(10_000_000);
    let withdrawal = seq
        .request_bridge_withdrawal(test_withdrawal_request(aid, 10))
        .unwrap();
    seq.produce_block(vec![], 1_000);

    let finalized = test_withdrawal_event(&withdrawal, L1WithdrawalStatus::Finalized, 11);
    seq.apply_bridge_withdrawal_l1_event(finalized).unwrap();
    seq.observe_bridge_l1_height(12).unwrap();
    assert_eq!(seq.accounts.get(aid).unwrap().balance, 6_000_000);

    let block = seq.produce_block(vec![], 2_000);
    assert!(seq.bridge_state().withdrawals.is_empty());
    let verification = sybil_verifier::verify_full(&block.witness, false);
    assert!(verification.valid, "{:?}", verification.violations);
}

#[test]
fn terminal_withdrawal_map_stays_bounded_across_many_lifecycles() {
    let lifecycle_count = 128u64;
    let (mut seq, aid) = make_sequencer(10_000_000);
    for index in 0..lifecycle_count {
        let withdrawal = seq
            .request_bridge_withdrawal(test_withdrawal_request(aid, 1_000 + index))
            .unwrap();
        seq.apply_bridge_withdrawal_l1_event(test_withdrawal_event(
            &withdrawal,
            L1WithdrawalStatus::Cancelled,
            index + 1,
        ))
        .unwrap();
        let block = seq.produce_block(vec![], 1_000 + index);
        assert!(seq.bridge_state().withdrawals.is_empty());
        let verification = sybil_verifier::verify_full(&block.witness, false);
        assert!(verification.valid, "{:?}", verification.violations);
    }
    assert_eq!(seq.accounts.get(aid).unwrap().balance, 10_000_000);
    assert_eq!(seq.bridge_state().next_withdrawal_id, lifecycle_count + 1);
}

#[test]
fn pending_l1_cancel_replay_restores_refund_once_after_crash() {
    let (mut seq, aid) = make_sequencer(10_000_000);
    let withdrawal = seq
        .request_bridge_withdrawal(test_withdrawal_request(aid, 10))
        .unwrap();
    seq.produce_block(vec![], 1_000);
    let event = test_withdrawal_event(&withdrawal, L1WithdrawalStatus::Cancelled, 5);

    let mut restored_state = restored_state_with_resting_orders(&seq, MarketSet::new(), vec![]);
    restored_state.acknowledged_writes =
        sequenced_acknowledged_writes(vec![AcknowledgedWrite::BridgeL1Input(
            crate::bridge::BridgeL1Input::WithdrawalEvent(event.clone()),
        )]);
    let mut restored = BlockSequencer::restore(restored_state, SequencerConfig::default());
    assert_eq!(restored.accounts.get(aid).unwrap().balance, 10_000_000);
    assert_eq!(
        restored
            .apply_bridge_withdrawal_l1_event(event)
            .unwrap()
            .unwrap()
            .l1_status,
        L1WithdrawalStatus::Refunded
    );
    assert_eq!(restored.accounts.get(aid).unwrap().balance, 10_000_000);

    let block = restored.produce_block(vec![], 2_000);
    assert!(restored.bridge_state().withdrawals.is_empty());
    assert!(sybil_verifier::verify_full(&block.witness, false).valid);
}

#[test]
fn bridge_deposit_requires_next_l1_cursor() {
    let (mut seq, aid) = make_sequencer(0);
    match seq.ingest_l1_deposit(l1_deposit(aid, 2, 10_000)) {
        Err(SequencerError::Bridge(_)) => {}
        _ => panic!("expected bridge error"),
    }
}

#[test]
fn same_id_substitution_cannot_reuse_the_canonical_deposit_root() {
    let (seq, aid) = make_sequencer(0);
    let canonical = next_l1_deposit(&seq, aid, 10_000);
    let canonical_root = canonical.deposit_root;

    // A malicious service changes a leaf field while retaining the vault's
    // canonical root. The sequencer reconstructs the root from every leaf
    // field and the persisted frontier, so this branch cannot be credited.
    let mut substituted = canonical.clone();
    substituted.amount_token_units += 1;
    assert!(matches!(
        seq.validate_l1_deposit(&substituted),
        Err(SequencerError::Bridge(message)) if message.contains("deposit root mismatch")
    ));

    // Recomputing a self-consistent root makes the substitution internally
    // valid, but necessarily changes the checkpoint. The L1 indexer and
    // SybilSettlement compare this value with vault.depositRootByCount(id).
    let mut frontier = seq.bridge_state().deposit_frontier;
    substituted.deposit_root = crate::bridge::append_deposit_frontier(
        &mut frontier,
        seq.bridge_state().deposit_cursor,
        &substituted,
    )
    .expect("test deposit fits in frontier");
    assert_ne!(substituted.deposit_root, canonical_root);
    assert!(seq.validate_l1_deposit(&substituted).is_ok());
}

#[test]
fn quarantined_deposit_is_auto_claimed_on_witnessed_key_registration_across_blocks() {
    use p256::ecdsa::signature::Signer as _;

    let (mut seq, aid) = make_sequencer(0);
    let primary_signing =
        <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
            &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
        );
    let primary = crate::crypto::PublicKey(*primary_signing.verifying_key());
    seq.register_pubkey_with_scheme(
        aid,
        primary.clone(),
        crate::crypto::AccountAuthScheme::RawP256,
    )
    .unwrap();
    seq.produce_block(vec![], 1_000);

    let mut deposit = next_l1_deposit(&seq, aid, 25_000);
    deposit.account_id = None;
    assert!(matches!(
        seq.ingest_l1_deposit(deposit),
        Ok(DepositDisposition::Quarantined {
            amount_nanos: 25_000_000,
            ..
        })
    ));
    assert_eq!(seq.bridge_state().deposit_cursor, 1);
    assert_eq!(
        seq.bridge_state().quarantine.get(&account_key(aid)),
        Some(&25_000_000)
    );
    let quarantine_block = seq.produce_block(vec![], 2_000);
    assert!(sybil_verifier::verify_full(&quarantine_block.witness, false).valid);

    let added = fresh_public_key();
    let binding = seq.accounts.get(aid).unwrap().clone();
    let added_record = sybil_verifier::KeyRecord {
        auth_scheme: crate::crypto::AccountAuthScheme::RawP256.canonical_byte(),
        pubkey_sec1: added.compressed_bytes().try_into().unwrap(),
        capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
    };
    let canonical = sybil_verifier::canonical_key_registration_bytes(
        seq.genesis_hash().unwrap(),
        aid.0,
        &added_record,
        binding.keys_digest,
        binding.events_digest,
    );
    let signature: p256::ecdsa::Signature = primary_signing.sign(&canonical);
    let mut signer_pubkey = [0u8; 33];
    signer_pubkey.copy_from_slice(&primary.compressed_bytes());
    seq.register_pubkey_with_meta_authorized(
        aid,
        added,
        crate::crypto::RegisteredPubkey::primary(aid, crate::crypto::AccountAuthScheme::RawP256),
        sybil_verifier::KeyOpAuth::RawP256 {
            signer_pubkey,
            signature: signature.to_bytes().into(),
        },
    )
    .unwrap();
    assert!(seq.bridge_state().quarantine.is_empty());
    assert_eq!(seq.accounts.get(aid).unwrap().balance, 25_000_000);

    let claim_block = seq.produce_block(vec![], 3_000);
    assert!(matches!(
        claim_block.witness.system_events.as_slice(),
        [
            SystemEventWitness::KeyRegistered { .. },
            SystemEventWitness::QuarantineClaimed {
                account_id,
                amount: 25_000_000,
                ..
            }
        ] if *account_id == aid.0
    ));
    assert!(sybil_verifier::verify_full(&claim_block.witness, false).valid);
}

#[test]
fn atomic_onboarding_prevalidates_nonzero_balance_quarantine_claim() {
    let accounts = AccountStore::new();
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        MarketSet::new(),
        vec![],
        SequencerConfig::default(),
    );
    let account_id = AccountId(seq.accounts.next_id());
    let mut deposit = next_l1_deposit(&seq, account_id, 25_000);
    deposit.account_id = None;
    assert!(matches!(
        seq.ingest_l1_deposit(deposit),
        Ok(DepositDisposition::Quarantined {
            amount_nanos: 25_000_000,
            ..
        })
    ));

    let pubkey = fresh_public_key();
    let prepared = seq
        .prepare_account_with_initial_key(
            17,
            123,
            pubkey.clone(),
            crate::crypto::RegisteredPubkey::primary(
                AccountId(0),
                crate::crypto::AccountAuthScheme::WebAuthn,
            ),
        )
        .unwrap();
    assert_eq!(
        seq.apply_prepared_account_with_initial_key(prepared),
        account_id
    );
    let account = seq.accounts.get(account_id).unwrap();
    assert_eq!(account.balance, 25_000_017);
    assert_eq!(account.total_deposited, 25_000_017);
    assert!(seq.bridge_state().quarantine.is_empty());
    assert_eq!(seq.signing_keys_for_account(account_id).len(), 1);

    let block = seq.produce_block(Vec::new(), 1_000);
    assert!(matches!(
        block.block.system_events.as_slice(),
        [
            SystemEvent::CreateAccount { initial_keys, .. },
            SystemEvent::DepositQuarantined { .. },
            SystemEvent::QuarantineClaimed { .. },
        ] if initial_keys.len() == 1
    ));
    assert!(sybil_verifier::verify_full(&block.witness, false).valid);
}

#[test]
fn restore_resumes_deposit_frontier_fold_for_next_block() {
    let (mut seq, aid) = make_sequencer(0);
    let first = next_l1_deposit(&seq, aid, 10_000);
    seq.ingest_l1_deposit(first).unwrap();
    let first_block = seq.produce_block(vec![], 1_000);
    assert_eq!(first_block.witness.deposit_accumulator.pre_count, 0);
    assert_eq!(
        first_block.witness.deposit_accumulator.new_deposits.len(),
        1
    );
    let committed_frontier = seq.bridge_state().deposit_frontier;
    let committed_root = seq.bridge_state().deposit_root;

    let state = restored_state_with_resting_orders(&seq, MarketSet::new(), vec![]);
    let mut restored = BlockSequencer::restore(state, SequencerConfig::default());
    assert_eq!(restored.bridge_state().deposit_cursor, 1);
    assert_eq!(restored.bridge_state().deposit_root, committed_root);
    assert_eq!(restored.bridge_state().deposit_frontier, committed_frontier);
    assert_eq!(
        crate::bridge::deposit_frontier_root(
            &restored.bridge_state().deposit_frontier,
            restored.bridge_state().deposit_cursor,
        ),
        Some(committed_root)
    );

    let second = next_l1_deposit(&restored, aid, 20_000);
    restored.ingest_l1_deposit(second.clone()).unwrap();
    let second_block = restored.produce_block(vec![], 2_000);

    assert_eq!(second_block.witness.deposit_accumulator.pre_count, 1);
    assert_eq!(
        second_block.witness.deposit_accumulator.pre_frontier,
        committed_frontier
    );
    assert_eq!(
        second_block.witness.deposit_accumulator.new_deposits,
        vec![l1_deposit_witness(&second)]
    );
    assert_eq!(second_block.witness.state_sidecar.bridge.deposit_cursor, 2);
    assert_eq!(
        second_block.witness.state_sidecar.bridge.deposit_root,
        second.deposit_root
    );
    let verification = sybil_verifier::verify_full(&second_block.witness, false);
    assert!(
        verification.valid,
        "violations: {:?}",
        verification.violations
    );
}

#[test]
fn test_market_position_totals_sums_all_accounts() {
    let (mut markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid0 = accounts.create_account(0);
    let aid1 = accounts.create_account(0);

    accounts.get_mut(aid0).unwrap().positions.insert((m0, 0), 7);
    accounts.get_mut(aid0).unwrap().positions.insert((m0, 1), 2);
    accounts
        .get_mut(aid1)
        .unwrap()
        .positions
        .insert((m0, 0), -3);
    accounts.get_mut(aid1).unwrap().positions.insert((m0, 1), 5);

    let totals = CanonicalState::from_accounts(&accounts)
        .market_position_totals()
        .totals_for(m0);
    assert_eq!(totals, (4, 7));

    let m1 = markets.add_binary("Unused");
    let unused_totals = CanonicalState::from_accounts(&accounts)
        .market_position_totals()
        .totals_for(m1);
    assert_eq!(unused_totals, (0, 0));
}

#[test]
fn test_expected_balance_delta_from_fills_respects_order_side() {
    let (markets, m0) = setup();
    let buy = outcome_buy(&markets, 1, m0, 0, 300_000_000, q(4));
    let sell = outcome_sell(&markets, 2, m0, 0, 700_000_000, q(2));
    let order_map = HashMap::from([(buy.id, &buy), (sell.id, &sell)]);

    let fills = vec![
        Fill::new(buy.id, Qty(q(4)), Nanos(300_000_000)),
        Fill::new(sell.id, Qty(q(2)), Nanos(700_000_000)),
    ];

    let expected_delta = expected_balance_delta_from_fills(&fills, &order_map, &[]);
    assert_eq!(
        expected_delta,
        -(notional_nanos(Nanos(300_000_000), Qty(q(4))).0 as i64)
            + notional_nanos(Nanos(700_000_000), Qty(q(2))).0 as i64
    );
}

#[test]
fn test_expected_balance_delta_includes_mint_adjustments() {
    let (markets, m0) = setup();
    let buy = outcome_buy(&markets, 1, m0, 0, 300_000_000, q(4));
    let order_map = HashMap::from([(buy.id, &buy)]);
    let fills = vec![Fill::new(buy.id, Qty(q(4)), Nanos(300_000_000))];
    let mint_adjustments = vec![matching_engine::MintAdjustment {
        market_id: m0,
        outcome: 0,
        position_delta: -qi(4),
        balance_delta: notional_nanos(Nanos(300_000_000), Qty(q(4))).0 as i64,
    }];

    let expected_delta = expected_balance_delta_from_fills(&fills, &order_map, &mint_adjustments);
    assert_eq!(expected_delta, 0);
}

#[test]
fn non_one_hot_payoff_submission_does_not_clear_or_break_conservation() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let custom_buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let no_buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );
    let initial_balance = total_balance(&seq);

    let custom = raw_single_market_order(m0, [2, 0], 900_000_000, q(1));
    let no_leg = outcome_buy(&markets, 0, m0, 1, 700_000_000, q(2));

    let bp = seq.produce_block(
        vec![
            single_order_sub(custom_buyer, custom),
            single_order_sub(no_buyer, no_leg),
        ],
        1_000,
    );

    assert!(
        bp.block.fills.is_empty(),
        "non-one-hot payoff order must be rejected before solve; fills={:?} rejections={:?}",
        bp.block.fills,
        bp.block.rejections
    );
    assert_eq!(
        total_balance(&seq),
        initial_balance,
        "malformed payoff submission must not change total account balance"
    );
}

#[test]
fn multi_market_bundle_submission_does_not_clear_or_break_conservation() {
    let mut markets = MarketSet::new();
    let m0 = markets.add_binary("A");
    let m1 = markets.add_binary("B");
    let mut accounts = AccountStore::new();
    let bundle_buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let no_buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );
    let initial_balance = total_balance(&seq);

    let bundle = matching_engine::bundle_yes(&markets, 0, &[m0, m1], 400_000_000, q(1));
    let no_leg = outcome_buy(&markets, 0, m0, 1, 600_000_000, q(1));

    let bp = seq.produce_block(
        vec![
            single_order_sub(bundle_buyer, bundle),
            single_order_sub(no_buyer, no_leg),
        ],
        1_000,
    );

    assert!(
        bp.block.fills.is_empty(),
        "multi-market bundle must be rejected before solve; fills={:?} rejections={:?}",
        bp.block.fills,
        bp.block.rejections
    );
    assert_eq!(
        total_balance(&seq),
        initial_balance,
        "multi-market bundle submission must not change total account balance"
    );
}

#[test]
fn test_minting_market_totals_include_markets_only_present_in_positions() {
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(0);
    let orphaned_market = MarketId::new(777);

    accounts
        .get_mut(aid)
        .expect("account should exist")
        .positions
        .insert((orphaned_market, 1), 9);

    let totals = CanonicalState::from_accounts(&accounts)
        .market_position_totals()
        .minting_inputs();

    assert_eq!(totals, vec![(orphaned_market, 0, 9)]);
}

#[test]
fn test_block_minting_uses_position_markets_outside_catalog() {
    let mut markets = MarketSet::new();
    let active_market = markets.add_binary("Active");
    let orphaned_market = MarketId::new(active_market.0 + 1);

    let mut accounts = AccountStore::new();
    let holder = accounts.create_account(0);
    let mut seq =
        BlockSequencer::with_default_solver(accounts, markets, vec![], SequencerConfig::default());
    *seq.analytics.price_tracker_mut() = crate::price_tracker::PriceTracker::with_state(
        HashMap::from([(
            orphaned_market,
            vec![Nanos(400_000_000), Nanos(600_000_000)],
        )]),
        HashMap::new(),
    );

    seq.accounts
        .get_mut(holder)
        .expect("holder should exist")
        .positions
        .insert((orphaned_market, 1), 7);

    let bp = seq.produce_block(vec![], 1_000);

    let mint = seq
        .accounts
        .get(crate::account::AccountId::MINT)
        .expect("mint should exist");
    assert_eq!(mint.position(orphaned_market, 1), -7);
    assert_eq!(
        bp.block.clearing_prices.get(&orphaned_market),
        Some(&vec![Nanos(400_000_000), Nanos(600_000_000)])
    );

    let verification = sybil_verifier::verify_full(&bp.witness, false);
    assert!(
        verification.valid,
        "Violations: {:?}",
        verification.violations
    );
}

#[test]
fn placed_order_stats_count_carried_resting_orders_each_batch() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    let sub = single_order_sub(
        buyer,
        outcome_buy(&markets, 0, m0, 0, NANOS_PER_DOLLAR / 2, 10),
    );
    let first = seq.produce_block(vec![sub], 1_000);
    assert_eq!(first.analytics.orders_by_market.get(&m0).unwrap().placed, 1);
    assert_eq!(seq.analytics().platform_order_stats(1_000).0.placed, 1);
    assert_eq!(seq.order_book.len(), 1, "unfilled order should rest");

    let second = seq.produce_block(vec![], 2_000);
    assert_eq!(
        second.analytics.orders_by_market.get(&m0).unwrap().placed,
        1,
        "carried resting order is live in the next batch"
    );
    assert_eq!(
        seq.analytics().platform_order_stats(2_000).0.placed,
        2,
        "placed is order-batch participation, not one-time admission"
    );
    // The distinct counter, by contrast, stays at 1: the order was
    // admitted once (block 1) and merely participated again in block 2.
    assert_eq!(
        seq.analytics()
            .platform_order_stats(2_000)
            .0
            .placed_distinct,
        1,
        "distinct counts admission once, not per-batch participation"
    );
}

#[test]
fn placed_order_stats_count_mm_batch_orders() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let mm = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    let mut constraint = MmConstraint::new(MmId::new(1), Nanos(50 * NANOS_PER_DOLLAR));
    constraint.add_order(0, matching_engine::MmSide::BuyYes);
    let sub = OrderSubmission {
        account_id: mm,
        orders: vec![outcome_buy(&markets, 0, m0, 0, NANOS_PER_DOLLAR / 2, 10)],
        mm_constraint: Some(constraint),
    };

    let production = seq.produce_block(vec![sub], 1_000);
    let m0_stats = production.analytics.orders_by_market.get(&m0).unwrap();
    assert_eq!(m0_stats.placed, 1);
    assert_eq!(seq.analytics().platform_order_stats(1_000).0.placed, 1);
    assert_eq!(
        production.analytics.unique_placers, 0,
        "MM orders count as orders but not as unique traders"
    );
    // MM flash orders are counted in matched/unmatched too — they live one
    // block and resolve in-place, so exactly one of the two ticks. This is
    // the property that lets distinct-placed reconcile with matched +
    // unmatched once carried orders have cycled out.
    let stats = seq.analytics().platform_order_stats(1_000).0;
    assert_eq!(stats.placed_distinct, 1, "MM flash order admitted once");
    assert_eq!(
        stats.matched + stats.unmatched,
        1,
        "MM flash order resolves to exactly one of matched/unmatched"
    );
    assert_eq!(
        m0_stats.matched + m0_stats.unmatched,
        1,
        "per-market MM outcome counted in its block"
    );
}

#[test]
fn derived_view_stream_rebuilds_order_stats_over_scenarios() {
    let scenarios = [
        ScenarioConfig::quick().with_seed(216),
        ScenarioConfig::small().with_seed(166),
    ];

    for config in scenarios {
        let problem = generate_scenario(config);
        let (mut sequencer, submissions, _account_ids) =
            sequencer_from_scenario_problem(problem, 5);
        let replay_config = sequencer.config.clone();

        let productions = vec![
            sequencer.produce_block(submissions, 1_000),
            sequencer.produce_block(Vec::new(), 2_000),
            sequencer.produce_block(Vec::new(), 3_000),
        ];

        let live_order_stats =
            rmp_serde::to_vec(&sequencer.analytics().order_stats_snapshot()).unwrap();
        let mut replay = AnalyticsState::new(&replay_config);
        for production in &productions {
            let sealed = production.sealed_block();
            replay.observe_block(
                &sealed,
                &production.derived_view_sidecar,
                &production.witness,
            );
        }

        let replay_order_stats = rmp_serde::to_vec(&replay.order_stats_snapshot()).unwrap();
        assert_eq!(
            replay_order_stats, live_order_stats,
            "stream-rebuilt order stats must match live analytics"
        );
    }
}

/// Helper: run a batch through the block sequencer, returning BatchResult.
fn run_batch(
    seq: &mut BlockSequencer,
    submissions: Vec<OrderSubmission>,
    markets: &MarketSet,
    market_groups: &[MarketGroup],
) -> BatchResult {
    // Temporarily swap markets/groups for this batch
    let old_markets = std::mem::replace(&mut seq.markets, markets.clone());
    let old_groups = std::mem::replace(&mut seq.market_groups, market_groups.to_vec());
    let bp = seq.produce_block(submissions, 0);
    seq.markets = old_markets;
    seq.market_groups = old_groups;
    batch_result_from_block(&bp.block, &bp.analytics, bp.pipeline)
}

fn snapshot_by_id(
    snapshots: &[AccountSnapshot],
    account_id: AccountId,
) -> Option<&AccountSnapshot> {
    snapshots
        .iter()
        .find(|snapshot| snapshot.id == account_id.0)
}

// --- Validation tests ---

#[test]
fn test_validate_buy_sufficient_balance() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
    let account = accounts.get(aid).unwrap();

    let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, q(10));
    assert!(validate_order(&order, account, &HashMap::new()).is_ok());
}

#[test]
fn test_validate_buy_insufficient_balance() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(3 * NANOS_PER_DOLLAR as i64);
    let account = accounts.get(aid).unwrap();

    let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, q(10));
    let result = validate_order(&order, account, &HashMap::new());
    assert!(result.is_err());
    match result.unwrap_err() {
        RejectionReason::InsufficientBalance {
            required,
            available,
        } => {
            assert_eq!(required, 5_000_000_000);
            assert_eq!(available, 3_000_000_000);
        }
        other => panic!("Expected InsufficientBalance, got {:?}", other),
    }
}

#[test]
fn test_validate_sell_sufficient_position() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(NANOS_PER_DOLLAR as i64);
    let account = accounts.get_mut(aid).unwrap();
    account.positions.insert((m0, 0), qi(10));

    let order = outcome_sell(&markets, 1, m0, 0, 500_000_000, q(5));
    assert!(validate_order(&order, account, &HashMap::new()).is_ok());
}

#[test]
fn test_validate_sell_insufficient_position() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(NANOS_PER_DOLLAR as i64);
    let account = accounts.get_mut(aid).unwrap();
    account.positions.insert((m0, 0), qi(3));

    let order = outcome_sell(&markets, 1, m0, 0, 500_000_000, q(5));
    let result = validate_order(&order, account, &HashMap::new());
    assert!(result.is_err());
    match result.unwrap_err() {
        RejectionReason::InsufficientPosition {
            market,
            outcome,
            required,
            available,
        } => {
            assert_eq!(market, m0);
            assert_eq!(outcome, 0);
            assert_eq!(required, qi(5));
            assert_eq!(available, qi(3));
        }
        other => panic!("Expected InsufficientPosition, got {:?}", other),
    }
}

// --- Balance reservation tests ---

#[test]
fn test_balance_reservation_returns_cost() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
    let account = accounts.get(aid).unwrap();

    let order = outcome_buy(&markets, 1, m0, 0, 600_000_000, q(5));
    let cost = validate_order_with_reservation(&order, account, 0, &HashMap::new()).unwrap();
    assert_eq!(cost, notional_nanos(Nanos(600_000_000), Qty(q(5))).0 as i64);
}

#[test]
fn test_balance_reservation_blocks_double_spend() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(8 * NANOS_PER_DOLLAR as i64);
    let account = accounts.get(aid).unwrap();

    let order1 = outcome_buy(&markets, 1, m0, 0, 500_000_000, q(10));
    let cost1 = validate_order_with_reservation(&order1, account, 0, &HashMap::new()).unwrap();
    assert_eq!(cost1, 5_000_000_000);

    let order2 = outcome_buy(&markets, 2, m0, 0, 500_000_000, q(10));
    let result = validate_order_with_reservation(&order2, account, cost1, &HashMap::new());
    assert!(result.is_err());
    match result.unwrap_err() {
        RejectionReason::InsufficientBalance {
            required,
            available,
        } => {
            assert_eq!(required, 5_000_000_000);
            assert_eq!(available, 3_000_000_000);
        }
        other => panic!("Expected InsufficientBalance, got {:?}", other),
    }
}

#[test]
fn test_balance_reservation_in_batch() {
    let (markets, m0) = setup();
    let (mut seq, aid) = make_sequencer(8 * NANOS_PER_DOLLAR as i64);

    let order1 = outcome_buy(&markets, 0, m0, 0, 500_000_000, q(10));
    let order2 = outcome_buy(&markets, 0, m0, 0, 500_000_000, q(10));

    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![order1, order2],
        mm_constraint: None,
    };

    let result = run_batch(&mut seq, vec![sub], &markets, &[]);

    assert_eq!(result.rejections.len(), 1);
    match &result.rejections[0].reason {
        RejectionReason::InsufficientBalance { .. } => {}
        other => panic!("Expected InsufficientBalance, got {:?}", other),
    }
}

#[test]
fn test_sell_order_does_not_reserve_balance() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(5 * NANOS_PER_DOLLAR as i64);
    let account = accounts.get_mut(aid).unwrap();
    account.positions.insert((m0, 0), qi(100));

    let sell = outcome_sell(&markets, 1, m0, 0, 500_000_000, q(10));
    let cost = validate_order_with_reservation(&sell, account, 0, &HashMap::new()).unwrap();
    assert_eq!(cost, 0);
}

// --- Account not found ---

#[test]
fn test_account_not_found_rejection() {
    let (markets, m0) = setup();
    let (mut seq, _) = make_sequencer(NANOS_PER_DOLLAR as i64);

    let bogus_id = AccountId(999);
    let order = outcome_buy(&markets, 0, m0, 0, 500_000_000, 1);
    let sub = OrderSubmission {
        account_id: bogus_id,
        orders: vec![order],
        mm_constraint: None,
    };

    let result = run_batch(&mut seq, vec![sub], &markets, &[]);
    assert_eq!(result.rejections.len(), 1);
    assert_eq!(result.rejections[0].account_id, bogus_id);
    match &result.rejections[0].reason {
        RejectionReason::AccountNotFound => {}
        other => panic!("Expected AccountNotFound, got {:?}", other),
    }
}

// --- MM validation skip ---

#[test]
fn test_mm_orders_skip_validation() {
    let (markets, m0) = setup();
    let (mut seq, aid) = make_sequencer(0);

    let order = outcome_buy(&markets, 0, m0, 0, 500_000_000, 100);
    let mut constraint = MmConstraint::new(MmId(1), Nanos(50 * NANOS_PER_DOLLAR));
    constraint.add_order(0, matching_engine::MmSide::BuyYes);

    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![order],
        mm_constraint: Some(constraint),
    };

    let result = run_batch(&mut seq, vec![sub], &markets, &[]);
    assert_eq!(result.rejections.len(), 0);
}

// --- Order ID assignment ---

#[test]
fn test_order_ids_are_unique() {
    let (markets, m0) = setup();
    let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

    let sub1 = OrderSubmission {
        account_id: aid,
        orders: vec![
            outcome_buy(&markets, 0, m0, 0, 100_000_000, 1),
            outcome_buy(&markets, 0, m0, 1, 100_000_000, 1),
        ],
        mm_constraint: None,
    };
    run_batch(&mut seq, vec![sub1], &markets, &[]);

    let sub2 = OrderSubmission {
        account_id: aid,
        orders: vec![
            outcome_buy(&markets, 0, m0, 0, 100_000_000, 1),
            outcome_buy(&markets, 0, m0, 1, 100_000_000, 1),
        ],
        mm_constraint: None,
    };
    run_batch(&mut seq, vec![sub2], &markets, &[]);

    assert_eq!(seq.next_order_id, 5);
}

// --- Order persistence tests ---

#[test]
fn test_unfilled_orders_persist() {
    let (markets, m0) = setup();
    let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
        mm_constraint: None,
    };

    let result = run_batch(&mut seq, vec![sub], &markets, &[]);
    assert_eq!(result.rejections.len(), 0);

    assert_eq!(seq.order_book.len(), 1);
    let (_, resting_aid, resting_created, _, _, _) =
        seq.order_book.resting_orders_full().next().unwrap();
    assert_eq!(resting_aid, aid);
    assert_eq!(resting_created, 1);
}

#[test]
fn test_pending_orders_included_in_next_batch() {
    let (markets, m0) = setup();
    let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

    let sub1 = OrderSubmission {
        account_id: aid,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
        mm_constraint: None,
    };
    run_batch(&mut seq, vec![sub1], &markets, &[]);
    assert_eq!(seq.order_book.len(), 1);

    let result = run_batch(&mut seq, vec![], &markets, &[]);
    assert!(result.orders_submitted >= 1);
}

#[test]
fn test_resting_orders_survive_restart_and_match() {
    let (markets, m0) = setup();

    let mut accounts = AccountStore::new();
    let aid_a = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let aid_b = accounts.create_account(0);
    accounts
        .get_mut(aid_b)
        .unwrap()
        .positions
        .insert((m0, 0), 10);
    accounts
        .get_mut(aid_b)
        .unwrap()
        .positions
        .insert((m0, 1), 10);
    let mut seq_a = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    let sub = OrderSubmission {
        account_id: aid_a,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 700_000_000, 5)],
        mm_constraint: None,
    };
    seq_a.produce_block(vec![sub], 1_000);
    assert_eq!(
        seq_a.order_book.len(),
        1,
        "expected unfilled buy to rest in book"
    );
    let reserved_before = seq_a.order_book.reserved_balance(aid_a);
    assert!(reserved_before > 0);

    // Build a RestoredState as the store would, then restore into seq_b.
    let state = RestoredState {
        accounts: seq_a.accounts.clone(),
        markets: markets.clone(),
        market_groups: vec![],
        market_statuses: HashMap::new(),
        market_metadata: HashMap::new(),
        height: seq_a.height(),
        last_header: seq_a.last_header().cloned(),
        genesis_hash: seq_a.genesis_hash().unwrap_or([0; 32]),
        next_order_id: seq_a.next_order_id(),
        pubkey_registry: seq_a.pubkey_registry().clone(),
        resting_orders: seq_a.order_book.snapshot(),
        data_feeds: Vec::new(),
        resolution_templates: Vec::new(),
        acknowledged_writes: Vec::new(),
        bridge_state: BridgeState::default(),
        analytics: crate::store::AnalyticsRestoredState {
            last_clearing_prices: seq_a.analytics().last_clearing_prices().clone(),
            market_volumes: seq_a.analytics().market_volumes().clone(),
            trader_tracker: Default::default(),
            rolling_volume: Default::default(),
            rolling_price_anchors: Default::default(),
            liquidity_tracker: Default::default(),
            order_stats_tracker: Default::default(),
            welfare_tracker: Default::default(),
            first_deposit_ms: HashMap::new(),
            fill_total_counts: HashMap::new(),
            cost_basis_tracker: Default::default(),
            next_product_event_seq: 0,
        },
    };

    let mut seq_b = BlockSequencer::restore(state, SequencerConfig::default());
    assert_eq!(
        seq_b.order_book.len(),
        1,
        "restored order book should contain A's resting buy"
    );
    assert_eq!(
        seq_b.order_book.reserved_balance(aid_a),
        reserved_before,
        "balance reservation should be reconstructed"
    );

    // A matching sell from B should clear A's resting buy in the next batch.
    let sell = outcome_sell(&markets, 1_000, m0, 0, 300_000_000, 5);
    let sub_b = OrderSubmission {
        account_id: aid_b,
        orders: vec![sell],
        mm_constraint: None,
    };
    let bp = seq_b.produce_block(vec![sub_b], 2_000);

    let total_fill_qty: u64 = bp.block.fills.iter().map(|f| f.fill_qty.0).sum();
    assert!(
        total_fill_qty > 0,
        "expected restored resting buy to match the new sell, got fills={:?}",
        bp.block.fills
    );
    assert_eq!(
        seq_b.order_book.reserved_balance(aid_a),
        0,
        "A's reservation should be released after the fill"
    );
}

#[test]
fn restore_advances_next_order_id_past_replayed_admit_log_before_pending_bundles() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut committed = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig {
            min_resting_order_notional_nanos: 0,
            ..SequencerConfig::default()
        },
    );
    committed.produce_block(Vec::new(), 1_000);
    assert_eq!(committed.next_order_id(), 1);

    let mut live = committed.clone();
    let direct = OrderSubmission {
        account_id: aid,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 1)],
        mm_constraint: None,
    };
    let replayed_admit = match live.try_admit_direct(direct, 1_001) {
        AdmitOutcome::Admitted {
            order_id,
            resting_order,
        } => {
            assert_eq!(order_id, 1);
            resting_order
        }
        other => panic!("expected direct admit, got {:?}", other),
    };

    let deferred = match live.try_admit_direct(
        OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 100_000_000, 1),
                outcome_buy(&markets, 0, m0, 0, 100_000_000, 1),
            ],
            mm_constraint: None,
        },
        1_002,
    ) {
        AdmitOutcome::Deferred { submission, .. } => submission,
        other => panic!("expected pending bundle deferral, got {:?}", other),
    };

    let state = RestoredState {
        accounts: committed.accounts.clone(),
        markets: markets.clone(),
        market_groups: vec![],
        market_statuses: HashMap::new(),
        market_metadata: HashMap::new(),
        height: committed.height(),
        last_header: committed.last_header().cloned(),
        genesis_hash: committed.genesis_hash().unwrap_or([0; 32]),
        next_order_id: committed.next_order_id(),
        pubkey_registry: committed.pubkey_registry().clone(),
        resting_orders: committed.order_book.snapshot(),
        data_feeds: Vec::new(),
        resolution_templates: Vec::new(),
        acknowledged_writes: sequenced_acknowledged_writes(vec![
            AcknowledgedWrite::DirectAdmit(replayed_admit),
            AcknowledgedWrite::DeferredBundle(deferred),
        ]),
        bridge_state: BridgeState::default(),
        analytics: crate::store::AnalyticsRestoredState {
            last_clearing_prices: committed.analytics().last_clearing_prices().clone(),
            market_volumes: committed.analytics().market_volumes().clone(),
            trader_tracker: Default::default(),
            rolling_volume: Default::default(),
            rolling_price_anchors: Default::default(),
            liquidity_tracker: Default::default(),
            order_stats_tracker: Default::default(),
            welfare_tracker: Default::default(),
            first_deposit_ms: HashMap::new(),
            fill_total_counts: HashMap::new(),
            cost_basis_tracker: Default::default(),
            next_product_event_seq: 0,
        },
    };

    let mut restored = BlockSequencer::restore(state, SequencerConfig::default());
    assert_eq!(
        restored.next_order_id(),
        4,
        "restore must not reuse IDs acknowledged for admits or deferred bundles"
    );
    let bp = restored.produce_block(Vec::new(), 2_000);
    let ids: Vec<u64> = bp
        .witness
        .orders
        .iter()
        .map(|witness_order| witness_order.order.id)
        .collect();
    let unique: std::collections::HashSet<u64> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "duplicate order IDs: {ids:?}");
    assert!(
        ids.contains(&1) && ids.contains(&2) && ids.contains(&3),
        "expected replayed direct admit plus two restored bundle orders, got {ids:?}"
    );
    let verification = sybil_verifier::verify_full(&bp.witness, false);
    assert!(
        verification.valid,
        "restored mixed admit/bundle block should verify: {:?}",
        verification.violations
    );
}

#[test]
fn restored_pending_bundle_revalidates_against_replayed_admit_reservations() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(NANOS_PER_DOLLAR as i64);
    let mut committed = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );
    committed.produce_block(Vec::new(), 1_000);

    let mut live = committed.clone();
    let replayed_admit = match live.try_admit_direct(
        OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 800_000_000, q(1))],
            mm_constraint: None,
        },
        1_001,
    ) {
        AdmitOutcome::Admitted { resting_order, .. } => resting_order,
        other => panic!("expected direct admit, got {:?}", other),
    };

    let deferred = match live.try_admit_direct(
        OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 600_000_000, q(1)),
                outcome_buy(&markets, 0, m0, 0, 600_000_000, q(1)),
            ],
            mm_constraint: None,
        },
        1_002,
    ) {
        AdmitOutcome::Deferred { submission, .. } => submission,
        other => panic!("expected pending bundle deferral, got {:?}", other),
    };

    let state = RestoredState {
        accounts: committed.accounts.clone(),
        markets: markets.clone(),
        market_groups: vec![],
        market_statuses: HashMap::new(),
        market_metadata: HashMap::new(),
        height: committed.height(),
        last_header: committed.last_header().cloned(),
        genesis_hash: committed.genesis_hash().unwrap_or([0; 32]),
        next_order_id: committed.next_order_id(),
        pubkey_registry: committed.pubkey_registry().clone(),
        resting_orders: committed.order_book.snapshot(),
        data_feeds: Vec::new(),
        resolution_templates: Vec::new(),
        acknowledged_writes: sequenced_acknowledged_writes(vec![
            AcknowledgedWrite::DirectAdmit(replayed_admit),
            AcknowledgedWrite::DeferredBundle(deferred),
        ]),
        bridge_state: BridgeState::default(),
        analytics: crate::store::AnalyticsRestoredState {
            last_clearing_prices: committed.analytics().last_clearing_prices().clone(),
            market_volumes: committed.analytics().market_volumes().clone(),
            trader_tracker: Default::default(),
            rolling_volume: Default::default(),
            rolling_price_anchors: Default::default(),
            liquidity_tracker: Default::default(),
            order_stats_tracker: Default::default(),
            welfare_tracker: Default::default(),
            first_deposit_ms: HashMap::new(),
            fill_total_counts: HashMap::new(),
            cost_basis_tracker: Default::default(),
            next_product_event_seq: 0,
        },
    };

    let mut restored = BlockSequencer::restore(state, SequencerConfig::default());
    assert_eq!(restored.order_book.reserved_balance(aid), 800_000_000);

    let bp = restored.produce_block(Vec::new(), 2_000);
    assert_eq!(
        bp.witness.orders.len(),
        1,
        "over-reserved pending bundle orders must not enter accepted witness orders"
    );
    assert_eq!(bp.witness.rejections.len(), 2);
    assert!(bp.witness.rejections.iter().all(|rejection| matches!(
        rejection.reason,
        sybil_verifier::RejectionReason::InsufficientBalance { .. }
    )));
    let verification = sybil_verifier::verify_full(&bp.witness, false);
    assert!(
        verification.valid,
        "rejected restored pending bundle should still produce a valid witness: {:?}",
        verification.violations
    );
}

#[test]
fn restore_expires_stale_resting_orders_before_bridge_wal_replay() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let config = SequencerConfig {
        order_ttl_blocks: 10,
        ..SequencerConfig::default()
    };
    let mut seq =
        BlockSequencer::with_default_solver(accounts, markets.clone(), vec![], config.clone());

    let mut expiring_order = outcome_buy(&markets, 0, m0, 0, 800_000_000, q(100));
    expiring_order.expires_at_block = Some(2);
    seq.produce_block(
        vec![OrderSubmission {
            account_id: aid,
            orders: vec![expiring_order],
            mm_constraint: None,
        }],
        1_000,
    );
    assert_eq!(seq.height(), 1);
    assert_eq!(seq.order_book.len(), 1);
    assert_eq!(
        seq.order_book.reserved_balance(aid),
        80 * NANOS_PER_DOLLAR as i64
    );

    let stale_checkpoint_resting_orders = seq.order_book.snapshot();

    seq.produce_block(Vec::new(), 2_000);
    assert_eq!(seq.height(), 2);
    assert!(seq.order_book.is_empty());
    assert_eq!(seq.order_book.reserved_balance(aid), 0);

    let committed_after_expiry = seq.clone();
    let withdrawal_request = BridgeWithdrawalRequest {
        account_id: aid,
        chain_id: 1,
        vault_address: [0x10; 20],
        recipient: [0x40; 20],
        token_address: [0x20; 20],
        amount_token_units: 90_000_000,
        expiry_height: 10,
    };
    let acknowledged_withdrawal = seq
        .request_bridge_withdrawal(withdrawal_request.clone())
        .unwrap();

    let mut state = restored_state_with_resting_orders(
        &committed_after_expiry,
        markets.clone(),
        stale_checkpoint_resting_orders,
    );
    state.acknowledged_writes =
        sequenced_acknowledged_writes(vec![AcknowledgedWrite::BridgeWithdrawal(
            withdrawal_request,
        )]);

    let restored = BlockSequencer::restore(state, config);

    assert!(restored.order_book.is_empty());
    assert_eq!(restored.order_book.reserved_balance(aid), 0);
    assert_eq!(
        restored.bridge_withdrawal(acknowledged_withdrawal.withdrawal_id),
        Some(&acknowledged_withdrawal)
    );
    assert_eq!(
        restored.accounts.get(aid).unwrap().balance,
        10 * NANOS_PER_DOLLAR as i64
    );
}

#[test]
fn restore_rejects_invalid_acknowledged_write_rows() {
    let (markets, _) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let config = SequencerConfig::default();
    let mut seq =
        BlockSequencer::with_default_solver(accounts, markets.clone(), vec![], config.clone());
    seq.produce_block(Vec::new(), 1_000);

    let invalid_deposit = L1Deposit {
        deposit_id: 2,
        account_id: Some(aid),
        chain_id: 1,
        vault_address: [0x10; 20],
        token_address: [0x20; 20],
        sender: [0x30; 20],
        sybil_account_key: account_key(aid),
        amount_token_units: 10_000_000,
        deposit_root: [0x02; 32],
    };
    let invalid_withdrawal = BridgeWithdrawalRequest {
        account_id: aid,
        chain_id: 1,
        vault_address: [0x10; 20],
        recipient: [0x40; 20],
        token_address: [0x20; 20],
        amount_token_units: 150_000_000,
        expiry_height: 10,
    };

    let mut state =
        restored_state_with_resting_orders(&seq, markets.clone(), seq.order_book.snapshot());
    state.acknowledged_writes = sequenced_acknowledged_writes(vec![
        AcknowledgedWrite::L1Deposit(invalid_deposit),
        AcknowledgedWrite::BridgeWithdrawal(invalid_withdrawal),
    ]);

    let error = match BlockSequencer::try_restore(state, config) {
        Ok(_) => panic!("invalid acknowledged write must fail closed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SequencerRestoreError::AcknowledgedWrite {
            sequence: 0,
            kind: "l1_deposit",
            ..
        }
    ));
}

#[test]
fn test_expired_orders_removed() {
    let (markets, m0) = setup();
    let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);
    seq.order_book.set_ttl(2);

    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
        mm_constraint: None,
    };
    run_batch(&mut seq, vec![sub], &markets, &[]);
    assert_eq!(seq.order_book.len(), 1);

    run_batch(&mut seq, vec![], &markets, &[]);
    assert_eq!(seq.order_book.len(), 1);

    run_batch(&mut seq, vec![], &markets, &[]);
    assert_eq!(seq.order_book.len(), 0);
}

#[test]
fn test_orders_for_resolved_markets_removed() {
    let mut markets = MarketSet::new();
    let m0 = markets.add_binary("Market A");
    let m1 = markets.add_binary("Market B");

    let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);
    // This legacy helper swaps the market registry out of band to isolate
    // order-book revalidation behavior; that is not a valid v3 witness
    // transition because market deletion must be event-authenticated.
    seq.config.debug_verify_full = false;

    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![
            outcome_buy(&markets, 0, m0, 0, 100_000_000, 5),
            outcome_buy(&markets, 0, m1, 0, 100_000_000, 5),
        ],
        mm_constraint: None,
    };
    run_batch(&mut seq, vec![sub], &markets, &[]);
    assert_eq!(seq.order_book.len(), 2);

    let mut reduced_markets = MarketSet::new();
    reduced_markets.add_binary("Market B");

    run_batch(&mut seq, vec![], &reduced_markets, &[]);
    assert_eq!(seq.order_book.len(), 1);
}

#[test]
fn test_bankrupt_account_orders_removed() {
    let (markets, m0) = setup();
    let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);
    // This test mutates the account store directly to isolate order-book
    // revalidation; direct balance edits are intentionally not a valid v3
    // authenticated state transition.
    seq.config.debug_verify_full = false;

    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
        mm_constraint: None,
    };
    run_batch(&mut seq, vec![sub], &markets, &[]);
    assert_eq!(seq.order_book.len(), 1);

    let account = seq.accounts.get_mut(aid).unwrap();
    account.balance = 0;

    run_batch(&mut seq, vec![], &markets, &[]);
    assert_eq!(seq.order_book.len(), 0);
}

#[test]
fn test_mm_orders_not_persisted() {
    let (markets, m0) = setup();
    let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

    let order = outcome_buy(&markets, 0, m0, 0, 100_000_000, 5);
    let mut constraint = MmConstraint::new(MmId(1), Nanos(50 * NANOS_PER_DOLLAR));
    constraint.add_order(0, matching_engine::MmSide::BuyYes);

    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![order],
        mm_constraint: Some(constraint),
    };

    run_batch(&mut seq, vec![sub], &markets, &[]);
    assert_eq!(seq.order_book.len(), 0);
}

// --- Fill settlement integration ---

#[test]
fn test_matching_buy_and_sell_settles_correctly() {
    let (markets, m0) = setup();

    let mut accounts = AccountStore::new();
    let buyer_id = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let seller_id = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
    accounts
        .get_mut(seller_id)
        .unwrap()
        .positions
        .insert((m0, 0), 50);

    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        MarketSet::new(),
        vec![],
        SequencerConfig::default(),
    );

    let buy_sub = OrderSubmission {
        account_id: buyer_id,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 10)],
        mm_constraint: None,
    };
    let sell_sub = OrderSubmission {
        account_id: seller_id,
        orders: vec![outcome_sell(&markets, 0, m0, 0, 400_000_000, 10)],
        mm_constraint: None,
    };

    let result = run_batch(&mut seq, vec![buy_sub, sell_sub], &markets, &[]);

    if result.orders_filled > 0 {
        let buyer = seq.accounts.get(buyer_id).unwrap();
        let seller = seq.accounts.get(seller_id).unwrap();

        assert!(buyer.balance < 100 * NANOS_PER_DOLLAR as i64);
        assert!(buyer.position(m0, 0) > 0);

        assert!(seller.balance > 10 * NANOS_PER_DOLLAR as i64);
        assert!(seller.position(m0, 0) < 50);
    }
}

#[test]
fn test_fill_updates_only_participating_account_digests() {
    let (markets, m0) = setup();

    let mut accounts = AccountStore::new();
    let buyer_id = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let seller_id = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
    let untouched_id = accounts.create_account(50 * NANOS_PER_DOLLAR as i64);
    accounts
        .get_mut(seller_id)
        .unwrap()
        .positions
        .insert((m0, 0), 50);

    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    let buy_sub = OrderSubmission {
        account_id: buyer_id,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 10)],
        mm_constraint: None,
    };
    let sell_sub = OrderSubmission {
        account_id: seller_id,
        orders: vec![outcome_sell(&markets, 0, m0, 0, 400_000_000, 10)],
        mm_constraint: None,
    };

    seq.produce_block(vec![buy_sub, sell_sub], 1000);

    assert_ne!(seq.accounts.get(buyer_id).unwrap().events_digest, [0u8; 32]);
    assert_ne!(
        seq.accounts.get(seller_id).unwrap().events_digest,
        [0u8; 32]
    );
    assert_eq!(
        seq.accounts.get(untouched_id).unwrap().events_digest,
        [0u8; 32]
    );
}

#[test]
fn key_registry_mutations_refresh_account_keys_digest() {
    let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);
    let empty_digest = sybil_verifier::empty_account_keys_digest(aid.0);

    assert_eq!(seq.accounts.get(aid).unwrap().keys_digest, empty_digest);

    let raw_key = fresh_public_key();
    seq.register_pubkey_with_scheme(
        aid,
        raw_key.clone(),
        crate::crypto::AccountAuthScheme::RawP256,
    )
    .unwrap();
    let one_key_digest = crate::digest::account_keys_digest(aid, seq.pubkey_registry());
    assert_eq!(seq.accounts.get(aid).unwrap().keys_digest, one_key_digest);
    assert_ne!(one_key_digest, empty_digest);

    let webauthn_key = fresh_public_key();
    seq.register_pubkey_with_scheme(
        aid,
        webauthn_key.clone(),
        crate::crypto::AccountAuthScheme::WebAuthn,
    )
    .unwrap();
    let two_key_digest = crate::digest::account_keys_digest(aid, seq.pubkey_registry());
    assert_eq!(seq.accounts.get(aid).unwrap().keys_digest, two_key_digest);
    assert_ne!(two_key_digest, one_key_digest);

    let mut signer_pubkey = [0u8; 33];
    signer_pubkey.copy_from_slice(&raw_key.compressed_bytes());
    seq.revoke_signing_key(
        aid,
        &raw_key,
        sybil_verifier::KeyOpAuth::RawP256 {
            signer_pubkey,
            signature: [0u8; 64],
        },
    )
    .unwrap();
    let remaining_key_digest = crate::digest::account_keys_digest(aid, seq.pubkey_registry());
    assert_eq!(
        seq.accounts.get(aid).unwrap().keys_digest,
        remaining_key_digest
    );
    assert_ne!(remaining_key_digest, two_key_digest);
}

#[test]
fn signing_key_churn_stops_at_verifier_block_limit_and_block_remains_valid() {
    use p256::ecdsa::signature::Signer as _;

    let (mut seq, aid) = make_sequencer(0);
    let primary_signing =
        <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
            &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
        );
    let primary = crate::crypto::PublicKey(*primary_signing.verifying_key());
    seq.register_pubkey_with_scheme(
        aid,
        primary.clone(),
        crate::crypto::AccountAuthScheme::RawP256,
    )
    .unwrap();
    seq.produce_block(vec![], 1_000);

    let secondary = fresh_public_key();
    let secondary_meta =
        crate::crypto::RegisteredPubkey::primary(aid, crate::crypto::AccountAuthScheme::RawP256);
    let secondary_record = crate::digest::key_record(&secondary, &secondary_meta);
    let mut signer_pubkey = [0u8; 33];
    signer_pubkey.copy_from_slice(&primary.compressed_bytes());

    for op_index in 0..sybil_verifier::MAX_KEY_OPS_PER_BLOCK {
        let account = seq.accounts.get(aid).unwrap().clone();
        if op_index % 2 == 0 {
            let canonical = sybil_verifier::canonical_key_registration_bytes(
                seq.genesis_hash().unwrap(),
                aid.0,
                &secondary_record,
                account.keys_digest,
                account.events_digest,
            );
            let signature: p256::ecdsa::Signature = primary_signing.sign(&canonical);
            seq.register_pubkey_with_meta_authorized(
                aid,
                secondary.clone(),
                secondary_meta.clone(),
                sybil_verifier::KeyOpAuth::RawP256 {
                    signer_pubkey,
                    signature: signature.to_bytes().into(),
                },
            )
            .unwrap();
        } else {
            let canonical = sybil_verifier::canonical_key_revocation_bytes(
                seq.genesis_hash().unwrap(),
                aid.0,
                &secondary_record,
                account.keys_digest,
                account.events_digest,
            );
            let signature: p256::ecdsa::Signature = primary_signing.sign(&canonical);
            seq.revoke_signing_key(
                aid,
                &secondary,
                sybil_verifier::KeyOpAuth::RawP256 {
                    signer_pubkey,
                    signature: signature.to_bytes().into(),
                },
            )
            .unwrap();
        }
    }

    assert!(matches!(
        seq.can_register_authorized_pubkey(aid, &secondary),
        Err(SequencerError::KeyOpLimit { count, limit })
            if count == sybil_verifier::MAX_KEY_OPS_PER_BLOCK
                && limit == sybil_verifier::MAX_KEY_OPS_PER_BLOCK
    ));
    let production = seq.produce_block(vec![], 2_000);
    let key_ops = production
        .witness
        .system_events
        .iter()
        .filter(|event| {
            matches!(
                event,
                sybil_verifier::SystemEventWitness::KeyRegistered { .. }
                    | sybil_verifier::SystemEventWitness::KeyRevoked { .. }
            )
        })
        .count();
    assert_eq!(key_ops, sybil_verifier::MAX_KEY_OPS_PER_BLOCK);
    assert!(sybil_verifier::verify_full(&production.witness, false).valid);
}

/// Wiring + accumulation: the live block path feeds each block's
/// authoritative `total_welfare` into the platform welfare tracker, and the
/// all-time + 24h figures accumulate across blocks. Guards the
/// `produce_block_in_place` injection point — if `record_welfare` were not
/// called, platform welfare would stay `(0, 0)`.
#[test]
fn platform_welfare_accumulates_across_blocks() {
    let (markets, m0) = setup();

    let mut accounts = AccountStore::new();
    let buyer_id = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let seller_id = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
    accounts
        .get_mut(seller_id)
        .unwrap()
        .positions
        .insert((m0, 0), 50);

    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    // No trades yet → zero platform welfare.
    assert_eq!(seq.analytics().platform_welfare(0), (0, 0));

    // Block 1: a crossing trade (bid 0.60 ≥ ask 0.40) → positive welfare.
    let buy1 = OrderSubmission {
        account_id: buyer_id,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 10)],
        mm_constraint: None,
    };
    let sell1 = OrderSubmission {
        account_id: seller_id,
        orders: vec![outcome_sell(&markets, 0, m0, 0, 400_000_000, 10)],
        mm_constraint: None,
    };
    let w1 = seq
        .produce_block(vec![buy1, sell1], 1_000)
        .analytics
        .total_welfare;
    assert!(w1 > 0, "crossing trade should produce positive welfare");
    // Live wiring: after one block, platform welfare == that block's scalar.
    assert_eq!(seq.analytics().platform_welfare(1_000), (w1, w1));

    // Block 2: another crossing trade — both sides still have capacity.
    let buy2 = OrderSubmission {
        account_id: buyer_id,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 10)],
        mm_constraint: None,
    };
    let sell2 = OrderSubmission {
        account_id: seller_id,
        orders: vec![outcome_sell(&markets, 0, m0, 0, 400_000_000, 10)],
        mm_constraint: None,
    };
    let w2 = seq
        .produce_block(vec![buy2, sell2], 2_000)
        .analytics
        .total_welfare;
    assert!(w2 > 0, "second crossing trade should also produce welfare");
    // All-time + 24h both accumulate the two blocks' welfare.
    assert_eq!(seq.analytics().platform_welfare(2_000), (w1 + w2, w1 + w2));
}

#[test]
fn complete_set_burning_reports_positive_welfare() {
    let (markets, market) = setup();
    let qty = shares_to_qty(5);

    let mut accounts = AccountStore::new();
    let yes_seller = accounts.create_account(0);
    let no_seller = accounts.create_account(0);
    accounts
        .get_mut(yes_seller)
        .unwrap()
        .positions
        .insert((market, 0), qty.0 as i64);
    accounts
        .get_mut(no_seller)
        .unwrap()
        .positions
        .insert((market, 1), qty.0 as i64);

    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );
    let production = seq.produce_block(
        vec![
            OrderSubmission {
                account_id: yes_seller,
                orders: vec![outcome_sell(&markets, 0, market, 0, 40_000_000, qty.0)],
                mm_constraint: None,
            },
            OrderSubmission {
                account_id: no_seller,
                orders: vec![outcome_sell(&markets, 0, market, 1, 950_000_000, qty.0)],
                mm_constraint: None,
            },
        ],
        1_000,
    );

    assert_eq!(production.analytics.total_welfare, 50_000_000);
    assert_eq!(
        production.analytics.welfare_by_market.get(&market),
        Some(&50_000_000)
    );
    assert_eq!(production.witness.minting_cost, -5_000_000_000);
    assert_eq!(production.witness.total_welfare, 50_000_000);
    assert_eq!(
        seq.analytics().platform_welfare(1_000),
        (50_000_000, 50_000_000)
    );
    assert!(sybil_verifier::verify_full(&production.witness, false).valid);
}

// --- Block height counter ---

#[test]
fn test_batch_counter_increments() {
    let (markets, _) = setup();
    let (mut seq, _) = make_sequencer(NANOS_PER_DOLLAR as i64);

    assert_eq!(seq.height, 0);
    run_batch(&mut seq, vec![], &markets, &[]);
    assert_eq!(seq.height, 1);
    run_batch(&mut seq, vec![], &markets, &[]);
    assert_eq!(seq.height, 2);
}

// --- Block-specific tests ---

#[test]
fn test_produce_block_returns_valid_header() {
    let (markets, _) = setup();
    let accounts = AccountStore::new();
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    let bp = seq.produce_block(vec![], 1000);
    assert_eq!(bp.block.header.height, 1);
    assert_eq!(bp.block.header.parent_hash, [0u8; 32]); // genesis
    assert_eq!(
        bp.block.header.events_root,
        sybil_verifier::event_commitment::compute_events_root(&bp.witness)
    );
    assert_eq!(bp.block.header.timestamp_ms, 1000);
}

#[test]
fn test_block_chain_parent_hash() {
    let (markets, _) = setup();
    let accounts = AccountStore::new();
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    let bp1 = seq.produce_block(vec![], 1000);
    let expected_parent = hash_header(&bp1.block.header);

    let bp2 = seq.produce_block(vec![], 2000);
    assert_eq!(bp2.block.header.parent_hash, expected_parent);
    assert_eq!(bp2.block.header.height, 2);
}

#[test]
fn test_create_account_uses_post_system_state_for_orders() {
    let (markets, m0) = setup();
    let accounts = AccountStore::new();
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    let aid = seq.create_account(10 * NANOS_PER_DOLLAR as i64);
    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 1)],
        mm_constraint: None,
    };

    let bp = seq.produce_block(vec![sub], 0);

    assert!(
        bp.witness
            .pre_state
            .iter()
            .all(|snapshot| snapshot.id != aid.0)
    );
    let post_system = bp
        .witness
        .post_system_state
        .iter()
        .find(|snapshot| snapshot.id == aid.0)
        .expect("created account should exist after system events");
    assert_eq!(post_system.balance, 10 * NANOS_PER_DOLLAR as i64);

    let verification = sybil_verifier::verify_full(&bp.witness, false);
    assert!(
        verification.valid,
        "Violations: {:?}",
        verification.violations
    );
}

#[test]
fn test_deposit_keeps_block_start_pre_state() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(0);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    seq.fund_account(aid, 10 * NANOS_PER_DOLLAR as i64).unwrap();
    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 1)],
        mm_constraint: None,
    };

    let bp = seq.produce_block(vec![sub], 0);

    let pre_state = bp
        .witness
        .pre_state
        .iter()
        .find(|snapshot| snapshot.id == aid.0)
        .expect("funded account should exist at block start");
    assert_eq!(pre_state.balance, 0);

    let post_system = bp
        .witness
        .post_system_state
        .iter()
        .find(|snapshot| snapshot.id == aid.0)
        .expect("funded account should exist after system events");
    assert_eq!(post_system.balance, 10 * NANOS_PER_DOLLAR as i64);

    let verification = sybil_verifier::verify_full(&bp.witness, false);
    assert!(
        verification.valid,
        "Violations: {:?}",
        verification.violations
    );
}

proptest! {
    #[test]
    fn prop_phase_builder_is_identity_without_system_baselines(
        balances in prop::collection::vec(0i64..=10_000i64, 0..6)
    ) {
        let mut accounts = AccountStore::new();
        for balance in balances {
            accounts.create_account(balance);
        }

        let (pre_state, post_system_state) =
            build_witness_phase_snapshots(&accounts, &HashMap::new());

        prop_assert_eq!(pre_state, post_system_state);
    }

    #[test]
    fn prop_created_account_is_only_in_post_system_state(
        initial_balances in prop::collection::vec(0i64..=10_000i64, 0..5),
        created_balance in 0i64..=10_000i64,
    ) {
        let mut accounts = AccountStore::new();
        for balance in initial_balances {
            accounts.create_account(balance);
        }
        let created_account = accounts.create_account(created_balance);

        let mut baselines = HashMap::new();
        baselines.insert(created_account, None);

        let (pre_state, post_system_state) =
            build_witness_phase_snapshots(&accounts, &baselines);

        prop_assert!(snapshot_by_id(&pre_state, created_account).is_none());
        let created_snapshot = snapshot_by_id(&post_system_state, created_account)
            .expect("created account must exist after system events");
        prop_assert_eq!(created_snapshot.balance, created_balance);
    }

    #[test]
    fn prop_baselined_account_uses_block_start_snapshot(
        initial_balance in 0i64..=10_000i64,
        funded_balance in 0i64..=20_000i64,
        initial_position in 0i64..=20,
        final_position in 0i64..=20,
    ) {
        let mut accounts = AccountStore::new();
        let account_id = accounts.create_account(initial_balance);
        {
            let account = accounts.get_mut(account_id).unwrap();
            account.positions.insert((MarketId::new(0), 0), initial_position);
        }

        let baseline = accounts.get(account_id).unwrap().clone();
        {
            let account = accounts.get_mut(account_id).unwrap();
            account.balance = funded_balance;
            account.total_deposited = baseline.total_deposited + 5;
            account.positions.insert((MarketId::new(0), 0), final_position);
        }

        let mut baselines = HashMap::new();
        baselines.insert(account_id, Some(baseline.clone()));

        let (pre_state, post_system_state) =
            build_witness_phase_snapshots(&accounts, &baselines);

        let pre_snapshot =
            snapshot_by_id(&pre_state, account_id).expect("baseline should appear in pre-state");
        let post_snapshot = snapshot_by_id(&post_system_state, account_id)
            .expect("live account should appear in post-system state");

        prop_assert_eq!(pre_snapshot.balance, baseline.balance);
        prop_assert_eq!(pre_snapshot.total_deposited, baseline.total_deposited);
        prop_assert_eq!(
            pre_snapshot.positions.iter().find(|&&(market, outcome, _)| market == MarketId::new(0) && outcome == 0).map(|&(_, _, qty)| qty).unwrap_or(0),
            initial_position
        );
        prop_assert_eq!(post_snapshot.balance, funded_balance);
        prop_assert_eq!(post_snapshot.total_deposited, baseline.total_deposited + 5);
        prop_assert_eq!(
            post_snapshot.positions.iter().find(|&&(market, outcome, _)| market == MarketId::new(0) && outcome == 0).map(|&(_, _, qty)| qty).unwrap_or(0),
            final_position
        );
    }

    #[test]
    fn prop_baseline_insertion_order_does_not_change_phase_snapshots(
        balance_a in 0i64..=10_000i64,
        balance_b in 0i64..=10_000i64,
        created_balance in 0i64..=10_000i64,
    ) {
        let mut accounts = AccountStore::new();
        let account_a = accounts.create_account(balance_a);
        let account_b = accounts.create_account(balance_b);
        let baseline_b = accounts.get(account_b).unwrap().clone();
        let created_account = accounts.create_account(created_balance);

        let mut baselines_ab = HashMap::new();
        baselines_ab.insert(created_account, None);
        baselines_ab.insert(account_b, Some(baseline_b.clone()));

        let mut baselines_ba = HashMap::new();
        baselines_ba.insert(account_b, Some(baseline_b));
        baselines_ba.insert(created_account, None);

        let (pre_ab, post_ab) = build_witness_phase_snapshots(&accounts, &baselines_ab);
        let (pre_ba, post_ba) = build_witness_phase_snapshots(&accounts, &baselines_ba);

        prop_assert!(snapshot_by_id(&pre_ab, account_a).is_some());
        prop_assert_eq!(pre_ab, pre_ba);
        prop_assert_eq!(post_ab, post_ba);
    }
}

#[test]
fn test_state_root_in_block() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    let bp1 = seq.produce_block(vec![], 0);

    // Submit an unfilled order that rests in the committed order book.
    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 1)],
        mm_constraint: None,
    };
    let bp2 = seq.produce_block(vec![sub], 0);

    // State root matches the witness post-state (what verifier will check)
    let expected_root = sybil_verifier::block::compute_state_root_with_sidecar(
        &bp2.witness.post_state,
        &bp2.witness.state_sidecar,
    );
    assert_eq!(bp2.block.header.state_root, expected_root);

    // It does not change account balances/positions, but it does change
    // committed order/reservation leaves.
    assert_ne!(bp1.block.header.state_root, bp2.block.header.state_root);
}

#[test]
fn test_resolution_followed_by_empty_block_still_verifies() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let yes_buyer = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
    let no_buyer = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    let opening_block = seq.produce_block(
        vec![
            OrderSubmission {
                account_id: yes_buyer,
                orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 1)],
                mm_constraint: None,
            },
            OrderSubmission {
                account_id: no_buyer,
                orders: vec![outcome_buy(&markets, 0, m0, 1, 500_000_000, 1)],
                mm_constraint: None,
            },
        ],
        1_000,
    );
    let opening_verification = sybil_verifier::verify_full(&opening_block.witness, false);
    assert!(
        opening_verification.valid,
        "Violations: {:?}",
        opening_verification.violations
    );

    assert_ne!(seq.accounts.get(yes_buyer).unwrap().position(m0, 0), 0);
    assert_ne!(seq.accounts.get(no_buyer).unwrap().position(m0, 1), 0);

    seq.resolve_market(m0, Nanos(NANOS_PER_DOLLAR), 2_000)
        .expect("resolution should succeed");

    assert_eq!(seq.accounts.get(yes_buyer).unwrap().position(m0, 0), 0);
    assert_eq!(seq.accounts.get(no_buyer).unwrap().position(m0, 1), 0);

    let resolution_block = seq.produce_block(vec![], 3_000);
    let resolution_verification = sybil_verifier::verify_full(&resolution_block.witness, false);
    assert!(
        resolution_verification.valid,
        "Violations: {:?}",
        resolution_verification.violations
    );
    assert_eq!(
        resolution_block.block.header.state_root,
        sybil_verifier::block::compute_state_root_with_sidecar(
            &resolution_block.witness.post_state,
            &resolution_block.witness.state_sidecar,
        )
    );
}

#[test]
fn test_witness_includes_untouched_accounts() {
    let (markets, _) = setup();
    let mut accounts = AccountStore::new();
    accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    accounts.create_account(200 * NANOS_PER_DOLLAR as i64);
    let mut seq =
        BlockSequencer::with_default_solver(accounts, markets, vec![], SequencerConfig::default());

    let bp = seq.produce_block(vec![], 0);

    assert_eq!(bp.witness.pre_state.len(), 3);
    assert_eq!(bp.witness.post_system_state.len(), 3);
    assert_eq!(bp.witness.post_state.len(), 3);
    assert_eq!(
        bp.block.header.state_root,
        crate::block::compute_complete_state_root(
            &seq.accounts,
            seq.bridge_state(),
            seq.order_book(),
            seq.markets(),
            seq.market_groups(),
            seq.market_lifecycle(),
            seq.analytics().last_clearing_prices(),
        )
    );
}

// --- Complete-set self-trade prevention ---

fn setup_group() -> (MarketSet, MarketId, MarketId, MarketId, MarketGroup) {
    let mut markets = MarketSet::new();
    let m0 = markets.add_binary("A");
    let m1 = markets.add_binary("B");
    let m2 = markets.add_binary("C");
    let mut group = MarketGroup::new("Election");
    group.add_market(m0);
    group.add_market(m1);
    group.add_market(m2);
    (markets, m0, m1, m2, group)
}

#[test]
fn admin_resolution_shrinks_three_market_group_and_survivors_stay_coherent() {
    let (markets, m0, m1, m2, group) = setup_group();
    let mut accounts = AccountStore::new();
    let buyer0 = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let buyer1 = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![group],
        SequencerConfig::default(),
    );

    seq.resolve_market(m2, Nanos::ZERO, 1_000).unwrap();

    assert_eq!(seq.market_groups().len(), 1);
    assert_eq!(seq.market_groups()[0].markets, vec![m0, m1]);

    let bp = seq.produce_block(
        vec![
            single_order_sub(buyer0, outcome_buy(&markets, 0, m0, 0, 600_000_000, q(1))),
            single_order_sub(buyer1, outcome_buy(&markets, 0, m1, 0, 500_000_000, q(1))),
        ],
        2_000,
    );

    assert_eq!(bp.block.rejections.len(), 0);
    assert_eq!(
        bp.block.fills.len(),
        2,
        "survivor YES buyers should fill through the remaining group mint"
    );
    let m0_yes = bp.block.clearing_prices.get(&m0).unwrap()[0];
    let m1_yes = bp.block.clearing_prices.get(&m1).unwrap()[0];
    assert_eq!(
        m0_yes + m1_yes,
        Nanos(NANOS_PER_DOLLAR),
        "survivor YES prices must retain group coherence"
    );
    assert_eq!(bp.witness.market_groups.len(), 1);
    assert_eq!(bp.witness.market_groups[0].markets, vec![m0, m1]);
    for market_id in [m0, m1] {
        let snapshot = bp
            .witness
            .state_sidecar
            .markets
            .iter()
            .find(|market| market.market_id == market_id)
            .expect("cleared market snapshot");
        assert_eq!(
            Some(&snapshot.last_clearing_prices),
            bp.block.clearing_prices.get(&market_id)
        );
    }

    let verification = sybil_verifier::verify_full(&bp.witness, false);
    assert!(
        verification.valid,
        "violations: {:?}",
        verification.violations
    );
}

#[test]
fn attested_resolution_dissolves_two_market_group() {
    let mut markets = MarketSet::new();
    let m0 = markets.add_binary("A");
    let m1 = markets.add_binary("B");
    let mut group = MarketGroup::new("Binary event");
    group.add_market(m0);
    group.add_market(m1);

    let mut accounts = AccountStore::new();
    accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets,
        vec![group],
        SequencerConfig::default(),
    );

    let signing_key =
        <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
            &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
        );
    let signed = sign_attestation(
        ResolutionAttestation {
            market_id: m0,
            payout_nanos: Nanos(NANOS_PER_DOLLAR),
            nonce: 1,
        },
        &signing_key,
    );
    let template = "attested_test";
    let feed_id = seq.register_feed(signed.signer.clone(), "attested feed".to_string(), 0);
    seq.install_template(ResolutionTemplate {
        id: TemplateId(template.to_string()),
        policy: ResolutionPolicy::Immediate { feed_id },
    });
    seq.set_market_metadata(
        m0,
        MarketMetadata {
            resolution_config: Some(ResolutionConfig {
                template: template.to_string(),
            }),
            ..MarketMetadata::default()
        },
    );

    seq.resolve_market_attested(m0, &signed, 1_000).unwrap();

    assert!(
        seq.market_groups().is_empty(),
        "two-market group should dissolve after one member resolves"
    );
    let bp = seq.produce_block(Vec::new(), 2_000);
    assert!(bp.witness.market_groups.is_empty());
    let verification = sybil_verifier::verify_full(&bp.witness, false);
    assert!(
        verification.valid,
        "violations: {:?}",
        verification.violations
    );
}

#[test]
fn extending_market_group_is_idempotent_and_rejects_cross_group_member() {
    let mut markets = MarketSet::new();
    let m0 = markets.add_binary("A");
    let m1 = markets.add_binary("B");
    let m2 = markets.add_binary("C");
    let mut group0 = MarketGroup::new("Event 0");
    group0.add_market(m0);
    group0.add_market(m1);
    let mut group1 = MarketGroup::new("Event 1");
    group1.add_market(m2);

    let accounts = AccountStore::new();
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets,
        vec![group0, group1],
        SequencerConfig::default(),
    );

    let (group, inserted) = seq.extend_market_group(0, m1).unwrap();
    assert!(!inserted);
    assert_eq!(group.markets, vec![m0, m1]);

    let err = seq.extend_market_group(0, m2).unwrap_err();
    assert!(matches!(
        err,
        SequencerError::MarketAlreadyGrouped { group_id: 1 }
    ));
}

#[test]
fn market_group_extension_composes_with_h13_resolved_member_shrink() {
    let (markets, m0, m1, m2, group) = setup_group();
    let mut accounts = AccountStore::new();
    accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets,
        vec![group],
        SequencerConfig::default(),
    );

    seq.resolve_market(m2, Nanos::ZERO, 1_000).unwrap();
    assert_eq!(seq.market_groups()[0].markets, vec![m0, m1]);

    let m3 = seq.create_market("D".to_string());
    let (extended, inserted) = seq.extend_market_group(0, m3).unwrap();
    assert!(inserted);
    assert_eq!(extended.markets, vec![m0, m1, m3]);

    let bp = seq.produce_block(Vec::new(), 2_000);
    assert_eq!(bp.witness.market_groups.len(), 1);
    assert_eq!(bp.witness.market_groups[0].markets, vec![m0, m1, m3]);
    let verification = sybil_verifier::verify_full(&bp.witness, false);
    assert!(
        verification.valid,
        "violations: {:?}",
        verification.violations
    );
}

#[test]
fn group_extension_after_preexisting_group_minting_conserves_cash() {
    let mut markets = MarketSet::new();
    let m0 = markets.add_binary("A");
    let m1 = markets.add_binary("B");
    let m2 = markets.add_binary("Late C");
    let mut group = MarketGroup::new("Expandable event");
    group.add_market(m0);
    group.add_market(m1);

    let mut accounts = AccountStore::new();
    let buyer0 = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let buyer1 = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let initial_total_balance = accounts.total_balance();
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![group],
        SequencerConfig::default(),
    );

    let minted = seq.produce_block(
        vec![
            single_order_sub(buyer0, outcome_buy(&markets, 0, m0, 0, 600_000_000, q(1))),
            single_order_sub(buyer1, outcome_buy(&markets, 0, m1, 0, 500_000_000, q(1))),
        ],
        1_000,
    );
    assert_eq!(
        minted.block.fills.len(),
        2,
        "old group complete set should mint before extension"
    );
    assert_eq!(seq.accounts.total_balance(), initial_total_balance);

    let (extended, inserted) = seq.extend_market_group(0, m2).unwrap();
    assert!(inserted);
    assert_eq!(extended.markets, vec![m0, m1, m2]);

    seq.resolve_market(m2, Nanos(NANOS_PER_DOLLAR), 2_000)
        .unwrap();
    seq.resolve_market(m0, Nanos::ZERO, 2_001).unwrap();
    seq.resolve_market(m1, Nanos::ZERO, 2_002).unwrap();

    let settled = seq.produce_block(Vec::new(), 3_000);
    assert!(seq.market_groups().is_empty());
    assert_eq!(
        seq.accounts.total_balance(),
        initial_total_balance,
        "extension must not create or destroy cash; old claims settle per-market and MINT absorbs the result"
    );
    for buyer in [buyer0, buyer1] {
        let buyer_account = seq.accounts.get(buyer).unwrap();
        assert_eq!(buyer_account.position(m0, 0), 0);
        assert_eq!(buyer_account.position(m1, 0), 0);
        assert_eq!(buyer_account.position(m2, 0), 0);
    }

    let verification = sybil_verifier::verify_full(&settled.witness, false);
    assert!(
        verification.valid,
        "violations: {:?}",
        verification.violations
    );
}

#[test]
fn test_mm_complete_set_buyyes_rejected() {
    let (markets, m0, m1, m2, group) = setup_group();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![group],
        SequencerConfig::default(),
    );

    let mut constraint = MmConstraint::new(MmId::new(1), Nanos(50 * NANOS_PER_DOLLAR));
    constraint.add_order(0, matching_engine::MmSide::BuyYes);
    constraint.add_order(1, matching_engine::MmSide::BuyYes);
    constraint.add_order(2, matching_engine::MmSide::BuyYes);

    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![
            outcome_buy(&markets, 0, m0, 0, 400_000_000, 10),
            outcome_buy(&markets, 0, m1, 0, 350_000_000, 10),
            outcome_buy(&markets, 0, m2, 0, 300_000_000, 10),
        ],
        mm_constraint: Some(constraint),
    };

    let bp = seq.produce_block(vec![sub], 1000);
    // Per-order STP: only the 3rd order (completing the set) is rejected
    assert_eq!(bp.block.rejections.len(), 1);
    assert!(bp.block.fills.is_empty());
}

#[test]
fn test_mm_non_crossing_complete_group_is_accepted_without_fills() {
    let (markets, m0, m1, m2, group) = setup_group();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![group],
        SequencerConfig::default(),
    );

    let mut constraint = MmConstraint::new(MmId::new(1), Nanos(50 * NANOS_PER_DOLLAR));
    for order_id in 0..3 {
        constraint.add_order(order_id, matching_engine::MmSide::BuyYes);
    }
    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![
            outcome_buy(&markets, 0, m0, 0, 300_000_000, 10),
            outcome_buy(&markets, 0, m1, 0, 300_000_000, 10),
            outcome_buy(&markets, 0, m2, 0, 300_000_000, 10),
        ],
        mm_constraint: Some(constraint),
    };

    let production = seq.produce_block(vec![sub], 1_000);
    assert!(production.block.rejections.is_empty());
    assert!(production.block.fills.is_empty());
}

#[test]
fn test_mm_partial_group_accepted() {
    let (markets, m0, m1, _m2, group) = setup_group();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![group],
        SequencerConfig::default(),
    );

    // Only quote 2 of 3 outcomes — not a complete set
    let mut constraint = MmConstraint::new(MmId::new(1), Nanos(50 * NANOS_PER_DOLLAR));
    constraint.add_order(0, matching_engine::MmSide::BuyYes);
    constraint.add_order(1, matching_engine::MmSide::BuyYes);

    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![
            outcome_buy(&markets, 0, m0, 0, 400_000_000, 10),
            outcome_buy(&markets, 0, m1, 0, 350_000_000, 10),
        ],
        mm_constraint: Some(constraint),
    };

    let bp = seq.produce_block(vec![sub], 1000);
    assert_eq!(
        bp.block.rejections.len(),
        0,
        "Partial group should be accepted"
    );
}

#[test]
fn test_mm_same_market_both_sides_accepted() {
    // A non-crossing BuyYes + BuyNo pair is legitimate MM behavior.
    let (markets, m0, _m1, _m2, group) = setup_group();
    let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

    let mut constraint = MmConstraint::new(MmId::new(1), Nanos(50 * NANOS_PER_DOLLAR));
    constraint.add_order(0, matching_engine::MmSide::BuyYes);
    constraint.add_order(1, matching_engine::MmSide::BuyNo);

    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![
            outcome_buy(&markets, 0, m0, 0, 400_000_000, 10),
            outcome_buy(&markets, 0, m0, 1, 400_000_000, 10),
        ],
        mm_constraint: Some(constraint),
    };

    let result = run_batch(&mut seq, vec![sub], &markets, &[group]);
    assert_eq!(
        result.rejections.len(),
        0,
        "Same-market BuyYes+BuyNo should be accepted"
    );
}

#[test]
fn test_mm_same_market_crossing_bids_are_rejected() {
    let (markets, m0, _m1, _m2, group) = setup_group();
    let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

    let mut constraint = MmConstraint::new(MmId::new(1), Nanos(50 * NANOS_PER_DOLLAR));
    constraint.add_order(0, matching_engine::MmSide::BuyYes);
    constraint.add_order(1, matching_engine::MmSide::BuyNo);
    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![
            outcome_buy(&markets, 0, m0, 0, 600_000_000, 10),
            outcome_buy(&markets, 0, m0, 1, 600_000_000, 10),
        ],
        mm_constraint: Some(constraint),
    };

    let result = run_batch(&mut seq, vec![sub], &markets, &[group]);
    assert_eq!(result.rejections.len(), 1);
    assert!(result.fills.is_empty());
}

#[test]
fn test_grouped_mm_complementary_bids_require_a_price_cross() {
    for (limit, expected_rejections) in [(400_000_000, 0), (600_000_000, 1)] {
        let (markets, m0, _m1, _m2, group) = setup_group();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![group],
            SequencerConfig::default(),
        );

        let mut constraint = MmConstraint::new(MmId::new(1), Nanos(50 * NANOS_PER_DOLLAR));
        constraint.add_order(0, matching_engine::MmSide::BuyYes);
        constraint.add_order(1, matching_engine::MmSide::BuyNo);
        let submission = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, limit, 10),
                outcome_buy(&markets, 0, m0, 1, limit, 10),
            ],
            mm_constraint: Some(constraint),
        };

        let production = seq.produce_block(vec![submission], 1_000);
        assert_eq!(production.block.rejections.len(), expected_rejections);
        assert!(production.block.fills.is_empty());
    }
}

#[test]
fn test_mm_buyno_coverage_without_a_cross_is_accepted() {
    // 3-market group: BuyNo on M0 covers {M1,M2}, BuyNo on M1 covers {M0,M2}
    // Union = {M0,M1,M2}, but each NO claim still costs its own binary mint.
    let (markets, m0, m1, _m2, group) = setup_group();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![group],
        SequencerConfig::default(),
    );

    let mut constraint = MmConstraint::new(MmId::new(1), Nanos(50 * NANOS_PER_DOLLAR));
    constraint.add_order(0, matching_engine::MmSide::BuyNo);
    constraint.add_order(1, matching_engine::MmSide::BuyNo);

    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![
            outcome_buy(&markets, 0, m0, 1, 800_000_000, 10), // BuyNo M0 → covers {M1,M2}
            outcome_buy(&markets, 0, m1, 1, 800_000_000, 10), // BuyNo M1 → covers {M0,M2}
        ],
        mm_constraint: Some(constraint),
    };

    let bp = seq.produce_block(vec![sub], 1000);
    assert!(bp.block.rejections.is_empty());
    assert!(bp.block.fills.is_empty());
}

// --- MM budget capping ---

#[test]
fn test_mm_budget_clamped_to_balance() {
    // MM has $10 balance but requests $50 budget
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
    let counter = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    // Give counterparty YES positions to sell
    seq.accounts
        .get_mut(counter)
        .unwrap()
        .positions
        .insert((m0, 0), 1000);

    let mut constraint = MmConstraint::new(MmId::new(1), Nanos(50 * NANOS_PER_DOLLAR));
    constraint.add_order(0, matching_engine::MmSide::BuyYes);

    let mm_sub = OrderSubmission {
        account_id: aid,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 100)],
        mm_constraint: Some(constraint),
    };
    let sell_sub = OrderSubmission {
        account_id: counter,
        orders: vec![outcome_sell(&markets, 0, m0, 0, 400_000_000, 100)],
        mm_constraint: None,
    };

    let _result = run_batch(&mut seq, vec![mm_sub, sell_sub], &markets, &[]);

    // MM balance should never go below 0
    let mm_acct = seq.accounts.get(aid).unwrap();
    assert!(
        mm_acct.balance >= 0,
        "MM balance negative: {}",
        mm_acct.balance
    );
}

#[test]
fn test_bankrupt_mm_skipped() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(0); // zero balance
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        MarketSet::new(),
        vec![],
        SequencerConfig::default(),
    );

    let mut constraint = MmConstraint::new(MmId::new(1), Nanos(50 * NANOS_PER_DOLLAR));
    constraint.add_order(0, matching_engine::MmSide::BuyYes);

    let sub = OrderSubmission {
        account_id: aid,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 100)],
        mm_constraint: Some(constraint),
    };

    let bp = seq.produce_block(vec![sub], 1000);
    assert!(
        bp.block.fills.is_empty(),
        "Bankrupt MM should not generate fills"
    );
}

/// Verify that group minting maintains position balance across multiple blocks.
///
/// This is the key test for the MINT account mechanism: when the MM buys
/// YES on all markets in a group, group minting creates YES without NO
/// counterparties. The sequencer must derive the minting and adjust MINT
/// so that total_yes == total_no for every market, every block.
#[test]
fn test_group_minting_position_balance_multi_block() {
    use matching_engine::{MarketGroup, simple_yes_buy};

    let mut markets = MarketSet::new();
    let m0 = markets.add_binary("A");
    let m1 = markets.add_binary("B");
    let m2 = markets.add_binary("C");

    let mut group = MarketGroup::new("Election");
    group.add_market(m0);
    group.add_market(m1);
    group.add_market(m2);

    let mut accounts = AccountStore::new();
    let buyer = accounts.create_account(1_000_000 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![group.clone()],
        SequencerConfig::default(),
    );

    // Run 5 blocks, each with BuyYes on all 3 group markets.
    // Group minting will fire each time. MINT must stay balanced.
    for block_num in 0..5 {
        let sub = OrderSubmission {
            account_id: buyer,
            orders: vec![
                simple_yes_buy(&markets, 0, m0, 400_000_000, 100),
                simple_yes_buy(&markets, 0, m1, 350_000_000, 100),
                simple_yes_buy(&markets, 0, m2, 300_000_000, 100),
            ],
            mm_constraint: None,
        };

        let bp = seq.produce_block(vec![sub], (block_num + 1) * 1000);

        // The position balance check inside produce_block should not fire,
        // but let's verify explicitly:
        for &mid in &[m0, m1, m2] {
            let total_yes: i64 = seq.accounts.iter().map(|(_, a)| a.position(mid, 0)).sum();
            let total_no: i64 = seq.accounts.iter().map(|(_, a)| a.position(mid, 1)).sum();
            assert_eq!(
                total_yes, total_no,
                "Position imbalance in market {:?} at block {}: YES={} NO={}",
                mid, block_num, total_yes, total_no
            );
        }

        // Money conservation: total balance should only change by resolution payouts
        // (none here), so it should equal the initial deposit.
        let total_balance: i64 = seq.accounts.iter().map(|(_, a)| a.balance).sum();
        assert_eq!(
            total_balance,
            1_000_000 * NANOS_PER_DOLLAR as i64,
            "Money conservation violated at block {}",
            block_num
        );

        // Verify MINT exists and has positions
        if !bp.block.fills.is_empty() {
            let mint = seq.accounts.get(crate::account::AccountId::MINT).unwrap();
            // MINT should have non-zero balance (revenue from selling)
            // and negative positions (shorts from minting)
            assert!(
                !mint.positions.is_empty(),
                "MINT should hold positions after group minting"
            );
        }
    }
}

#[test]
fn test_mm_balance_nonnegative_across_blocks() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    let mm_id = accounts.create_account(1000 * NANOS_PER_DOLLAR as i64);
    let counter_id = accounts.create_account(100_000 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    // Give counterparty massive YES position to sell
    seq.accounts
        .get_mut(counter_id)
        .unwrap()
        .positions
        .insert((m0, 0), 100_000);

    for block_num in 0..10 {
        let mut constraint = MmConstraint::new(MmId::new(1), Nanos(500 * NANOS_PER_DOLLAR));
        constraint.add_order(0, matching_engine::MmSide::BuyYes);

        let mm_sub = OrderSubmission {
            account_id: mm_id,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 1000)],
            mm_constraint: Some(constraint),
        };
        let counter_sub = OrderSubmission {
            account_id: counter_id,
            orders: vec![outcome_sell(&markets, 0, m0, 0, 400_000_000, 1000)],
            mm_constraint: None,
        };

        run_batch(&mut seq, vec![mm_sub, counter_sub], &markets, &[]);

        let mm_acct = seq.accounts.get(mm_id).unwrap();
        assert!(
            mm_acct.balance >= 0,
            "MM balance negative at block {}: {}",
            block_num,
            mm_acct.balance
        );
    }
}

// --- Cross-block STP (SYB-110) ---

fn make_grouped_sequencer(
    balance: i64,
) -> (BlockSequencer, AccountId, MarketSet, MarketId, MarketId) {
    let mut markets = MarketSet::new();
    let m0 = markets.add_binary("A");
    let m1 = markets.add_binary("B");
    let mut group = MarketGroup::new("Event");
    group.add_market(m0);
    group.add_market(m1);
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(balance);
    let seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![group],
        SequencerConfig::default(),
    );
    (seq, aid, markets, m0, m1)
}

fn single_order_sub(account_id: AccountId, order: Order) -> OrderSubmission {
    OrderSubmission {
        account_id,
        orders: vec![order],
        mm_constraint: None,
    }
}

#[test]
fn open_batch_unique_placers_filters_resting_orders_by_market() {
    let mut markets = MarketSet::new();
    let m0 = markets.add_binary("A");
    let m1 = markets.add_binary("B");
    let mut accounts = AccountStore::new();
    let a0 = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let a1 = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let a2 = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    for (account_id, market_id) in [(a0, m0), (a1, m0), (a2, m1)] {
        let order = outcome_buy(&markets, 0, market_id, 0, 400_000_000, q(1));
        assert!(matches!(
            seq.try_admit_direct(single_order_sub(account_id, order), 0),
            AdmitOutcome::Admitted { .. }
        ));
    }

    assert_eq!(seq.open_batch_unique_placers(m0), 2);
    assert_eq!(seq.open_batch_unique_placers(m1), 1);
}

#[test]
fn direct_admission_rejects_non_one_hot_order() {
    let (mut seq, aid, _markets, m0, _) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);
    let order = raw_single_market_order(m0, [2, 0], 500_000_000, q(1));

    match seq.try_admit_direct(single_order_sub(aid, order), 0) {
        AdmitOutcome::Rejected(_) => {}
        other => panic!("expected non-one-hot rejection, got {:?}", other),
    }
}

#[test]
fn direct_admission_rejects_multi_market_order() {
    let (mut seq, aid, markets, m0, m1) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);
    let order = matching_engine::bundle_yes(&markets, 0, &[m0, m1], 400_000_000, q(1));

    match seq.try_admit_direct(single_order_sub(aid, order), 0) {
        AdmitOutcome::Rejected(_) => {}
        other => panic!("expected multi-market rejection, got {:?}", other),
    }
}

#[test]
fn direct_admission_rejects_oversized_quantity() {
    let (mut seq, aid, _markets, m0, _) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);
    let order =
        raw_single_market_order(m0, [1, 0], 500_000_000, matching_engine::MAX_ORDER_QTY + 1);

    match seq.try_admit_direct(single_order_sub(aid, order), 0) {
        AdmitOutcome::Rejected(_) => {}
        other => panic!("expected oversized quantity rejection, got {:?}", other),
    }
}

#[test]
fn direct_write_path_rejects_zero_and_subminimum_resting_orders() {
    let (mut seq, aid, _markets, m0, _) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);
    assert_eq!(
        seq.config.min_resting_order_notional_nanos,
        crate::sequencer::DEFAULT_MIN_RESTING_ORDER_NOTIONAL_NANOS
    );

    for (order, expected) in [
        (
            raw_single_market_order(m0, [1, 0], 0, q(1)),
            "price must be greater than zero",
        ),
        (
            raw_single_market_order(m0, [1, 0], NANOS_PER_DOLLAR, 0),
            "quantity must be greater than zero",
        ),
        (raw_single_market_order(m0, [1, 0], 1, 1), "below minimum"),
    ] {
        match seq.try_admit_direct(single_order_sub(aid, order), 0) {
            AdmitOutcome::Rejected(SequencerError::Rejected(Rejection {
                reason: RejectionReason::InvalidOrder(reason),
                ..
            })) => assert!(reason.contains(expected), "unexpected reason: {reason}"),
            other => panic!("expected admission rejection, got {other:?}"),
        }
    }
    assert_eq!(seq.open_orders_for_account(aid), 0);

    // One minimum quantity unit at a $1 limit is exactly the default floor.
    let boundary = raw_single_market_order(m0, [1, 0], NANOS_PER_DOLLAR, 1);
    assert!(matches!(
        seq.try_admit_direct(single_order_sub(aid, boundary), 0),
        AdmitOutcome::Admitted { .. }
    ));
    assert_eq!(seq.open_orders_for_account(aid), 1);
}

#[test]
fn cross_block_stp_rejects_set_formation_across_blocks() {
    let (mut seq, aid, markets, m0, m1) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);

    let first = single_order_sub(aid, outcome_buy(&markets, 0, m0, 0, 600_000_000, 10));
    let outcome = seq.try_admit_direct(first, 0);
    assert!(matches!(outcome, AdmitOutcome::Admitted { .. }));

    seq.produce_block(vec![], 1000);
    assert_eq!(seq.height, 1);

    let second = single_order_sub(aid, outcome_buy(&markets, 0, m1, 0, 600_000_000, 10));
    let outcome = seq.try_admit_direct(second, 0);
    match outcome {
        AdmitOutcome::Rejected(SequencerError::Rejected(r)) => {
            assert!(matches!(r.reason, RejectionReason::CompleteSetFormation));
        }
        other => panic!("expected CompleteSetFormation rejection, got {:?}", other),
    }
}

#[test]
fn cross_block_stp_accepts_non_crossing_complete_coverage() {
    let (mut seq, aid, markets, m0, m1) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);

    let first = single_order_sub(aid, outcome_buy(&markets, 0, m0, 0, 400_000_000, 10));
    assert!(matches!(
        seq.try_admit_direct(first, 0),
        AdmitOutcome::Admitted { .. }
    ));
    seq.produce_block(vec![], 1_000);

    let second = single_order_sub(aid, outcome_buy(&markets, 0, m1, 0, 400_000_000, 10));
    assert!(matches!(
        seq.try_admit_direct(second, 0),
        AdmitOutcome::Admitted { .. }
    ));
}

#[test]
fn stp_undo_preserves_other_accounts_same_block_expired_history_and_state_root() {
    let (markets, m0, m1, _m2, mut group) = setup_group();
    group.markets = vec![m0, m1];
    let mut accounts = AccountStore::new();
    let stp_account = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let expiring_account = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![group],
        SequencerConfig {
            debug_verify_full: true,
            ..SequencerConfig::default()
        },
    );

    let stp_resting = outcome_buy(&markets, 0, m0, 0, 600_000_000, q(1));
    assert!(matches!(
        seq.try_admit_direct(single_order_sub(stp_account, stp_resting), 100),
        AdmitOutcome::Admitted { .. }
    ));

    let mut expiring = outcome_buy(&markets, 0, m0, 0, 300_000_000, q(1));
    expiring.expires_at_block = Some(1);
    let expiring_order_id =
        match seq.try_admit_direct(single_order_sub(expiring_account, expiring), 200) {
            AdmitOutcome::Admitted { order_id, .. } => order_id,
            other => panic!("expected expiring order admission, got {other:?}"),
        };

    let balance_before = seq.accounts.get(stp_account).unwrap().balance;
    let reservation_before = seq.order_book.reserved_balance(stp_account);
    let completing = single_order_sub(
        stp_account,
        outcome_buy(&markets, 0, m1, 0, 600_000_000, q(1)),
    );

    let production = seq.produce_block(vec![completing], 1_000);

    assert_eq!(production.block.rejections.len(), 1);
    assert!(matches!(
        production.block.rejections[0].reason,
        RejectionReason::CompleteSetFormation
    ));
    assert_eq!(production.derived_view_sidecar.rejection_history.len(), 1);
    assert!(matches!(
        production.derived_view_sidecar.rejection_history[0].reason,
        RejectionReason::CompleteSetFormation
    ));
    assert_eq!(
        seq.order_book.reserved_balance(stp_account),
        reservation_before,
        "the rejected order's reservation must be fully released"
    );
    assert_eq!(
        seq.accounts.get(stp_account).unwrap().balance,
        balance_before
    );

    assert!(
        production
            .derived_view_sidecar
            .removed_orders
            .iter()
            .any(|removed| {
                removed.account_id == expiring_account.0
                    && removed.order_id == expiring_order_id
                    && removed.phase == crate::block::RemovedOrderPhase::PostSolve
                    && removed.exit_reason == crate::block::RemovedOrderExitReason::Expired
            })
    );
    assert_eq!(
        production.block.header.state_root,
        production.witness.header.state_root
    );
    assert_eq!(
        production.block.header.state_root,
        crate::block::compute_complete_state_root(
            &seq.accounts,
            seq.bridge_state(),
            seq.order_book(),
            seq.markets(),
            seq.market_groups(),
            seq.market_lifecycle(),
            seq.analytics().last_clearing_prices(),
        )
    );
    let verification = sybil_verifier::verify_full(&production.witness, false);
    assert!(
        verification.valid,
        "violations: {:?}",
        verification.violations
    );
}

#[test]
fn cross_block_stp_allows_after_cancel() {
    let (mut seq, aid, markets, m0, m1) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);

    let first = single_order_sub(aid, outcome_buy(&markets, 0, m0, 0, 400_000_000, 10));
    let first_id = match seq.try_admit_direct(first, 0) {
        AdmitOutcome::Admitted { order_id, .. } => order_id,
        other => panic!("expected Admitted, got {:?}", other),
    };

    seq.produce_block(vec![], 1000);

    seq.cancel_pending_order(aid, first_id).expect("cancel ok");

    let second = single_order_sub(aid, outcome_buy(&markets, 0, m1, 0, 400_000_000, 10));
    let outcome = seq.try_admit_direct(second, 0);
    assert!(
        matches!(outcome, AdmitOutcome::Admitted { .. }),
        "expected Admitted after cancel, got {:?}",
        outcome
    );
}

#[test]
fn direct_ioc_order_participates_once_and_never_rests() {
    let (mut seq, aid, markets, m0, _) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);
    let mut order = outcome_buy(&markets, 0, m0, 0, 400_000_000, 10);
    order.expires_at_block = Some(1);

    assert!(matches!(
        seq.try_admit_direct(single_order_sub(aid, order), 0),
        AdmitOutcome::Admitted { .. }
    ));

    let bp = seq.produce_block(vec![], 1000);
    assert_eq!(bp.flow_metrics.carried_resting_orders, 1);
    assert_eq!(seq.pending_orders_info(Some(aid)).len(), 0);
}

#[test]
fn gtd_order_expires_after_requested_block() {
    let (mut seq, aid, markets, m0, _) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);
    let mut order = outcome_buy(&markets, 0, m0, 0, 400_000_000, 10);
    order.expires_at_block = Some(2);

    assert!(matches!(
        seq.try_admit_direct(single_order_sub(aid, order), 0),
        AdmitOutcome::Admitted { .. }
    ));

    seq.produce_block(vec![], 1000);
    assert_eq!(seq.pending_orders_info(Some(aid)).len(), 1);

    seq.produce_block(vec![], 2000);
    assert_eq!(seq.pending_orders_info(Some(aid)).len(), 0);
}

#[test]
fn direct_gtd_order_rejects_when_it_cannot_reach_next_batch() {
    let (mut seq, aid, markets, m0, _) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);
    let mut order = outcome_buy(&markets, 0, m0, 0, 400_000_000, 10);
    order.expires_at_block = Some(0);

    match seq.try_admit_direct(single_order_sub(aid, order), 0) {
        AdmitOutcome::Rejected(SequencerError::Rejected(rejection)) => {
            assert!(matches!(rejection.reason, RejectionReason::Expired { .. }));
        }
        other => panic!("expected expired rejection, got {:?}", other),
    }
}

#[test]
fn cross_block_stp_accepts_non_crossing_buyno_coverage() {
    let (markets, m0, m1, _m2, group) = setup_group();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![group],
        SequencerConfig::default(),
    );

    let first = single_order_sub(aid, outcome_buy(&markets, 0, m0, 1, 800_000_000, 10));
    assert!(matches!(
        seq.try_admit_direct(first, 0),
        AdmitOutcome::Admitted { .. }
    ));

    seq.produce_block(vec![], 1000);

    let second = single_order_sub(aid, outcome_buy(&markets, 0, m1, 1, 800_000_000, 10));
    assert!(matches!(
        seq.try_admit_direct(second, 0),
        AdmitOutcome::Admitted { .. }
    ));
}

#[test]
fn cross_block_stp_sells_do_not_contribute() {
    let (mut seq, aid, markets, m0, m1) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);

    seq.accounts
        .get_mut(aid)
        .unwrap()
        .positions
        .insert((m0, 0), 50);
    seq.accounts
        .get_mut(aid)
        .unwrap()
        .positions
        .insert((m0, 1), 50);

    let sell_first = single_order_sub(aid, outcome_sell(&markets, 0, m0, 0, 400_000_000, 10));
    assert!(matches!(
        seq.try_admit_direct(sell_first, 0),
        AdmitOutcome::Admitted { .. }
    ));

    seq.produce_block(vec![], 1000);

    let buy_other = single_order_sub(aid, outcome_buy(&markets, 0, m1, 0, 400_000_000, 10));
    assert!(
        matches!(
            seq.try_admit_direct(buy_other, 0),
            AdmitOutcome::Admitted { .. }
        ),
        "sell on m0 + buy on m1 is only partial coverage — must be admitted"
    );
}

#[test]
fn cross_block_stp_mm_path_sees_prior_resting() {
    // Account first places a non-MM BuyYes m0 through the admit path, then in
    // a later block submits an MM bundle that includes BuyYes m1. The MM
    // bundle's STP check (inside prepare_block) must see the prior-block
    // resting order and reject the completing leg.
    let (mut seq, aid, markets, m0, m1) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);

    let first = single_order_sub(aid, outcome_buy(&markets, 0, m0, 0, 600_000_000, 10));
    assert!(matches!(
        seq.try_admit_direct(first, 0),
        AdmitOutcome::Admitted { .. }
    ));
    seq.produce_block(vec![], 1000);

    let mut constraint = MmConstraint::new(MmId::new(1), Nanos(50 * NANOS_PER_DOLLAR));
    constraint.add_order(0, matching_engine::MmSide::BuyYes);
    let mm_sub = OrderSubmission {
        account_id: aid,
        orders: vec![outcome_buy(&markets, 0, m1, 0, 600_000_000, 10)],
        mm_constraint: Some(constraint),
    };

    let bp = seq.produce_block(vec![mm_sub], 2000);
    assert_eq!(
        bp.block.rejections.len(),
        1,
        "MM completing leg should be rejected because prior-block resting covers m0"
    );
    assert!(matches!(
        bp.block.rejections[0].reason,
        RejectionReason::CompleteSetFormation
    ));
}

#[test]
fn cross_block_stp_pending_bundle_contributes_to_admit_check() {
    // A multi-order non-MM bundle stays in pending_bundles (not single-order
    // so try_admit_direct defers it). A later single-order admit must see the
    // bundled coverage and reject if it would complete the set.
    let (markets, m0, m1, m2, group) = setup_group();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![group],
        SequencerConfig::default(),
    );

    let bundle = OrderSubmission {
        account_id: aid,
        orders: vec![
            outcome_buy(&markets, 0, m0, 0, 400_000_000, 10),
            outcome_buy(&markets, 0, m1, 0, 400_000_000, 10),
        ],
        mm_constraint: None,
    };
    match seq.try_admit_direct(bundle, 0) {
        AdmitOutcome::Deferred { submission, .. } => seq.push_pending_bundle(submission),
        other => panic!("expected Deferred for multi-order bundle, got {:?}", other),
    }

    let completing = single_order_sub(aid, outcome_buy(&markets, 0, m2, 0, 400_000_000, 10));
    match seq.try_admit_direct(completing, 0) {
        AdmitOutcome::Rejected(SequencerError::Rejected(r)) => {
            assert!(matches!(r.reason, RejectionReason::CompleteSetFormation));
        }
        other => panic!(
            "expected CompleteSetFormation rejection from pending-bundle coverage, got {:?}",
            other
        ),
    }
}

#[test]
fn first_deposit_records_once() {
    // fund_account stamps first_deposit_ms; a subsequent fund_account
    // for the same account must NOT overwrite it.
    let (markets, _m0) = setup();
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(0);
    let mut seq =
        BlockSequencer::with_default_solver(accounts, markets, vec![], SequencerConfig::default());

    assert!(seq.analytics().first_deposit_ms(aid).is_none());

    seq.fund_account(aid, 10 * NANOS_PER_DOLLAR as i64).unwrap();
    let ts_first = seq
        .analytics()
        .first_deposit_ms(aid)
        .expect("first deposit should be recorded after fund_account");

    // Sleep a tiny bit so the second SystemTime::now() differs.
    std::thread::sleep(std::time::Duration::from_millis(2));

    seq.fund_account(aid, NANOS_PER_DOLLAR as i64).unwrap();
    let ts_second = seq
        .analytics()
        .first_deposit_ms(aid)
        .expect("first_deposit_ms must persist after a second deposit");

    assert_eq!(
        ts_first, ts_second,
        "second deposit must not overwrite the first-deposit timestamp"
    );
}

/// D1: cancelling a resting order must stage a `SystemEvent::OrderCancelled`
/// with the order's primary market, derived direction, and unfilled
/// remainder. The next produced block surfaces it in `system_events` and
/// the cancelling account's `events_digest` advances.
#[test]
fn cancel_emits_order_cancelled() {
    let (mut seq, aid, markets, m0, _) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);

    let order = outcome_buy(&markets, 0, m0, 0, 400_000_000, 7);
    let order_id = match seq.try_admit_direct(single_order_sub(aid, order), 0) {
        AdmitOutcome::Admitted { order_id, .. } => order_id,
        other => panic!("expected Admitted, got {:?}", other),
    };

    seq.produce_block(vec![], 1_000);

    let digest_before = seq.accounts.get(aid).expect("account exists").events_digest;

    seq.cancel_pending_order(aid, order_id).expect("cancel ok");

    let pending = &seq.pending_system_events;
    let event = pending
        .iter()
        .find(|e| matches!(e, SystemEvent::OrderCancelled { .. }))
        .expect("OrderCancelled staged");
    match event {
        SystemEvent::OrderCancelled {
            account_id,
            order_id: oid,
            market_ids,
            side,
            remaining_quantity,
        } => {
            assert_eq!(*account_id, aid);
            assert_eq!(*oid, order_id);
            assert_eq!(market_ids, &vec![m0]);
            assert_eq!(*side, matching_engine::OrderDirection::BuyYes);
            assert_eq!(*remaining_quantity, 7);
        }
        _ => unreachable!(),
    }

    let bp = seq.produce_block(vec![], 2_000);
    assert!(
        bp.block.system_events.iter().any(|e| matches!(
            e,
            SystemEvent::OrderCancelled { order_id: oid, .. } if *oid == order_id
        )),
        "block must surface the OrderCancelled SystemEvent"
    );

    let digest_after = seq.accounts.get(aid).expect("account exists").events_digest;
    assert_ne!(
        digest_before, digest_after,
        "cancelling account's events_digest must advance"
    );
}

/// D1: cancelling a non-existent order must NOT stage any SystemEvent.
#[test]
fn cancel_nonexistent_does_not_emit_order_cancelled() {
    let (mut seq, aid, _markets, _m0, _m1) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);

    let pending_before = seq.pending_system_events.len();
    let result = seq.cancel_pending_order(aid, 9_999);
    assert!(result.is_err());
    assert_eq!(seq.pending_system_events.len(), pending_before);
}

#[test]
fn can_cancel_pending_order_matches_apply_validation() {
    let (mut seq, owner, markets, market_id, _) =
        make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);
    let order_id = match seq.try_admit_direct(
        single_order_sub(
            owner,
            outcome_buy(&markets, 0, market_id, 0, 400_000_000, 7),
        ),
        0,
    ) {
        AdmitOutcome::Admitted { order_id, .. } => order_id,
        other => panic!("expected Admitted, got {other:?}"),
    };
    let wrong_owner = AccountId(owner.0 + 1);

    let cases = [
        ("owned order", owner, order_id, true),
        ("wrong owner", wrong_owner, order_id, false),
        ("missing order", owner, order_id + 1, false),
    ];

    for (name, account_id, candidate_order_id, expected_ok) in cases {
        let preflight = seq.can_cancel_pending_order(account_id, candidate_order_id, 1_000);
        let mut applying = seq.clone();
        let apply = applying.cancel_pending_order_at(account_id, candidate_order_id, 1_000);

        assert_eq!(preflight.is_ok(), expected_ok, "preflight: {name}");
        assert_eq!(apply.is_ok(), expected_ok, "apply: {name}");
        assert_eq!(
            preflight.is_ok(),
            apply.is_ok(),
            "preflight/apply parity: {name}"
        );
        if let (Err(preflight), Err(apply)) = (preflight, apply) {
            assert_eq!(
                std::mem::discriminant(&preflight),
                std::mem::discriminant(&apply),
                "preflight/apply error parity: {name}"
            );
        }
    }
}

// --- Mark-price portfolio valuation ---

/// After a crossing batch at price P, the mark is set to the clearing
/// price P.  In the next batch, two resting orders form a two-sided
/// spread (bid 40c / ask 60c) but do NOT cross.  The mark should move
/// to the book midpoint (50c), and `portfolio_summary` must reflect
/// that midpoint — not the old clearing price — for the holder's
/// unrealized PnL valuation.
#[test]
fn portfolio_summary_values_positions_at_book_midpoint_mark() {
    let (markets, m0) = setup();
    let mut accounts = AccountStore::new();
    // buyer: will end up holding YES after the crossing batch
    let buyer_id = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    // seller: provides YES supply so the cross can happen
    let seller_id = accounts.create_account(0);
    accounts
        .get_mut(seller_id)
        .unwrap()
        .positions
        .insert((m0, 0), 50);
    // maker accounts for the resting spread in batch 2
    let bidder_id = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let asker_id = accounts.create_account(0);
    accounts
        .get_mut(asker_id)
        .unwrap()
        .positions
        .insert((m0, 0), 50);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig::default(),
    );

    // --- Batch 1: crossing at 70c (buyer) / 30c (seller) ---
    // buyer buys YES at 70c, seller sells YES at 30c — they must cross.
    let buy_sub = OrderSubmission {
        account_id: buyer_id,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 700_000_000, 10)],
        mm_constraint: None,
    };
    let sell_sub = OrderSubmission {
        account_id: seller_id,
        orders: vec![outcome_sell(&markets, 0, m0, 0, 300_000_000, 10)],
        mm_constraint: None,
    };
    seq.produce_block(vec![buy_sub, sell_sub], 1_000);

    // Sanity: the buyer now holds YES units.
    assert!(
        seq.accounts.get(buyer_id).unwrap().position(m0, 0) > 0,
        "buyer must have a YES position after the crossing batch"
    );

    // The clearing price after a 70c bid / 30c ask cross is NOT 50c.
    // Verify the mark at this point differs from 50c so the subsequent
    // assertion is meaningful.
    let mark_after_cross = seq
        .analytics()
        .last_mark_prices()
        .get(&m0)
        .and_then(|v| v.first().copied())
        .expect("mark price must be set after a filled batch");
    assert_ne!(
        mark_after_cross,
        Nanos(500_000_000),
        "clearing mark must differ from 50c so the midpoint assertion is non-trivial"
    );

    // --- Batch 2: resting spread, no cross ---
    // bid at 40c, ask at 60c → midpoint = 50c, nothing crosses.
    let bid_sub = OrderSubmission {
        account_id: bidder_id,
        orders: vec![outcome_buy(&markets, 0, m0, 0, 400_000_000, 5)],
        mm_constraint: None,
    };
    let ask_sub = OrderSubmission {
        account_id: asker_id,
        orders: vec![outcome_sell(&markets, 0, m0, 0, 600_000_000, 5)],
        mm_constraint: None,
    };
    seq.produce_block(vec![bid_sub, ask_sub], 2_000);

    // The mark must now be the book midpoint: (400_000_000 + 600_000_000) / 2 = 500_000_000.
    let mark_after_spread = seq
        .analytics()
        .last_mark_prices()
        .get(&m0)
        .and_then(|v| v.first().copied())
        .expect("mark price must be set after a no-cross batch with a two-sided book");
    assert_eq!(
        mark_after_spread,
        Nanos(500_000_000),
        "mark must equal the 50c book midpoint after a non-crossing batch"
    );

    // portfolio_summary must value the YES position at the 50c mark.
    let summary = seq
        .portfolio_summary(buyer_id)
        .expect("portfolio_summary must succeed for a known account");

    let pos = summary
        .positions
        .iter()
        .find(|p| p.market_id == m0 && p.outcome == 0)
        .expect("buyer must have a valued YES position in portfolio summary");

    assert_eq!(
        pos.current_price_nanos,
        Nanos(500_000_000),
        "portfolio must value the YES position at the 50c book-midpoint mark, not the old clearing price"
    );
}
