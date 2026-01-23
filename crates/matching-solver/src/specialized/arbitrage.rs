//! Cross-market order matching.
//!
//! Handles multi-market orders:
//! 1. **Bundles** (e.g., "A AND B"): Require joint liquidity - leg liquidity cannot replicate
//! 2. **Spreads** (e.g., "long A, short B"): Can use leg liquidity - buy one side, sell the other

use std::collections::HashSet;

use matching_engine::{Fill, JointOutcome, LiquidityPool, MarketId, Nanos, Order, Problem, Qty};

use crate::{MatchingResult, Solver};

/// Type of cross-market order.
#[derive(Clone, Copy, Debug, PartialEq)]
enum CrossMarketOrderType {
    /// Bundle: positive payoff only when all markets have specific outcomes
    /// e.g., "A AND B" pays $1 only if both A and B happen
    Bundle,
    /// Spread: positive and negative payoffs based on relative outcomes
    /// e.g., "long A, short B" pays +$1 if A wins, -$1 if B wins
    Spread,
    /// Unknown or unsupported structure
    Unknown,
}

/// Detected opportunity.
#[derive(Clone, Debug)]
pub struct ArbitrageOpportunity {
    /// Order index in the problem
    pub order_idx: usize,
    /// Expected profit per unit
    pub profit_per_unit: Nanos,
    /// Type of order
    order_type: CrossMarketOrderType,
}

/// Handles cross-market order matching (bundles and spreads).
pub struct ArbitrageDetector {
    /// Minimum profit threshold (in nanos) to consider an opportunity
    min_profit_threshold: Nanos,
}

impl ArbitrageDetector {
    /// Create a new cross-market matcher with default settings.
    pub fn new() -> Self {
        Self {
            min_profit_threshold: 1_000_000, // 0.001 dollars
        }
    }

    /// Detect all cross-market opportunities in a problem.
    pub fn detect_opportunities(&self, problem: &Problem) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = Vec::new();

        for (order_idx, order) in problem.orders.iter().enumerate() {
            if order.num_markets <= 1 {
                continue;
            }

            let order_type = self.classify_order(order);

            let opportunity = match order_type {
                CrossMarketOrderType::Bundle => {
                    self.detect_bundle_opportunity(order_idx, order, problem)
                }
                CrossMarketOrderType::Spread => {
                    self.detect_spread_opportunity(order_idx, order, problem)
                }
                CrossMarketOrderType::Unknown => None,
            };

            if let Some(opp) = opportunity {
                if opp.profit_per_unit >= self.min_profit_threshold {
                    opportunities.push(opp);
                }
            }
        }

        // Sort by profit (highest first)
        opportunities.sort_by(|a, b| b.profit_per_unit.cmp(&a.profit_per_unit));

        opportunities
    }

    /// Classify an order as bundle, spread, or unknown.
    fn classify_order(&self, order: &Order) -> CrossMarketOrderType {
        if order.num_markets <= 1 {
            return CrossMarketOrderType::Unknown;
        }

        let mut has_positive = false;
        let mut has_negative = false;
        let mut positive_states = 0;

        for i in 0..order.num_states as usize {
            let payoff = order.payoffs[i];
            if payoff > 0 {
                has_positive = true;
                positive_states += 1;
            }
            if payoff < 0 {
                has_negative = true;
            }
        }

        if has_positive && has_negative {
            // Spread: has both positive and negative payoffs
            CrossMarketOrderType::Spread
        } else if has_positive && positive_states == 1 {
            // Bundle: positive payoff in exactly one state (all conditions met)
            CrossMarketOrderType::Bundle
        } else if has_positive {
            // Could be a more complex bundle, try as bundle
            CrossMarketOrderType::Bundle
        } else {
            CrossMarketOrderType::Unknown
        }
    }

    /// Detect opportunity for a bundle order.
    fn detect_bundle_opportunity(
        &self,
        order_idx: usize,
        order: &Order,
        problem: &Problem,
    ) -> Option<ArbitrageOpportunity> {
        let joint_outcome = self.build_joint_outcome(order)?;

        if let Some(joint_book) = problem.liquidity.joint_book(&joint_outcome) {
            let (avail, best_price) = joint_book.available_to_buy(order.limit_price);
            if avail >= order.min_fill && best_price <= order.limit_price {
                let profit_per_unit = order.limit_price.saturating_sub(best_price);
                return Some(ArbitrageOpportunity {
                    order_idx,
                    profit_per_unit,
                    order_type: CrossMarketOrderType::Bundle,
                });
            }
        }

        None
    }

    /// Detect opportunity for a spread order.
    fn detect_spread_opportunity(
        &self,
        order_idx: usize,
        order: &Order,
        problem: &Problem,
    ) -> Option<ArbitrageOpportunity> {
        // For spreads, we need to check leg liquidity
        let legs = self.analyze_spread_legs(order)?;

        // Calculate total cost to construct the spread from legs
        let mut total_cost: u64 = 0;
        let mut min_available = order.max_fill;

        for (market, outcome, is_buy) in &legs {
            if let Some(book) = problem.liquidity.book(*market, *outcome) {
                if *is_buy {
                    // Buying this outcome
                    let (avail, price) = book.available_to_buy(order.limit_price);
                    if avail < order.min_fill {
                        return None;
                    }
                    min_available = min_available.min(avail);
                    total_cost = total_cost.saturating_add(price);
                } else {
                    // Selling this outcome = buying the opposite
                    // For binary markets, selling YES = buying NO
                    let opposite_outcome = 1 - *outcome;
                    if let Some(opposite_book) = problem.liquidity.book(*market, opposite_outcome) {
                        let (avail, price) = opposite_book.available_to_buy(order.limit_price);
                        if avail < order.min_fill {
                            return None;
                        }
                        min_available = min_available.min(avail);
                        total_cost = total_cost.saturating_add(price);
                    } else {
                        return None;
                    }
                }
            } else {
                return None;
            }
        }

        // Average cost per leg
        let avg_cost = total_cost / legs.len() as u64;

        // Check if spread can be filled profitably
        if min_available >= order.min_fill && avg_cost <= order.limit_price {
            let profit_per_unit = order.limit_price.saturating_sub(avg_cost);
            return Some(ArbitrageOpportunity {
                order_idx,
                profit_per_unit,
                order_type: CrossMarketOrderType::Spread,
            });
        }

        None
    }

    /// Analyze spread legs to determine what to buy/sell in each market.
    /// Returns: Vec<(market_id, outcome, is_buy)>
    fn analyze_spread_legs(&self, order: &Order) -> Option<Vec<(MarketId, u8, bool)>> {
        if order.num_markets < 2 {
            return None;
        }

        let num_markets = order.num_markets as usize;
        let market_sizes: Vec<u8> = vec![2; num_markets]; // Binary markets

        // For each market, determine net exposure
        let mut market_exposures: Vec<(MarketId, i32, i32)> = Vec::new(); // (market, yes_exposure, no_exposure)

        for market_idx in 0..num_markets {
            let market = order.markets[market_idx];
            if market.is_none() {
                continue;
            }

            let mut yes_exposure: i32 = 0;
            let mut no_exposure: i32 = 0;

            for state_idx in 0..order.num_states as usize {
                let payoff = order.payoffs[state_idx] as i32;
                let outcome = self.extract_outcome(state_idx, market_idx, &market_sizes);

                if outcome == 0 {
                    // YES outcome
                    yes_exposure += payoff;
                } else {
                    // NO outcome
                    no_exposure += payoff;
                }
            }

            market_exposures.push((market, yes_exposure, no_exposure));
        }

        // Convert exposures to buy/sell decisions
        let mut legs = Vec::new();

        for (market, yes_exp, no_exp) in market_exposures {
            let net = yes_exp - no_exp;
            if net > 0 {
                // Net long YES - we want to buy YES
                legs.push((market, 0u8, true)); // Buy YES
            } else if net < 0 {
                // Net short YES (long NO) - we want to buy NO
                legs.push((market, 1u8, true)); // Buy NO
            }
            // If net == 0, no position needed in this market
        }

        if legs.len() >= 2 {
            Some(legs)
        } else {
            None
        }
    }

    /// Exploit detected opportunities.
    fn exploit_opportunities(
        &self,
        opportunities: &[ArbitrageOpportunity],
        problem: &Problem,
        result: &mut MatchingResult,
    ) {
        let mut filled_orders: HashSet<u64> = HashSet::new();

        for opp in opportunities.iter() {
            if let Some(order) = problem.orders.get(opp.order_idx) {
                if filled_orders.contains(&order.id) {
                    continue;
                }

                let fill_result = match opp.order_type {
                    CrossMarketOrderType::Bundle => {
                        self.try_fill_bundle(order, &mut result.remaining_liquidity)
                    }
                    CrossMarketOrderType::Spread => {
                        self.try_fill_spread(order, &mut result.remaining_liquidity)
                    }
                    CrossMarketOrderType::Unknown => None,
                };

                if let Some(fill) = fill_result {
                    let welfare = fill.welfare(order);
                    if welfare > 0 {
                        result.add_fill(fill, order);
                        filled_orders.insert(order.id);
                    }
                }
            }
        }
    }

    /// Try to fill a bundle order using joint liquidity.
    fn try_fill_bundle(&self, order: &Order, liquidity: &mut LiquidityPool) -> Option<Fill> {
        if order.num_markets <= 1 {
            return None;
        }

        let joint_outcome = self.build_joint_outcome(order)?;

        if let Some(joint_book) = liquidity.joint_book_get_mut(&joint_outcome) {
            let (avail, _) = joint_book.available_to_buy(order.limit_price);
            if avail >= order.min_fill {
                let fill_qty = avail.min(order.max_fill);
                let (consumed, avg_price) = joint_book.consume_asks(fill_qty, order.limit_price);
                if consumed >= order.min_fill {
                    return Some(Fill::new(order.id, consumed, avg_price));
                }
            }
        }

        None
    }

    /// Try to fill a spread order using leg liquidity.
    fn try_fill_spread(&self, order: &Order, liquidity: &mut LiquidityPool) -> Option<Fill> {
        let legs = self.analyze_spread_legs(order)?;

        // First pass: check availability across all legs
        let mut min_available = order.max_fill;
        let mut leg_info: Vec<(MarketId, u8, Qty, Nanos)> = Vec::new(); // (market, outcome, avail, price)

        for (market, outcome, _is_buy) in &legs {
            // For spreads, we always buy the outcome we want exposure to
            if let Some(book) = liquidity.book(*market, *outcome) {
                let (avail, price) = book.available_to_buy(order.limit_price);
                if avail < order.min_fill {
                    return None;
                }
                min_available = min_available.min(avail);
                leg_info.push((*market, *outcome, avail, price));
            } else {
                return None;
            }
        }

        if min_available < order.min_fill {
            return None;
        }

        let fill_qty = min_available.min(order.max_fill);

        // Calculate average fill price
        let total_cost: u128 = leg_info.iter().map(|(_, _, _, p)| *p as u128).sum();
        let avg_price = (total_cost / leg_info.len() as u128) as Nanos;

        // Check if within limit
        if avg_price > order.limit_price {
            return None;
        }

        // Second pass: consume liquidity from all legs
        for (market, outcome, _, _) in &leg_info {
            if let Some(book) = liquidity.books.get_mut(&(*market, *outcome)) {
                book.consume_asks(fill_qty, order.limit_price);
            }
        }

        Some(Fill::new(order.id, fill_qty, avg_price))
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

    /// Determine which outcome is being bought for a market in a bundle order.
    fn determine_bundle_outcome(&self, order: &Order, market_idx: usize) -> u8 {
        let num_markets = order.num_markets as usize;
        if market_idx >= num_markets {
            return 0;
        }

        let market_sizes: Vec<u8> = vec![2; num_markets];
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
        "CrossMarket"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{bundle_yes, spread};

    #[test]
    fn test_classify_bundle() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("m1");
        let m2 = problem.markets.add_binary("m2");

        // Bundle order: A AND B
        let bundle = bundle_yes(&problem.markets, 1, &[m1, m2], 500_000_000, 100);

        let detector = ArbitrageDetector::new();
        let order_type = detector.classify_order(&bundle);
        assert_eq!(order_type, CrossMarketOrderType::Bundle);
    }

    #[test]
    fn test_classify_spread() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("m1");
        let m2 = problem.markets.add_binary("m2");

        // Spread order: long A, short B
        let spread_order = spread(&problem.markets, 1, m1, m2, 500_000_000, 100);

        let detector = ArbitrageDetector::new();
        let order_type = detector.classify_order(&spread_order);
        assert_eq!(order_type, CrossMarketOrderType::Spread);
    }

    #[test]
    fn test_spread_legs_analysis() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("m1");
        let m2 = problem.markets.add_binary("m2");

        // Spread: long m1 YES, short m2 YES (= long m2 NO)
        let spread_order = spread(&problem.markets, 1, m1, m2, 500_000_000, 100);

        let detector = ArbitrageDetector::new();
        let legs = detector.analyze_spread_legs(&spread_order);

        assert!(legs.is_some());
        let legs = legs.unwrap();
        assert_eq!(legs.len(), 2);
    }
}
