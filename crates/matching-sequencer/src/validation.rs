use std::collections::HashMap;

use matching_engine::{MarketId, Order, SHARE_SCALE};

use crate::account::Account;
use crate::error::RejectionReason;

/// Position reservation key: (account_id-scoped market, outcome).
pub type PositionKey = (MarketId, u8);

pub fn validate_order_shape(order: &Order) -> Result<(), RejectionReason> {
    order
        .validate_binary_one_hot()
        .map_err(|reason| RejectionReason::InvalidOrder(reason.to_string()))
}

/// Admission-only resource floor for orders that can become durable resting
/// state. This intentionally does not alter the engine/witness wire validator:
/// historical and guest inputs retain their existing format and semantics.
pub fn validate_resting_order_shape(
    order: &Order,
    min_notional_nanos: u64,
) -> Result<(), RejectionReason> {
    validate_order_shape(order)?;
    if order.max_fill.0 == 0 {
        return Err(RejectionReason::InvalidOrder(
            "resting order quantity must be greater than zero".to_string(),
        ));
    }
    if order.limit_price.0 == 0 {
        return Err(RejectionReason::InvalidOrder(
            "resting order price must be greater than zero".to_string(),
        ));
    }
    let notional = checked_notional_ceil_i64(order.limit_price.0, order.max_fill.0)? as u64;
    if notional < min_notional_nanos {
        return Err(RejectionReason::InvalidOrder(format!(
            "resting order notional {notional} is below minimum {min_notional_nanos} nanos"
        )));
    }
    Ok(())
}

fn checked_notional_ceil_i64(price: u64, qty: u64) -> Result<i64, RejectionReason> {
    let numerator = (price as i128)
        .checked_mul(qty as i128)
        .ok_or_else(|| RejectionReason::InvalidOrder("price*quantity overflow".to_string()))?;
    let rounded = numerator
        .checked_add(SHARE_SCALE as i128 - 1)
        .ok_or_else(|| RejectionReason::InvalidOrder("price*quantity overflow".to_string()))?
        / SHARE_SCALE as i128;
    i64::try_from(rounded)
        .map_err(|_| RejectionReason::InvalidOrder("order notional exceeds i64".to_string()))
}

fn checked_sell_qty_i64(payoff: i8, max_fill: u64) -> Option<i64> {
    if payoff >= 0 {
        return None;
    }
    let qty = (-(payoff as i128)).checked_mul(max_fill as i128)?;
    i64::try_from(qty).ok()
}

/// Compute the balance reservation implied by an order at admission.
///
/// This is also the canonical restore-time derivation for never-matched
/// resting orders: their persisted `reserved_balance` is redundant integrity
/// data and must agree with this value exactly. Matched remainders carry a
/// proportionally-scaled reservation instead (see `OrderBook::settle`), for
/// which this formula is a lower bound — the exact worst-case cost of the
/// remaining quantity.
pub(crate) fn balance_reservation(order: &Order) -> Result<i64, RejectionReason> {
    let num_states = order.num_states as usize;
    let has_positive = order.payoffs[..num_states].iter().any(|&p| p > 0);
    let has_negative = order.payoffs[..num_states].iter().any(|&p| p < 0);

    if has_positive && !has_negative {
        checked_notional_ceil_i64(order.limit_price.0, order.max_fill.0)
    } else {
        Ok(0)
    }
}

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
    validate_order_shape(order)?;

    let num_states = order.num_states as usize;

    // Check if this is a buy (positive payoffs somewhere) or sell (negative payoffs)
    let has_positive = order.payoffs[..num_states].iter().any(|&p| p > 0);
    let has_negative = order.payoffs[..num_states].iter().any(|&p| p < 0);

    if has_positive && !has_negative {
        // Pure buy: check balance covers worst-case cost (minus already reserved)
        let max_cost = balance_reservation(order)?;
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
                    let sell_qty = checked_sell_qty_i64(order.payoffs[s], order.max_fill.0)
                        .ok_or_else(|| {
                            RejectionReason::InvalidOrder("sell quantity overflow".to_string())
                        })?;
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
            if let Some(sell_qty) = checked_sell_qty_i64(order.payoffs[s], order.max_fill.0) {
                reservations.push(((market, outcome), sell_qty));
            }
        }
    }
    reservations
}
