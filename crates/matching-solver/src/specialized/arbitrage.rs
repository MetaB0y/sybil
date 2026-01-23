//! Bundle order matching against joint liquidity.
//!
//! Matches bundle orders (multi-market orders) against joint liquidity books.
//! Bundle orders have payoffs that depend on multiple market outcomes simultaneously
//! (e.g., "A AND B" pays $1 only if both A and B happen). These cannot be replicated
//! by buying individual leg positions, so they require dedicated joint liquidity.

use std::collections::HashSet;

use matching_engine::{Fill, JointOutcome, LiquidityPool, Nanos, Order, Problem, Qty};

use crate::{MatchingResult, Solver};

/// Detected arbitrage opportunity.
#[derive(Clone, Debug)]
pub struct ArbitrageOpportunity {
    /// Orders involved in this arbitrage
    pub order_indices: Vec<usize>,
    /// Expected profit per unit
    pub profit_per_unit: Nanos,
}

/// Detects and exploits arbitrage opportunities.
pub struct ArbitrageDetector {
    /// Minimum profit threshold (in nanos) to consider an opportunity
    min_profit_threshold: Nanos,
}

impl ArbitrageDetector {
    /// Create a new arbitrage detector with default settings.
    pub fn new() -> Self {
        Self {
            min_profit_threshold: 1_000_000, // 0.001 dollars
        }
    }

    /// Detect all arbitrage opportunities in a problem.
    pub fn detect_opportunities(&self, problem: &Problem) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = Vec::new();

        // Bundle underpricing
        let bundle_arbs = self.detect_bundle_arbitrage(problem);
        opportunities.extend(bundle_arbs);

        // Filter by minimum profit
        opportunities.retain(|opp| opp.profit_per_unit >= self.min_profit_threshold);

        // Sort by profit (highest first)
        opportunities.sort_by(|a, b| b.profit_per_unit.cmp(&a.profit_per_unit));

        opportunities
    }

    /// Identify bundle orders that can potentially be filled against joint liquidity.
    ///
    /// We check if joint liquidity exists at a price the order is willing to pay.
    /// This is the only valid way to fill bundles - leg liquidity cannot replicate
    /// bundle payoffs (e.g., "A AND B" pays only if both happen, unlike buying
    /// A YES + B YES separately which pays if either happens).
    fn detect_bundle_arbitrage(&self, problem: &Problem) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = Vec::new();

        for (order_idx, order) in problem.orders.iter().enumerate() {
            if order.num_markets <= 1 {
                continue;
            }

            // Build the joint outcome for this bundle
            let joint_outcome = match self.build_joint_outcome(order) {
                Some(jo) => jo,
                None => continue,
            };

            // Check if there's joint liquidity available at a price the order accepts
            if let Some(joint_book) = problem.liquidity.joint_book(&joint_outcome) {
                let (avail, best_price) = joint_book.available_to_buy(order.limit_price);
                if avail >= order.min_fill && best_price <= order.limit_price {
                    // Profit = what order is willing to pay - what liquidity costs
                    let profit_per_unit = order.limit_price.saturating_sub(best_price);
                    opportunities.push(ArbitrageOpportunity {
                        order_indices: vec![order_idx],
                        profit_per_unit,
                    });
                }
            }
        }

        opportunities
    }

    /// Determine which outcome is being bought for a market in a bundle order.
    fn determine_bundle_outcome(&self, order: &Order, market_idx: usize) -> u8 {
        let num_markets = order.num_markets as usize;
        if market_idx >= num_markets {
            return 0;
        }

        let market_sizes: Vec<u8> = vec![2; num_markets]; // Binary markets
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
        let mut filled_orders: HashSet<u64> = HashSet::new();

        for opp in opportunities.iter() {
            // Currently only BundleUnderpricing arbitrage is supported
            self.exploit_bundle_underpricing(opp, problem, result, &mut filled_orders);
        }
    }

    /// Exploit bundle underpricing by filling bundle orders where sum(leg_prices) < limit.
    fn exploit_bundle_underpricing(
        &self,
        opp: &ArbitrageOpportunity,
        problem: &Problem,
        result: &mut MatchingResult,
        filled_orders: &mut HashSet<u64>,
    ) {
        for &order_idx in &opp.order_indices {
            if let Some(order) = problem.orders.get(order_idx) {
                if filled_orders.contains(&order.id) {
                    continue;
                }

                if let Some(fill) =
                    self.try_fill_bundle(order, &result.remaining_liquidity)
                {
                    let welfare = fill.welfare(order);
                    if welfare > 0 {
                        if self.consume_bundle_liquidity(
                            order,
                            fill.fill_qty,
                            &mut result.remaining_liquidity,
                        ) {
                            result.add_fill(fill, order);
                            filled_orders.insert(order.id);
                        }
                    }
                }
            }
        }
    }

    /// Try to fill a bundle order using joint liquidity.
    ///
    /// Bundle orders can ONLY be filled from joint liquidity books.
    /// Leg liquidity has different payoff structure and cannot replicate bundles.
    fn try_fill_bundle(
        &self,
        order: &Order,
        liquidity: &LiquidityPool,
    ) -> Option<Fill> {
        if order.num_markets <= 1 {
            return None;
        }

        // Build the joint outcome for this bundle
        let joint_outcome = self.build_joint_outcome(order)?;

        // Try joint liquidity - the ONLY valid way to fill bundles
        if let Some(joint_book) = liquidity.joint_book(&joint_outcome) {
            let (avail, avg_price) = joint_book.available_to_buy(order.limit_price);
            if avail >= order.min_fill {
                let fill_qty = avail.min(order.max_fill);
                return Some(Fill::new(order.id, fill_qty, avg_price));
            }
        }

        // No joint liquidity available - cannot fill this bundle
        None
    }

    /// Build a JointOutcome from a bundle order.
    fn build_joint_outcome(&self, order: &Order) -> Option<JointOutcome> {
        if order.num_markets <= 1 {
            return None;
        }

        let mut legs = Vec::new();
        for market_idx in 0..order.num_markets as usize {
            let market = order.markets[market_idx];
            if market.is_none() {
                continue;
            }

            let outcome = self.determine_bundle_outcome(order, market_idx);
            legs.push((market, outcome));
        }

        if legs.len() >= 2 {
            Some(JointOutcome::new(legs))
        } else {
            None
        }
    }

    /// Consume liquidity for a bundle order from joint liquidity only.
    fn consume_bundle_liquidity(
        &self,
        order: &Order,
        qty: Qty,
        liquidity: &mut LiquidityPool,
    ) -> bool {
        // Build joint outcome
        let joint_outcome = match self.build_joint_outcome(order) {
            Some(jo) => jo,
            None => return false,
        };

        // Consume from joint liquidity - the only valid source for bundles
        if let Some(joint_book) = liquidity.joint_book_get_mut(&joint_outcome) {
            let (avail, _) = joint_book.available_to_buy(order.limit_price);
            if avail >= qty {
                joint_book.consume_asks(qty, order.limit_price);
                return true;
            }
        }

        false
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

        let opportunities = self.detect_opportunities(problem);
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

    #[test]
    fn test_bundle_arbitrage_detection() {
        let mut problem = Problem::new("test");

        let m1 = problem.markets.add_binary("market_1");
        let m2 = problem.markets.add_binary("market_2");

        // Set up liquidity
        problem.liquidity.add_ask(m1, 0, 400_000_000, 1000);
        problem.liquidity.add_ask(m2, 0, 300_000_000, 1000);

        let detector = ArbitrageDetector::new();
        // With no bundle orders, should find no arbitrage
        let opportunities = detector.detect_opportunities(&problem);
        assert!(opportunities.is_empty());
    }
}
