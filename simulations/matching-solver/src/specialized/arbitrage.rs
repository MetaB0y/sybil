//! Arbitrage detection and exploitation.
//!
//! Finds riskless profit opportunities from:
//! 1. Constraint arbitrage: If A→B and price(A) > price(B), profit is possible
//! 2. Bundle underpricing: Sum of legs < bundle price
//! 3. Cross-market mispricing: Same effective exposure at different prices

use std::collections::HashMap;

use matching_engine::{
    ConstraintSet, Fill, LiquidityPool, MarketConstraint, MarketId, Nanos, Order, Problem,
};

use crate::{MatchingResult, Solver};

/// Capabilities of a specialized solver.
#[derive(Clone, Debug, Default)]
pub struct SolverCapabilities {
    /// Can handle single-market orders
    pub simple_orders: bool,
    /// Can handle multi-market bundles
    pub bundles: bool,
    /// Can handle conditional orders
    pub conditionals: bool,
    /// Can handle all-or-none orders
    pub all_or_none: bool,
    /// Can handle arbitrage opportunities
    pub arbitrage: bool,
}

/// Detected arbitrage opportunity.
#[derive(Clone, Debug)]
pub struct ArbitrageOpportunity {
    /// Type of arbitrage
    pub kind: ArbitrageKind,
    /// Orders involved in this arbitrage
    pub order_indices: Vec<usize>,
    /// Expected profit per unit
    pub profit_per_unit: Nanos,
    /// Maximum quantity available
    pub max_quantity: u64,
    /// Confidence in the arbitrage (0.0 to 1.0)
    pub confidence: f64,
}

/// Types of arbitrage opportunities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArbitrageKind {
    /// Constraint-based: A→B with price(A) > price(B)
    Constraint,
    /// Bundle underpricing: bundle cheaper than sum of legs
    BundleUnderpricing,
    /// Cross-market: same exposure at different prices
    CrossMarket,
}

/// Detects and exploits arbitrage opportunities.
pub struct ArbitrageDetector {
    /// Minimum profit threshold (in nanos) to consider an opportunity
    min_profit_threshold: Nanos,
    /// Capabilities of this solver
    capabilities: SolverCapabilities,
}

impl ArbitrageDetector {
    /// Create a new arbitrage detector with default settings.
    pub fn new() -> Self {
        Self {
            min_profit_threshold: 1_000_000, // 0.001 dollars
            capabilities: SolverCapabilities {
                arbitrage: true,
                ..Default::default()
            },
        }
    }

    /// Set the minimum profit threshold.
    pub fn with_min_profit(mut self, threshold: Nanos) -> Self {
        self.min_profit_threshold = threshold;
        self
    }

    /// Get the capabilities of this solver.
    pub fn capabilities(&self) -> &SolverCapabilities {
        &self.capabilities
    }

    /// Detect all arbitrage opportunities in a problem.
    pub fn detect_opportunities(&self, problem: &Problem) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = Vec::new();

        // 1. Constraint arbitrage
        let constraint_arbs = self.detect_constraint_arbitrage(problem);
        opportunities.extend(constraint_arbs);

        // 2. Bundle underpricing
        let bundle_arbs = self.detect_bundle_arbitrage(problem);
        opportunities.extend(bundle_arbs);

        // Filter by minimum profit
        opportunities.retain(|opp| opp.profit_per_unit >= self.min_profit_threshold);

        // Sort by profit (highest first)
        opportunities.sort_by(|a, b| b.profit_per_unit.cmp(&a.profit_per_unit));

        opportunities
    }

    /// Detect constraint-based arbitrage.
    ///
    /// If A→B (A implies B) and we can buy A cheaper than B,
    /// buying A effectively gets us B exposure for less.
    fn detect_constraint_arbitrage(&self, problem: &Problem) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = Vec::new();

        for constraint in problem.constraints.iter() {
            if let MarketConstraint::Implication { if_true, then_true } = constraint {
                // Get best prices for buying each outcome
                let price_if = self.best_ask_price(&problem.liquidity, if_true.0, if_true.1);
                let price_then = self.best_ask_price(&problem.liquidity, then_true.0, then_true.1);

                if let (Some(p_if), Some(p_then)) = (price_if, price_then) {
                    // If we can buy A (which implies B) for less than B's price,
                    // we could potentially exploit this
                    if p_if < p_then {
                        let profit_per_unit = p_then - p_if;
                        let max_qty = self.available_quantity(&problem.liquidity, if_true.0, if_true.1);

                        opportunities.push(ArbitrageOpportunity {
                            kind: ArbitrageKind::Constraint,
                            order_indices: Vec::new(), // No specific orders yet
                            profit_per_unit,
                            max_quantity: max_qty,
                            confidence: 0.9, // High confidence for constraint arbitrage
                        });
                    }
                }
            }
        }

        opportunities
    }

    /// Detect bundle underpricing arbitrage.
    ///
    /// If an order buys a bundle for less than the sum of individual leg costs,
    /// that's potential arbitrage.
    fn detect_bundle_arbitrage(&self, problem: &Problem) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = Vec::new();

        for (order_idx, order) in problem.orders.iter().enumerate() {
            if order.num_markets <= 1 {
                continue;
            }

            // Calculate the cost of buying each leg separately
            let mut total_leg_cost: u128 = 0;
            let mut can_price_all_legs = true;

            for market_idx in 0..order.num_markets as usize {
                let market = order.markets[market_idx];
                if market.is_none() {
                    continue;
                }

                let outcome = self.determine_bundle_outcome(order, market_idx);
                if let Some(price) = self.best_ask_price(&problem.liquidity, market, outcome) {
                    total_leg_cost += price as u128;
                } else {
                    can_price_all_legs = false;
                    break;
                }
            }

            if can_price_all_legs && order.num_markets > 1 {
                let avg_leg_cost = (total_leg_cost / order.num_markets as u128) as Nanos;
                let bundle_limit = order.limit_price;

                // If bundle limit is higher than actual leg cost, there might be value
                if bundle_limit > avg_leg_cost {
                    let profit_per_unit = bundle_limit - avg_leg_cost;
                    opportunities.push(ArbitrageOpportunity {
                        kind: ArbitrageKind::BundleUnderpricing,
                        order_indices: vec![order_idx],
                        profit_per_unit,
                        max_quantity: order.max_fill,
                        confidence: 0.7, // Medium confidence (depends on execution)
                    });
                }
            }
        }

        opportunities
    }

    /// Get the best ask price for a (market, outcome) pair.
    fn best_ask_price(&self, liquidity: &LiquidityPool, market: MarketId, outcome: u8) -> Option<Nanos> {
        liquidity.book(market, outcome).and_then(|book| book.best_ask())
    }

    /// Get available quantity at a (market, outcome) pair.
    fn available_quantity(&self, liquidity: &LiquidityPool, market: MarketId, outcome: u8) -> u64 {
        liquidity
            .book(market, outcome)
            .map(|book| book.total_ask_qty())
            .unwrap_or(0)
    }

    /// Determine which outcome is being bought for a market in a bundle order.
    fn determine_bundle_outcome(&self, order: &Order, market_idx: usize) -> u8 {
        let num_markets = order.num_markets as usize;
        if market_idx >= num_markets {
            return 0;
        }

        let market_sizes: Vec<u8> = vec![2; num_markets]; // Assume binary
        let mut outcome_votes: [i32; 4] = [0; 4];

        for state_idx in 0..order.num_states as usize {
            let payoff = order.payoffs[state_idx];
            if payoff > 0 {
                let outcome = self.extract_outcome(state_idx, market_idx, &market_sizes);
                if (outcome as usize) < outcome_votes.len() {
                    outcome_votes[outcome as usize] += payoff as i32;
                }
            }
        }

        outcome_votes
            .iter()
            .enumerate()
            .max_by_key(|(_, &v)| v)
            .map(|(idx, _)| idx as u8)
            .unwrap_or(0)
    }

    /// Extract outcome for a market from a state index.
    fn extract_outcome(&self, state_idx: usize, market_idx: usize, market_sizes: &[u8]) -> u8 {
        let mut remaining = state_idx;
        for (i, &size) in market_sizes.iter().enumerate() {
            let outcome = (remaining % size as usize) as u8;
            if i == market_idx {
                return outcome;
            }
            remaining /= size as usize;
        }
        0
    }

    /// Exploit detected arbitrage opportunities.
    fn exploit_opportunities(
        &self,
        opportunities: &[ArbitrageOpportunity],
        problem: &Problem,
        result: &mut MatchingResult,
    ) {
        for opp in opportunities {
            match opp.kind {
                ArbitrageKind::BundleUnderpricing => {
                    // Try to fill the bundle orders
                    for &order_idx in &opp.order_indices {
                        if let Some(order) = problem.orders.get(order_idx) {
                            // This would need actual liquidity consumption
                            // For now, just mark as potential welfare gain
                            // The actual filling happens in the main solver
                        }
                    }
                }
                ArbitrageKind::Constraint | ArbitrageKind::CrossMarket => {
                    // These require synthetic order creation
                    // which is beyond the current implementation scope
                }
            }
        }
    }
}

impl Default for ArbitrageDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for ArbitrageDetector {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        let mut result = MatchingResult::new(problem.liquidity.snapshot());

        // Detect opportunities
        let opportunities = self.detect_opportunities(problem);

        // Try to exploit them
        self.exploit_opportunities(&opportunities, problem, &mut result);

        result
    }

    fn name(&self) -> &str {
        "Arbitrage"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{BookLevel, LiquidityBook, Market, MarketConstraint, OrderBuilder, Side};

    #[test]
    fn test_arbitrage_detection() {
        let mut problem = Problem::new("test");

        let m1 = problem.markets.add_binary("A_wins");
        let m2 = problem.markets.add_binary("A_advances");

        // A wins → A advances (implication)
        problem.constraints.add(MarketConstraint::implies(m1, 0, m2, 0));

        // Set up liquidity where buying "A wins" is cheaper than "A advances"
        problem.liquidity.add_ask(m1, 0, 400_000_000, 1000); // A wins at 0.40
        problem.liquidity.add_ask(m2, 0, 600_000_000, 1000); // A advances at 0.60

        let detector = ArbitrageDetector::new();
        let opportunities = detector.detect_opportunities(&problem);

        // Should detect constraint arbitrage
        assert!(!opportunities.is_empty());
        assert_eq!(opportunities[0].kind, ArbitrageKind::Constraint);
    }

    #[test]
    fn test_no_arbitrage_when_prices_correct() {
        let mut problem = Problem::new("test");

        let m1 = problem.markets.add_binary("market_1");
        let m2 = problem.markets.add_binary("market_2");

        problem.constraints.add(MarketConstraint::implies(m1, 0, m2, 0));

        // Prices correctly ordered: A wins more expensive than A advances
        problem.liquidity.add_ask(m1, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(m2, 0, 400_000_000, 1000);

        let detector = ArbitrageDetector::new();
        let opportunities = detector.detect_opportunities(&problem);

        // Should not detect constraint arbitrage when prices are correctly ordered
        let constraint_arbs: Vec<_> = opportunities
            .iter()
            .filter(|o| o.kind == ArbitrageKind::Constraint)
            .collect();
        assert!(constraint_arbs.is_empty());
    }
}
