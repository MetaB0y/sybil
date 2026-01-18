//! Unified order representation using payoff vectors.
//!
//! Every order is represented as a payoff vector over atomic world states.
//! This unified representation handles: simple limits, spreads, butterflies,
//! iron condors, ratio spreads, baskets, and any other derivative structure.

use crate::types::{MarketId, Nanos, Qty};

/// Maximum number of markets a single order can span.
pub const MAX_MARKETS_PER_ORDER: usize = 5;

/// Maximum number of atomic states (2^5 = 32 for 5 binary markets).
pub const MAX_STATES: usize = 32;

/// Simple price-based condition for order activation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceCondition {
    pub market: MarketId,
    pub threshold: Nanos,
    pub direction: ConditionDir,
}

/// Direction for conditional activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConditionDir {
    /// Activate if clearing_price > threshold
    Above,
    /// Activate if clearing_price < threshold
    Below,
}

/// An order represented as a payoff vector over atomic world states.
///
/// # Example
///
/// For a simple limit order "Buy YES on market M0" (binary market):
/// - markets: [M0, NONE, NONE, NONE, NONE]
/// - num_states: 2
/// - payoffs: [0, +1, 0, 0, ...] (state 0 = NO, state 1 = YES)
///   Actually: [+1, 0, ...] if state 0 = YES wins, state 1 = NO wins
///
/// For a spread "Buy A YES, Sell B YES" across two binary markets:
/// - markets: [A, B, NONE, NONE, NONE]
/// - num_states: 4
/// - payoffs depend on state indexing convention
#[derive(Clone, Debug)]
pub struct Order {
    pub id: u64,

    /// Markets spanned (max 5, unused = MarketId::NONE)
    pub markets: [MarketId; MAX_MARKETS_PER_ORDER],

    /// Number of active markets (precomputed for convenience)
    pub num_markets: u8,

    /// Payoff coefficient per atomic state.
    /// +N = long N shares, -N = short N shares, 0 = no exposure.
    /// i8 sufficient: range -128 to +127 shares per state.
    pub payoffs: [i8; MAX_STATES],

    /// Number of atomic states (= product of outcomes per market)
    pub num_states: u8,

    /// Maximum cost willing to pay (in nanos per unit of position).
    /// For a buyer, this is the max price.
    /// For a seller, use a negative payoff and this represents min acceptable.
    pub limit_price: Nanos,

    /// Minimum fill quantity. 0 = partial OK.
    /// Set equal to max_fill for all-or-none orders.
    pub min_fill: Qty,

    /// Maximum fill quantity.
    pub max_fill: Qty,

    /// Optional price-threshold condition for activation.
    pub condition: Option<PriceCondition>,
}

impl Order {
    /// Create a new order with default values.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            markets: [MarketId::NONE; MAX_MARKETS_PER_ORDER],
            num_markets: 0,
            payoffs: [0; MAX_STATES],
            num_states: 0,
            limit_price: 0,
            min_fill: 0,
            max_fill: 0,
            condition: None,
        }
    }

    /// Check if this is an all-or-none order.
    pub fn is_all_or_none(&self) -> bool {
        self.min_fill > 0 && self.min_fill == self.max_fill
    }

    /// Check if this is a conditional order.
    pub fn is_conditional(&self) -> bool {
        self.condition.is_some()
    }

    /// Calculate the expected payoff at a given state.
    pub fn payoff_at_state(&self, state_idx: usize) -> i8 {
        if state_idx < self.num_states as usize {
            self.payoffs[state_idx]
        } else {
            0
        }
    }

    /// Calculate the expected value of this order given state probabilities.
    /// probs should have length = num_states, values summing to 1.
    pub fn expected_value(&self, probs: &[f64]) -> f64 {
        let mut ev = 0.0;
        for (i, &payoff) in self.payoffs.iter().take(self.num_states as usize).enumerate() {
            if i < probs.len() {
                ev += payoff as f64 * probs[i];
            }
        }
        ev
    }

    /// Get the active markets (non-NONE) for this order.
    pub fn active_markets(&self) -> impl Iterator<Item = MarketId> + '_ {
        self.markets.iter().take(self.num_markets as usize).copied()
    }

    /// Calculate welfare contribution if this order fills at the given price.
    /// Welfare = (limit_price - fill_price) * quantity for buyers
    /// For payoff vectors, welfare is the difference between willingness to pay
    /// and actual cost.
    pub fn welfare_contribution(&self, fill_price: Nanos, fill_qty: Qty) -> i64 {
        if fill_qty == 0 {
            return 0;
        }
        // Welfare = (what they were willing to pay - what they paid) * qty
        // For a buyer: limit_price >= fill_price, positive welfare
        // For a seller (represented with negative payoffs): similar logic
        let surplus_per_unit = self.limit_price as i64 - fill_price as i64;
        surplus_per_unit * fill_qty as i64
    }

    /// Check if this order would be satisfied at a given expected value.
    /// For a buyer (positive payoff expectation), satisfied if limit_price >= expected_cost.
    pub fn is_satisfied_at_price(&self, price: Nanos) -> bool {
        price <= self.limit_price
    }
}

impl std::fmt::Display for Order {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let markets: Vec<_> = self.active_markets().map(|m| format!("{}", m)).collect();
        let payoffs: Vec<_> = self.payoffs.iter().take(self.num_states as usize).copied().collect();
        write!(
            f,
            "Order#{} markets=[{}] payoffs={:?} limit={} qty=[{},{}]",
            self.id,
            markets.join(","),
            payoffs,
            self.limit_price,
            self.min_fill,
            self.max_fill
        )
    }
}

/// Result of matching: how much of an order was filled.
#[derive(Clone, Debug)]
pub struct Fill {
    pub order_id: u64,
    pub fill_qty: Qty,
    pub fill_price: Nanos,
}

impl Fill {
    pub fn new(order_id: u64, fill_qty: Qty, fill_price: Nanos) -> Self {
        Self {
            order_id,
            fill_qty,
            fill_price,
        }
    }

    pub fn welfare(&self, order: &Order) -> i64 {
        order.welfare_contribution(self.fill_price, self.fill_qty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_creation() {
        let order = Order::new(1);
        assert_eq!(order.id, 1);
        assert_eq!(order.num_markets, 0);
        assert_eq!(order.num_states, 0);
        assert!(!order.is_all_or_none());
        assert!(!order.is_conditional());
    }

    #[test]
    fn test_all_or_none() {
        let mut order = Order::new(1);
        order.min_fill = 100;
        order.max_fill = 100;
        assert!(order.is_all_or_none());
    }

    #[test]
    fn test_expected_value() {
        let mut order = Order::new(1);
        order.num_states = 2;
        order.payoffs[0] = 0;  // NO outcome
        order.payoffs[1] = 1;  // YES outcome

        // 50/50 probability
        let ev = order.expected_value(&[0.5, 0.5]);
        assert!((ev - 0.5).abs() < 1e-9);

        // 100% YES
        let ev = order.expected_value(&[0.0, 1.0]);
        assert!((ev - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_welfare_contribution() {
        let mut order = Order::new(1);
        order.limit_price = 600_000_000; // 0.60 in nanos

        // Fill at 0.50
        let welfare = order.welfare_contribution(500_000_000, 100);
        // (0.60 - 0.50) * 100 = 10 in nano-equivalent
        assert_eq!(welfare, 100_000_000 * 100);
    }
}
