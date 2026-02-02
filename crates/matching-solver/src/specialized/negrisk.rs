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

use matching_engine::{Fill, MarketGroup, MarketId, Nanos, Order, NANOS_PER_DOLLAR};

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
    /// * `market_groups` - Groups of mutually exclusive markets
    /// * `next_order_id` - Mutable counter for generating unique order IDs
    /// * `available_volume` - Per-market volume available for arbitrage (e.g.
    ///   fill volumes from price discovery, or liquidity book depth)
    pub fn find_arbitrage(
        &self,
        prices: &HashMap<MarketId, Vec<Nanos>>,
        market_groups: &[MarketGroup],
        next_order_id: &mut u64,
        available_volume: &HashMap<MarketId, u64>,
    ) -> NegriskResult {
        let mut fills = Vec::new();
        let mut arbitrage_orders = Vec::new();
        let mut total_welfare: i64 = 0;
        let mut total_shares: u64 = 0;

        for group in market_groups {
            if let Some((arb_fill, orders)) =
                self.check_group(group, prices, next_order_id, available_volume)
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
    fn check_group(
        &self,
        group: &MarketGroup,
        prices: &HashMap<MarketId, Vec<Nanos>>,
        next_order_id: &mut u64,
        available_volume: &HashMap<MarketId, u64>,
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

        if sum_yes == 0 {
            return None;
        }

        // Check for arbitrage opportunity
        let target = NANOS_PER_DOLLAR as u128;

        // Negrisk (sum < $1): Buy YES on all outcomes for < $1, guaranteed $1 payout
        // Posrisk (sum > $1): Buy NO on all outcomes for < $1, guaranteed $1 payout
        //
        // Both are modeled as buy orders (welfare = limit - fill_price). For posrisk,
        // buying NO increases NO demand → raises NO price → lowers YES price → sum drops.
        let is_negrisk = sum_yes < target;
        let profit_per_share = if is_negrisk {
            (target - sum_yes) as Nanos
        } else if sum_yes > target {
            (sum_yes - target) as Nanos
        } else {
            return None;
        };

        if profit_per_share < self.config.min_profit_threshold {
            return None;
        }

        // Max shares = minimum available volume across all markets in the group
        let mut max_shares = u64::MAX;
        for &(market_id, _) in &market_yes_prices {
            let &volume = available_volume.get(&market_id)?;
            max_shares = max_shares.min(volume);
        }

        if let Some(limit) = self.config.max_shares_per_arb {
            max_shares = max_shares.min(limit);
        }

        if max_shares == 0 {
            return None;
        }

        // Create arbitrage orders and fills for each market.
        //
        // Welfare attribution: ALL profit goes to the FIRST order via its limit_price.
        // Other orders have limit = fill_price (zero individual welfare).
        let mut orders = Vec::new();
        let mut market_fills = Vec::new();

        for &(market_id, yes_price) in &market_yes_prices {
            let order_id = *next_order_id;
            *next_order_id += 1;

            let no_price = NANOS_PER_DOLLAR.saturating_sub(yes_price);

            let mut order = Order::new(order_id);
            order.markets[0] = market_id;
            order.num_markets = 1;
            order.num_states = 2;

            // Negrisk: buy YES (payoff when YES wins = state 0)
            // Posrisk: buy NO (payoff when NO wins = state 1) — pushes YES prices down
            let fill_price = if is_negrisk { yes_price } else { no_price };

            if is_negrisk {
                order.payoffs[0] = 1; // YES state payoff
                order.payoffs[1] = 0;
            } else {
                order.payoffs[0] = 0;
                order.payoffs[1] = 1; // NO state payoff
            }

            // Fair-share limit: scale current price proportionally so group sums to $1.
            // This creates price pressure in unified clearing — arb orders are willing to
            // pay up to the fair value, not just the current mispriced value.
            let fair_yes = (yes_price as u128 * NANOS_PER_DOLLAR as u128 / sum_yes) as Nanos;
            let fair_no = NANOS_PER_DOLLAR.saturating_sub(fair_yes);
            order.limit_price = if is_negrisk { fair_yes } else { fair_no };

            order.min_fill = 1;
            order.max_fill = max_shares;

            orders.push(order);
            market_fills.push(Fill {
                order_id,
                fill_price,
                fill_qty: max_shares,
            });
        }

        let total_cost = if is_negrisk {
            sum_yes as Nanos
        } else {
            // Posrisk: cost is sum of NO prices
            (group.markets.len() as u64 * NANOS_PER_DOLLAR - sum_yes as u64) as Nanos
        };
        let welfare = (profit_per_share as i64) * (max_shares as i64);

        Some((
            NegriskFill {
                group_name: group.name.clone(),
                market_fills,
                total_cost,
                payout: NANOS_PER_DOLLAR,
                profit_per_share,
                shares: max_shares,
                welfare,
            },
            orders,
        ))
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
    use matching_engine::{MarketSet, Problem};

    /// Set up 3-candidate election: markets, groups, and 1000 shares available per market.
    fn create_election_setup() -> (MarketSet, Vec<MarketGroup>, HashMap<MarketId, u64>) {
        let mut problem = Problem::new("election");
        let trump = problem.markets.add_binary("Trump wins");
        let biden = problem.markets.add_binary("Biden wins");
        let other = problem.markets.add_binary("Other wins");

        let groups = vec![MarketGroup::new("2024 Election")
            .with_market(trump)
            .with_market(biden)
            .with_market(other)];

        let mut volumes = HashMap::new();
        volumes.insert(trump, 1000);
        volumes.insert(biden, 1000);
        volumes.insert(other, 1000);

        (problem.markets, groups, volumes)
    }

    #[test]
    fn test_negrisk_detection() {
        let (markets, groups, volumes) = create_election_setup();
        let solver = NegriskSolver::new();

        // Prices that sum to 90 cents (10 cent arbitrage)
        let mut prices = HashMap::new();
        for market in markets.iter() {
            match market.name.as_str() {
                "Trump wins" => prices.insert(market.id, vec![400_000_000, 600_000_000]),
                "Biden wins" => prices.insert(market.id, vec![350_000_000, 650_000_000]),
                "Other wins" => prices.insert(market.id, vec![150_000_000, 850_000_000]),
                _ => None,
            };
        }

        let mut next_order_id = 1_000_000_000u64;
        let result = solver.find_arbitrage(&prices, &groups, &mut next_order_id, &volumes);

        assert_eq!(result.opportunities_found, 1);
        assert!(result.total_welfare > 0);

        let fill = &result.fills[0];
        assert_eq!(fill.group_name, "2024 Election");
        assert_eq!(fill.profit_per_share, 100_000_000); // 10 cents
        assert_eq!(fill.market_fills.len(), 3);
    }

    #[test]
    fn test_no_arbitrage_when_prices_sum_to_one() {
        let (markets, groups, volumes) = create_election_setup();
        let solver = NegriskSolver::new();

        let mut prices = HashMap::new();
        for market in markets.iter() {
            match market.name.as_str() {
                "Trump wins" => prices.insert(market.id, vec![400_000_000, 600_000_000]),
                "Biden wins" => prices.insert(market.id, vec![400_000_000, 600_000_000]),
                "Other wins" => prices.insert(market.id, vec![200_000_000, 800_000_000]),
                _ => None,
            };
        }

        let mut next_order_id = 1_000_000_000u64;
        let result = solver.find_arbitrage(&prices, &groups, &mut next_order_id, &volumes);

        assert_eq!(result.opportunities_found, 0);
        assert_eq!(result.total_welfare, 0);
    }

    #[test]
    fn test_posrisk_arbitrage_when_prices_exceed_one() {
        let (markets, groups, volumes) = create_election_setup();
        let solver = NegriskSolver::new();

        let mut prices = HashMap::new();
        for market in markets.iter() {
            match market.name.as_str() {
                "Trump wins" => prices.insert(market.id, vec![500_000_000, 500_000_000]),
                "Biden wins" => prices.insert(market.id, vec![400_000_000, 600_000_000]),
                "Other wins" => prices.insert(market.id, vec![200_000_000, 800_000_000]),
                _ => None,
            };
        }

        let mut next_order_id = 1_000_000_000u64;
        let result = solver.find_arbitrage(&prices, &groups, &mut next_order_id, &volumes);

        assert_eq!(result.opportunities_found, 1);
        assert!(result.total_welfare > 0);
        assert_eq!(result.fills[0].profit_per_share, 100_000_000);
    }

    #[test]
    fn test_min_profit_threshold() {
        let (markets, groups, volumes) = create_election_setup();
        let solver = NegriskSolver::with_config(NegriskConfig {
            min_profit_threshold: 200_000_000, // 20 cents
            max_shares_per_arb: None,
        });

        // Prices sum to 95 cents (only 5 cent arbitrage — below 20 cent threshold)
        let mut prices = HashMap::new();
        for market in markets.iter() {
            match market.name.as_str() {
                "Trump wins" => prices.insert(market.id, vec![450_000_000, 550_000_000]),
                "Biden wins" => prices.insert(market.id, vec![350_000_000, 650_000_000]),
                "Other wins" => prices.insert(market.id, vec![150_000_000, 850_000_000]),
                _ => None,
            };
        }

        let mut next_order_id = 1_000_000_000u64;
        let result = solver.find_arbitrage(&prices, &groups, &mut next_order_id, &volumes);

        assert_eq!(result.opportunities_found, 0);
    }
}
