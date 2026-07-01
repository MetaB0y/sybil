use std::collections::HashMap;

use matching_engine::{notional_nanos_ceil, MarketId, Order};

use crate::account::Account;
use crate::error::RejectionReason;

/// Position reservation key: (account_id-scoped market, outcome).
pub type PositionKey = (MarketId, u8);

/// Validate an order against account state (used for pending order re-validation).
pub fn validate_order(
    order: &Order,
    account: &Account,
    reserved_positions: &HashMap<PositionKey, i64>,
) -> Result<(), RejectionReason> {
    validate_order_with_reservation(order, account, 0, reserved_positions).map(|_| ())
}

/// Validate an order against account state, accounting for already-reserved
/// balance (buys) and already-reserved positions (sells).
///
/// Returns the cost to reserve on success (for buy orders).
/// Caller must update `reserved_positions` for accepted sell orders.
pub fn validate_order_with_reservation(
    order: &Order,
    account: &Account,
    reserved_balance: i64,
    reserved_positions: &HashMap<PositionKey, i64>,
) -> Result<i64, RejectionReason> {
    let num_states = order.num_states as usize;

    // Check if this is a buy (positive payoffs somewhere) or sell (negative payoffs)
    let has_positive = order.payoffs[..num_states].iter().any(|&p| p > 0);
    let has_negative = order.payoffs[..num_states].iter().any(|&p| p < 0);

    if has_positive && !has_negative {
        // Pure buy: check balance covers worst-case cost (minus already reserved)
        let max_cost = notional_nanos_ceil(order.limit_price, order.max_fill) as i64;
        let available = account.balance - reserved_balance;
        if max_cost > available {
            return Err(RejectionReason::InsufficientBalance {
                required: max_cost,
                available,
            });
        }
        return Ok(max_cost);
    } else if has_negative && !has_positive {
        // Pure sell: check position covers the sell (minus already reserved)
        if order.num_markets == 1 {
            let market = order.markets[0];
            for s in 0..num_states {
                if order.payoffs[s] < 0 {
                    let outcome = s as u8;
                    let sell_qty = (-order.payoffs[s] as i64) * order.max_fill as i64;
                    let reserved = reserved_positions
                        .get(&(market, outcome))
                        .copied()
                        .unwrap_or(0);
                    let available = account.position(market, outcome) - reserved;
                    if sell_qty > available {
                        return Err(RejectionReason::InsufficientPosition {
                            market,
                            outcome,
                            required: sell_qty,
                            available,
                        });
                    }
                }
            }
        }
    }

    Ok(0)
}

/// Compute the position reservations for a sell order (to be added to reserved_positions).
/// Returns a vec of (key, qty) pairs. Empty for non-sell orders.
pub fn sell_reservations(order: &Order) -> Vec<(PositionKey, i64)> {
    let num_states = order.num_states as usize;
    let has_positive = order.payoffs[..num_states].iter().any(|&p| p > 0);
    let has_negative = order.payoffs[..num_states].iter().any(|&p| p < 0);

    if !has_negative || has_positive || order.num_markets != 1 {
        return Vec::new();
    }

    let market = order.markets[0];
    let mut reservations = Vec::new();
    for s in 0..num_states {
        if order.payoffs[s] < 0 {
            let outcome = s as u8;
            let sell_qty = (-order.payoffs[s] as i64) * order.max_fill as i64;
            reservations.push(((market, outcome), sell_qty));
        }
    }
    reservations
}
