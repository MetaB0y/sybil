//! Convenient constructors for common order types.
//!
//! This module provides builder patterns for creating orders with proper
//! payoff vectors without manually calculating state indices.

use crate::market::MarketSet;
use crate::order::{ConditionDir, Order, PriceCondition, MAX_MARKETS_PER_ORDER, MAX_STATES};
use crate::state::StateSpace;
use crate::types::{MarketId, Nanos, Qty};

/// Builder for creating orders with various payoff structures.
pub struct OrderBuilder<'a> {
    markets: &'a MarketSet,
    order: Order,
    state_space: Option<StateSpace>,
}

impl<'a> OrderBuilder<'a> {
    /// Start building an order with the given ID.
    pub fn new(markets: &'a MarketSet, id: u64) -> Self {
        Self {
            markets,
            order: Order::new(id),
            state_space: None,
        }
    }

    /// Set the markets this order spans.
    pub fn spanning(mut self, market_ids: &[MarketId]) -> Self {
        assert!(
            market_ids.len() <= MAX_MARKETS_PER_ORDER,
            "Too many markets"
        );

        // Copy market IDs
        for (i, &id) in market_ids.iter().enumerate() {
            self.order.markets[i] = id;
        }
        self.order.num_markets = market_ids.len() as u8;

        // Calculate state space
        let sizes: Vec<u8> = market_ids
            .iter()
            .map(|id| self.markets.num_outcomes(*id))
            .collect();
        self.state_space = Some(StateSpace::new(&sizes));
        self.order.num_states = self.state_space.as_ref().unwrap().total_states() as u8;

        self
    }

    /// Set the limit price.
    pub fn limit(mut self, price: Nanos) -> Self {
        self.order.limit_price = price;
        self
    }

    /// Set the quantity constraints.
    pub fn quantity(mut self, min: Qty, max: Qty) -> Self {
        self.order.min_fill = min;
        self.order.max_fill = max;
        self
    }

    /// Set as all-or-none order.
    pub fn all_or_none(mut self, qty: Qty) -> Self {
        self.order.min_fill = qty;
        self.order.max_fill = qty;
        self
    }

    /// Set a price condition for activation.
    pub fn condition(
        mut self,
        market: MarketId,
        threshold: Nanos,
        direction: ConditionDir,
    ) -> Self {
        self.order.condition = Some(PriceCondition {
            market,
            threshold,
            direction,
        });
        self
    }

    /// Set payoff for a specific state.
    pub fn payoff_at(mut self, state_idx: usize, payoff: i8) -> Self {
        if state_idx < MAX_STATES {
            self.order.payoffs[state_idx] = payoff;
        }
        self
    }

    /// Set payoff when specific outcomes occur.
    /// outcomes: array of outcome indices for each market in the order.
    pub fn payoff_when(mut self, outcomes: &[u8], payoff: i8) -> Self {
        if let Some(ref space) = self.state_space {
            let idx = space.state_index(outcomes);
            if idx < MAX_STATES {
                self.order.payoffs[idx] = payoff;
            }
        }
        self
    }

    /// Build and return the order.
    pub fn build(self) -> Order {
        self.order
    }
}

// ============================================================================
// Convenience functions for common order types
// ============================================================================

/// Create a simple limit order: Buy YES on a single binary market.
pub fn simple_yes_buy(
    markets: &MarketSet,
    id: u64,
    market: MarketId,
    limit_price: Nanos,
    qty: Qty,
) -> Order {
    // For a binary market:
    // - State 0 = outcome 0 (typically "Yes")
    // - State 1 = outcome 1 (typically "No")
    // Buying YES means payoff of +1 when outcome 0 happens
    OrderBuilder::new(markets, id)
        .spanning(&[market])
        .limit(limit_price)
        .quantity(0, qty)
        .payoff_at(0, 1) // Win when outcome 0 (Yes) happens
        .payoff_at(1, 0) // Nothing when outcome 1 (No) happens
        .build()
}

/// Create a simple limit order: Buy NO on a single binary market.
pub fn simple_no_buy(
    markets: &MarketSet,
    id: u64,
    market: MarketId,
    limit_price: Nanos,
    qty: Qty,
) -> Order {
    OrderBuilder::new(markets, id)
        .spanning(&[market])
        .limit(limit_price)
        .quantity(0, qty)
        .payoff_at(0, 0) // Nothing when Yes
        .payoff_at(1, 1) // Win when No
        .build()
}

/// Internal helper for spread orders (both buy and sell).
fn spread_order(
    markets: &MarketSet,
    id: u64,
    market_a: MarketId,
    market_b: MarketId,
    limit_price: Nanos,
    qty: Qty,
    sign: i8,
) -> Order {
    // For two binary markets A and B:
    // States (using our indexing convention):
    // 0: A=0 (Yes), B=0 (Yes) -> 0
    // 1: A=1 (No),  B=0 (Yes) -> -sign (B wins, A loses)
    // 2: A=0 (Yes), B=1 (No)  -> +sign (A wins, B loses)
    // 3: A=1 (No),  B=1 (No)  -> 0

    OrderBuilder::new(markets, id)
        .spanning(&[market_a, market_b])
        .limit(limit_price)
        .quantity(0, qty)
        .payoff_when(&[0, 0], 0)
        .payoff_when(&[1, 0], -sign)
        .payoff_when(&[0, 1], sign)
        .payoff_when(&[1, 1], 0)
        .build()
}

/// Create a spread order: Buy A YES, Sell B YES (net: A - B).
/// Payoff: +1 if A wins and B loses, -1 if B wins and A loses, 0 if same.
pub fn spread(
    markets: &MarketSet,
    id: u64,
    market_a: MarketId,
    market_b: MarketId,
    limit_price: Nanos,
    qty: Qty,
) -> Order {
    spread_order(markets, id, market_a, market_b, limit_price, qty, 1)
}

/// Create a butterfly spread across 3 binary markets representing outcomes of the same event.
/// Classic volatility trade: profit if middle outcome, lose if extremes.
///
/// For a 3-candidate election (A, B, C) represented as 3 binary markets:
/// - market_a: "A wins?" (YES/NO)
/// - market_b: "B wins?" (YES/NO)
/// - market_c: "C wins?" (YES/NO)
///
/// Payoff: +1 if A wins, -2 if B wins, +1 if C wins
/// (Profits when middle outcome B occurs)
pub fn butterfly(
    markets: &MarketSet,
    id: u64,
    market_a: MarketId,
    market_b: MarketId,
    market_c: MarketId,
    limit_price: Nanos,
    qty: Qty,
) -> Order {
    // With 3 binary markets, we have 8 states (2^3)
    // But only 3 are "valid" (exactly one wins): A, B, or C
    // States where A=Yes: payoff +1
    // States where B=Yes: payoff -2
    // States where C=Yes: payoff +1
    // States with multiple Yes or all No: payoff 0 (invalid/shouldn't happen)

    OrderBuilder::new(markets, id)
        .spanning(&[market_a, market_b, market_c])
        .limit(limit_price)
        .quantity(0, qty)
        // State encoding: [market_a, market_b, market_c]
        // 0: [0,0,0] = A=Yes, B=Yes, C=Yes -> invalid, 0
        // 1: [1,0,0] = A=No,  B=Yes, C=Yes -> invalid, 0
        // 2: [0,1,0] = A=Yes, B=No,  C=Yes -> invalid, 0
        // 3: [1,1,0] = A=No,  B=No,  C=Yes -> C wins, +1
        // 4: [0,0,1] = A=Yes, B=Yes, C=No  -> invalid, 0
        // 5: [1,0,1] = A=No,  B=Yes, C=No  -> B wins, -2
        // 6: [0,1,1] = A=Yes, B=No,  C=No  -> A wins, +1
        // 7: [1,1,1] = A=No,  B=No,  C=No  -> invalid, 0
        .payoff_when(&[1, 1, 0], 1) // C wins: +1
        .payoff_when(&[1, 0, 1], -2) // B wins: -2
        .payoff_when(&[0, 1, 1], 1) // A wins: +1
        .build()
}

/// Internal helper for bundle orders (both buy and sell).
fn bundle_order(
    markets: &MarketSet,
    id: u64,
    market_ids: &[MarketId],
    limit_price: Nanos,
    qty: Qty,
    payoff_sign: i8,
) -> Order {
    let num_markets = market_ids.len();
    let all_yes: Vec<u8> = vec![0; num_markets];

    let mut builder = OrderBuilder::new(markets, id)
        .spanning(market_ids)
        .limit(limit_price)
        .all_or_none(qty);

    let sizes: Vec<u8> = market_ids
        .iter()
        .map(|id| markets.num_outcomes(*id))
        .collect();
    let state_space = StateSpace::new(&sizes);
    let winning_state = state_space.state_index(&all_yes);

    builder = builder.payoff_at(winning_state, payoff_sign);
    builder.build()
}

/// Create a bundle order: Buy YES on multiple markets (all must win).
/// This is an all-or-none atomic bundle.
pub fn bundle_yes(
    markets: &MarketSet,
    id: u64,
    market_ids: &[MarketId],
    limit_price: Nanos,
    qty: Qty,
) -> Order {
    bundle_order(markets, id, market_ids, limit_price, qty, 1)
}

/// Create a bundle sell order: Sell YES on multiple markets (all must win).
/// This is the counterparty to bundle_yes — pays out -1 when all markets are YES.
pub fn bundle_sell(
    markets: &MarketSet,
    id: u64,
    market_ids: &[MarketId],
    limit_price: Nanos,
    qty: Qty,
) -> Order {
    bundle_order(markets, id, market_ids, limit_price, qty, -1)
}

/// Create a spread sell order: Sell A YES, Buy B YES (net: B - A).
/// This is the counterparty to spread — negated payoffs.
pub fn spread_sell(
    markets: &MarketSet,
    id: u64,
    market_a: MarketId,
    market_b: MarketId,
    limit_price: Nanos,
    qty: Qty,
) -> Order {
    spread_order(markets, id, market_a, market_b, limit_price, qty, -1)
}

/// Create a multi-outcome position: Buy a specific outcome in a multi-outcome market.
pub fn outcome_buy(
    markets: &MarketSet,
    id: u64,
    market: MarketId,
    outcome_idx: u8,
    limit_price: Nanos,
    qty: Qty,
) -> Order {
    let mut builder = OrderBuilder::new(markets, id)
        .spanning(&[market])
        .limit(limit_price)
        .quantity(0, qty);

    // Payoff of 1 only when the target outcome happens
    builder = builder.payoff_at(outcome_idx as usize, 1);

    builder.build()
}

/// Create a multi-outcome position: Sell a specific outcome in a multi-outcome market.
/// Selling YES means: receive premium upfront, owe $1 if outcome happens.
pub fn outcome_sell(
    markets: &MarketSet,
    id: u64,
    market: MarketId,
    outcome_idx: u8,
    limit_price: Nanos,
    qty: Qty,
) -> Order {
    let mut builder = OrderBuilder::new(markets, id)
        .spanning(&[market])
        .limit(limit_price)
        .quantity(0, qty);

    // Payoff of -1 when the target outcome happens (seller owes $1)
    builder = builder.payoff_at(outcome_idx as usize, -1);

    builder.build()
}

/// Create a ratio spread: Buy N units of A YES, Sell M units of B YES.
#[allow(clippy::too_many_arguments)]
pub fn ratio_spread(
    markets: &MarketSet,
    id: u64,
    market_a: MarketId,
    ratio_a: i8,
    market_b: MarketId,
    ratio_b: i8,
    limit_price: Nanos,
    qty: Qty,
) -> Order {
    // For two binary markets with ratio r_a:r_b
    // Payoff when A wins: +r_a
    // Payoff when B wins: -r_b (selling B)

    OrderBuilder::new(markets, id)
        .spanning(&[market_a, market_b])
        .limit(limit_price)
        .quantity(0, qty)
        .payoff_when(&[0, 0], ratio_a - ratio_b) // Both Yes
        .payoff_when(&[1, 0], -ratio_b) // A=No, B=Yes
        .payoff_when(&[0, 1], ratio_a) // A=Yes, B=No
        .payoff_when(&[1, 1], 0) // Both No
        .build()
}

/// Create a conditional order that activates based on another market's price.
#[allow(clippy::too_many_arguments)]
pub fn conditional_buy(
    markets: &MarketSet,
    id: u64,
    market: MarketId,
    limit_price: Nanos,
    qty: Qty,
    condition_market: MarketId,
    threshold: Nanos,
    direction: ConditionDir,
) -> Order {
    OrderBuilder::new(markets, id)
        .spanning(&[market])
        .limit(limit_price)
        .quantity(0, qty)
        .payoff_at(0, 1) // Win when Yes
        .condition(condition_market, threshold, direction)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::conversions::price_to_nanos;

    fn setup_markets() -> MarketSet {
        let mut markets = MarketSet::new();
        markets.add_binary("Market A"); // M0
        markets.add_binary("Market B"); // M1
        markets.add_binary("Market C"); // M2
        markets
    }

    #[test]
    fn test_simple_yes_buy() {
        let markets = setup_markets();
        let m0 = MarketId::new(0);

        let order = simple_yes_buy(&markets, 1, m0, price_to_nanos(0.55), 100);

        assert_eq!(order.num_states, 2);
        assert_eq!(order.payoffs[0], 1); // Win on Yes
        assert_eq!(order.payoffs[1], 0); // Nothing on No
        assert_eq!(order.max_fill, 100);
    }

    #[test]
    fn test_spread() {
        let markets = setup_markets();
        let m0 = MarketId::new(0);
        let m1 = MarketId::new(1);

        let order = spread(&markets, 1, m0, m1, price_to_nanos(0.10), 50);

        assert_eq!(order.num_states, 4);
        // A=Yes, B=No should be +1
        assert_eq!(order.payoffs[2], 1);
        // A=No, B=Yes should be -1
        assert_eq!(order.payoffs[1], -1);
    }

    #[test]
    fn test_butterfly() {
        let markets = setup_markets();
        let m0 = MarketId::new(0); // Market A
        let m1 = MarketId::new(1); // Market B
        let m2 = MarketId::new(2); // Market C

        let order = butterfly(&markets, 1, m0, m1, m2, price_to_nanos(0.05), 100);

        // 3 binary markets = 8 states
        assert_eq!(order.num_states, 8);

        // Check payoffs for valid states (exactly one wins):
        // State 6: [0,1,1] = A=Yes, B=No, C=No -> A wins: +1
        assert_eq!(order.payoffs[6], 1);
        // State 5: [1,0,1] = A=No, B=Yes, C=No -> B wins: -2
        assert_eq!(order.payoffs[5], -2);
        // State 3: [1,1,0] = A=No, B=No, C=Yes -> C wins: +1
        assert_eq!(order.payoffs[3], 1);

        // Invalid states should have 0 payoff
        assert_eq!(order.payoffs[0], 0); // All Yes
        assert_eq!(order.payoffs[7], 0); // All No
    }

    #[test]
    fn test_bundle_all_or_none() {
        let markets = setup_markets();
        let m0 = MarketId::new(0);
        let m1 = MarketId::new(1);

        let order = bundle_yes(&markets, 1, &[m0, m1], price_to_nanos(0.80), 100);

        assert!(order.is_all_or_none());
        assert_eq!(order.min_fill, 100);
        assert_eq!(order.max_fill, 100);

        // Only state 0 (both Yes) should have payoff
        assert_eq!(order.payoffs[0], 1);
        assert_eq!(order.payoffs[1], 0);
        assert_eq!(order.payoffs[2], 0);
        assert_eq!(order.payoffs[3], 0);
    }

    #[test]
    fn test_conditional_order() {
        let markets = setup_markets();
        let m0 = MarketId::new(0);
        let m1 = MarketId::new(1);

        let order = conditional_buy(
            &markets,
            1,
            m0,
            price_to_nanos(0.55),
            100,
            m1,
            price_to_nanos(0.45),
            ConditionDir::Above,
        );

        assert!(order.is_conditional());
        let cond = order.condition.as_ref().unwrap();
        assert_eq!(cond.market, m1);
        assert_eq!(cond.direction, ConditionDir::Above);
    }
}
