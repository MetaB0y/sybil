//! Cross-market order matching.
//!
//! Handles multi-market orders:
//! 1. **Bundles** (e.g., "A AND B"): Require joint liquidity - leg liquidity cannot replicate
//! 2. **Spreads** (e.g., "long A, short B"): Can use leg liquidity - buy one side, sell the other

use matching_engine::{Nanos, Order, Problem};
#[cfg(test)]
use matching_engine::MarketId;

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
    #[allow(dead_code)]
    pub order_idx: usize,
    /// Expected profit per unit
    pub profit_per_unit: Nanos,
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
    ///
    /// Without platform liquidity, bundle opportunities are detected based on
    /// whether matching sell orders exist. For now, returns None since bundle
    /// matching is order-vs-order only.
    fn detect_bundle_opportunity(
        &self,
        _order_idx: usize,
        _order: &Order,
        _problem: &Problem,
    ) -> Option<ArbitrageOpportunity> {
        // Without platform liquidity, bundle matching relies on order-vs-order clearing
        // which is handled by the LocalSolver. No separate detection needed.
        None
    }

    /// Detect opportunity for a spread order.
    ///
    /// Without platform liquidity, spread opportunities are detected based on
    /// whether matching sell orders exist. For now, returns None since spread
    /// matching is order-vs-order only.
    fn detect_spread_opportunity(
        &self,
        _order_idx: usize,
        _order: &Order,
        _problem: &Problem,
    ) -> Option<ArbitrageOpportunity> {
        // Without platform liquidity, spread matching relies on order-vs-order clearing
        // which is handled by the LocalSolver. No separate detection needed.
        None
    }

    /// Analyze spread legs to determine what to buy/sell in each market.
    /// Returns: Vec<(market_id, outcome, is_buy)>
    #[cfg(test)]
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
    ///
    /// Without platform liquidity, this is a no-op since detect methods return None.
    fn exploit_opportunities(
        &self,
        opportunities: &[ArbitrageOpportunity],
        problem: &Problem,
        result: &mut MatchingResult,
    ) {
        let _ = (opportunities, problem, result);
    }

    /// Extract outcome for a market from a state index.
    #[cfg(test)]
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
        let mut result = MatchingResult::new();

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
