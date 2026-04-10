use matching_engine::{
    compute_fill_settlement, Fill, MarketId, MintAdjustment, Nanos, Order, NANOS_PER_DOLLAR,
};

use crate::account::{Account, AccountId, AccountStore};

/// Settle a single fill: update the account's balance and positions.
///
/// Delegates to `matching_engine::compute_fill_settlement` for the pure math,
/// then applies the computed deltas to the account.
pub fn settle_fill(account: &mut Account, order: &Order, fill: &Fill) {
    if let Some(delta) = compute_fill_settlement(order, fill) {
        account.balance += delta.balance_delta;
        for (market, outcome, qty_delta) in delta.position_deltas {
            *account.positions.entry((market, outcome)).or_insert(0) += qty_delta;
        }
    }
}

/// Settle all fills from a batch result, mapping order IDs to accounts.
pub fn settle_batch(
    accounts: &mut AccountStore,
    fills: &[Fill],
    orders: &[Order],
    order_account_map: &std::collections::HashMap<u64, AccountId>,
) {
    // Build order lookup
    let order_map: std::collections::HashMap<u64, &Order> =
        orders.iter().map(|o| (o.id, o)).collect();

    for fill in fills {
        if fill.fill_qty == 0 {
            continue;
        }

        let Some(&account_id) = order_account_map.get(&fill.order_id) else {
            continue;
        };
        let Some(order) = order_map.get(&fill.order_id) else {
            continue;
        };
        let Some(account) = accounts.get_mut(account_id) else {
            continue;
        };

        settle_fill(account, order, fill);
    }
}

/// Apply minting adjustments to the MINT account.
pub fn apply_minting(mint: &mut Account, adjustments: &[MintAdjustment]) {
    for adj in adjustments {
        *mint
            .positions
            .entry((adj.market_id, adj.outcome))
            .or_insert(0) += adj.position_delta;
        mint.balance += adj.balance_delta;
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
pub fn resolve_market(accounts: &mut AccountStore, market: MarketId, yes_payout_nanos: Nanos) {
    let no_payout_nanos = NANOS_PER_DOLLAR - yes_payout_nanos;

    // Collect account IDs first to avoid borrow issues
    let account_ids: Vec<AccountId> = accounts.iter().map(|(&id, _)| id).collect();

    // Check position balance and total balance before resolution
    let mut total_yes: i64 = 0;
    let mut total_no: i64 = 0;
    let pre_total_balance: i64 = accounts.iter().map(|(_, a)| a.balance).sum();
    for &account_id in &account_ids {
        let account = accounts.get(account_id).unwrap();
        total_yes += account.position(market, 0);
        total_no += account.position(market, 1);
    }
    eprintln!(
        "RESOLVE market {:?}: YES_total={} NO_total={} diff={} payout_yes={} pre_balance={}",
        market,
        total_yes,
        total_no,
        total_yes - total_no,
        yes_payout_nanos,
        pre_total_balance
    );

    for account_id in account_ids {
        let account = accounts.get_mut(account_id).unwrap();

        let yes_pos = account.positions.remove(&(market, 0)).unwrap_or(0);
        let no_pos = account.positions.remove(&(market, 1)).unwrap_or(0);

        if yes_pos != 0 {
            account.balance += (yes_pos as i128 * yes_payout_nanos as i128) as i64;
        }
        if no_pos != 0 {
            account.balance += (no_pos as i128 * no_payout_nanos as i128) as i64;
        }
    }

    let post_total_balance: i64 = accounts.iter().map(|(_, a)| a.balance).sum();
    let balance_delta = post_total_balance - pre_total_balance;
    eprintln!(
        "RESOLVE market {:?} DONE: post_balance={} delta={} total_final={}",
        market, post_total_balance, balance_delta, post_total_balance
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use matching_engine::{outcome_buy, outcome_sell, MarketSet, NANOS_PER_DOLLAR};

    fn setup() -> (MarketSet, AccountStore) {
        let mut markets = MarketSet::new();
        markets.add_binary("Test Market");
        let mut accounts = AccountStore::new();
        accounts.create_account(100 * NANOS_PER_DOLLAR as i64); // $100
        (markets, accounts)
    }

    #[test]
    fn test_settle_yes_buy() {
        let (markets, mut accounts) = setup();
        let m0 = MarketId::new(0);
        let aid = AccountId(0);

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10); // Buy YES at 0.50, qty 10
        let fill = Fill::new(1, 10, 500_000_000); // Filled at 0.50

        let account = accounts.get_mut(aid).unwrap();
        settle_fill(account, &order, &fill);

        // Should have paid 0.50 * 10 = 5 nanos * 10
        let expected_cost = 500_000_000i64 * 10;
        assert_eq!(
            account.balance,
            100 * NANOS_PER_DOLLAR as i64 - expected_cost
        );
        assert_eq!(account.position(m0, 0), 10); // 10 YES shares
    }

    #[test]
    fn test_settle_yes_sell() {
        let (markets, mut accounts) = setup();
        let m0 = MarketId::new(0);
        let aid = AccountId(0);

        // First give the account some position
        let account = accounts.get_mut(aid).unwrap();
        account.positions.insert((m0, 0), 10);

        let order = outcome_sell(&markets, 2, m0, 0, 500_000_000, 5); // Sell YES at 0.50, qty 5
        let fill = Fill::new(2, 5, 500_000_000);

        settle_fill(account, &order, &fill);

        // Should have received 0.50 * 5
        let expected_revenue = 500_000_000i64 * 5;
        assert_eq!(
            account.balance,
            100 * NANOS_PER_DOLLAR as i64 + expected_revenue
        );
        assert_eq!(account.position(m0, 0), 5); // 10 - 5 = 5 YES shares left
    }

    #[test]
    fn test_resolve_market_yes_wins() {
        let (_, mut accounts) = setup();
        let m0 = MarketId::new(0);
        let aid = AccountId(0);

        let account = accounts.get_mut(aid).unwrap();
        account.positions.insert((m0, 0), 10); // 10 YES shares
        account.positions.insert((m0, 1), 5); // 5 NO shares
        let initial_balance = account.balance;

        resolve_market(&mut accounts, m0, NANOS_PER_DOLLAR); // YES wins ($1 per YES share)

        let account = accounts.get(aid).unwrap();
        // YES pays $1: 10 * $1 = $10 added
        // NO pays $0: 5 * $0 = $0 added
        assert_eq!(
            account.balance,
            initial_balance + 10 * NANOS_PER_DOLLAR as i64
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

        let account = accounts.get_mut(aid).unwrap();
        account.positions.insert((m0, 0), 10); // 10 YES shares
        account.positions.insert((m0, 1), 5); // 5 NO shares
        let initial_balance = account.balance;

        resolve_market(&mut accounts, m0, 0); // NO wins ($0 per YES share)

        let account = accounts.get(aid).unwrap();
        // YES pays $0: 10 * $0 = $0
        // NO pays $1: 5 * $1 = $5
        assert_eq!(
            account.balance,
            initial_balance + 5 * NANOS_PER_DOLLAR as i64
        );
        assert_eq!(account.position(m0, 0), 0);
        assert_eq!(account.position(m0, 1), 0);
    }

    #[test]
    fn test_resolve_market_fractional() {
        let (_, mut accounts) = setup();
        let m0 = MarketId::new(0);
        let aid = AccountId(0);

        let account = accounts.get_mut(aid).unwrap();
        account.positions.insert((m0, 0), 10); // 10 YES shares
        account.positions.insert((m0, 1), 5); // 5 NO shares
        let initial_balance = account.balance;

        // Resolve at 70% — YES pays $0.70, NO pays $0.30
        resolve_market(&mut accounts, m0, 700_000_000);

        let account = accounts.get(aid).unwrap();
        // YES: 10 * $0.70 = $7.00
        // NO: 5 * $0.30 = $1.50
        let expected = initial_balance + 10 * 700_000_000i64 + 5 * 300_000_000i64;
        assert_eq!(account.balance, expected);
        assert_eq!(account.position(m0, 0), 0);
        assert_eq!(account.position(m0, 1), 0);
    }
}
