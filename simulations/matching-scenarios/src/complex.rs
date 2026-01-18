//! Complex scenario generators for testing solver composition.
//!
//! These scenarios are designed to test specific solver capabilities:
//! - Nested bundles
//! - Conditional chains
//! - Deep implications
//! - Liquidity cliffs
//! - Adversarial orders
//! - Large interconnected problems

use matching_engine::{
    ConditionDir, MarketConstraint, MarketId, OrderBuilder, PriceCondition, Problem,
    simple_yes_buy,
};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;

/// Configuration for nested bundle scenario.
#[derive(Clone, Debug)]
pub struct NestedBundleConfig {
    /// Random seed
    pub seed: u64,
    /// Number of markets
    pub num_markets: usize,
    /// Number of bundles to create
    pub num_bundles: usize,
    /// Markets per bundle
    pub markets_per_bundle: usize,
    /// Fraction of bundles that share markets with others
    pub overlap_fraction: f64,
    /// Base liquidity per market
    pub liquidity_per_market: u64,
}

impl Default for NestedBundleConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            num_markets: 6,
            num_bundles: 10,
            markets_per_bundle: 2,
            overlap_fraction: 0.5,
            liquidity_per_market: 1000,
        }
    }
}

/// Generate a nested bundle scenario.
///
/// Bundles share markets with each other, creating complex dependencies.
pub fn generate_nested_bundle_scenario(config: NestedBundleConfig) -> Problem {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
    let mut problem = Problem::new("nested-bundles");

    // Create markets
    let mut market_ids = Vec::new();
    for i in 0..config.num_markets {
        let market_id = problem.markets.add_binary(format!("Market{}", i));
        market_ids.push(market_id);

        // Add liquidity to both outcomes
        for outcome in 0..2 {
            let price = rng.gen_range(400_000_000u64..600_000_000);
            problem
                .liquidity
                .add_ask(market_id, outcome, price, config.liquidity_per_market);
        }
    }

    // Create bundles with overlapping markets
    let mut order_id = 1u64;
    for i in 0..config.num_bundles {
        let should_overlap = rng.gen::<f64>() < config.overlap_fraction && i > 0;

        let bundle_markets: Vec<MarketId> = if should_overlap {
            // Pick some markets that overlap with previous bundles
            let start =
                rng.gen_range(0..(config.num_markets.saturating_sub(config.markets_per_bundle)));
            (start..start + config.markets_per_bundle)
                .map(|j| market_ids[j % config.num_markets])
                .collect()
        } else {
            // Pick random non-overlapping markets
            let indices: Vec<usize> = (0..config.num_markets)
                .choose_multiple(&mut rng, config.markets_per_bundle);
            indices.iter().map(|&j| market_ids[j]).collect()
        };

        // Create payoff vector for bundle (buying YES on all markets)

        let limit_price = rng.gen_range(300_000_000..800_000_000);
        let max_fill = rng.gen_range(50..200);

        // Build the order using OrderBuilder
        let mut builder = OrderBuilder::new(&problem.markets, order_id)
            .spanning(&bundle_markets)
            .limit(limit_price)
            .quantity(0, max_fill);

        // Positive payoff only when all markets are YES (state = 0, i.e., all outcome 0)
        builder = builder.payoff_at(0, 1);

        problem.orders.push(builder.build());
        order_id += 1;
    }

    problem
}

/// Configuration for conditional chain scenario.
#[derive(Clone, Debug)]
pub struct ConditionalChainConfig {
    /// Random seed
    pub seed: u64,
    /// Length of the conditional chain
    pub chain_length: usize,
    /// Base price threshold
    pub base_threshold: u64,
    /// Threshold increment per step
    pub threshold_step: u64,
}

impl Default for ConditionalChainConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            chain_length: 4,
            base_threshold: 400_000_000,
            threshold_step: 50_000_000,
        }
    }
}

/// Generate a conditional chain scenario.
///
/// Order A triggers B triggers C triggers D...
/// Each conditional depends on the previous order's fill price.
pub fn generate_conditional_chain_scenario(config: ConditionalChainConfig) -> Problem {
    let mut problem = Problem::new("conditional-chain");

    let m1 = problem.markets.add_binary("Market1");

    // Add substantial liquidity
    for outcome in 0..2 {
        problem.liquidity.add_ask(m1, outcome, 500_000_000, 10000);
    }

    // First order is unconditional
    problem.orders.push(
        simple_yes_buy(&problem.markets, 1, m1, 600_000_000, 100)
    );

    // Create chain of conditional orders
    for i in 1..config.chain_length {
        let threshold = config.base_threshold + (i as u64 - 1) * config.threshold_step;
        let limit_price = threshold + 100_000_000;

        let mut order = OrderBuilder::new(&problem.markets, i as u64 + 1)
            .spanning(&[m1])
            .limit(limit_price)
            .quantity(0, 100)
            .payoff_at(0, 1)  // Win on YES
            .build();

        order.condition = Some(PriceCondition {
            market: m1,
            threshold,
            direction: ConditionDir::Above,
        });

        problem.orders.push(order);
    }

    problem
}

/// Configuration for deep implication scenario.
#[derive(Clone, Debug)]
pub struct DeepImplicationConfig {
    /// Random seed
    pub seed: u64,
    /// Depth of implication chain (A→B→C→D...)
    pub chain_depth: usize,
    /// Number of orders per market
    pub orders_per_market: usize,
}

impl Default for DeepImplicationConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            chain_depth: 4,
            orders_per_market: 3,
        }
    }
}

/// Generate a deep implication scenario.
///
/// Creates a chain of markets with implications: M0→M1→M2→M3...
/// Orders placed on each market must respect the constraint hierarchy.
pub fn generate_deep_implication_scenario(config: DeepImplicationConfig) -> Problem {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
    let mut problem = Problem::new("deep-implications");

    let mut market_ids = Vec::new();

    // Create markets
    for i in 0..config.chain_depth {
        let market_id = problem.markets.add_binary(format!("Level{}", i));
        market_ids.push(market_id);

        // Add liquidity
        for outcome in 0..2 {
            // Price decreases along chain (deeper implies more likely)
            let base_price = 700_000_000 - (i as u64 * 100_000_000);
            let price = base_price.max(200_000_000);
            problem.liquidity.add_ask(market_id, outcome, price, 1000);
        }
    }

    // Add implication constraints: M0 → M1 → M2 → ...
    for i in 0..config.chain_depth - 1 {
        problem.constraints.add(MarketConstraint::implies(
            market_ids[i],
            0, // outcome 0 (YES) of earlier market
            market_ids[i + 1],
            0, // implies outcome 0 (YES) of later market
        ));
    }

    // Add orders for each market
    let mut order_id = 1u64;
    for &market in market_ids.iter().take(config.chain_depth) {

        for _j in 0..config.orders_per_market {
            let limit_price = rng.gen_range(300_000_000..800_000_000);
            let max_fill = rng.gen_range(50..150);

            problem.orders.push(
                simple_yes_buy(&problem.markets, order_id, market, limit_price, max_fill)
            );
            order_id += 1;
        }
    }

    problem
}

/// Configuration for liquidity cliff scenario.
#[derive(Clone, Debug)]
pub struct LiquidityCliffConfig {
    /// Random seed
    pub seed: u64,
    /// Number of markets
    pub num_markets: usize,
    /// Number of orders
    pub num_orders: usize,
    /// Price at which liquidity drops sharply
    pub cliff_price: u64,
    /// Liquidity above cliff
    pub liquidity_above_cliff: u64,
    /// Liquidity below cliff (much smaller)
    pub liquidity_below_cliff: u64,
}

impl Default for LiquidityCliffConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            num_markets: 3,
            num_orders: 20,
            cliff_price: 500_000_000,
            liquidity_above_cliff: 5000,
            liquidity_below_cliff: 100,
        }
    }
}

/// Generate a liquidity cliff scenario.
///
/// Liquidity drops sharply at certain price levels, creating
/// discontinuities that challenge greedy solvers.
pub fn generate_liquidity_cliff_scenario(config: LiquidityCliffConfig) -> Problem {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
    let mut problem = Problem::new("liquidity-cliff");

    let mut market_ids = Vec::new();

    // Create markets with cliff liquidity structure
    for i in 0..config.num_markets {
        let market_id = problem.markets.add_binary(format!("Cliff{}", i));
        market_ids.push(market_id);

        for outcome in 0..2 {
            // Multiple price levels with cliff
            // Cheap liquidity below cliff (small quantity)
            problem.liquidity.add_ask(
                market_id,
                outcome,
                config.cliff_price - 100_000_000,
                config.liquidity_below_cliff,
            );

            // Expensive liquidity above cliff (large quantity)
            problem.liquidity.add_ask(
                market_id,
                outcome,
                config.cliff_price + 100_000_000,
                config.liquidity_above_cliff,
            );
        }
    }

    // Create orders that span the cliff
    let mut order_id = 1u64;
    for _ in 0..config.num_orders {
        let market = market_ids[rng.gen_range(0..config.num_markets)];

        // Some orders can only fill below cliff, some only above
        let limit_price = if rng.gen::<bool>() {
            config.cliff_price - 50_000_000 // Will only get small fills
        } else {
            config.cliff_price + 150_000_000 // Can access deep liquidity
        };

        let max_fill = rng.gen_range(100..500);

        problem.orders.push(
            simple_yes_buy(&problem.markets, order_id, market, limit_price, max_fill)
        );
        order_id += 1;
    }

    problem
}

/// Configuration for adversarial scenario.
#[derive(Clone, Debug)]
pub struct AdversarialConfig {
    /// Random seed
    pub seed: u64,
    /// Number of conflicting groups
    pub num_groups: usize,
    /// Orders per conflicting group
    pub orders_per_group: usize,
    /// Shared liquidity that groups compete for
    pub shared_liquidity: u64,
}

impl Default for AdversarialConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            num_groups: 3,
            orders_per_group: 4,
            shared_liquidity: 500,
        }
    }
}

/// Generate an adversarial scenario.
///
/// Creates groups of orders that compete for the same limited liquidity.
/// Optimal solution requires careful selection across groups.
pub fn generate_adversarial_scenario(config: AdversarialConfig) -> Problem {
    let mut problem = Problem::new("adversarial");

    let m1 = problem.markets.add_binary("Contested");

    // Limited shared liquidity
    problem
        .liquidity
        .add_ask(m1, 0, 500_000_000, config.shared_liquidity);

    let mut order_id = 1u64;

    // Create conflicting groups
    for group in 0..config.num_groups {
        // Each group has orders with varying welfare but same liquidity demand
        for i in 0..config.orders_per_group {
            // Welfare varies within group
            let base_price = 600_000_000 + (group as u64 * 50_000_000);
            let price_variation = i as u64 * 20_000_000;
            let limit_price = base_price + price_variation;

            // All orders want same amount of liquidity
            let max_fill = config.shared_liquidity / (config.num_groups as u64);

            problem.orders.push(
                simple_yes_buy(&problem.markets, order_id, m1, limit_price, max_fill)
            );
            order_id += 1;
        }
    }

    problem
}

/// Configuration for large interconnected scenario.
#[derive(Clone, Debug)]
pub struct LargeInterconnectedConfig {
    /// Random seed
    pub seed: u64,
    /// Number of markets (should be >5 to test decomposition)
    pub num_markets: usize,
    /// Number of orders
    pub num_orders: usize,
    /// Fraction of orders that are bundles
    pub bundle_fraction: f64,
    /// Maximum markets per bundle
    pub max_bundle_size: usize,
}

impl Default for LargeInterconnectedConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            num_markets: 10,
            num_orders: 50,
            bundle_fraction: 0.3,
            max_bundle_size: 3,
        }
    }
}

/// Generate a large interconnected scenario.
///
/// Many markets connected by bundle orders, testing decomposition algorithms.
pub fn generate_large_interconnected_scenario(config: LargeInterconnectedConfig) -> Problem {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
    let mut problem = Problem::new("large-interconnected");

    let mut market_ids = Vec::new();

    // Create markets
    for i in 0..config.num_markets {
        let market_id = problem.markets.add_binary(format!("M{}", i));
        market_ids.push(market_id);

        // Add liquidity
        for outcome in 0..2 {
            let price = rng.gen_range(400_000_000u64..600_000_000);
            problem.liquidity.add_ask(market_id, outcome, price, 1000);
        }
    }

    // Create orders
    let mut order_id = 1u64;
    for _ in 0..config.num_orders {
        let is_bundle = rng.gen::<f64>() < config.bundle_fraction;

        if is_bundle {
            // Bundle order spanning multiple markets
            let bundle_size = rng.gen_range(2..=config.max_bundle_size.min(config.num_markets));
            let bundle_markets: Vec<MarketId> = (0..config.num_markets)
                .choose_multiple(&mut rng, bundle_size)
                .iter()
                .map(|&i| market_ids[i])
                .collect();

            let limit_price = rng.gen_range(300_000_000..800_000_000);
            let max_fill = rng.gen_range(50..200);

            // Build bundle order - payoff of 1 when all are YES (state 0)
            let order = OrderBuilder::new(&problem.markets, order_id)
                .spanning(&bundle_markets)
                .limit(limit_price)
                .quantity(0, max_fill)
                .payoff_at(0, 1)  // Win when all YES
                .build();

            problem.orders.push(order);
        } else {
            // Simple single-market order
            let market = market_ids[rng.gen_range(0..config.num_markets)];
            let limit_price = rng.gen_range(300_000_000..800_000_000);
            let max_fill = rng.gen_range(50..200);

            problem.orders.push(
                simple_yes_buy(&problem.markets, order_id, market, limit_price, max_fill)
            );
        }

        order_id += 1;
    }

    problem
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nested_bundle_generation() {
        let config = NestedBundleConfig::default();
        let problem = generate_nested_bundle_scenario(config.clone());

        assert_eq!(problem.num_markets(), config.num_markets);
        assert_eq!(problem.num_orders(), config.num_bundles);
    }

    #[test]
    fn test_conditional_chain_generation() {
        let config = ConditionalChainConfig::default();
        let problem = generate_conditional_chain_scenario(config.clone());

        assert_eq!(problem.num_markets(), 1);
        assert_eq!(problem.num_orders(), config.chain_length);

        // First order should not be conditional
        assert!(!problem.orders[0].is_conditional());

        // Rest should be conditional
        for order in problem.orders.iter().skip(1) {
            assert!(order.is_conditional());
        }
    }

    #[test]
    fn test_deep_implication_generation() {
        let config = DeepImplicationConfig::default();
        let problem = generate_deep_implication_scenario(config.clone());

        assert_eq!(problem.num_markets(), config.chain_depth);
        assert_eq!(
            problem.num_orders(),
            config.chain_depth * config.orders_per_market
        );
        assert_eq!(problem.constraints.len(), config.chain_depth - 1);
    }

    #[test]
    fn test_liquidity_cliff_generation() {
        let config = LiquidityCliffConfig::default();
        let problem = generate_liquidity_cliff_scenario(config.clone());

        assert_eq!(problem.num_markets(), config.num_markets);
        assert_eq!(problem.num_orders(), config.num_orders);
    }

    #[test]
    fn test_adversarial_generation() {
        let config = AdversarialConfig::default();
        let problem = generate_adversarial_scenario(config.clone());

        assert_eq!(problem.num_markets(), 1);
        assert_eq!(
            problem.num_orders(),
            config.num_groups * config.orders_per_group
        );
    }

    #[test]
    fn test_large_interconnected_generation() {
        let config = LargeInterconnectedConfig::default();
        let problem = generate_large_interconnected_scenario(config.clone());

        assert_eq!(problem.num_markets(), config.num_markets);
        assert_eq!(problem.num_orders(), config.num_orders);
    }
}
