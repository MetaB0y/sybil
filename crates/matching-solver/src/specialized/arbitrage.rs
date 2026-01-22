//! Arbitrage detection and exploitation.
//!
//! Finds riskless profit opportunities from:
//! 1. Bundle underpricing: Sum of legs < bundle price
//! 2. Cross-market mispricing: Same effective exposure at different prices

use std::collections::HashSet;

use matching_engine::{Fill, JointOutcome, LiquidityPool, MarketId, Nanos, Order, Problem, Qty};

use crate::{MatchingResult, Solver};

/// Detected arbitrage opportunity.
#[derive(Clone, Debug)]
pub struct ArbitrageOpportunity {
    /// Type of arbitrage
    pub kind: ArbitrageKind,
    /// Orders involved in this arbitrage
    pub order_indices: Vec<usize>,
    /// Expected profit per unit
    pub profit_per_unit: Nanos,
}

/// Types of arbitrage opportunities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArbitrageKind {
    /// Bundle underpricing: bundle cheaper than sum of legs
    BundleUnderpricing,
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
                    });
                }
            }
        }

        opportunities
    }

    /// Get the best ask price for a (market, outcome) pair.
    fn best_ask_price(
        &self,
        liquidity: &LiquidityPool,
        market: MarketId,
        outcome: u8,
    ) -> Option<Nanos> {
        liquidity
            .book(market, outcome)
            .and_then(|book| book.best_ask())
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

        for opp in opportunities.iter().take(20) {
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

        // First try joint liquidity (the correct approach for bundles)
        if let Some(joint_book) = liquidity.joint_book(&joint_outcome) {
            let (avail, avg_price) = joint_book.available_to_buy(order.limit_price);
            if avail >= order.min_fill {
                let fill_qty = avail.min(order.max_fill);
                return Some(Fill::new(order.id, fill_qty, avg_price));
            }
        }

        // Fallback: try to match using individual leg liquidity
        // This is less accurate but allows matching when joint liquidity isn't available
        self.try_fill_bundle_via_legs(order, liquidity)
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

    /// Fallback: try to fill bundle by matching individual legs.
    fn try_fill_bundle_via_legs(
        &self,
        order: &Order,
        liquidity: &LiquidityPool,
    ) -> Option<Fill> {
        let mut min_available = order.max_fill;
        let mut total_cost: u128 = 0;
        let mut legs = 0;

        for market_idx in 0..order.num_markets as usize {
            let market = order.markets[market_idx];
            if market.is_none() {
                continue;
            }

            let outcome = self.determine_bundle_outcome(order, market_idx);

            if let Some(book) = liquidity.book(market, outcome) {
                let (avail, avg_price) = book.available_to_buy(order.limit_price);
                if avail < order.min_fill {
                    return None;
                }
                min_available = min_available.min(avail);
                total_cost += avg_price as u128;
                legs += 1;
            } else {
                return None;
            }
        }

        if min_available >= order.min_fill && legs > 0 {
            let avg_price = (total_cost / legs as u128) as Nanos;
            Some(Fill::new(order.id, min_available, avg_price))
        } else {
            None
        }
    }

    /// Consume liquidity for a bundle order.
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

        // First try to consume from joint liquidity
        if let Some(joint_book) = liquidity.joint_book_get_mut(&joint_outcome) {
            let (avail, _) = joint_book.available_to_buy(order.limit_price);
            if avail >= qty {
                joint_book.consume_asks(qty, order.limit_price);
                return true;
            }
        }

        // Fallback: consume from individual legs
        // First verify all legs have sufficient liquidity
        for market_idx in 0..order.num_markets as usize {
            let market = order.markets[market_idx];
            if market.is_none() {
                continue;
            }

            let outcome = self.determine_bundle_outcome(order, market_idx);

            if let Some(book) = liquidity.book(market, outcome) {
                let (avail, _) = book.available_to_buy(order.limit_price);
                if avail < qty {
                    return false;
                }
            } else {
                return false;
            }
        }

        // Now consume from all legs
        for market_idx in 0..order.num_markets as usize {
            let market = order.markets[market_idx];
            if market.is_none() {
                continue;
            }

            let outcome = self.determine_bundle_outcome(order, market_idx);

            if let Some(book) = liquidity.books.get_mut(&(market, outcome)) {
                book.consume_asks(qty, order.limit_price);
            }
        }

        true
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
