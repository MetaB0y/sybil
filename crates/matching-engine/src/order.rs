//! Unified order representation using payoff vectors.
//!
//! Every order is represented as a payoff vector over atomic world states.
//! This unified representation handles: simple limits, spreads, butterflies,
//! iron condors, ratio spreads, baskets, and any other derivative structure.

use serde::{Deserialize, Serialize};

use crate::types::{
    MAX_ORDER_QTY, MarketId, NANOS_PER_DOLLAR, Nanos, OrderDirection, Qty, notional_nanos,
    signed_price_delta_notional,
};

/// A (MarketId, value) pair from marginal payoff computation.
pub type MarginalPayoff<T> = (MarketId, T);

/// Maximum number of markets a single order can span.
pub const MAX_MARKETS_PER_ORDER: usize = 5;

/// Maximum number of atomic states (2^5 = 32 for 5 binary markets).
pub const MAX_STATES: usize = 32;

/// Simple price-based condition for order activation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceCondition {
    pub market: MarketId,
    pub threshold: Nanos,
    pub direction: ConditionDir,
}

/// Direction for conditional activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub id: u64,

    /// Markets spanned (max 5, unused = MarketId::NONE)
    pub markets: [MarketId; MAX_MARKETS_PER_ORDER],

    /// Number of active markets (precomputed for convenience)
    pub num_markets: u8,

    /// Payoff coefficient per atomic state.
    /// +N = long N units of the payoff per filled quantity unit, -N = short,
    /// 0 = no exposure. Fill quantities are fixed-point share units.
    /// i8 sufficient: range -128 to +127 payoff units per state.
    pub payoffs: [i8; MAX_STATES],

    /// Number of atomic states (= product of outcomes per market)
    pub num_states: u8,

    /// Maximum cost willing to pay (in nanos per unit of position).
    /// For a buyer, this is the max price.
    /// For a seller, use a negative payoff and this represents min acceptable.
    pub limit_price: Nanos,

    /// Maximum fill quantity.
    pub max_fill: Qty,

    /// Optional price-threshold condition for activation.
    pub condition: Option<PriceCondition>,

    /// Last block height where the order is eligible. `None` means the
    /// sequencer's system TTL decides the expiry.
    #[serde(default)]
    pub expires_at_block: Option<u64>,
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
            limit_price: Nanos::ZERO,
            max_fill: Qty::ZERO,
            condition: None,
            expires_at_block: None,
        }
    }

    /// Return the last block height where this order may participate, capped
    /// by the sequencer's system TTL.
    pub fn effective_expires_at_block(&self, created_at: u64, system_ttl_blocks: u64) -> u64 {
        let system_expiry = created_at.saturating_add(system_ttl_blocks);
        self.expires_at_block
            .unwrap_or(system_expiry)
            .min(system_expiry)
    }

    /// Returns true if this order is not eligible for the given block height.
    pub fn is_expired_at_block(&self, current_height: u64, created_at: u64, ttl: u64) -> bool {
        current_height > self.effective_expires_at_block(created_at, ttl)
    }

    /// Check if this is a conditional order.
    pub fn is_conditional(&self) -> bool {
        self.condition.is_some()
    }

    /// Get the active markets (non-NONE) for this order.
    pub fn active_markets(&self) -> impl Iterator<Item = MarketId> + '_ {
        self.markets.iter().take(self.num_markets as usize).copied()
    }

    /// Calculate welfare contribution if this order fills at the given price.
    /// Welfare = (limit_price - fill_price) * quantity for buyers
    /// (happy when fill < limit), scaled by `SHARE_SCALE`.
    /// Welfare = (fill_price - limit_price) * quantity for sellers
    /// (happy when fill > limit), scaled by `SHARE_SCALE`.
    pub fn welfare_contribution(&self, fill_price: Nanos, fill_qty: Qty) -> i64 {
        if fill_qty == Qty::ZERO {
            return 0;
        }
        let surplus_per_unit = if self.is_seller() {
            fill_price.0 as i64 - self.limit_price.0 as i64
        } else {
            self.limit_price.0 as i64 - fill_price.0 as i64
        };
        signed_price_delta_notional(surplus_per_unit, fill_qty)
    }

    /// Gross objective contribution before protocol mint/burn cost.
    ///
    /// Buyers contribute `+limit_price * qty`; sellers contribute
    /// `-limit_price * qty`. Subtracting the signed settlement-derived
    /// mint/burn cost from the sum of these terms gives protocol welfare.
    pub fn gross_welfare_contribution(&self, fill_qty: Qty) -> i64 {
        let value = notional_nanos(self.limit_price, fill_qty).0 as i64;
        if self.is_seller() { -value } else { value }
    }

    /// Check if this order would be satisfied at a given price.
    /// For a buyer: satisfied if price <= limit_price (pay no more than limit).
    /// For a seller: satisfied if price >= limit_price (receive at least limit).
    pub fn is_satisfied_at_price(&self, price: Nanos) -> bool {
        if self.is_seller() {
            price >= self.limit_price
        } else {
            price <= self.limit_price
        }
    }

    /// Returns true if this order has any negative payoffs (i.e., it's a seller).
    pub fn is_seller(&self) -> bool {
        self.payoffs[..self.num_states as usize]
            .iter()
            .any(|&p| p < 0)
    }

    /// Validate the production-supported public order shape.
    ///
    /// The core payoff-vector representation intentionally remains more general
    /// for research and tests, but current public admission and solvers only
    /// support one binary market with exactly one ±1 payoff entry.
    pub fn validate_binary_one_hot(&self) -> Result<(), &'static str> {
        if self.num_markets != 1 {
            return Err("orders must span exactly one market");
        }
        if self.num_states != 2 {
            return Err("orders must have exactly two binary states");
        }
        if self.markets[0].is_none() {
            return Err("orders must reference a concrete market");
        }
        if self.markets[1..].iter().any(|market| !market.is_none()) {
            return Err("inactive market entries must be NONE");
        }
        if self.limit_price.0 > NANOS_PER_DOLLAR {
            return Err("limit price exceeds NANOS_PER_DOLLAR");
        }
        if self.max_fill.0 > MAX_ORDER_QTY {
            return Err("order quantity exceeds MAX_ORDER_QTY");
        }

        let active = &self.payoffs[..2];
        let non_zero = active.iter().filter(|&&payoff| payoff != 0).count();
        if non_zero != 1 {
            return Err("orders must have exactly one non-zero payoff");
        }
        if !active.iter().any(|&payoff| payoff == 1 || payoff == -1) {
            return Err("non-zero payoff must be +1 or -1");
        }
        if self.payoffs[2..].iter().any(|&payoff| payoff != 0) {
            return Err("inactive payoff entries must be zero");
        }

        Ok(())
    }

    /// Per-market marginal payoff using stride-based decomposition (integer version).
    ///
    /// For each market, computes (sum of payoffs where market=YES) - (sum where market=NO),
    /// normalized by 2^(N-1) (the number of "other" state pairs). Truncating integer division
    /// matches verifier semantics.
    ///
    /// +1 = long 1 YES per fill, -1 = short 1 YES per fill.
    /// Non-separable bundles truncate to 0 (e.g., `[1,0,0,0]` → marginal 1/2 → truncated to 0).
    pub fn marginal_payoffs_i64(&self) -> Vec<MarginalPayoff<i64>> {
        let num_markets = self.num_markets as usize;
        let num_states = self.num_states as usize;
        let mut result = Vec::new();

        for m_idx in 0..num_markets {
            let market = self.markets[m_idx];
            if market.is_none() {
                continue;
            }
            let stride = 1usize << m_idx;
            let mut marginal: i64 = 0;

            for s in 0..num_states {
                let outcome = (s / stride) % 2;
                let payoff = self.payoffs[s] as i64;
                if outcome == 0 {
                    marginal += payoff;
                } else {
                    marginal -= payoff;
                }
            }

            let other_states = (num_states / 2) as i64;
            if other_states > 0 {
                let normalized = marginal / other_states;
                if normalized != 0 {
                    result.push((market, normalized));
                }
            }
        }

        result
    }

    /// Per-market marginal payoff using stride-based decomposition (f64 version).
    ///
    /// Same as `marginal_payoffs_i64` but without truncation. Needed for non-separable
    /// bundles where the marginal is fractional (e.g., `[1,0,0,0]` spanning 2 markets
    /// gives marginal 0.5 per market).
    // Exempt from the f64 ban: this is an indicative/off-consensus helper used
    // by the floating-point solver for fractional bundle marginals; it never
    // feeds the state root (the integer `marginal_payoffs_i64` does that).
    #[allow(clippy::disallowed_types)]
    pub fn marginal_payoffs_f64(&self) -> Vec<MarginalPayoff<f64>> {
        let num_markets = self.num_markets as usize;
        let num_states = self.num_states as usize;
        let mut result = Vec::new();

        for m_idx in 0..num_markets {
            let market = self.markets[m_idx];
            if market.is_none() {
                continue;
            }
            let stride = 1usize << m_idx;
            let mut marginal: i64 = 0;

            for s in 0..num_states {
                let outcome = (s / stride) % 2;
                let payoff = self.payoffs[s] as i64;
                if outcome == 0 {
                    marginal += payoff;
                } else {
                    marginal -= payoff;
                }
            }

            let other_states = (num_states / 2) as f64;
            if other_states > 0.0 {
                let normalized = marginal as f64 / other_states;
                if normalized.abs() > 1e-12 {
                    result.push((market, normalized));
                }
            }
        }

        result
    }
}

/// Derive a categorical `OrderDirection` (BuyYes/SellYes/BuyNo/SellNo) for an
/// order with respect to its primary binary market.
///
/// For single-market binary orders this is exact, matching the `outcome_buy` /
/// `outcome_sell` constructions in [`crate::order_builder`]:
/// * `BuyYes` ↔ payoffs `[+N, 0]`
/// * `SellYes` ↔ payoffs `[-N, 0]`
/// * `BuyNo` ↔ payoffs `[0, +N]`
/// * `SellNo` ↔ payoffs `[0, -N]`
///
/// For multi-market orders (spreads, bundles, butterflies) it picks the side
/// dominant for `primary_market`. The result is a categorical label used by
/// the on-chain `OrderCancelled` event and FE cancel feed — not a complete
/// description of the order's payoff structure.
pub fn derive_order_direction(order: &Order, primary_market: MarketId) -> OrderDirection {
    let m_idx = order
        .markets
        .iter()
        .take(order.num_markets as usize)
        .position(|&m| m == primary_market)
        .unwrap_or(0);

    let stride = 1usize << m_idx;
    let num_states = order.num_states as usize;

    let mut sum_yes: i64 = 0;
    let mut sum_no: i64 = 0;
    for s in 0..num_states {
        let outcome = (s / stride) % 2;
        let p = order.payoffs[s] as i64;
        if outcome == 0 {
            sum_yes += p;
        } else {
            sum_no += p;
        }
    }

    if sum_yes.abs() >= sum_no.abs() {
        if sum_yes >= 0 {
            OrderDirection::BuyYes
        } else {
            OrderDirection::SellYes
        }
    } else if sum_no >= 0 {
        OrderDirection::BuyNo
    } else {
        OrderDirection::SellNo
    }
}

impl std::fmt::Display for Order {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let markets: Vec<_> = self.active_markets().map(|m| format!("{}", m)).collect();
        let payoffs: Vec<_> = self
            .payoffs
            .iter()
            .take(self.num_states as usize)
            .copied()
            .collect();
        write!(
            f,
            "Order#{} markets=[{}] payoffs={:?} limit={} qty={}",
            self.id,
            markets.join(","),
            payoffs,
            self.limit_price,
            self.max_fill
        )
    }
}

/// Result of matching: how much of an order was filled.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fill {
    pub order_id: u64,
    pub fill_qty: Qty,
    pub fill_price: Nanos,
    /// Account that placed the order. Populated by the sequencer after solving;
    /// 0 when created by the solver (which has no account context).
    #[serde(default)]
    pub account_id: u64,
}

impl Fill {
    pub fn new(order_id: u64, fill_qty: Qty, fill_price: Nanos) -> Self {
        Self {
            order_id,
            fill_qty,
            fill_price,
            account_id: 0,
        }
    }

    pub fn welfare(&self, order: &Order) -> i64 {
        order.welfare_contribution(self.fill_price, self.fill_qty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NANOS_PER_DOLLAR, shares_to_qty, signed_price_delta_notional};
    use proptest::prelude::*;

    #[test]
    fn test_order_creation() {
        let order = Order::new(1);
        assert_eq!(order.id, 1);
        assert_eq!(order.num_markets, 0);
        assert_eq!(order.num_states, 0);
        assert!(!order.is_conditional());
    }

    #[test]
    fn test_welfare_contribution() {
        let mut order = Order::new(1);
        order.limit_price = Nanos(600_000_000); // 0.60 in nanos

        // Fill at 0.50
        let welfare = order.welfare_contribution(Nanos(500_000_000), shares_to_qty(100));
        // (0.60 - 0.50) * 100 = 10 in nano-equivalent
        assert_eq!(welfare, 100_000_000 * 100);
    }

    fn binary_order(payoff_yes: i8, payoff_no: i8) -> Order {
        let mut o = Order::new(0);
        o.markets[0] = MarketId::new(7);
        o.num_markets = 1;
        o.num_states = 2;
        o.payoffs[0] = payoff_yes;
        o.payoffs[1] = payoff_no;
        o
    }

    #[test]
    fn order_direction_derivation() {
        let m = MarketId::new(7);

        assert_eq!(
            derive_order_direction(&binary_order(1, 0), m),
            OrderDirection::BuyYes
        );
        assert_eq!(
            derive_order_direction(&binary_order(-1, 0), m),
            OrderDirection::SellYes
        );
        assert_eq!(
            derive_order_direction(&binary_order(0, 1), m),
            OrderDirection::BuyNo
        );
        assert_eq!(
            derive_order_direction(&binary_order(0, -1), m),
            OrderDirection::SellNo
        );
    }

    #[test]
    fn order_direction_spread_picks_primary_side() {
        // Buy A YES, Sell B YES (per `spread_order` with sign=+1):
        //   state 0 [A=YES, B=YES]: 0
        //   state 1 [A=NO,  B=YES]: -1
        //   state 2 [A=YES, B=NO ]: +1
        //   state 3 [A=NO,  B=NO ]: 0
        let mut o = Order::new(0);
        o.markets[0] = MarketId::new(1);
        o.markets[1] = MarketId::new(2);
        o.num_markets = 2;
        o.num_states = 4;
        o.payoffs[0] = 0;
        o.payoffs[1] = -1;
        o.payoffs[2] = 1;
        o.payoffs[3] = 0;

        // Primary market A — long A YES.
        assert_eq!(
            derive_order_direction(&o, MarketId::new(1)),
            OrderDirection::BuyYes
        );
        // Primary market B — short B YES.
        assert_eq!(
            derive_order_direction(&o, MarketId::new(2)),
            OrderDirection::SellYes
        );
    }

    #[test]
    fn order_direction_byte_mapping_is_stable() {
        // The byte mapping is committed by the verifier leaf encoding and
        // must not drift. If this test fails, an old block's events_root
        // would no longer verify under the new encoding.
        assert_eq!(OrderDirection::BuyYes.to_byte(), 0);
        assert_eq!(OrderDirection::SellYes.to_byte(), 1);
        assert_eq!(OrderDirection::BuyNo.to_byte(), 2);
        assert_eq!(OrderDirection::SellNo.to_byte(), 3);
    }

    proptest! {
        #[test]
        fn welfare_and_satisfaction_match_order_side(
            payoff_kind in 0u8..4,
            limit_price in 0u64..=NANOS_PER_DOLLAR,
            fill_price in 0u64..=NANOS_PER_DOLLAR,
            fill_qty in 0u64..=shares_to_qty(1_000_000).0,
        ) {
            let (yes_payoff, no_payoff) = match payoff_kind {
                0 => (1, 0),
                1 => (-1, 0),
                2 => (0, 1),
                _ => (0, -1),
            };
            let mut order = binary_order(yes_payoff, no_payoff);
            order.limit_price = Nanos(limit_price);
            order.max_fill = Qty(fill_qty);

            let seller = order.is_seller();
            let surplus_per_unit = if seller {
                fill_price as i64 - limit_price as i64
            } else {
                limit_price as i64 - fill_price as i64
            };
            let expected_welfare = signed_price_delta_notional(surplus_per_unit, Qty(fill_qty));

            prop_assert_eq!(
                order.welfare_contribution(Nanos(fill_price), Qty(fill_qty)),
                expected_welfare
            );
            prop_assert_eq!(
                order.is_satisfied_at_price(Nanos(fill_price)),
                if seller {
                    fill_price >= limit_price
                } else {
                    fill_price <= limit_price
                }
            );
            if fill_qty > 0 {
                if order.is_satisfied_at_price(Nanos(fill_price)) {
                    prop_assert!(expected_welfare >= 0);
                } else {
                    prop_assert!(expected_welfare <= 0);
                }
            }
        }
    }
}
