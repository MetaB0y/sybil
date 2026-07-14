//! Shared settlement logic: pure functions computing balance and position
//! deltas from fills and minting adjustments. Used by both the sequencer
//! (to apply fills) and the verifier (to re-derive post-state for ZK
//! verification).

use std::collections::HashMap;

use crate::order::{Fill, Order};
use crate::types::{MarketId, Nanos, Qty, checked_notional_i64, checked_signed_notional_nanos};

/// Balance and position changes resulting from settling one fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettlementDelta {
    /// Change to the account's balance (negative = debit, positive = credit).
    pub balance_delta: i64,
    /// Position changes: `(market, outcome, qty_delta)`. Only non-zero deltas included.
    pub position_deltas: Vec<(MarketId, u8, i64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettlementArithmeticError {
    PriceQuantityOverflow,
    PositionQuantityOverflow,
    BalanceOverflow,
}

/// Balance and inventory delta for an explicit binary complete-set operation.
///
/// A positive `quantity` collateralizes cash into equal YES and NO inventory;
/// a negative `quantity` redeems equal inventory back into cash. Quantity is
/// expressed in protocol share-units and the $1/share cash leg is exact because
/// `NANOS_PER_DOLLAR` is divisible by `SHARE_SCALE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompleteSetDelta {
    pub balance_delta: i64,
    pub yes_delta: i64,
    pub no_delta: i64,
}

/// Compute the deterministic account delta for collateralizing `quantity` of
/// one binary market into a complete YES+NO pair.
pub fn collateralize_complete_set(
    quantity: Qty,
) -> Result<CompleteSetDelta, SettlementArithmeticError> {
    complete_set_delta(quantity, true)
}

/// Compute the deterministic account delta for redeeming `quantity` of an
/// existing binary complete set.
pub fn redeem_complete_set(quantity: Qty) -> Result<CompleteSetDelta, SettlementArithmeticError> {
    complete_set_delta(quantity, false)
}

fn complete_set_delta(
    quantity: Qty,
    collateralize: bool,
) -> Result<CompleteSetDelta, SettlementArithmeticError> {
    let position = i64::try_from(quantity.0)
        .map_err(|_| SettlementArithmeticError::PositionQuantityOverflow)?;
    let cash = checked_notional_i64(Nanos(crate::types::NANOS_PER_DOLLAR), quantity)
        .ok_or(SettlementArithmeticError::PriceQuantityOverflow)?;
    if collateralize {
        Ok(CompleteSetDelta {
            balance_delta: cash
                .checked_neg()
                .ok_or(SettlementArithmeticError::BalanceOverflow)?,
            yes_delta: position,
            no_delta: position,
        })
    } else {
        Ok(CompleteSetDelta {
            balance_delta: cash,
            yes_delta: position
                .checked_neg()
                .ok_or(SettlementArithmeticError::PositionQuantityOverflow)?,
            no_delta: position
                .checked_neg()
                .ok_or(SettlementArithmeticError::PositionQuantityOverflow)?,
        })
    }
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
    compute_fill_settlement_checked(order, fill).ok().flatten()
}

/// Checked version of [`compute_fill_settlement`].
pub fn compute_fill_settlement_checked(
    order: &Order,
    fill: &Fill,
) -> Result<Option<SettlementDelta>, SettlementArithmeticError> {
    if fill.fill_qty == Qty::ZERO {
        return Ok(None);
    }

    let num_markets = order.num_markets as usize;
    let num_states = order.num_states as usize;
    let fill_qty = i64::try_from(fill.fill_qty.0)
        .map_err(|_| SettlementArithmeticError::PositionQuantityOverflow)?;

    // Single binary market: optimized fast path
    if num_markets == 1 && num_states == 2 {
        let market = order.markets[0];
        let yes_payoff = order.payoffs[0]; // outcome 0 = YES
        let no_payoff = order.payoffs[1]; // outcome 1 = NO

        if yes_payoff > 0 && no_payoff == 0 {
            // Buying YES
            let cost = checked_notional_i64(fill.fill_price, fill.fill_qty)
                .ok_or(SettlementArithmeticError::PriceQuantityOverflow)?;
            return Ok(Some(SettlementDelta {
                balance_delta: -cost,
                position_deltas: vec![(market, 0, fill_qty)],
            }));
        } else if yes_payoff == 0 && no_payoff > 0 {
            // Buying NO
            let cost = checked_notional_i64(fill.fill_price, fill.fill_qty)
                .ok_or(SettlementArithmeticError::PriceQuantityOverflow)?;
            return Ok(Some(SettlementDelta {
                balance_delta: -cost,
                position_deltas: vec![(market, 1, fill_qty)],
            }));
        } else if yes_payoff < 0 && no_payoff == 0 {
            // Selling YES
            let revenue = checked_notional_i64(fill.fill_price, fill.fill_qty)
                .ok_or(SettlementArithmeticError::PriceQuantityOverflow)?;
            return Ok(Some(SettlementDelta {
                balance_delta: revenue,
                position_deltas: vec![(
                    market,
                    0,
                    fill_qty
                        .checked_neg()
                        .ok_or(SettlementArithmeticError::PositionQuantityOverflow)?,
                )],
            }));
        } else if yes_payoff == 0 && no_payoff < 0 {
            // Selling NO
            let revenue = checked_notional_i64(fill.fill_price, fill.fill_qty)
                .ok_or(SettlementArithmeticError::PriceQuantityOverflow)?;
            return Ok(Some(SettlementDelta {
                balance_delta: revenue,
                position_deltas: vec![(
                    market,
                    1,
                    fill_qty
                        .checked_neg()
                        .ok_or(SettlementArithmeticError::PositionQuantityOverflow)?,
                )],
            }));
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
) -> Result<Option<SettlementDelta>, SettlementArithmeticError> {
    // Debit the cost
    let cost = checked_notional_i64(fill.fill_price, fill.fill_qty)
        .ok_or(SettlementArithmeticError::PriceQuantityOverflow)?;
    let fill_qty = i64::try_from(fill.fill_qty.0)
        .map_err(|_| SettlementArithmeticError::PositionQuantityOverflow)?;
    let mut position_deltas = Vec::new();

    if num_markets == 1 {
        // Single binary market with general payoff vector
        let market = order.markets[0];
        let yes_payoff = order.payoffs[0] as i64;
        let no_payoff = order.payoffs[1] as i64;

        if yes_payoff != 0 {
            position_deltas.push((market, 0, checked_position_delta(yes_payoff, fill_qty, 1)?));
        }
        if no_payoff != 0 {
            position_deltas.push((market, 1, checked_position_delta(no_payoff, fill_qty, 1)?));
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
                    checked_position_delta(yes_per_unit, fill_qty, yes_count as i64)?,
                ));
            }
            if no_count > 0 && no_sum != 0 {
                let no_per_unit = no_sum;
                position_deltas.push((
                    market,
                    1,
                    checked_position_delta(no_per_unit, fill_qty, no_count as i64)?,
                ));
            }
        }
    }

    Ok(Some(SettlementDelta {
        balance_delta: -cost,
        position_deltas,
    }))
}

fn checked_position_delta(
    payoff_units: i64,
    fill_qty: i64,
    divisor: i64,
) -> Result<i64, SettlementArithmeticError> {
    payoff_units
        .checked_mul(fill_qty)
        .and_then(|value| value.checked_div(divisor))
        .ok_or(SettlementArithmeticError::PositionQuantityOverflow)
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
    derive_minting_checked(market_totals, clearing_prices).unwrap_or_default()
}

/// Checked version of [`derive_minting`].
pub fn derive_minting_checked(
    market_totals: &[(MarketId, i64, i64)],
    clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
) -> Result<Vec<MintAdjustment>, SettlementArithmeticError> {
    let mut adjustments = Vec::new();

    for &(market_id, total_yes, total_no) in market_totals {
        let diff = total_yes
            .checked_sub(total_no)
            .ok_or(SettlementArithmeticError::PositionQuantityOverflow)?;
        if diff == 0 {
            continue;
        }

        if diff > 0 {
            // More YES than NO → MINT shorts YES, receives yes_price revenue
            let yes_price = clearing_prices
                .get(&market_id)
                .and_then(|p| p.first().copied())
                .unwrap_or(Nanos::ZERO);
            adjustments.push(MintAdjustment {
                market_id,
                outcome: 0,
                position_delta: -diff,
                balance_delta: if yes_price == Nanos::ZERO {
                    0
                } else {
                    checked_signed_notional_nanos(yes_price, diff)
                        .ok_or(SettlementArithmeticError::PriceQuantityOverflow)?
                },
            });
        } else {
            // More NO than YES → MINT shorts NO, receives no_price revenue
            let no_price = clearing_prices
                .get(&market_id)
                .and_then(|p| p.get(1).copied())
                .unwrap_or(Nanos::ZERO);
            adjustments.push(MintAdjustment {
                market_id,
                outcome: 1,
                position_delta: diff, // negative: MINT shorts NO
                balance_delta: if no_price == Nanos::ZERO {
                    0
                } else {
                    checked_notional_i64(no_price, Qty(diff.unsigned_abs()))
                        .ok_or(SettlementArithmeticError::PriceQuantityOverflow)?
                },
            });
        }
    }

    Ok(adjustments)
}

/// Derive the signed complete-set mint/burn cost from real-account cash flow.
///
/// Creation removes collateral from real accounts, so a negative fill balance
/// delta becomes a positive cost. Burning releases collateral to real
/// accounts, so a positive fill balance delta becomes a negative cost. The
/// sign is economically load-bearing: for net demand `D`, the zero-temperature
/// minting cost is `max(D)`, which is negative when a complete set is burned.
///
/// MINT-account adjustments balance outcome inventory after settlement; they
/// are not the complete-set cost and must not be used to clamp burn proceeds
/// to zero.
pub fn minting_cost_from_fill_balance_delta(fill_balance_delta: i64) -> i64 {
    minting_cost_from_fill_balance_delta_checked(fill_balance_delta).unwrap_or_default()
}

/// Checked version of [`minting_cost_from_fill_balance_delta`].
pub fn minting_cost_from_fill_balance_delta_checked(
    fill_balance_delta: i64,
) -> Result<i64, SettlementArithmeticError> {
    fill_balance_delta
        .checked_neg()
        .ok_or(SettlementArithmeticError::BalanceOverflow)
}

/// Protocol welfare convention: gross order value net of signed mint/burn cost.
pub fn net_welfare(gross_welfare: i64, minting_cost: i64) -> i64 {
    gross_welfare - minting_cost
}

/// Compute gross order-value objective from real participant fills.
pub fn gross_welfare_from_fills<'a>(
    orders: impl IntoIterator<Item = &'a Order>,
    fills: &[Fill],
) -> i64 {
    let order_map: HashMap<u64, &Order> =
        orders.into_iter().map(|order| (order.id, order)).collect();
    fills
        .iter()
        .filter_map(|fill| {
            order_map
                .get(&fill.order_id)
                .map(|order| order.gross_welfare_contribution(fill.fill_qty))
        })
        .sum()
}

/// Compute real-account cash delta implied by fills.
pub fn fill_balance_delta_from_fills<'a>(
    orders: impl IntoIterator<Item = &'a Order>,
    fills: &[Fill],
) -> i64 {
    let order_map: HashMap<u64, &Order> =
        orders.into_iter().map(|order| (order.id, order)).collect();
    fills
        .iter()
        .filter_map(|fill| {
            order_map
                .get(&fill.order_id)
                .and_then(|order| compute_fill_settlement(order, fill))
                .map(|delta| delta.balance_delta)
        })
        .sum()
}

/// Compute the settlement-derived minting cost implied by fills.
pub fn minting_cost_from_fills<'a>(
    orders: impl IntoIterator<Item = &'a Order>,
    fills: &[Fill],
) -> i64 {
    let fill_balance_delta = fill_balance_delta_from_fills(orders, fills);
    minting_cost_from_fill_balance_delta(fill_balance_delta)
}

/// Compute per-market position totals implied by fills alone.
///
/// Callers with live account state should prefer deriving totals from that
/// state after applying fills. Solver and simulation paths do not have account
/// state, and a balanced pre-state means the fill deltas determine the same
/// incremental minting adjustment.
pub fn market_totals_from_fills<'a>(
    orders: impl IntoIterator<Item = &'a Order>,
    fills: &[Fill],
) -> Vec<(MarketId, i64, i64)> {
    let order_map: HashMap<u64, &Order> =
        orders.into_iter().map(|order| (order.id, order)).collect();
    let mut totals: HashMap<MarketId, (i64, i64)> = HashMap::new();

    for fill in fills {
        let Some(order) = order_map.get(&fill.order_id) else {
            continue;
        };
        let Some(delta) = compute_fill_settlement(order, fill) else {
            continue;
        };
        for (market, outcome, qty_delta) in delta.position_deltas {
            let entry = totals.entry(market).or_insert((0, 0));
            match outcome {
                0 => entry.0 += qty_delta,
                1 => entry.1 += qty_delta,
                _ => {}
            }
        }
    }

    let mut totals: Vec<_> = totals
        .into_iter()
        .map(|(market, (total_yes, total_no))| (market, total_yes, total_no))
        .collect();
    totals.sort_by_key(|(market, _, _)| *market);
    totals
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MarketSet, NANOS_PER_DOLLAR, notional_nanos, outcome_buy, outcome_sell, shares_to_qty,
    };
    use proptest::prelude::*;

    #[test]
    fn test_zero_qty_returns_none() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let fill = Fill::new(1, Qty(0), Nanos(500_000_000));
        assert!(compute_fill_settlement(&order, &fill).is_none());
    }

    #[test]
    fn test_buy_yes() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let qty = shares_to_qty(10);
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, qty.0);
        let fill = Fill::new(1, qty, Nanos(500_000_000));

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        assert_eq!(delta.balance_delta, -(500_000_000i64 * 10));
        assert_eq!(delta.position_deltas, vec![(m0, 0, qty.0 as i64)]);
    }

    #[test]
    fn test_buy_no() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let qty = shares_to_qty(5);
        let order = outcome_buy(&markets, 1, m0, 1, 300_000_000, qty.0);
        let fill = Fill::new(1, qty, Nanos(300_000_000));

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        assert_eq!(delta.balance_delta, -(300_000_000i64 * 5));
        assert_eq!(delta.position_deltas, vec![(m0, 1, qty.0 as i64)]);
    }

    #[test]
    fn test_sell_yes() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let qty = shares_to_qty(5);
        let order = outcome_sell(&markets, 2, m0, 0, 500_000_000, qty.0);
        let fill = Fill::new(2, qty, Nanos(500_000_000));

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        assert_eq!(delta.balance_delta, 500_000_000i64 * 5);
        assert_eq!(delta.position_deltas, vec![(m0, 0, -(qty.0 as i64))]);
    }

    #[test]
    fn test_sell_no() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let qty = shares_to_qty(3);
        let order = outcome_sell(&markets, 3, m0, 1, 400_000_000, qty.0);
        let fill = Fill::new(3, qty, Nanos(400_000_000));

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        assert_eq!(delta.balance_delta, 400_000_000i64 * 3);
        assert_eq!(delta.position_deltas, vec![(m0, 1, -(qty.0 as i64))]);
    }

    #[test]
    fn test_bundle_yes_two_markets() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("A");
        let m1 = markets.add_binary("B");
        let qty = shares_to_qty(4);
        let order = crate::bundle_yes(&markets, 10, &[m0, m1], 250_000_000, qty.0);
        let fill = Fill::new(10, qty, Nanos(250_000_000));

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        // Cost: 0.25 * 4 = 1.0
        assert_eq!(delta.balance_delta, -(250_000_000i64 * 4));
        // Bundle YES: payoffs[0]=1 (both YES), payoffs[1..3]=0
        // Each market gets +1 YES position per fill unit
        // m0: yes_sum=1, yes_count=2, delta = 1*4000/2 = 2000 units
        // m1: yes_sum=1, yes_count=2, delta = 1*4000/2 = 2000 units
        assert!(
            delta
                .position_deltas
                .iter()
                .any(|&(m, o, q)| m == m0 && o == 0 && q == shares_to_qty(2).0 as i64)
        );
        assert!(
            delta
                .position_deltas
                .iter()
                .any(|&(m, o, q)| m == m1 && o == 0 && q == shares_to_qty(2).0 as i64)
        );
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
        let fill = Fill::new(10, Qty(3), Nanos(250_000_000));

        let delta = compute_fill_settlement(&order, &fill).unwrap();
        // Bundle YES: payoffs = [1, 0, 0, 0]
        // m0: yes_sum=1, yes_count=2, delta = 1*3/2 = 1 (truncated from 1.5)
        // m1: yes_sum=1, yes_count=2, delta = 1*3/2 = 1 (truncated from 1.5)
        assert!(
            delta
                .position_deltas
                .iter()
                .any(|&(m, o, q)| m == m0 && o == 0 && q == 1)
        );
        assert!(
            delta
                .position_deltas
                .iter()
                .any(|&(m, o, q)| m == m1 && o == 0 && q == 1)
        );
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
        let fill = Fill::new(1, Qty(qty), Nanos(NANOS_PER_DOLLAR - 1));

        // Should not panic — i128 intermediate handles the multiplication
        let delta = compute_fill_settlement(&order, &fill);
        assert!(delta.is_some());
    }

    #[test]
    fn checked_settlement_reports_notional_overflow() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let qty = 9_223_372_036_855u64;
        let order = outcome_buy(&markets, 1, m0, 0, NANOS_PER_DOLLAR, qty);
        let fill = Fill::new(1, Qty(qty), Nanos(NANOS_PER_DOLLAR));

        assert_eq!(
            compute_fill_settlement_checked(&order, &fill),
            Err(SettlementArithmeticError::PriceQuantityOverflow)
        );
    }

    // --- Minting tests ---

    #[test]
    fn complete_set_collateralize_and_redeem_are_exact_inverses() {
        let qty = Qty(12_345);
        let mint = collateralize_complete_set(qty).unwrap();
        let burn = redeem_complete_set(qty).unwrap();

        assert_eq!(mint.balance_delta, -12_345_000_000);
        assert_eq!((mint.yes_delta, mint.no_delta), (12_345, 12_345));
        assert_eq!(mint.balance_delta + burn.balance_delta, 0);
        assert_eq!(mint.yes_delta + burn.yes_delta, 0);
        assert_eq!(mint.no_delta + burn.no_delta, 0);
    }

    #[test]
    fn complete_set_zero_quantity_is_a_zero_delta() {
        assert_eq!(
            collateralize_complete_set(Qty::ZERO).unwrap(),
            CompleteSetDelta {
                balance_delta: 0,
                yes_delta: 0,
                no_delta: 0,
            }
        );
    }

    #[test]
    fn test_minting_no_imbalance() {
        let m0 = MarketId(0);
        let totals = vec![(m0, shares_to_qty(100).0 as i64, shares_to_qty(100).0 as i64)];
        let prices = HashMap::new();
        assert!(derive_minting(&totals, &prices).is_empty());
    }

    #[test]
    fn test_minting_yes_surplus() {
        let m0 = MarketId(0);
        let totals = vec![(m0, shares_to_qty(150).0 as i64, shares_to_qty(100).0 as i64)]; // 50 more YES than NO
        let mut prices = HashMap::new();
        prices.insert(m0, vec![Nanos(400_000_000), Nanos(600_000_000)]); // 0.40 / 0.60

        let adj = derive_minting(&totals, &prices);
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0].market_id, m0);
        assert_eq!(adj[0].outcome, 0); // shorts YES
        assert_eq!(adj[0].position_delta, -(shares_to_qty(50).0 as i64));
        assert_eq!(adj[0].balance_delta, 400_000_000i64 * 50); // yes_price * qty
    }

    #[test]
    fn test_minting_no_surplus() {
        let m0 = MarketId(0);
        let totals = vec![(m0, shares_to_qty(100).0 as i64, shares_to_qty(180).0 as i64)]; // 80 more NO than YES
        let mut prices = HashMap::new();
        prices.insert(m0, vec![Nanos(700_000_000), Nanos(300_000_000)]);

        let adj = derive_minting(&totals, &prices);
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0].outcome, 1); // shorts NO
        assert_eq!(adj[0].position_delta, -(shares_to_qty(80).0 as i64)); // total_yes - total_no = -80
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
        prices.insert(m0, vec![Nanos(500_000_000), Nanos(500_000_000)]);

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

    #[test]
    fn signed_minting_cost_matches_complete_set_creation_and_burning() {
        let mut markets = MarketSet::new();
        let market = markets.add_binary("complete set");
        let qty = shares_to_qty(5);

        let buy_yes = outcome_buy(&markets, 1, market, 0, 60_000_000, qty.0);
        let buy_no = outcome_buy(&markets, 2, market, 1, 950_000_000, qty.0);
        let buy_fills = vec![
            Fill::new(buy_yes.id, qty, Nanos(55_000_000)),
            Fill::new(buy_no.id, qty, Nanos(945_000_000)),
        ];
        let buys = [buy_yes, buy_no];
        assert_eq!(
            minting_cost_from_fills(buys.iter(), &buy_fills),
            5 * NANOS_PER_DOLLAR as i64
        );

        let sell_yes = outcome_sell(&markets, 3, market, 0, 40_000_000, qty.0);
        let sell_no = outcome_sell(&markets, 4, market, 1, 950_000_000, qty.0);
        let sell_fills = vec![
            Fill::new(sell_yes.id, qty, Nanos(45_000_000)),
            Fill::new(sell_no.id, qty, Nanos(955_000_000)),
        ];
        let sells = [sell_yes, sell_no];
        assert_eq!(
            minting_cost_from_fills(sells.iter(), &sell_fills),
            -(5 * NANOS_PER_DOLLAR as i64)
        );
    }

    #[test]
    fn complete_set_burning_welfare_equals_non_negative_fill_surplus() {
        let mut markets = MarketSet::new();
        let market = markets.add_binary("complete set");
        let qty = shares_to_qty(5);
        let sell_yes = outcome_sell(&markets, 1, market, 0, 40_000_000, qty.0);
        let sell_no = outcome_sell(&markets, 2, market, 1, 950_000_000, qty.0);
        let fills = vec![
            Fill::new(sell_yes.id, qty, Nanos(45_000_000)),
            Fill::new(sell_no.id, qty, Nanos(955_000_000)),
        ];
        let orders = [sell_yes, sell_no];

        let gross = gross_welfare_from_fills(orders.iter(), &fills);
        let minting_cost = minting_cost_from_fills(orders.iter(), &fills);
        let expected_fill_surplus: i64 = fills
            .iter()
            .zip(&orders)
            .map(|(fill, order)| order.welfare_contribution(fill.fill_price, fill.fill_qty))
            .sum();

        assert_eq!(gross, -4_950_000_000);
        assert_eq!(minting_cost, -5_000_000_000);
        assert_eq!(net_welfare(gross, minting_cost), 50_000_000);
        assert_eq!(net_welfare(gross, minting_cost), expected_fill_surplus);
    }

    proptest! {
        #[test]
        fn simple_binary_settlement_matches_notional_and_position(
            outcome in 0u8..=1,
            is_sell in any::<bool>(),
            price in 0u64..=NANOS_PER_DOLLAR,
            qty in 1u64..=shares_to_qty(1_000_000).0,
        ) {
            let mut markets = MarketSet::new();
            let market = markets.add_binary("prop");
            let order = if is_sell {
                outcome_sell(&markets, 9, market, outcome, price, qty)
            } else {
                outcome_buy(&markets, 9, market, outcome, price, qty)
            };
            let fill = Fill::new(order.id, Qty(qty), Nanos(price));

            let delta = compute_fill_settlement(&order, &fill).expect("nonzero fill settles");
            let notional = notional_nanos(Nanos(price), Qty(qty)).0 as i64;
            let signed_qty = if is_sell { -(qty as i64) } else { qty as i64 };

            prop_assert_eq!(
                delta.balance_delta,
                if is_sell { notional } else { -notional }
            );
            prop_assert_eq!(delta.position_deltas, vec![(market, outcome, signed_qty)]);
        }

        #[test]
        fn zero_quantity_fill_is_always_noop(
            outcome in 0u8..=1,
            is_sell in any::<bool>(),
            price in 0u64..=NANOS_PER_DOLLAR,
            order_qty in 0u64..=shares_to_qty(1_000_000).0,
        ) {
            let mut markets = MarketSet::new();
            let market = markets.add_binary("zero");
            let order = if is_sell {
                outcome_sell(&markets, 4, market, outcome, price, order_qty)
            } else {
                outcome_buy(&markets, 4, market, outcome, price, order_qty)
            };
            let fill = Fill::new(order.id, Qty(0), Nanos(price));

            prop_assert!(compute_fill_settlement(&order, &fill).is_none());
        }

        #[test]
        fn minting_adjustment_restores_yes_no_balance(
            market_id in 0u32..1000,
            total_yes in -1_000_000i64..=1_000_000,
            total_no in -1_000_000i64..=1_000_000,
            yes_price in 0u64..=NANOS_PER_DOLLAR,
            no_price in 0u64..=NANOS_PER_DOLLAR,
        ) {
            let market = MarketId::new(market_id);
            let totals = vec![(market, total_yes, total_no)];
            let mut prices = HashMap::new();
            prices.insert(market, vec![Nanos(yes_price), Nanos(no_price)]);

            let adjustments = derive_minting(&totals, &prices);
            if total_yes == total_no {
                prop_assert!(adjustments.is_empty());
                return Ok(());
            }

            prop_assert_eq!(adjustments.len(), 1);
            let adjustment = &adjustments[0];
            prop_assert_eq!(adjustment.market_id, market);

            let mut adjusted_yes = total_yes;
            let mut adjusted_no = total_no;
            if total_yes > total_no {
                let diff = total_yes - total_no;
                prop_assert_eq!(adjustment.outcome, 0);
                prop_assert_eq!(adjustment.position_delta, -diff);
                prop_assert_eq!(
                    adjustment.balance_delta,
                    notional_nanos(Nanos(yes_price), Qty(diff as u64)).0 as i64
                );
                adjusted_yes += adjustment.position_delta;
            } else {
                let diff = total_yes - total_no;
                let abs_diff = diff.unsigned_abs();
                prop_assert_eq!(adjustment.outcome, 1);
                prop_assert_eq!(adjustment.position_delta, diff);
                prop_assert_eq!(
                    adjustment.balance_delta,
                    notional_nanos(Nanos(no_price), Qty(abs_diff)).0 as i64
                );
                adjusted_no += adjustment.position_delta;
            }

            prop_assert_eq!(adjusted_yes, adjusted_no);
        }
    }
}
