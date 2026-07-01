//! Shared settlement logic: pure functions computing balance and position
//! deltas from fills and minting adjustments. Used by both the sequencer
//! (to apply fills) and the verifier (to re-derive post-state for ZK
//! verification).

use std::collections::HashMap;

use crate::order::{Fill, Order};
use crate::types::{notional_nanos, signed_notional_nanos, MarketId, Nanos};

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
///   `balance -= price * qty / SHARE_SCALE`, `position(outcome) += qty`
/// - Negative payoff at outcome = SELL that outcome:
///   `balance += price * qty / SHARE_SCALE`, `position(outcome) -= qty`
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
            let cost = notional_nanos(fill.fill_price, fill.fill_qty) as i64;
            return Some(SettlementDelta {
                balance_delta: -cost,
                position_deltas: vec![(market, 0, fill.fill_qty as i64)],
            });
        } else if yes_payoff == 0 && no_payoff > 0 {
            // Buying NO
            let cost = notional_nanos(fill.fill_price, fill.fill_qty) as i64;
            return Some(SettlementDelta {
                balance_delta: -cost,
                position_deltas: vec![(market, 1, fill.fill_qty as i64)],
            });
        } else if yes_payoff < 0 && no_payoff == 0 {
            // Selling YES
            let revenue = notional_nanos(fill.fill_price, fill.fill_qty) as i64;
            return Some(SettlementDelta {
                balance_delta: revenue,
                position_deltas: vec![(market, 0, -(fill.fill_qty as i64))],
            });
        } else if yes_payoff == 0 && no_payoff < 0 {
            // Selling NO
            let revenue = notional_nanos(fill.fill_price, fill.fill_qty) as i64;
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
    let cost = notional_nanos(fill.fill_price, fill.fill_qty) as i64;
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

// ---------------------------------------------------------------------------
// Minting
// ---------------------------------------------------------------------------

/// An adjustment to the MINT account for one market, restoring YES/NO balance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MintAdjustment {
    pub market_id: MarketId,
    /// Which outcome MINT shorts (0 = YES, 1 = NO).
    pub outcome: u8,
    /// Position delta for MINT (negative = short).
    pub position_delta: i64,
    /// Balance delta for MINT (clearing_price × quantity). Zero if no clearing price.
    pub balance_delta: i64,
}

/// Derive minting adjustments from position imbalances.
///
/// Pure function: takes pre-computed per-market position totals (summed across
/// ALL accounts including MINT) and clearing prices. Returns adjustments that
/// would restore `total_yes == total_no` for each market.
///
/// The sum must include MINT's existing positions so each block only adjusts
/// by the incremental imbalance, not the cumulative total.
///
/// If a market has an imbalance but no clearing price, the adjustment has
/// `balance_delta = 0` — the caller decides how to handle (sequencer panics,
/// verifier records a violation).
pub fn derive_minting(
    market_totals: &[(MarketId, i64, i64)],
    clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
) -> Vec<MintAdjustment> {
    let mut adjustments = Vec::new();

    for &(market_id, total_yes, total_no) in market_totals {
        let diff = total_yes - total_no;
        if diff == 0 {
            continue;
        }

        if diff > 0 {
            // More YES than NO → MINT shorts YES, receives yes_price revenue
            let yes_price = clearing_prices
                .get(&market_id)
                .and_then(|p| p.first().copied())
                .unwrap_or(0);
            adjustments.push(MintAdjustment {
                market_id,
                outcome: 0,
                position_delta: -diff,
                balance_delta: signed_notional_nanos(yes_price, diff),
            });
        } else {
            // More NO than YES → MINT shorts NO, receives no_price revenue
            let no_price = clearing_prices
                .get(&market_id)
                .and_then(|p| p.get(1).copied())
                .unwrap_or(0);
            adjustments.push(MintAdjustment {
                market_id,
                outcome: 1,
                position_delta: diff, // negative: MINT shorts NO
                balance_delta: notional_nanos(no_price, diff.unsigned_abs()) as i64,
            });
        }
    }

    adjustments
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{outcome_buy, outcome_sell, shares_to_qty, MarketSet, NANOS_PER_DOLLAR};

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
        let qty = shares_to_qty(10);
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, qty);
        let fill = Fill::new(1, qty, 500_000_000);

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        assert_eq!(delta.balance_delta, -(500_000_000i64 * 10));
        assert_eq!(delta.position_deltas, vec![(m0, 0, qty as i64)]);
    }

    #[test]
    fn test_buy_no() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let qty = shares_to_qty(5);
        let order = outcome_buy(&markets, 1, m0, 1, 300_000_000, qty);
        let fill = Fill::new(1, qty, 300_000_000);

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        assert_eq!(delta.balance_delta, -(300_000_000i64 * 5));
        assert_eq!(delta.position_deltas, vec![(m0, 1, qty as i64)]);
    }

    #[test]
    fn test_sell_yes() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let qty = shares_to_qty(5);
        let order = outcome_sell(&markets, 2, m0, 0, 500_000_000, qty);
        let fill = Fill::new(2, qty, 500_000_000);

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        assert_eq!(delta.balance_delta, 500_000_000i64 * 5);
        assert_eq!(delta.position_deltas, vec![(m0, 0, -(qty as i64))]);
    }

    #[test]
    fn test_sell_no() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let qty = shares_to_qty(3);
        let order = outcome_sell(&markets, 3, m0, 1, 400_000_000, qty);
        let fill = Fill::new(3, qty, 400_000_000);

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        assert_eq!(delta.balance_delta, 400_000_000i64 * 3);
        assert_eq!(delta.position_deltas, vec![(m0, 1, -(qty as i64))]);
    }

    #[test]
    fn test_bundle_yes_two_markets() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("A");
        let m1 = markets.add_binary("B");
        let qty = shares_to_qty(4);
        let order = crate::bundle_yes(&markets, 10, &[m0, m1], 250_000_000, qty);
        let fill = Fill::new(10, qty, 250_000_000);

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        // Cost: 0.25 * 4 = 1.0
        assert_eq!(delta.balance_delta, -(250_000_000i64 * 4));
        // Bundle YES: payoffs[0]=1 (both YES), payoffs[1..3]=0
        // Each market gets +1 YES position per fill unit
        // m0: yes_sum=1, yes_count=2, delta = 1*4000/2 = 2000 units
        // m1: yes_sum=1, yes_count=2, delta = 1*4000/2 = 2000 units
        assert!(delta
            .position_deltas
            .iter()
            .any(|&(m, o, q)| m == m0 && o == 0 && q == shares_to_qty(2) as i64));
        assert!(delta
            .position_deltas
            .iter()
            .any(|&(m, o, q)| m == m1 && o == 0 && q == shares_to_qty(2) as i64));
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
        // Cost is scaled by SHARE_SCALE: 0.003 shares at $0.25 = 750,000 nanos.
        assert_eq!(delta.balance_delta, -750_000);
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

    // --- Minting tests ---

    #[test]
    fn test_minting_no_imbalance() {
        let m0 = MarketId(0);
        let totals = vec![(m0, shares_to_qty(100) as i64, shares_to_qty(100) as i64)];
        let prices = HashMap::new();
        assert!(derive_minting(&totals, &prices).is_empty());
    }

    #[test]
    fn test_minting_yes_surplus() {
        let m0 = MarketId(0);
        let totals = vec![(m0, shares_to_qty(150) as i64, shares_to_qty(100) as i64)]; // 50 more YES than NO
        let mut prices = HashMap::new();
        prices.insert(m0, vec![400_000_000, 600_000_000]); // 0.40 / 0.60

        let adj = derive_minting(&totals, &prices);
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0].market_id, m0);
        assert_eq!(adj[0].outcome, 0); // shorts YES
        assert_eq!(adj[0].position_delta, -(shares_to_qty(50) as i64));
        assert_eq!(adj[0].balance_delta, 400_000_000i64 * 50); // yes_price * qty
    }

    #[test]
    fn test_minting_no_surplus() {
        let m0 = MarketId(0);
        let totals = vec![(m0, shares_to_qty(100) as i64, shares_to_qty(180) as i64)]; // 80 more NO than YES
        let mut prices = HashMap::new();
        prices.insert(m0, vec![700_000_000, 300_000_000]);

        let adj = derive_minting(&totals, &prices);
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0].outcome, 1); // shorts NO
        assert_eq!(adj[0].position_delta, -(shares_to_qty(80) as i64)); // total_yes - total_no = -80
        assert_eq!(adj[0].balance_delta, 300_000_000i64 * 80); // no_price * |diff|
    }

    #[test]
    fn test_minting_multiple_markets() {
        let m0 = MarketId(0);
        let m1 = MarketId(1);
        let totals = vec![
            (m0, 110, 100), // YES surplus = 10
            (m1, 100, 100), // balanced
        ];
        let mut prices = HashMap::new();
        prices.insert(m0, vec![500_000_000, 500_000_000]);

        let adj = derive_minting(&totals, &prices);
        assert_eq!(adj.len(), 1); // only m0
        assert_eq!(adj[0].market_id, m0);
    }

    #[test]
    fn test_minting_missing_price_gives_zero_balance() {
        let m0 = MarketId(0);
        let totals = vec![(m0, 150, 100)];
        let prices = HashMap::new(); // no prices

        let adj = derive_minting(&totals, &prices);
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0].position_delta, -50);
        assert_eq!(adj[0].balance_delta, 0); // no price → zero revenue
    }
}
