//! Arbitrage detection and exploitation.
//!
//! Finds riskless profit opportunities from:
//! 1. Constraint arbitrage: If A→B and price(A) > price(B), profit is possible
//! 2. Bundle underpricing: Sum of legs < bundle price
//! 3. Cross-market mispricing: Same effective exposure at different prices

use matching_engine::{
    ConstraintSet, Fill, LiquidityPool, MarketConstraint, MarketId, Nanos, Order, Problem, Qty,
};

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
    /// Constraint-based: A→B with price(A) > price(B)
    Constraint,
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

                        opportunities.push(ArbitrageOpportunity {
                            kind: ArbitrageKind::Constraint,
                            order_indices: Vec::new(), // No specific orders yet
                            profit_per_unit,
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
    ///
    /// Fills orders that present arbitrage opportunities, prioritizing by profit.
    fn exploit_opportunities(
        &self,
        opportunities: &[ArbitrageOpportunity],
        problem: &Problem,
        result: &mut MatchingResult,
    ) {
        // Track which orders we've already filled
        let mut filled_orders: std::collections::HashSet<u64> = std::collections::HashSet::new();

        // Process opportunities by profit (already sorted)
        for opp in opportunities.iter().take(20) {
            match opp.kind {
                ArbitrageKind::BundleUnderpricing => {
                    self.exploit_bundle_underpricing(opp, problem, result, &mut filled_orders);
                }
                ArbitrageKind::Constraint => {
                    self.exploit_constraint_arbitrage(opp, problem, result, &mut filled_orders);
                }
            }
        }
    }

    /// Exploit bundle underpricing by filling bundle orders where sum(leg_prices) < limit.
    fn exploit_bundle_underpricing(
        &self,
        opp: &ArbitrageOpportunity,
        problem: &Problem,
        result: &mut MatchingResult,
        filled_orders: &mut std::collections::HashSet<u64>,
    ) {
        for &order_idx in &opp.order_indices {
            if let Some(order) = problem.orders.get(order_idx) {
                if filled_orders.contains(&order.id) {
                    continue;
                }

                // Try to fill the bundle order
                if let Some(fill) = self.try_fill_bundle(order, &result.remaining_liquidity, problem) {
                    // Verify the fill is profitable
                    let welfare = fill.welfare(order);
                    if welfare > 0 {
                        // Consume liquidity for each leg
                        if self.consume_bundle_liquidity(order, fill.fill_qty, &mut result.remaining_liquidity) {
                            result.add_fill(fill, order);
                            filled_orders.insert(order.id);
                        }
                    }
                }
            }
        }
    }

    /// Exploit constraint arbitrage by prioritizing orders early in implication chains.
    fn exploit_constraint_arbitrage(
        &self,
        _opp: &ArbitrageOpportunity,
        problem: &Problem,
        result: &mut MatchingResult,
        filled_orders: &mut std::collections::HashSet<u64>,
    ) {
        // For constraint arbitrage, we want to find orders buying the "if" side of implications
        // where price(if) < price(then)

        // Find all orders buying outcomes that are at the start of implication chains
        for order in &problem.orders {
            if filled_orders.contains(&order.id) {
                continue;
            }

            if order.num_markets != 1 {
                continue;
            }

            let market = order.markets[0];
            let outcome = self.determine_buying_outcome(order);

            // Check if this (market, outcome) is the "if" side of any implication
            // with a price advantage
            if self.is_cheap_implicant(&problem.constraints, &problem.liquidity, market, outcome) {
                if let Some(fill) = self.try_fill_simple(order, &result.remaining_liquidity) {
                    if fill.welfare(order) > 0 {
                        if let Some(book) = result.remaining_liquidity.books.get_mut(&(market, outcome)) {
                            let (filled, price) = book.consume_asks(fill.fill_qty, order.limit_price);
                            if filled >= order.min_fill && filled > 0 {
                                let actual_fill = Fill::new(order.id, filled, price);
                                result.add_fill(actual_fill, order);
                                filled_orders.insert(order.id);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Check if (market, outcome) is the antecedent of an implication and is priced cheaper.
    fn is_cheap_implicant(
        &self,
        constraints: &ConstraintSet,
        liquidity: &LiquidityPool,
        market: MarketId,
        outcome: u8,
    ) -> bool {
        for constraint in constraints.iter() {
            if let MarketConstraint::Implication { if_true, then_true } = constraint {
                if if_true.0 == market && if_true.1 == outcome {
                    // Check prices
                    let price_if = self.best_ask_price(liquidity, if_true.0, if_true.1);
                    let price_then = self.best_ask_price(liquidity, then_true.0, then_true.1);

                    if let (Some(p_if), Some(p_then)) = (price_if, price_then) {
                        if p_if < p_then {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Try to fill a simple (single-market) order.
    fn try_fill_simple(&self, order: &Order, liquidity: &LiquidityPool) -> Option<Fill> {
        if order.num_markets != 1 {
            return None;
        }

        let market = order.markets[0];
        let outcome = self.determine_buying_outcome(order);

        if let Some(book) = liquidity.book(market, outcome) {
            let (avail, avg_price) = book.available_to_buy(order.limit_price);
            if avail >= order.min_fill && avail > 0 {
                let fill_qty = avail.min(order.max_fill);
                return Some(Fill::new(order.id, fill_qty, avg_price));
            }
        }

        None
    }

    /// Try to fill a bundle order.
    fn try_fill_bundle(&self, order: &Order, liquidity: &LiquidityPool, _problem: &Problem) -> Option<Fill> {
        if order.num_markets <= 1 {
            return None;
        }

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
                // Check how much we can buy at prices that fit the limit
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
    fn consume_bundle_liquidity(&self, order: &Order, qty: Qty, liquidity: &mut LiquidityPool) -> bool {
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

    /// Determine which outcome is being bought for a simple order.
    fn determine_buying_outcome(&self, order: &Order) -> u8 {
        let mut best_outcome = 0u8;
        let mut best_payoff = i8::MIN;

        for (i, &payoff) in order.payoffs.iter().take(order.num_states as usize).enumerate() {
            if payoff > best_payoff {
                best_payoff = payoff;
                best_outcome = i as u8;
            }
        }

        best_outcome
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
