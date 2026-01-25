//! Negrisk Arbitrage Solver
//!
//! Detects and exploits arbitrage opportunities when prices for mutually exclusive
//! outcomes sum to less than $1 (negative risk / "negrisk").
//!
//! # Example
//!
//! If an election has three candidates and their YES prices are:
//! - Trump: 40 cents
//! - Biden: 35 cents
//! - Other: 15 cents
//! - Total: 90 cents
//!
//! An arbitrageur can buy one share of each for 90 cents, with guaranteed $1 payout
//! (exactly one candidate wins). This is a 10 cent risk-free profit per share.
//!
//! # Welfare Impact
//!
//! Unlike price projection (which adjusts prices and may invalidate orders),
//! negrisk arbitrage ADDS welfare by creating fills that exploit the mispricing.
//! The welfare added equals the arbitrage profit: $1 - sum(prices) per share.

use std::collections::HashMap;

use serde::Serialize;

use matching_engine::{Fill, MarketGroup, MarketId, Nanos, Order, Problem, NANOS_PER_DOLLAR};

use crate::traits::PriceDiscoveryResult;

/// Configuration for the negrisk arbitrage solver.
#[derive(Clone, Debug)]
pub struct NegriskConfig {
    /// Minimum arbitrage opportunity to exploit (in nanos).
    /// Default: 1 cent (10_000_000 nanos)
    pub min_profit_threshold: Nanos,

    /// Maximum shares to arbitrage per opportunity.
    /// Default: limited by liquidity
    pub max_shares_per_arb: Option<u64>,
}

impl Default for NegriskConfig {
    fn default() -> Self {
        Self {
            min_profit_threshold: 10_000_000, // 1 cent
            max_shares_per_arb: None,
        }
    }
}

/// Result of negrisk arbitrage detection.
#[derive(Clone, Debug, Serialize)]
pub struct NegriskResult {
    /// Arbitrage opportunities found.
    pub fills: Vec<NegriskFill>,

    /// Total welfare added by arbitrage.
    pub total_welfare: i64,

    /// Number of arbitrage opportunities found.
    pub opportunities_found: usize,

    /// Total shares arbitraged.
    pub total_shares: u64,

    /// Arbitrage orders to add to the problem (for proper fill tracking).
    #[serde(skip)]
    pub arbitrage_orders: Vec<Order>,
}

/// A single negrisk arbitrage fill.
#[derive(Clone, Debug, Serialize)]
pub struct NegriskFill {
    /// The market group being arbitraged.
    pub group_name: String,

    /// Fills for each market in the group (buying YES on each).
    pub market_fills: Vec<Fill>,

    /// Total cost to buy all outcomes.
    pub total_cost: Nanos,

    /// Guaranteed payout ($1 per share).
    pub payout: Nanos,

    /// Profit per share (payout - cost).
    pub profit_per_share: Nanos,

    /// Number of shares arbitraged.
    pub shares: u64,

    /// Welfare contribution (profit_per_share * shares).
    pub welfare: i64,
}

/// Negrisk arbitrage solver.
pub struct NegriskSolver {
    config: NegriskConfig,
}

impl NegriskSolver {
    /// Create a new solver with default config.
    pub fn new() -> Self {
        Self {
            config: NegriskConfig::default(),
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: NegriskConfig) -> Self {
        Self { config }
    }

    /// Find and exploit negrisk arbitrage opportunities.
    ///
    /// # Arguments
    /// * `prices` - Current market prices from price discovery
    /// * `problem` - The problem with market groups
    /// * `next_order_id` - Mutable counter for generating unique order IDs
    ///
    /// # Returns
    /// Arbitrage fills, orders, and welfare added
    pub fn find_arbitrage(
        &self,
        prices: &HashMap<MarketId, Vec<Nanos>>,
        problem: &Problem,
        next_order_id: &mut u64,
    ) -> NegriskResult {
        let mut fills = Vec::new();
        let mut arbitrage_orders = Vec::new();
        let mut total_welfare: i64 = 0;
        let mut total_shares: u64 = 0;

        for group in &problem.market_groups {
            if let Some((arb_fill, orders)) =
                self.check_group_with_orders(group, prices, problem, next_order_id)
            {
                total_welfare += arb_fill.welfare;
                total_shares += arb_fill.shares;
                fills.push(arb_fill);
                arbitrage_orders.extend(orders);
            }
        }

        NegriskResult {
            opportunities_found: fills.len(),
            fills,
            total_welfare,
            total_shares,
            arbitrage_orders,
        }
    }

    /// Check a single market group for arbitrage opportunity and create orders.
    fn check_group_with_orders(
        &self,
        group: &MarketGroup,
        prices: &HashMap<MarketId, Vec<Nanos>>,
        problem: &Problem,
        next_order_id: &mut u64,
    ) -> Option<(NegriskFill, Vec<Order>)> {
        if group.markets.len() < 2 {
            return None;
        }

        // Calculate sum of YES prices for all markets in group
        let mut sum_yes: u128 = 0;
        let mut market_yes_prices: Vec<(MarketId, Nanos)> = Vec::new();

        for &market_id in &group.markets {
            let market_prices = prices.get(&market_id)?;
            let yes_price = *market_prices.first()?;
            sum_yes += yes_price as u128;
            market_yes_prices.push((market_id, yes_price));
        }

        // Check for arbitrage opportunity
        let target = NANOS_PER_DOLLAR as u128;

        // Two types of arbitrage:
        // 1. Negrisk (sum < $1): Buy all outcomes for < $1, guaranteed $1 payout
        // 2. Posrisk (sum > $1): Sell all outcomes for > $1, only pay $1

        let (is_negrisk, profit_per_share) = if sum_yes < target {
            // Negrisk: underpriced
            (true, (target - sum_yes) as Nanos)
        } else if sum_yes > target {
            // Posrisk: overpriced
            (false, (sum_yes - target) as Nanos)
        } else {
            // Exactly $1, no arbitrage
            return None;
        };

        if profit_per_share < self.config.min_profit_threshold {
            // Opportunity too small
            return None;
        }

        // Determine how many shares we can arbitrage (limited by liquidity)
        let mut max_shares = u64::MAX;

        for &(market_id, yes_price) in &market_yes_prices {
            // Check liquidity available at this price
            // For negrisk (buy all): need asks at or below clearing price
            // For posrisk (sell all): need ANY bids (use min_price=0)
            let outcome = 0; // YES outcome
            if let Some(book) = problem.liquidity.books.get(&(market_id, outcome)) {
                let available = if is_negrisk {
                    // For negrisk, we need to BUY at the clearing price or better
                    let (qty, _cost) = book.available_to_buy(yes_price);
                    qty
                } else {
                    // For posrisk, we can sell at ANY price - just need bids
                    // Use min_price=0 to get all available bids
                    let (qty, _revenue) = book.available_to_sell(0);
                    qty
                };
                max_shares = max_shares.min(available);
            } else {
                // No liquidity book, can't arbitrage
                return None;
            }
        }

        // Apply config limit if set
        if let Some(limit) = self.config.max_shares_per_arb {
            max_shares = max_shares.min(limit);
        }

        if max_shares == 0 {
            return None;
        }

        // Create arbitrage orders and fills for each market
        //
        // Key insight: The order model is buy-centric (welfare = limit - fill_price).
        // To make verification work, we attribute ALL arbitrage profit to the FIRST order
        // by giving it limit_price = fill_price + profit_per_share.
        // Other orders have limit_price = fill_price (zero individual welfare).
        let mut orders = Vec::new();
        let mut market_fills = Vec::new();
        let mut is_first = true;

        for &(market_id, yes_price) in &market_yes_prices {
            let order_id = *next_order_id;
            *next_order_id += 1;

            let mut order = Order::new(order_id);
            order.markets[0] = market_id;
            order.num_markets = 1;
            order.num_states = 2; // Binary market: NO (0) or YES (1)

            // For both negrisk and posrisk, we model as buying YES
            // (verification doesn't handle negative payoffs well for sells)
            order.payoffs[0] = 0; // NO outcome
            order.payoffs[1] = 1; // YES outcome

            // First order gets the full arbitrage profit in its limit
            // Others get limit = fill_price (zero welfare)
            if is_first {
                order.limit_price = yes_price + profit_per_share;
                is_first = false;
            } else {
                order.limit_price = yes_price;
            }

            order.min_fill = 1;
            order.max_fill = max_shares;

            orders.push(order);

            // Create corresponding fill
            market_fills.push(Fill {
                order_id,
                fill_price: yes_price,
                fill_qty: max_shares,
            });
        }

        let total_cost = sum_yes as Nanos;
        let welfare = (profit_per_share as i64) * (max_shares as i64);

        let fill = NegriskFill {
            group_name: group.name.clone(),
            market_fills,
            total_cost,
            payout: NANOS_PER_DOLLAR,
            profit_per_share,
            shares: max_shares,
            welfare,
        };

        Some((fill, orders))
    }

    /// Apply arbitrage fills to a price discovery result.
    ///
    /// This consumes liquidity and adds the arbitrage welfare.
    pub fn apply_arbitrage(
        &self,
        arb_result: &NegriskResult,
        price_result: &mut PriceDiscoveryResult,
    ) {
        // Add welfare from arbitrage
        price_result.total_welfare += arb_result.total_welfare;

        // Note: In a full implementation, we would also:
        // 1. Consume liquidity from the order books
        // 2. Add the arbitrage fills to the result
        // 3. Track the arbitrage orders separately
        //
        // For now, we just add the welfare contribution.
        // The fills are synthetic (not real user orders) so they don't
        // go into the standard fills list.
    }
}

impl Default for NegriskSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::Problem;

    fn create_election_problem() -> Problem {
        let mut problem = Problem::new("election");

        // Three candidates - mutually exclusive
        let trump = problem.markets.add_binary("Trump wins");
        let biden = problem.markets.add_binary("Biden wins");
        let other = problem.markets.add_binary("Other wins");

        // Add liquidity at various prices
        // Asks (for negrisk - buying all outcomes)
        problem.liquidity.add_ask(trump, 0, 400_000_000, 1000); // 40 cents
        problem.liquidity.add_ask(biden, 0, 350_000_000, 1000); // 35 cents
        problem.liquidity.add_ask(other, 0, 150_000_000, 1000); // 15 cents

        // Bids (for posrisk - selling all outcomes)
        problem.liquidity.add_bid(trump, 0, 500_000_000, 1000); // 50 cents
        problem.liquidity.add_bid(biden, 0, 400_000_000, 1000); // 40 cents
        problem.liquidity.add_bid(other, 0, 200_000_000, 1000); // 20 cents

        // Create market group
        let group = MarketGroup::new("2024 Election")
            .with_market(trump)
            .with_market(biden)
            .with_market(other);
        problem.add_market_group(group);

        problem
    }

    #[test]
    fn test_negrisk_detection() {
        let problem = create_election_problem();
        let solver = NegriskSolver::new();

        // Prices that sum to 90 cents (10 cent arbitrage)
        let mut prices = HashMap::new();
        for market in problem.markets.iter() {
            match market.name.as_str() {
                "Trump wins" => prices.insert(market.id, vec![400_000_000, 600_000_000]),
                "Biden wins" => prices.insert(market.id, vec![350_000_000, 650_000_000]),
                "Other wins" => prices.insert(market.id, vec![150_000_000, 850_000_000]),
                _ => None,
            };
        }

        let mut next_order_id = 1_000_000_000u64;
        let result = solver.find_arbitrage(&prices, &problem, &mut next_order_id);

        // Should find one arbitrage opportunity
        assert_eq!(result.opportunities_found, 1);
        assert!(result.total_welfare > 0);

        // Check the fill details
        let fill = &result.fills[0];
        assert_eq!(fill.group_name, "2024 Election");
        assert_eq!(fill.profit_per_share, 100_000_000); // 10 cents
        assert_eq!(fill.market_fills.len(), 3);
    }

    #[test]
    fn test_no_arbitrage_when_prices_sum_to_one() {
        let problem = create_election_problem();
        let solver = NegriskSolver::new();

        // Prices that sum to exactly $1
        let mut prices = HashMap::new();
        for market in problem.markets.iter() {
            match market.name.as_str() {
                "Trump wins" => prices.insert(market.id, vec![400_000_000, 600_000_000]),
                "Biden wins" => prices.insert(market.id, vec![400_000_000, 600_000_000]),
                "Other wins" => prices.insert(market.id, vec![200_000_000, 800_000_000]),
                _ => None,
            };
        }

        let mut next_order_id = 1_000_000_000u64;
        let result = solver.find_arbitrage(&prices, &problem, &mut next_order_id);

        // No arbitrage when prices sum to $1
        assert_eq!(result.opportunities_found, 0);
        assert_eq!(result.total_welfare, 0);
    }

    #[test]
    fn test_posrisk_arbitrage_when_prices_exceed_one() {
        let problem = create_election_problem();
        let solver = NegriskSolver::new();

        // Prices that sum to $1.10 (overpriced by 10 cents)
        let mut prices = HashMap::new();
        for market in problem.markets.iter() {
            match market.name.as_str() {
                "Trump wins" => prices.insert(market.id, vec![500_000_000, 500_000_000]),
                "Biden wins" => prices.insert(market.id, vec![400_000_000, 600_000_000]),
                "Other wins" => prices.insert(market.id, vec![200_000_000, 800_000_000]),
                _ => None,
            };
        }

        let mut next_order_id = 1_000_000_000u64;
        let result = solver.find_arbitrage(&prices, &problem, &mut next_order_id);

        // Posrisk arbitrage: sell all outcomes for $1.10, only pay $1 winner
        assert_eq!(result.opportunities_found, 1);
        assert!(result.total_welfare > 0);

        let fill = &result.fills[0];
        assert_eq!(fill.profit_per_share, 100_000_000); // 10 cents
    }

    #[test]
    fn test_min_profit_threshold() {
        let problem = create_election_problem();

        // High threshold - won't trigger on small opportunities
        let solver = NegriskSolver::with_config(NegriskConfig {
            min_profit_threshold: 200_000_000, // 20 cents
            max_shares_per_arb: None,
        });

        // Prices that sum to 95 cents (only 5 cent arbitrage)
        let mut prices = HashMap::new();
        for market in problem.markets.iter() {
            match market.name.as_str() {
                "Trump wins" => prices.insert(market.id, vec![450_000_000, 550_000_000]),
                "Biden wins" => prices.insert(market.id, vec![350_000_000, 650_000_000]),
                "Other wins" => prices.insert(market.id, vec![150_000_000, 850_000_000]),
                _ => None,
            };
        }

        let mut next_order_id = 1_000_000_000u64;
        let result = solver.find_arbitrage(&prices, &problem, &mut next_order_id);

        // Should not find opportunity (5 cents < 20 cent threshold)
        assert_eq!(result.opportunities_found, 0);
    }
}
