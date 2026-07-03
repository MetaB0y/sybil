//! Unified scenario generator for testing and benchmarking.
//!
//! This module provides a single comprehensive scenario generator with configurable
//! parameters for all testing needs: quick tests, stress tests, MILP-killer scenarios, etc.

use std::collections::HashMap;

use rand::prelude::*;
use rand_chacha::ChaCha8Rng;

use matching_engine::{
    outcome_buy, outcome_sell, price_to_nanos, MarketGroup, MarketId, MmConstraint, MmId, MmSide,
    Nanos, Order, Problem, Qty, NANOS_PER_DOLLAR,
};

/// Unified configuration for scenario generation.
#[derive(Clone, Debug)]
pub struct ScenarioConfig {
    /// Random seed for reproducibility
    pub seed: u64,

    // Market configuration
    /// Number of binary markets
    pub num_markets: usize,

    // Order configuration
    /// Total number of orders to generate
    pub num_orders: usize,
    /// Order size range
    pub order_size_min: Qty,
    pub order_size_max: Qty,

    // Liquidity configuration
    /// Liquidity scarcity (0.0-1.0, lower = more scarcity, more competition)
    pub liquidity_scarcity: f64,
    /// Fraction of markets that are "hot" (get extra demand)
    pub hot_market_fraction: f64,

    // Market maker configuration
    /// Number of market makers (0 = no MMs)
    pub num_mms: usize,
    /// MM budget range in dollars
    pub mm_budget_min: u64,
    pub mm_budget_max: u64,
    /// MM spread in basis points (lower = tighter = more aggressive)
    pub mm_spread_bps: u32,
    /// Capacity multiplier: total notional of MM orders = budget × this.
    /// Higher = more orders relative to budget = tighter budget constraint.
    pub mm_capacity_multiplier: u64,
}

impl Default for ScenarioConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            num_markets: 30,
            num_orders: 1000,
            order_size_min: 10,
            order_size_max: 200,
            liquidity_scarcity: 0.5,
            hot_market_fraction: 0.0,
            num_mms: 2,
            mm_budget_min: 2_500,
            mm_budget_max: 12_500,
            mm_spread_bps: 50, // 0.5% spread
            mm_capacity_multiplier: 10,
        }
    }
}

impl ScenarioConfig {
    /// Quick test scenario (~50 orders, fast to run)
    pub fn quick() -> Self {
        Self {
            num_markets: 5,
            num_orders: 50,

            num_mms: 0,
            liquidity_scarcity: 0.8,
            ..Default::default()
        }
    }

    /// Small scenario for unit tests (~300 orders)
    pub fn small() -> Self {
        Self {
            num_markets: 10,
            num_orders: 300,

            num_mms: 1,
            mm_budget_min: 1_250,
            mm_budget_max: 5_000,
            liquidity_scarcity: 0.6,
            ..Default::default()
        }
    }

    /// Medium scenario for integration tests (~3000 orders)
    pub fn medium() -> Self {
        Self {
            num_markets: 30,
            num_orders: 3000,

            num_mms: 2,
            liquidity_scarcity: 0.5,
            ..Default::default()
        }
    }

    /// Large scenario for stress testing (~10000 orders)
    pub fn large() -> Self {
        Self {
            num_markets: 50,
            num_orders: 10000,

            num_mms: 3,
            mm_budget_min: 5_000,
            mm_budget_max: 25_000,
            liquidity_scarcity: 0.4,
            ..Default::default()
        }
    }

    /// Extreme scenario for benchmarking limits (~100k orders)
    pub fn extreme() -> Self {
        Self {
            num_markets: 200,
            num_orders: 100_000,

            num_mms: 10,
            mm_budget_min: 12_500,
            mm_budget_max: 50_000,
            liquidity_scarcity: 0.3,
            ..Default::default()
        }
    }

    /// MILP-killer scenario designed to force MILP timeout
    pub fn milp_killer() -> Self {
        Self {
            num_markets: 50,
            num_orders: 5000,

            order_size_min: 30,
            order_size_max: 100,
            num_mms: 0,
            liquidity_scarcity: 0.2,  // Severe scarcity = competing solutions
            hot_market_fraction: 0.1, // Hot markets create conflicts
            ..Default::default()
        }
    }

    /// Set seed (builder pattern)
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }
}

/// Generate a scenario from configuration.
pub fn generate_scenario(config: ScenarioConfig) -> Problem {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
    let mut problem = Problem::new(format!(
        "Scenario(m={},o={})",
        config.num_markets, config.num_orders,
    ));

    // Generate binary markets, some grouped into multi-outcome events
    // About 60% of markets will be in groups of 3-4 (mutually exclusive outcomes)
    let mut market_info: Vec<(MarketId, f64)> = Vec::new();
    let mut market_idx = 0;
    let mut group_id = 0;

    while market_idx < config.num_markets {
        // Decide if this should be a group or standalone market
        let remaining = config.num_markets - market_idx;
        let make_group = remaining >= 3 && rng.random_bool(0.6);

        if make_group {
            // Create a group of 3-4 mutually exclusive markets
            let group_size = if remaining >= 4 && rng.random_bool(0.5) {
                4
            } else {
                3
            };
            let group_size = group_size.min(remaining);

            // Generate fair prices that sum to ~1.0 (with some variance for negrisk opportunities)
            // Use Box-Muller transform for normal distribution around 1.0 with stddev 0.1
            let target_sum: f64 = {
                // Box-Muller transform: convert uniform to normal
                let u1: f64 = rng.random_range(0.0001..1.0); // Avoid log(0)
                let u2: f64 = rng.random();
                let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
                (1.0 + 0.1 * z).clamp(0.85, 1.15) // Normal(1.0, 0.1), clamped
            };
            let mut raw_prices: Vec<f64> = (0..group_size)
                .map(|_| rng.random_range(0.1..1.0))
                .collect();
            let sum: f64 = raw_prices.iter().sum();
            for p in &mut raw_prices {
                *p = (*p / sum) * target_sum; // Normalize to target_sum (may be != 1.0)
            }

            let mut group = MarketGroup::new(format!("Group{}", group_id));

            for (i, &fair_price) in raw_prices.iter().enumerate() {
                let market_id = problem.markets.add_binary(format!("G{}M{}", group_id, i));
                market_info.push((market_id, fair_price));
                group.add_market(market_id);
            }

            problem.add_market_group(group);
            market_idx += group_size;
            group_id += 1;
        } else {
            // Standalone market - use wider price range for more variety
            let market_id = problem.markets.add_binary(format!("M{}", market_idx));
            let fair_price = rng.random_range(0.05..0.95);
            market_info.push((market_id, fair_price));
            market_idx += 1;
        }
    }

    // Identify hot markets
    let num_hot = (config.num_markets as f64 * config.hot_market_fraction).ceil() as usize;
    let mut hot_markets: Vec<MarketId> = market_info.iter().map(|(id, _)| *id).collect();
    hot_markets.shuffle(&mut rng);
    hot_markets.truncate(num_hot);

    // Add liquidity based on scarcity
    add_liquidity(&mut problem, &market_info, &hot_markets, &config, &mut rng);

    // Generate orders
    let mut order_id = 1u64;
    // Simple orders
    for _ in 0..config.num_orders {
        let order = generate_simple_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_info,
            &hot_markets,
            &config,
        );
        problem.orders.push(order);
    }

    // Generate MM constraints
    if config.num_mms > 0 {
        generate_mm_constraints(&mut problem, &market_info, &config, &mut rng, &mut order_id);
    }

    // Shuffle orders to avoid order-dependent behavior
    problem.orders.shuffle(&mut rng);

    problem
}

fn add_liquidity(
    problem: &mut Problem,
    market_info: &[(MarketId, f64)],
    hot_markets: &[MarketId],
    config: &ScenarioConfig,
    _rng: &mut ChaCha8Rng,
) {
    let avg_order_qty = (config.order_size_min + config.order_size_max) / 2;
    let total_demand_estimate = config.num_orders as u64 * avg_order_qty;
    let total_supply = (total_demand_estimate as f64 * config.liquidity_scarcity) as Qty;
    let supply_per_market = total_supply / config.num_markets as Qty;

    // Use high order IDs for supply sell orders to avoid collisions
    let mut sell_order_id = 5_000_000u64;

    for (market_id, fair_price) in market_info {
        let is_hot = hot_markets.contains(market_id);
        let market_supply = if is_hot {
            supply_per_market / 3 // Hot markets get less supply
        } else {
            supply_per_market
        };

        // YES outcome — sell orders provide supply (asks)
        for level in 0..3 {
            let offset = 0.05 * (level as f64 + 1.0);
            let level_supply = market_supply / 6;

            let ask_price = (*fair_price + offset).min(0.98);
            problem.orders.push(outcome_sell(
                &problem.markets,
                sell_order_id,
                *market_id,
                0,
                price_to_nanos(ask_price),
                level_supply.max(10),
            ));
            sell_order_id += 1;

            // Buy orders at bid prices (buying YES)
            let bid_price = (*fair_price - offset).max(0.02);
            problem.orders.push(outcome_buy(
                &problem.markets,
                sell_order_id,
                *market_id,
                0,
                price_to_nanos(bid_price),
                level_supply.max(10),
            ));
            sell_order_id += 1;
        }

        // NO outcome
        let no_price = 1.0 - fair_price;
        for level in 0..3 {
            let offset = 0.05 * (level as f64 + 1.0);
            let level_supply = market_supply / 6;

            let ask_price = (no_price + offset).min(0.98);
            problem.orders.push(outcome_sell(
                &problem.markets,
                sell_order_id,
                *market_id,
                1,
                price_to_nanos(ask_price),
                level_supply.max(10),
            ));
            sell_order_id += 1;

            // Buy orders at bid prices (buying NO)
            let bid_price = (no_price - offset).max(0.02);
            problem.orders.push(outcome_buy(
                &problem.markets,
                sell_order_id,
                *market_id,
                1,
                price_to_nanos(bid_price),
                level_supply.max(10),
            ));
            sell_order_id += 1;
        }
    }
}

fn generate_simple_order(
    markets: &matching_engine::MarketSet,
    rng: &mut ChaCha8Rng,
    order_id: &mut u64,
    market_info: &[(MarketId, f64)],
    hot_markets: &[MarketId],
    config: &ScenarioConfig,
) -> Order {
    let id = *order_id;
    *order_id += 1;

    // Bias towards hot markets if any
    let market_idx = if !hot_markets.is_empty() && rng.random_bool(0.6) {
        let hot_id = hot_markets[rng.random_range(0..hot_markets.len())];
        market_info
            .iter()
            .position(|(id, _)| *id == hot_id)
            .unwrap_or(0)
    } else {
        rng.random_range(0..market_info.len())
    };

    let (market, fair_price) = market_info[market_idx];
    let outcome = rng.random_range(0..2u8);

    let base_price = if outcome == 0 {
        fair_price
    } else {
        1.0 - fair_price
    };

    let is_sell = rng.random_bool(0.4); // 40% sells, 60% buys
    let aggressiveness = rng.random_range(-0.05..0.15);
    let limit = if is_sell {
        // Sellers want price >= limit (lower limit = more aggressive)
        (base_price - aggressiveness).clamp(0.05, 0.95)
    } else {
        // Buyers want price <= limit (higher limit = more aggressive)
        (base_price + aggressiveness).clamp(0.05, 0.95)
    };

    let qty = rng.random_range(config.order_size_min..config.order_size_max);

    if is_sell {
        outcome_sell(markets, id, market, outcome, price_to_nanos(limit), qty)
    } else {
        outcome_buy(markets, id, market, outcome, price_to_nanos(limit), qty)
    }
}

fn generate_mm_constraints(
    problem: &mut Problem,
    market_info: &[(MarketId, f64)],
    config: &ScenarioConfig,
    rng: &mut ChaCha8Rng,
    order_id: &mut u64,
) {
    let fair_prices: HashMap<MarketId, f64> = market_info.iter().cloned().collect();

    for mm_idx in 0..config.num_mms {
        let budget_dollars = rng.random_range(config.mm_budget_min..config.mm_budget_max);
        let budget_nanos = budget_dollars as Nanos * NANOS_PER_DOLLAR;

        let mut constraint = MmConstraint::new(MmId::new(mm_idx as u64 + 1), budget_nanos);

        // MM covers a subset of markets
        let markets_to_cover = market_info.len().min(20);
        let mut selected_markets: Vec<MarketId> = market_info.iter().map(|(id, _)| *id).collect();
        selected_markets.shuffle(rng);
        selected_markets.truncate(markets_to_cover);

        // MMs post on both sides of each market but only one side fills per market
        // (whichever side the clearing price moves to). Total notional can exceed
        // budget since most orders won't fill.
        let total_capacity = budget_dollars * config.mm_capacity_multiplier;
        let qty_per_market = (total_capacity / markets_to_cover as u64).max(100);

        let half_spread = config.mm_spread_bps as f64 / 10_000.0 / 2.0;

        for market_id in selected_markets {
            let fair_price = fair_prices.get(&market_id).copied().unwrap_or(0.5);

            // MM quotes at 3 depth levels around fair price, with increasing spread
            for level in 0..3 {
                let level_qty = qty_per_market / 3;
                let level_spread = half_spread * (1.0 + level as f64); // wider at deeper levels

                // Normal spread: bid BELOW fair, ask ABOVE fair
                let bid_price = (fair_price - level_spread).max(0.02);
                let ask_price = (fair_price + level_spread).min(0.98);

                // MM sell order (selling YES at ask price — wants price ≥ ask)
                let sell_order = outcome_sell(
                    &problem.markets,
                    *order_id,
                    market_id,
                    0,
                    price_to_nanos(ask_price),
                    level_qty.max(10),
                );
                constraint.add_order(*order_id, MmSide::SellYes);
                problem.orders.push(sell_order);
                *order_id += 1;

                // MM buy order (buying YES at bid price — wants price ≤ bid)
                let buy_order = outcome_buy(
                    &problem.markets,
                    *order_id,
                    market_id,
                    0,
                    price_to_nanos(bid_price),
                    level_qty.max(10),
                );
                constraint.add_order(*order_id, MmSide::BuyYes);
                problem.orders.push(buy_order);
                *order_id += 1;
            }
        }

        problem.mm_constraints.push(constraint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quick_scenario() {
        let problem = generate_scenario(ScenarioConfig::quick());
        assert!(problem.num_markets() >= 5);
        assert!(problem.num_orders() > 0);
    }

    #[test]
    fn test_small_scenario() {
        let problem = generate_scenario(ScenarioConfig::small());
        assert!(problem.num_markets() >= 10);
        assert!(problem.num_orders() >= 100);
        assert!(!problem.mm_constraints.is_empty());
    }

    #[test]
    fn test_medium_scenario() {
        let problem = generate_scenario(ScenarioConfig::medium());
        assert!(problem.num_markets() >= 30);
        assert!(problem.num_orders() >= 1000);
    }

    #[test]
    fn test_seed_reproducibility() {
        let config = ScenarioConfig::small().with_seed(123);
        let p1 = generate_scenario(config.clone());
        let p2 = generate_scenario(config);
        assert_eq!(p1.num_orders(), p2.num_orders());
        assert_eq!(p1.orders[0].id, p2.orders[0].id);
    }

    #[test]
    fn test_all_markets_binary() {
        let problem = generate_scenario(ScenarioConfig::medium());
        for market in problem.markets.iter() {
            assert!(market.is_binary());
        }
    }
}
