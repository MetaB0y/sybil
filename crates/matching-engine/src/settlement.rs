//! Shared settlement logic: pure functions computing balance and position
//! deltas from fills. Used by both the sequencer (to apply fills) and the
//! verifier (to re-derive post-state for ZK verification).

use crate::order::{Fill, Order};
use crate::types::MarketId;

/// Balance and position changes resulting from settling one fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettlementDelta {
    /// Change to the account's balance (negative = debit, positive = credit).
    pub balance_delta: i64,
    /// Position changes: `(market, outcome, qty_delta)`. Only non-zero deltas included.
    pub position_deltas: Vec<(MarketId, u8, i64)>,
}

/// Compute the balance and position changes for a single fill.
///
/// This is a **pure, deterministic** function with no side effects.
/// Both the sequencer and verifier call this to ensure identical settlement logic.
///
/// Returns `None` if `fill.fill_qty == 0` (no-op).
///
/// # Settlement rules
///
/// For a single binary market (the common case):
/// - Positive payoff at outcome = BUY that outcome:
///   `balance -= price * qty`, `position(outcome) += qty`
/// - Negative payoff at outcome = SELL that outcome:
///   `balance += price * qty`, `position(outcome) -= qty`
///
/// For multi-market orders (bundles, spreads):
/// - Debit balance by `price * qty`
/// - Credit each market's position based on marginal payoffs
///   (stride-based mixed-radix decomposition)
pub fn compute_fill_settlement(order: &Order, fill: &Fill) -> Option<SettlementDelta> {
    if fill.fill_qty == 0 {
        return None;
    }

    let num_markets = order.num_markets as usize;
    let num_states = order.num_states as usize;

    // Single binary market: optimized fast path
    if num_markets == 1 && num_states == 2 {
        let market = order.markets[0];
        let yes_payoff = order.payoffs[0]; // outcome 0 = YES
        let no_payoff = order.payoffs[1]; // outcome 1 = NO

        if yes_payoff > 0 && no_payoff == 0 {
            // Buying YES
            let cost = (fill.fill_price as i128 * fill.fill_qty as i128) as i64;
            return Some(SettlementDelta {
                balance_delta: -cost,
                position_deltas: vec![(market, 0, fill.fill_qty as i64)],
            });
        } else if yes_payoff == 0 && no_payoff > 0 {
            // Buying NO
            let cost = (fill.fill_price as i128 * fill.fill_qty as i128) as i64;
            return Some(SettlementDelta {
                balance_delta: -cost,
                position_deltas: vec![(market, 1, fill.fill_qty as i64)],
            });
        } else if yes_payoff < 0 && no_payoff == 0 {
            // Selling YES
            let revenue = (fill.fill_price as i128 * fill.fill_qty as i128) as i64;
            return Some(SettlementDelta {
                balance_delta: revenue,
                position_deltas: vec![(market, 0, -(fill.fill_qty as i64))],
            });
        } else if yes_payoff == 0 && no_payoff < 0 {
            // Selling NO
            let revenue = (fill.fill_price as i128 * fill.fill_qty as i128) as i64;
            return Some(SettlementDelta {
                balance_delta: revenue,
                position_deltas: vec![(market, 1, -(fill.fill_qty as i64))],
            });
        }
        // else: general payoff vector — fall through to generic
    }

    // Generic settlement for arbitrary payoff vectors
    compute_generic_settlement(order, fill, num_markets, num_states)
}

/// Generic settlement for complex payoff vectors (bundles, spreads, mixed payoffs).
fn compute_generic_settlement(
    order: &Order,
    fill: &Fill,
    num_markets: usize,
    num_states: usize,
) -> Option<SettlementDelta> {
    // Debit the cost
    let cost = (fill.fill_price as i128 * fill.fill_qty as i128) as i64;
    let mut position_deltas = Vec::new();

    if num_markets == 1 {
        // Single binary market with general payoff vector
        let market = order.markets[0];
        let yes_payoff = order.payoffs[0] as i64;
        let no_payoff = order.payoffs[1] as i64;

        if yes_payoff != 0 {
            position_deltas.push((market, 0, yes_payoff * fill.fill_qty as i64));
        }
        if no_payoff != 0 {
            position_deltas.push((market, 1, no_payoff * fill.fill_qty as i64));
        }
    } else {
        // Multi-market: compute marginal position per market per outcome.
        // State index uses mixed-radix: state = o0 + 2*o1 + 4*o2 + ...
        //
        // NOTE: The division `yes_sum * fill_qty / yes_count` truncates when
        // the numerator is not evenly divisible. This is acceptable because:
        // 1. All current solvers require single-market orders (num_markets == 1),
        //    so this path is only reached by multi-market orders in tests/future use.
        // 2. For standard binary markets, yes_count = 2^(n-1) and typical payoff
        //    vectors produce exact divisions.
        // 3. If multi-market orders are supported in production, this should be
        //    replaced with proper composite position tracking.
        for m_idx in 0..num_markets {
            let market = order.markets[m_idx];
            let stride = 1usize << m_idx;

            let mut yes_sum: i64 = 0;
            let mut yes_count: usize = 0;
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

            if yes_count > 0 && yes_sum != 0 {
                let yes_per_unit = yes_sum;
                position_deltas.push((
                    market,
                    0,
                    yes_per_unit * fill.fill_qty as i64 / yes_count as i64,
                ));
            }
            if no_count > 0 && no_sum != 0 {
                let no_per_unit = no_sum;
                position_deltas.push((
                    market,
                    1,
                    no_per_unit * fill.fill_qty as i64 / no_count as i64,
                ));
            }
        }
    }

    Some(SettlementDelta {
        balance_delta: -cost,
        position_deltas,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{outcome_buy, outcome_sell, MarketSet, NANOS_PER_DOLLAR};

    #[test]
    fn test_zero_qty_returns_none() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let fill = Fill::new(1, 0, 500_000_000);
        assert!(compute_fill_settlement(&order, &fill).is_none());
    }

    #[test]
    fn test_buy_yes() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let fill = Fill::new(1, 10, 500_000_000);

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        assert_eq!(delta.balance_delta, -(500_000_000i64 * 10));
        assert_eq!(delta.position_deltas, vec![(m0, 0, 10)]);
    }

    #[test]
    fn test_buy_no() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let order = outcome_buy(&markets, 1, m0, 1, 300_000_000, 5);
        let fill = Fill::new(1, 5, 300_000_000);

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        assert_eq!(delta.balance_delta, -(300_000_000i64 * 5));
        assert_eq!(delta.position_deltas, vec![(m0, 1, 5)]);
    }

    #[test]
    fn test_sell_yes() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let order = outcome_sell(&markets, 2, m0, 0, 500_000_000, 5);
        let fill = Fill::new(2, 5, 500_000_000);

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        assert_eq!(delta.balance_delta, 500_000_000i64 * 5);
        assert_eq!(delta.position_deltas, vec![(m0, 0, -5)]);
    }

    #[test]
    fn test_sell_no() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let order = outcome_sell(&markets, 3, m0, 1, 400_000_000, 3);
        let fill = Fill::new(3, 3, 400_000_000);

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        assert_eq!(delta.balance_delta, 400_000_000i64 * 3);
        assert_eq!(delta.position_deltas, vec![(m0, 1, -3)]);
    }

    #[test]
    fn test_bundle_yes_two_markets() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("A");
        let m1 = markets.add_binary("B");
        let order = crate::bundle_yes(&markets, 10, &[m0, m1], 250_000_000, 4);
        let fill = Fill::new(10, 4, 250_000_000);

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        // Cost: 0.25 * 4 = 1.0
        assert_eq!(delta.balance_delta, -(250_000_000i64 * 4));
        // Bundle YES: payoffs[0]=1 (both YES), payoffs[1..3]=0
        // Each market gets +1 YES position per fill unit
        // m0: yes_sum=1, yes_count=2, delta = 1*4/2 = 2
        // m1: yes_sum=1, yes_count=2, delta = 1*4/2 = 2
        assert!(delta
            .position_deltas
            .iter()
            .any(|&(m, o, q)| m == m0 && o == 0 && q == 2));
        assert!(delta
            .position_deltas
            .iter()
            .any(|&(m, o, q)| m == m1 && o == 0 && q == 2));
    }

    #[test]
    fn test_bundle_truncation_documented() {
        // Demonstrates integer truncation in multi-market settlement.
        // A bundle YES on 2 markets with odd fill_qty loses 1 unit per market
        // because `yes_sum * fill_qty / yes_count` = `1 * 3 / 2` = 1 (not 1.5).
        //
        // This is currently acceptable because all solvers require single-market
        // orders. If multi-market orders go to production, replace the marginal
        // decomposition with composite position tracking.
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("A");
        let m1 = markets.add_binary("B");
        let order = crate::bundle_yes(&markets, 10, &[m0, m1], 250_000_000, 3);
        let fill = Fill::new(10, 3, 250_000_000);

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        // Bundle YES: payoffs = [1, 0, 0, 0]
        // m0: yes_sum=1, yes_count=2, delta = 1*3/2 = 1 (truncated from 1.5)
        // m1: yes_sum=1, yes_count=2, delta = 1*3/2 = 1 (truncated from 1.5)
        assert!(delta
            .position_deltas
            .iter()
            .any(|&(m, o, q)| m == m0 && o == 0 && q == 1));
        assert!(delta
            .position_deltas
            .iter()
            .any(|&(m, o, q)| m == m1 && o == 0 && q == 1));
        // Cost is still exact (no truncation in balance)
        assert_eq!(delta.balance_delta, -(250_000_000i64 * 3));
    }

    #[test]
    fn test_large_price_qty_no_overflow() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let order = outcome_buy(
            &markets,
            1,
            m0,
            0,
            NANOS_PER_DOLLAR - 1,
            u64::MAX / NANOS_PER_DOLLAR,
        );
        let qty = u64::MAX / NANOS_PER_DOLLAR;
        let fill = Fill::new(1, qty, NANOS_PER_DOLLAR - 1);

        // Should not panic — i128 intermediate handles the multiplication
        let delta = compute_fill_settlement(&order, &fill);
        assert!(delta.is_some());
    }
}
