use matching_engine::{Fill, MarketId, Order, NANOS_PER_DOLLAR};

use crate::account::{Account, AccountId, AccountStore};

/// Settle a single fill: update the account's balance and positions.
///
/// For a single-market order:
/// - Positive payoff at outcome = BUY that outcome
///   balance -= fill_price * fill_qty, position += fill_qty
/// - Negative payoff at outcome = SELL that outcome
///   balance += fill_price * fill_qty, position -= fill_qty
///
/// For multi-market orders (bundles):
/// - Each market's YES position is adjusted based on payoffs
pub fn settle_fill(account: &mut Account, order: &Order, fill: &Fill) {
    if fill.fill_qty == 0 {
        return;
    }

    let num_markets = order.num_markets as usize;
    let num_states = order.num_states as usize;

    if num_markets == 1 && num_states == 2 {
        // Single binary market: simple case
        let market = order.markets[0];
        let yes_payoff = order.payoffs[0]; // outcome 0 = YES
        let no_payoff = order.payoffs[1]; // outcome 1 = NO

        if yes_payoff > 0 && no_payoff == 0 {
            // Buying YES
            let cost = (fill.fill_price as i128 * fill.fill_qty as i128) as i64;
            account.balance -= cost;
            *account.positions.entry((market, 0)).or_insert(0) += fill.fill_qty as i64;
        } else if yes_payoff == 0 && no_payoff > 0 {
            // Buying NO
            let cost = (fill.fill_price as i128 * fill.fill_qty as i128) as i64;
            account.balance -= cost;
            *account.positions.entry((market, 1)).or_insert(0) += fill.fill_qty as i64;
        } else if yes_payoff < 0 && no_payoff == 0 {
            // Selling YES
            let revenue = (fill.fill_price as i128 * fill.fill_qty as i128) as i64;
            account.balance += revenue;
            *account.positions.entry((market, 0)).or_insert(0) -= fill.fill_qty as i64;
        } else if yes_payoff == 0 && no_payoff < 0 {
            // Selling NO
            let revenue = (fill.fill_price as i128 * fill.fill_qty as i128) as i64;
            account.balance += revenue;
            *account.positions.entry((market, 1)).or_insert(0) -= fill.fill_qty as i64;
        } else {
            // General payoff vector - use generic settlement
            settle_generic(account, order, fill);
        }
    } else {
        // Multi-market or complex order
        settle_generic(account, order, fill);
    }
}

/// Generic settlement for arbitrary payoff vectors.
///
/// The order paid fill_price per unit for a payoff vector.
/// We debit balance by fill_price * fill_qty and credit positions
/// according to the payoff structure.
fn settle_generic(account: &mut Account, order: &Order, fill: &Fill) {
    // Debit the cost
    let cost = (fill.fill_price as i128 * fill.fill_qty as i128) as i64;
    account.balance -= cost;

    // Credit positions: for each market, determine net exposure
    let num_markets = order.num_markets as usize;

    if num_markets == 1 {
        let market = order.markets[0];
        // For a single binary market, payoffs[0] = YES payoff, payoffs[1] = NO payoff
        let yes_payoff = order.payoffs[0] as i64;
        let no_payoff = order.payoffs[1] as i64;

        if yes_payoff != 0 {
            *account.positions.entry((market, 0)).or_insert(0) +=
                yes_payoff * fill.fill_qty as i64;
        }
        if no_payoff != 0 {
            *account.positions.entry((market, 1)).or_insert(0) +=
                no_payoff * fill.fill_qty as i64;
        }
    } else {
        // Multi-market: compute marginal position per market per outcome.
        // For binary markets, outcome 0 = YES, outcome 1 = NO.
        // State index uses mixed-radix: state = o0 + 2*o1 + 4*o2 + ...
        let num_states = order.num_states as usize;

        for m_idx in 0..num_markets {
            let market = order.markets[m_idx];
            // Compute marginal payoff for each outcome of this market
            // by averaging over all states where this market has that outcome
            let stride = 1usize << m_idx;

            // Payoff when this market outcome = 0 (YES)
            let mut yes_sum: i64 = 0;
            let mut yes_count: usize = 0;
            // Payoff when this market outcome = 1 (NO)
            let mut no_sum: i64 = 0;
            let mut no_count: usize = 0;

            for s in 0..num_states {
                let outcome_for_market = (s / stride) % 2;
                let payoff = order.payoffs[s] as i64;
                if outcome_for_market == 0 {
                    yes_sum += payoff;
                    yes_count += 1;
                } else {
                    no_sum += payoff;
                    no_count += 1;
                }
            }

            // Net position is the difference in expected payoff across outcomes
            // Simplified: if payoff is only non-zero when all YES, this gives +1 for YES position
            // For a bundle of N markets, each market gets +1 YES position
            if yes_count > 0 && yes_sum != 0 {
                // The position reflects how many shares of this outcome the order represents
                // For a bundle_yes across N markets: payoff[0]=1, all others=0
                // YES sum = 1, NO sum = 0, so net YES position = 1 per unit
                let yes_per_unit = yes_sum; // Since payoff is per unit of fill
                *account.positions.entry((market, 0)).or_insert(0) +=
                    yes_per_unit * fill.fill_qty as i64 / yes_count as i64;
            }
            if no_count > 0 && no_sum != 0 {
                let no_per_unit = no_sum;
                *account.positions.entry((market, 1)).or_insert(0) +=
                    no_per_unit * fill.fill_qty as i64 / no_count as i64;
            }
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

/// Resolve a market: convert positions to balance based on the winning outcome.
/// winning_outcome: 0 for YES, 1 for NO.
pub fn resolve_market(accounts: &mut AccountStore, market: MarketId, winning_outcome: u8) {
    // Collect account IDs first to avoid borrow issues
    let account_ids: Vec<AccountId> = accounts.iter().map(|(&id, _)| id).collect();

    for account_id in account_ids {
        let account = accounts.get_mut(account_id).unwrap();

        // Winning positions pay out $1 per share
        let winning_pos = account
            .positions
            .remove(&(market, winning_outcome))
            .unwrap_or(0);
        if winning_pos != 0 {
            account.balance += winning_pos * NANOS_PER_DOLLAR as i64;
        }

        // Losing positions are worthless - just remove them
        for outcome in 0..2u8 {
            if outcome != winning_outcome {
                account.positions.remove(&(market, outcome));
            }
        }
    }
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
        assert_eq!(account.balance, 100 * NANOS_PER_DOLLAR as i64 - expected_cost);
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
    fn test_resolve_market() {
        let (_, mut accounts) = setup();
        let m0 = MarketId::new(0);
        let aid = AccountId(0);

        let account = accounts.get_mut(aid).unwrap();
        account.positions.insert((m0, 0), 10); // 10 YES shares
        account.positions.insert((m0, 1), 5); // 5 NO shares
        let initial_balance = account.balance;

        resolve_market(&mut accounts, m0, 0); // YES wins

        let account = accounts.get(aid).unwrap();
        // YES wins: 10 * $1 = $10 added
        assert_eq!(
            account.balance,
            initial_balance + 10 * NANOS_PER_DOLLAR as i64
        );
        // All positions for this market should be gone
        assert_eq!(account.position(m0, 0), 0);
        assert_eq!(account.position(m0, 1), 0);
    }
}
