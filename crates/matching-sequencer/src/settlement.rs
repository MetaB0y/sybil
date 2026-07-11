use matching_engine::{
    compute_fill_settlement, notional_nanos, Fill, MarketId, MintAdjustment, Nanos, Order, Qty,
    SettlementDelta, NANOS_PER_DOLLAR,
};

use crate::account::{Account, AccountId, AccountStore};
use crate::digest;
use crate::error::{BlockInvariantFailure, UnsettleableFillReason};

/// Settle a single fill: update the account's balance and positions.
///
/// Delegates to `matching_engine::compute_fill_settlement` for the pure math,
/// then applies the computed deltas to the account.
pub fn settle_fill(account: &mut Account, order: &Order, fill: &Fill) -> bool {
    settle_fill_with_delta(account, order, fill).is_some()
}

fn settle_fill_with_delta(
    account: &mut Account,
    order: &Order,
    fill: &Fill,
) -> Option<SettlementDelta> {
    let delta = compute_fill_settlement(order, fill)?;

    account.balance += delta.balance_delta;
    for &(market, outcome, qty_delta) in &delta.position_deltas {
        *account.positions.entry((market, outcome)).or_insert(0) += qty_delta;
    }
    Some(delta)
}

/// Settle all fills from a batch result. Each fill carries its own `account_id`.
#[tracing::instrument(
    skip_all,
    fields(height = block_height, fills = fills.len(), orders = orders.len())
)]
pub fn settle_batch(
    accounts: &mut AccountStore,
    fills: &[Fill],
    orders: &[Order],
    block_height: u64,
) -> Vec<BlockInvariantFailure> {
    settle_batch_with_position_deltas(accounts, fills, orders, block_height).0
}

/// Settle a batch and return the position deltas that were actually applied.
///
/// The finalize path uses these deltas to advance its pre-settlement market
/// totals without rebuilding canonical account state between fills and MINT
/// settlement. Deltas for missing orders/accounts and zero or unsettleable
/// fills are excluded, exactly matching account mutation.
pub(crate) fn settle_batch_with_position_deltas(
    accounts: &mut AccountStore,
    fills: &[Fill],
    orders: &[Order],
    block_height: u64,
) -> (Vec<BlockInvariantFailure>, Vec<(MarketId, u8, i64)>) {
    // Build order lookup
    let order_map: std::collections::HashMap<u64, &Order> =
        orders.iter().map(|o| (o.id, o)).collect();
    let mut failures = Vec::new();
    let mut position_deltas = Vec::new();

    for fill in fills {
        if fill.fill_qty.0 == 0 {
            continue;
        }

        let account_id = AccountId(fill.account_id);
        let Some(order) = order_map.get(&fill.order_id) else {
            failures.push(BlockInvariantFailure::UnsettleableFill {
                order_id: fill.order_id,
                account_id: fill.account_id,
                reason: UnsettleableFillReason::MissingOrder,
            });
            continue;
        };
        let Some(account) = accounts.get_mut(account_id) else {
            failures.push(BlockInvariantFailure::UnsettleableFill {
                order_id: fill.order_id,
                account_id: fill.account_id,
                reason: UnsettleableFillReason::MissingAccount,
            });
            continue;
        };

        let Some(delta) = settle_fill_with_delta(account, order, fill) else {
            failures.push(BlockInvariantFailure::UnsettleableFill {
                order_id: fill.order_id,
                account_id: fill.account_id,
                reason: UnsettleableFillReason::SettlementOverflow,
            });
            continue;
        };
        position_deltas.extend(delta.position_deltas);
        let event =
            digest::encode_fill_event(fill.order_id, fill.fill_qty, fill.fill_price, block_height);
        account.events_digest = digest::update_digest(&account.events_digest, &event);
    }

    (failures, position_deltas)
}

/// Apply minting adjustments to the MINT account.
pub fn apply_minting(mint: &mut Account, adjustments: &[MintAdjustment], block_height: u64) {
    for adj in adjustments {
        *mint
            .positions
            .entry((adj.market_id, adj.outcome))
            .or_insert(0) += adj.position_delta;
        mint.balance += adj.balance_delta;
    }
    if !adjustments.is_empty() {
        let event = digest::encode_mint_event(adjustments, block_height);
        mint.events_digest = digest::update_digest(&mint.events_digest, &event);
    }
}

/// Resolve a market: convert positions to balance based on fractional payouts.
///
/// `yes_payout_nanos`: payout per YES share in nanos (0 to NANOS_PER_DOLLAR).
/// NO shares receive `NANOS_PER_DOLLAR - yes_payout_nanos` per share.
///
/// Special cases:
/// - `yes_payout_nanos = NANOS_PER_DOLLAR` → YES wins (traditional binary)
/// - `yes_payout_nanos = 0` → NO wins (traditional binary)
/// - `yes_payout_nanos = 700_000_000` → YES pays $0.70, NO pays $0.30
pub fn resolve_market(
    accounts: &mut AccountStore,
    market: MarketId,
    yes_payout_nanos: Nanos,
) -> Vec<AccountId> {
    debug_assert!(
        yes_payout_nanos.0 <= NANOS_PER_DOLLAR,
        "YES payout must not exceed one dollar"
    );
    let no_payout_nanos = Nanos(NANOS_PER_DOLLAR.saturating_sub(yes_payout_nanos.0));
    let mut affected_accounts = Vec::new();

    // Collect account IDs first to avoid borrow issues
    let account_ids: Vec<AccountId> = accounts.iter().map(|(&id, _)| id).collect();

    for account_id in account_ids {
        let account = accounts
            .get_mut(account_id)
            .expect("account present: id sourced from this AccountStore");

        let yes_pos = account.positions.remove(&(market, 0)).unwrap_or(0);
        let no_pos = account.positions.remove(&(market, 1)).unwrap_or(0);

        if yes_pos != 0 || no_pos != 0 {
            affected_accounts.push(account_id);
        }

        if yes_pos != 0 {
            account.balance += notional_nanos(yes_payout_nanos, Qty(yes_pos.unsigned_abs())).0
                as i64
                * yes_pos.signum();
        }
        if no_pos != 0 {
            account.balance += notional_nanos(no_payout_nanos, Qty(no_pos.unsigned_abs())).0 as i64
                * no_pos.signum();
        }
    }

    affected_accounts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use crate::canonical_state::CanonicalState;
    use matching_engine::{outcome_buy, outcome_sell, shares_to_qty, MarketSet, NANOS_PER_DOLLAR};
    use proptest::prelude::*;
    use std::collections::HashMap;
    use sybil_verifier::{BlockWitness, WitnessBlockHeader, WitnessOrder};

    fn setup() -> (MarketSet, AccountStore) {
        let mut markets = MarketSet::new();
        markets.add_binary("Test Market");
        let mut accounts = AccountStore::new();
        accounts.create_account(100 * NANOS_PER_DOLLAR as i64); // $100
        (markets, accounts)
    }

    fn snapshot_accounts(accounts: &AccountStore) -> Vec<sybil_verifier::AccountSnapshot> {
        CanonicalState::from_accounts(accounts).into_snapshots()
    }

    fn empty_header() -> WitnessBlockHeader {
        WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [0u8; 32],
            events_root: sybil_verifier::event_commitment::empty_events_root(),
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 0,
        }
    }

    fn q(shares: u64) -> u64 {
        shares_to_qty(shares).0
    }

    #[test]
    fn test_settle_yes_buy() {
        let (markets, mut accounts) = setup();
        let m0 = MarketId::new(0);
        let aid = AccountId(0);

        let qty = q(10);
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, qty); // Buy YES at 0.50, qty 10
        let fill = Fill::new(1, Qty(qty), Nanos(500_000_000)); // Filled at 0.50

        let account = accounts
            .get_mut(aid)
            .expect("account present: id sourced from this AccountStore");
        assert!(settle_fill(account, &order, &fill));

        // Should have paid 0.50 * 10 = 5 nanos * 10
        let expected_cost = notional_nanos(Nanos(500_000_000), Qty(qty)).0 as i64;
        assert_eq!(
            account.balance,
            100 * NANOS_PER_DOLLAR as i64 - expected_cost
        );
        assert_eq!(account.position(m0, 0), qty as i64); // 10 YES shares
    }

    #[test]
    fn test_settle_yes_sell() {
        let (markets, mut accounts) = setup();
        let m0 = MarketId::new(0);
        let aid = AccountId(0);

        // First give the account some position
        let account = accounts
            .get_mut(aid)
            .expect("account present: id sourced from this AccountStore");
        account.positions.insert((m0, 0), q(10) as i64);

        let qty = q(5);
        let order = outcome_sell(&markets, 2, m0, 0, 500_000_000, qty); // Sell YES at 0.50, qty 5
        let fill = Fill::new(2, Qty(qty), Nanos(500_000_000));

        assert!(settle_fill(account, &order, &fill));

        // Should have received 0.50 * 5
        let expected_revenue = notional_nanos(Nanos(500_000_000), Qty(qty)).0 as i64;
        assert_eq!(
            account.balance,
            100 * NANOS_PER_DOLLAR as i64 + expected_revenue
        );
        assert_eq!(account.position(m0, 0), q(5) as i64); // 10 - 5 = 5 YES shares left
    }

    #[test]
    fn test_resolve_market_yes_wins() {
        let (_, mut accounts) = setup();
        let m0 = MarketId::new(0);
        let aid = AccountId(0);

        let account = accounts
            .get_mut(aid)
            .expect("account present: id sourced from this AccountStore");
        account.positions.insert((m0, 0), q(10) as i64); // 10 YES shares
        account.positions.insert((m0, 1), q(5) as i64); // 5 NO shares
        let initial_balance = account.balance;

        resolve_market(&mut accounts, m0, Nanos(NANOS_PER_DOLLAR)); // YES wins ($1 per YES share)

        let account = accounts
            .get(aid)
            .expect("account present: id sourced from this AccountStore");
        // YES pays $1: 10 * $1 = $10 added
        // NO pays $0: 5 * $0 = $0 added
        assert_eq!(
            account.balance,
            initial_balance + notional_nanos(Nanos(NANOS_PER_DOLLAR), Qty(q(10))).0 as i64
        );
        // All positions for this market should be gone
        assert_eq!(account.position(m0, 0), 0);
        assert_eq!(account.position(m0, 1), 0);
    }

    #[test]
    fn test_resolve_market_no_wins() {
        let (_, mut accounts) = setup();
        let m0 = MarketId::new(0);
        let aid = AccountId(0);

        let account = accounts
            .get_mut(aid)
            .expect("account present: id sourced from this AccountStore");
        account.positions.insert((m0, 0), q(10) as i64); // 10 YES shares
        account.positions.insert((m0, 1), q(5) as i64); // 5 NO shares
        let initial_balance = account.balance;

        resolve_market(&mut accounts, m0, Nanos::ZERO); // NO wins ($0 per YES share)

        let account = accounts
            .get(aid)
            .expect("account present: id sourced from this AccountStore");
        // YES pays $0: 10 * $0 = $0
        // NO pays $1: 5 * $1 = $5
        assert_eq!(
            account.balance,
            initial_balance + notional_nanos(Nanos(NANOS_PER_DOLLAR), Qty(q(5))).0 as i64
        );
        assert_eq!(account.position(m0, 0), 0);
        assert_eq!(account.position(m0, 1), 0);
    }

    #[test]
    fn test_resolve_market_fractional() {
        let (_, mut accounts) = setup();
        let m0 = MarketId::new(0);
        let aid = AccountId(0);

        let account = accounts
            .get_mut(aid)
            .expect("account present: id sourced from this AccountStore");
        account.positions.insert((m0, 0), q(10) as i64); // 10 YES shares
        account.positions.insert((m0, 1), q(5) as i64); // 5 NO shares
        let initial_balance = account.balance;

        // Resolve at 70% — YES pays $0.70, NO pays $0.30
        resolve_market(&mut accounts, m0, Nanos(700_000_000));

        let account = accounts
            .get(aid)
            .expect("account present: id sourced from this AccountStore");
        // YES: 10 * $0.70 = $7.00
        // NO: 5 * $0.30 = $1.50
        let expected = initial_balance
            + notional_nanos(Nanos(700_000_000), Qty(q(10))).0 as i64
            + notional_nanos(Nanos(300_000_000), Qty(q(5))).0 as i64;
        assert_eq!(account.balance, expected);
        assert_eq!(account.position(m0, 0), 0);
        assert_eq!(account.position(m0, 1), 0);
    }

    #[test]
    fn nonzero_fill_with_missing_order_reports_failure_without_mutating_balances() {
        let (_, mut accounts) = setup();
        let aid = AccountId(0);
        let mut fill = Fill::new(999, Qty(1), Nanos(500_000_000));
        fill.account_id = aid.0;
        let balances_before: Vec<_> = accounts
            .iter()
            .map(|(account_id, account)| (*account_id, account.balance))
            .collect();

        let failures = settle_batch(&mut accounts, &[fill], &[], 1);

        assert_eq!(
            failures,
            vec![BlockInvariantFailure::UnsettleableFill {
                order_id: 999,
                account_id: aid.0,
                reason: UnsettleableFillReason::MissingOrder,
            }]
        );
        let balances_after: Vec<_> = accounts
            .iter()
            .map(|(account_id, account)| (*account_id, account.balance))
            .collect();
        assert_eq!(balances_before, balances_after);
    }

    #[test]
    fn nonzero_fill_with_missing_account_reports_failure_without_mutating_balances() {
        let (markets, mut accounts) = setup();
        let missing_account_id = 999;
        let order = outcome_buy(&markets, 1, MarketId::new(0), 0, 500_000_000, 1);
        let mut fill = Fill::new(order.id, Qty(1), Nanos(500_000_000));
        fill.account_id = missing_account_id;
        let balances_before: Vec<_> = accounts
            .iter()
            .map(|(account_id, account)| (*account_id, account.balance))
            .collect();

        let failures = settle_batch(&mut accounts, &[fill], &[order], 1);

        assert_eq!(
            failures,
            vec![BlockInvariantFailure::UnsettleableFill {
                order_id: 1,
                account_id: missing_account_id,
                reason: UnsettleableFillReason::MissingAccount,
            }]
        );
        let balances_after: Vec<_> = accounts
            .iter()
            .map(|(account_id, account)| (*account_id, account.balance))
            .collect();
        assert_eq!(balances_before, balances_after);
    }

    proptest! {
        #[test]
        fn prop_zero_fill_does_not_mutate_store(
            balance in 0i64..=10_000_000_000,
            limit_price in prop_oneof![Just(100_000_000u64), Just(300_000_000u64), Just(500_000_000u64)],
            max_fill in 1u64..=10,
        ) {
            let mut markets = MarketSet::new();
            let m0 = markets.add_binary("M0");
            let mut accounts = AccountStore::new();
            let aid = accounts.create_account(balance);

            let order = outcome_buy(&markets, 1, m0, 0, limit_price, max_fill);
            let mut fill = Fill::new(order.id, Qty(0), Nanos(limit_price));
            fill.account_id = aid.0;

            let before = snapshot_accounts(&accounts);
            let failures = settle_batch(&mut accounts, &[fill], &[order], 1);
            let after = snapshot_accounts(&accounts);

            prop_assert!(failures.is_empty());
            prop_assert_eq!(before, after);
        }

        #[test]
        fn prop_settle_batch_matches_verifier_for_simple_buys(
            balance in 1_000_000_000i64..=20_000_000_000,
            limit_price in prop_oneof![Just(100_000_000u64), Just(300_000_000u64), Just(500_000_000u64), Just(700_000_000u64)],
            fill_shares in 1u64..=5,
        ) {
            let mut markets = MarketSet::new();
            let m0 = markets.add_binary("M0");
            let mut accounts = AccountStore::new();
            let fill_qty = q(fill_shares);
            let required_balance = notional_nanos(Nanos(limit_price), Qty(fill_qty)).0 as i64 + 1_000_000_000;
            let aid = accounts.create_account(balance.max(required_balance));

            let order = outcome_buy(&markets, 1, m0, 0, limit_price, fill_qty);
            let witness_order = WitnessOrder {
                order: order.clone(),
                account_id: aid.0,
                is_mm: false,
            };
            let mut fill = Fill::new(order.id, Qty(fill_qty), Nanos(limit_price));
            fill.account_id = aid.0;

            let post_system_state = snapshot_accounts(&accounts);
            let failures = settle_batch(&mut accounts, &[fill.clone()], &[order], 7);
            prop_assert!(failures.is_empty());
            let mut clearing_prices = HashMap::new();
            clearing_prices.insert(
                m0,
                vec![Nanos(limit_price), Nanos(NANOS_PER_DOLLAR - limit_price)],
            );
            let mint_adjustments =
                matching_engine::derive_minting(&[(m0, fill_qty as i64, 0)], &clearing_prices);
            let mint = accounts.get_mut(AccountId::MINT).unwrap();
            apply_minting(mint, &mint_adjustments, 7);
            let post_state = snapshot_accounts(&accounts);

            let mut header = empty_header();
            header.height = 7;
            let witness = BlockWitness {
                header,
                previous_header: None,
                orders: vec![witness_order],
                rejections: vec![],
                system_events: vec![],
                deposit_accumulator: sybil_verifier::DepositAccumulatorWitness::default(),
                fills: vec![fill],
                clearing_prices,
                total_welfare: 0,
                minting_cost: notional_nanos(Nanos(limit_price), Qty(fill_qty)).0 as i64,
                mm_constraints: vec![],
                market_groups: vec![],
                pre_state: post_system_state.clone(),
                    post_system_state,
                    post_state,
                    account_keys: vec![],
                    state_sidecar: Default::default(),
                    pre_state_sidecar: Default::default(),

                    resolved_markets: vec![],
                };

            let result = sybil_verifier::verify_settlement(&witness);
            prop_assert!(result.valid, "violations: {:?}", result.violations);
        }

        #[test]
        fn prop_fill_order_is_irrelevant_for_distinct_accounts_and_markets(
            balance_a in 1_000_000_000i64..=10_000_000_000,
            balance_b in 1_000_000_000i64..=10_000_000_000,
            qty_a in 1u64..=5,
            qty_b in 1u64..=5,
            price_a in prop_oneof![Just(100_000_000u64), Just(300_000_000u64), Just(500_000_000u64)],
            price_b in prop_oneof![Just(200_000_000u64), Just(400_000_000u64), Just(600_000_000u64)],
        ) {
            let mut markets = MarketSet::new();
            let m0 = markets.add_binary("M0");
            let m1 = markets.add_binary("M1");

            let mut accounts_ab = AccountStore::new();
            let aid_a = accounts_ab.create_account(balance_a);
            let aid_b = accounts_ab.create_account(balance_b);
            let mut accounts_ba = AccountStore::new();
            let aid_a_2 = accounts_ba.create_account(balance_a);
            let aid_b_2 = accounts_ba.create_account(balance_b);

            let order_a = outcome_buy(&markets, 1, m0, 0, price_a, qty_a);
            let order_b = outcome_buy(&markets, 2, m1, 0, price_b, qty_b);
            let orders = vec![order_a.clone(), order_b.clone()];

            let mut fill_a = Fill::new(order_a.id, Qty(qty_a), Nanos(price_a));
            fill_a.account_id = aid_a.0;
            let mut fill_b = Fill::new(order_b.id, Qty(qty_b), Nanos(price_b));
            fill_b.account_id = aid_b.0;

            let mut fill_a_2 = Fill::new(order_a.id, Qty(qty_a), Nanos(price_a));
            fill_a_2.account_id = aid_a_2.0;
            let mut fill_b_2 = Fill::new(order_b.id, Qty(qty_b), Nanos(price_b));
            fill_b_2.account_id = aid_b_2.0;

            let failures_ab = settle_batch(&mut accounts_ab, &[fill_a, fill_b], &orders, 1);
            let failures_ba = settle_batch(&mut accounts_ba, &[fill_b_2, fill_a_2], &orders, 1);

            prop_assert!(failures_ab.is_empty());
            prop_assert!(failures_ba.is_empty());
            prop_assert_eq!(snapshot_accounts(&accounts_ab), snapshot_accounts(&accounts_ba));
        }
    }
}
