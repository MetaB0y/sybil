//! Stress scenarios for testing solver scalability.
//!
//! These scenarios are designed to stress-test solvers, particularly MILP,
//! with large numbers of orders and complex interdependencies.

use rand::Rng;
use rand::rngs::StdRng;
use rand::SeedableRng;
use rand::seq::SliceRandom;

use matching_engine::{
    ConstraintBuilder, MarketSet, Order, MarketId, Qty,
    price_to_nanos, outcome_buy, bundle_yes, spread,
};

use matching_engine::Problem;

use crate::{
    generate_presidential_scenario, generate_tournament_scenario,
    generate_random_scenario, PresidentialConfig, TournamentConfig, RandomConfig,
};
use crate::complex::{
    generate_nested_bundle_scenario, generate_large_interconnected_scenario,
    NestedBundleConfig, LargeInterconnectedConfig,
};

/// Configuration for mega stress scenarios.
#[derive(Clone, Debug)]
pub struct MegaScenarioConfig {
    /// Random seed
    pub seed: u64,
    /// Number of markets (20-50 for stress)
    pub num_markets: usize,
    /// Number of orders (500-2000 for stress)
    pub num_orders: usize,
    /// Fraction of orders that are multi-market bundles (0.0-1.0)
    pub bundle_fraction: f64,
    /// Number of implication chains to add complexity
    pub implication_chains: usize,
    /// Liquidity scarcity (0.0-1.0, lower = more scarcity)
    pub liquidity_scarcity: f64,
    /// Fraction of spread orders
    pub spread_fraction: f64,
    /// Price spread for liquidity tiers
    pub price_spread: f64,
}

impl Default for MegaScenarioConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            num_markets: 30,
            num_orders: 1000,
            bundle_fraction: 0.25,
            implication_chains: 5,
            liquidity_scarcity: 0.4,
            spread_fraction: 0.1,
            price_spread: 0.08,
        }
    }
}

impl MegaScenarioConfig {
    /// Small mega scenario for quick testing.
    pub fn small() -> Self {
        Self {
            num_markets: 20,
            num_orders: 500,
            bundle_fraction: 0.2,
            implication_chains: 3,
            liquidity_scarcity: 0.5,
            ..Default::default()
        }
    }

    /// Medium mega scenario.
    pub fn medium() -> Self {
        Self {
            num_markets: 30,
            num_orders: 1000,
            bundle_fraction: 0.25,
            implication_chains: 5,
            liquidity_scarcity: 0.4,
            ..Default::default()
        }
    }

    /// Large mega scenario for serious stress testing.
    pub fn large() -> Self {
        Self {
            num_markets: 50,
            num_orders: 2000,
            bundle_fraction: 0.3,
            implication_chains: 10,
            liquidity_scarcity: 0.3,
            ..Default::default()
        }
    }

    /// Extreme scenario for benchmarking solver limits.
    pub fn extreme() -> Self {
        Self {
            num_markets: 75,
            num_orders: 5000,
            bundle_fraction: 0.35,
            implication_chains: 15,
            liquidity_scarcity: 0.25,
            ..Default::default()
        }
    }
}

/// Generate a mega stress scenario.
///
/// This scenario combines multiple complexity factors:
/// - Large number of markets and orders
/// - Multi-market bundles
/// - Implication constraints
/// - Liquidity scarcity
/// - Conflicting high-value orders
pub fn generate_mega_scenario(config: MegaScenarioConfig) -> Problem {
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut problem = Problem::new(format!(
        "Mega(markets={}, orders={}, bundles={}%, liq={:.0}%)",
        config.num_markets,
        config.num_orders,
        (config.bundle_fraction * 100.0) as i32,
        config.liquidity_scarcity * 100.0
    ));

    // Create markets with varying outcomes
    let mut market_ids: Vec<MarketId> = Vec::new();
    let mut market_prices: Vec<f64> = Vec::new();
    let mut market_outcomes: Vec<u8> = Vec::new();

    for i in 0..config.num_markets {
        // Mix of binary and multi-outcome markets
        let outcomes = if rng.gen_bool(0.8) {
            2 // 80% binary
        } else {
            rng.gen_range(3..=4) // 20% multi-outcome
        };

        let outcome_names: Vec<String> = (0..outcomes)
            .map(|j| format!("O{}", j))
            .collect();
        let market = problem.markets.add(format!("M{}", i), outcome_names);
        market_ids.push(market);

        let mid_price = rng.gen_range(0.2..0.8);
        market_prices.push(mid_price);
        market_outcomes.push(outcomes as u8);
    }

    // Add implication constraints (creates constraint complexity)
    if config.implication_chains > 0 && market_ids.len() >= 2 {
        let mut constraint_builder = ConstraintBuilder::new();

        // Create chains of implications
        for chain in 0..config.implication_chains {
            let chain_length = rng.gen_range(2..=4.min(market_ids.len()));
            let mut chain_markets: Vec<usize> = (0..market_ids.len()).collect();
            chain_markets.shuffle(&mut rng);
            chain_markets.truncate(chain_length);

            // Create A → B → C chain
            for i in 0..chain_markets.len() - 1 {
                let m1 = market_ids[chain_markets[i]];
                let m2 = market_ids[chain_markets[i + 1]];
                constraint_builder = constraint_builder.implies(m1, 0, m2, 0);
            }
        }

        // Add some exclusions
        let num_exclusions = config.implication_chains / 2;
        for _ in 0..num_exclusions {
            let m1_idx = rng.gen_range(0..market_ids.len());
            let mut m2_idx = rng.gen_range(0..market_ids.len());
            while m2_idx == m1_idx {
                m2_idx = rng.gen_range(0..market_ids.len());
            }

            constraint_builder = constraint_builder.mutually_exclusive(vec![
                (market_ids[m1_idx], 0),
                (market_ids[m2_idx], 0),
            ]);
        }

        problem.constraints = constraint_builder.build();
    }

    // Add liquidity with scarcity
    let avg_order_qty = 60u64;
    let total_demand_estimate = config.num_orders as u64 * avg_order_qty;
    let total_supply = (total_demand_estimate as f64 * config.liquidity_scarcity) as Qty;
    let supply_per_market = total_supply / config.num_markets as Qty;

    for (i, &market) in market_ids.iter().enumerate() {
        let mid_price = market_prices[i];
        let outcomes = market_outcomes[i];

        for outcome in 0..outcomes {
            let outcome_mid_price = if outcome == 0 {
                mid_price
            } else if outcomes == 2 {
                1.0 - mid_price
            } else {
                (1.0 - mid_price) / (outcomes as f64 - 1.0)
            };

            // Multiple price levels
            for level in 0..4 {
                let offset = config.price_spread * (level as f64 + 1.0) / 4.0;
                let level_supply = supply_per_market / (outcomes as Qty * 4);

                // Asks (sellers)
                let ask_price = (outcome_mid_price + offset).min(0.98);
                problem.liquidity.add_ask(
                    market,
                    outcome,
                    price_to_nanos(ask_price),
                    level_supply.max(10),
                );

                // Bids (buyers)
                let bid_price = (outcome_mid_price - offset).max(0.02);
                problem.liquidity.add_bid(
                    market,
                    outcome,
                    price_to_nanos(bid_price),
                    level_supply.max(10),
                );
            }
        }
    }

    // Generate orders
    let num_bundles = (config.num_orders as f64 * config.bundle_fraction) as usize;
    let num_spreads = (config.num_orders as f64 * config.spread_fraction) as usize;
    let num_simple = config.num_orders - num_bundles - num_spreads;

    let mut order_id = 1u64;

    // Simple orders
    for _ in 0..num_simple {
        let order = generate_stress_simple_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_ids,
            &market_prices,
            &market_outcomes,
        );
        problem.orders.push(order);
    }

    // Bundle orders (multi-market)
    for _ in 0..num_bundles {
        let order = generate_stress_bundle_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_ids,
            &market_prices,
        );
        problem.orders.push(order);
    }

    // Spread orders
    for _ in 0..num_spreads {
        let order = generate_stress_spread_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_ids,
            &market_prices,
        );
        problem.orders.push(order);
    }

    // Inject high-value conflicts (makes problem harder for MILP)
    inject_stress_conflicts(&mut problem, &mut rng, &market_ids);

    problem
}

fn generate_stress_simple_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    market_ids: &[MarketId],
    market_prices: &[f64],
    market_outcomes: &[u8],
) -> Order {
    let id = *order_id;
    *order_id += 1;

    let market_idx = rng.gen_range(0..market_ids.len());
    let market = market_ids[market_idx];
    let outcomes = market_outcomes[market_idx];
    let outcome = rng.gen_range(0..outcomes);

    let base_price = if outcome == 0 {
        market_prices[market_idx]
    } else if outcomes == 2 {
        1.0 - market_prices[market_idx]
    } else {
        (1.0 - market_prices[market_idx]) / (outcomes as f64 - 1.0)
    };

    // Varying aggressiveness
    let aggressiveness = rng.gen_range(-0.05..0.15);
    let limit = (base_price + aggressiveness).clamp(0.05, 0.95);

    // Mix of quantities
    let qty: Qty = if rng.gen_bool(0.7) {
        rng.gen_range(20..80)
    } else {
        rng.gen_range(100..300) // Some large orders
    };

    outcome_buy(markets, id, market, outcome, price_to_nanos(limit), qty)
}

fn generate_stress_bundle_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    market_ids: &[MarketId],
    market_prices: &[f64],
) -> Order {
    let id = *order_id;
    *order_id += 1;

    // Only select binary markets for bundling to avoid exceeding MAX_STATES (32)
    // With binary markets, we can bundle up to 5 markets (2^5 = 32 states)
    let binary_markets: Vec<usize> = (0..market_ids.len())
        .filter(|&i| markets.num_outcomes(market_ids[i]) == 2)
        .collect();

    if binary_markets.is_empty() {
        // Fallback to simple order if no binary markets
        return outcome_buy(
            markets,
            id,
            market_ids[0],
            0,
            price_to_nanos(0.5),
            50,
        );
    }

    // Bundle 2-5 binary markets (max 5 to stay within 32 states)
    let max_bundle = binary_markets.len().min(5);
    let num_to_bundle = if max_bundle >= 2 {
        rng.gen_range(2..=max_bundle)
    } else {
        1
    };

    let mut selected = binary_markets.clone();
    selected.shuffle(rng);
    selected.truncate(num_to_bundle);

    if selected.len() < 2 {
        // Not enough binary markets, fallback to simple order
        let market_idx = selected.first().copied().unwrap_or(0);
        return outcome_buy(
            markets,
            id,
            market_ids[market_idx],
            0,
            price_to_nanos(market_prices[market_idx]),
            50,
        );
    }

    let bundle_markets: Vec<MarketId> = selected.iter().map(|&i| market_ids[i]).collect();
    let combined_prob: f64 = selected.iter().map(|&i| market_prices[i]).product();

    let limit = (combined_prob * rng.gen_range(0.8..1.3)).clamp(0.01, 0.95);
    let qty: Qty = rng.gen_range(15..60);

    bundle_yes(markets, id, &bundle_markets, price_to_nanos(limit), qty)
}

fn generate_stress_spread_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    market_ids: &[MarketId],
    market_prices: &[f64],
) -> Order {
    let id = *order_id;
    *order_id += 1;

    if market_ids.len() < 2 {
        return outcome_buy(
            markets,
            id,
            market_ids[0],
            0,
            price_to_nanos(0.5),
            50,
        );
    }

    let m1_idx = rng.gen_range(0..market_ids.len());
    let mut m2_idx = rng.gen_range(0..market_ids.len());
    while m2_idx == m1_idx {
        m2_idx = rng.gen_range(0..market_ids.len());
    }

    let market_a = market_ids[m1_idx];
    let market_b = market_ids[m2_idx];

    let price_diff = (market_prices[m1_idx] - market_prices[m2_idx]).abs();
    let limit = (price_diff + rng.gen_range(-0.05..0.15)).clamp(0.01, 0.5);
    let qty: Qty = rng.gen_range(25..80);

    spread(markets, id, market_a, market_b, price_to_nanos(limit), qty)
}

fn inject_stress_conflicts(problem: &mut Problem, rng: &mut StdRng, market_ids: &[MarketId]) {
    // Sort orders by potential welfare
    problem.orders.sort_by(|a, b| {
        let a_welfare = a.limit_price as u128 * a.max_fill as u128;
        let b_welfare = b.limit_price as u128 * b.max_fill as u128;
        b_welfare.cmp(&a_welfare)
    });

    // Make top orders compete for scarce liquidity
    let conflict_count = problem.orders.len() / 4;
    if conflict_count > 1 && !market_ids.is_empty() {
        // Pick a few "hot" markets with extra demand
        let num_hot_markets = (market_ids.len() / 5).max(2).min(5);
        let mut hot_markets: Vec<MarketId> = market_ids.to_vec();
        hot_markets.shuffle(rng);
        hot_markets.truncate(num_hot_markets);

        for order in problem.orders.iter_mut().take(conflict_count) {
            // Check if order touches a hot market
            let touches_hot = order.active_markets().any(|m| hot_markets.contains(&m));
            if touches_hot {
                // Increase demand to create conflicts
                order.max_fill = (order.max_fill as f64 * rng.gen_range(1.3..1.8)) as Qty;
                // Also increase limit price to make these orders more attractive
                order.limit_price = ((order.limit_price as f64) * rng.gen_range(1.05..1.15)) as u64;
            }
        }
    }
}

/// Generate a combined scenario from multiple existing scenarios.
///
/// This merges several scenario types into one large problem by:
/// - Generating each sub-scenario
/// - Remapping market IDs to avoid collisions
/// - Combining all orders and liquidity
pub fn generate_combined_scenario(seed: u64) -> Problem {
    let mut problem = Problem::new("Combined(presidential+tournament+random+nested)");

    // Keep track of market ID offset
    let mut market_id_offset = 0u32;
    let mut order_id = 1u64;

    // Generate presidential scenario
    let presidential = generate_presidential_scenario(PresidentialConfig {
        seed,
        ..Default::default()
    });
    merge_subproblem(&mut problem, presidential, &mut market_id_offset, &mut order_id);

    // Generate tournament scenario
    let tournament = generate_tournament_scenario(TournamentConfig {
        seed: seed + 1,
        num_teams: 8,
        ..Default::default()
    });
    merge_subproblem(&mut problem, tournament, &mut market_id_offset, &mut order_id);

    // Generate random hard scenario
    let random = generate_random_scenario(RandomConfig {
        seed: seed + 2,
        num_markets: 8,
        num_orders: 100,
        ..RandomConfig::hard()
    });
    merge_subproblem(&mut problem, random, &mut market_id_offset, &mut order_id);

    // Generate nested bundles
    let nested = generate_nested_bundle_scenario(NestedBundleConfig {
        seed: seed + 3,
        ..Default::default()
    });
    merge_subproblem(&mut problem, nested, &mut market_id_offset, &mut order_id);

    // Generate large interconnected
    let interconnected = generate_large_interconnected_scenario(LargeInterconnectedConfig {
        seed: seed + 4,
        num_markets: 12,
        num_orders: 80,
        ..Default::default()
    });
    merge_subproblem(&mut problem, interconnected, &mut market_id_offset, &mut order_id);

    // Update problem name with final stats
    problem.name = format!(
        "Combined(markets={}, orders={})",
        problem.markets.len(),
        problem.orders.len()
    );

    problem
}

/// Merge a sub-problem into the main problem.
fn merge_subproblem(
    main: &mut Problem,
    sub: Problem,
    market_id_offset: &mut u32,
    order_id: &mut u64,
) {
    // Create mapping from sub market IDs to new IDs
    let mut market_mapping = std::collections::HashMap::new();

    for market in sub.markets.iter() {
        let old_id = market.id;
        let new_id = MarketId::new(*market_id_offset);
        *market_id_offset += 1;

        // Add market to main problem
        let outcomes: Vec<String> = market.outcomes.iter().cloned().collect();
        let created_id = main.markets.add(&market.name, outcomes);
        market_mapping.insert(old_id, created_id);
    }

    // Copy liquidity with remapped market IDs
    for (&(old_market, outcome), book) in sub.liquidity.books.iter() {
        if let Some(&new_market) = market_mapping.get(&old_market) {
            for level in book.asks() {
                main.liquidity.add_ask(new_market, outcome, level.price, level.available_qty);
            }
            for level in book.bids() {
                main.liquidity.add_bid(new_market, outcome, level.price, level.available_qty);
            }
        }
    }

    // Copy orders with new IDs and remapped market IDs
    for order in sub.orders {
        let new_id = *order_id;
        *order_id += 1;

        // Create new order with remapped markets
        let mut new_order = order.clone();
        new_order.id = new_id;

        for i in 0..new_order.num_markets as usize {
            let old_market = new_order.markets[i];
            if !old_market.is_none() {
                if let Some(&new_market) = market_mapping.get(&old_market) {
                    new_order.markets[i] = new_market;
                }
            }
        }

        main.orders.push(new_order);
    }

    // Note: Constraints are not merged as they reference old market IDs
    // and may not make sense across different scenarios
}

/// Configuration for MILP-killer scenarios designed to force MILP timeout.
///
/// Key insight: MILP struggles with:
/// - High all-or-none fraction (binary variables)
/// - Hot markets with severe scarcity (creates competing solutions)
/// - Deep constraint chains (complex branch-and-bound)
#[derive(Clone, Debug)]
pub struct MilpKillerConfig {
    /// Random seed
    pub seed: u64,
    /// Number of markets (100-200)
    pub num_markets: usize,
    /// Number of orders (10000-50000)
    pub num_orders: usize,
    /// Fraction of all-or-none orders (0.4-0.6 creates more binary vars)
    pub aon_fraction: f64,
    /// Fraction of multi-market bundle orders (0.3-0.5)
    pub bundle_fraction: f64,
    /// Liquidity scarcity factor (0.15-0.25 is severe)
    pub liquidity_scarcity: f64,
    /// Fraction of markets that are "hot" (10% get 80% demand)
    pub hot_market_fraction: f64,
    /// Depth of implication chains (5-10 deep)
    pub implication_chains: usize,
    /// Number of mutual exclusion groups (20-50)
    pub exclusion_groups: usize,
}

impl Default for MilpKillerConfig {
    fn default() -> Self {
        Self::timeout_guaranteed()
    }
}

impl MilpKillerConfig {
    /// Configuration guaranteed to cause MILP timeout on most systems.
    /// 10k orders, 100 markets.
    pub fn timeout_guaranteed() -> Self {
        Self {
            seed: 42,
            num_markets: 100,
            num_orders: 10000,
            aon_fraction: 0.45,
            bundle_fraction: 0.35,
            liquidity_scarcity: 0.2,
            hot_market_fraction: 0.1,
            implication_chains: 8,
            exclusion_groups: 30,
        }
    }

    /// Extreme configuration: 50k orders, 200 markets.
    pub fn extreme() -> Self {
        Self {
            seed: 42,
            num_markets: 200,
            num_orders: 50000,
            aon_fraction: 0.5,
            bundle_fraction: 0.4,
            liquidity_scarcity: 0.15,
            hot_market_fraction: 0.1,
            implication_chains: 10,
            exclusion_groups: 50,
        }
    }

    /// Smaller config for faster testing (still hard for MILP).
    pub fn test() -> Self {
        Self {
            seed: 42,
            num_markets: 50,
            num_orders: 3000,
            aon_fraction: 0.4,
            bundle_fraction: 0.3,
            liquidity_scarcity: 0.25,
            hot_market_fraction: 0.1,
            implication_chains: 5,
            exclusion_groups: 15,
        }
    }
}

/// Generate a MILP-killer scenario designed to force solver timeout.
///
/// This scenario maximizes problem complexity for MILP solvers:
/// - High all-or-none fraction creates many binary variables
/// - Severe liquidity scarcity creates competing solutions
/// - Deep implication chains make branch-and-bound expensive
/// - Hot markets concentrate demand, creating conflicts
pub fn generate_milp_killer_scenario(config: MilpKillerConfig) -> Problem {
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut problem = Problem::new(format!(
        "MilpKiller(markets={}, orders={}, aon={}%, bundles={}%, liq={}%)",
        config.num_markets,
        config.num_orders,
        (config.aon_fraction * 100.0) as i32,
        (config.bundle_fraction * 100.0) as i32,
        (config.liquidity_scarcity * 100.0) as i32
    ));

    // Create markets (all binary for simplicity)
    let mut market_ids: Vec<MarketId> = Vec::new();
    let mut market_prices: Vec<f64> = Vec::new();

    for i in 0..config.num_markets {
        let market = problem.markets.add(format!("M{}", i), vec!["Yes".to_string(), "No".to_string()]);
        market_ids.push(market);
        market_prices.push(rng.gen_range(0.2..0.8));
    }

    // Identify "hot" markets (10% get 80% of demand)
    let num_hot = (config.num_markets as f64 * config.hot_market_fraction).max(1.0) as usize;
    let mut hot_markets: Vec<MarketId> = market_ids.clone();
    hot_markets.shuffle(&mut rng);
    hot_markets.truncate(num_hot);

    // Add deep implication chains (A→B→C→D→E)
    // This creates complex constraint propagation for MILP
    let mut constraint_builder = ConstraintBuilder::new();

    for chain in 0..config.implication_chains {
        // Create a chain of length 5-10
        let chain_length = rng.gen_range(5..=10.min(config.num_markets));
        let mut chain_markets: Vec<usize> = (0..market_ids.len()).collect();
        chain_markets.shuffle(&mut rng);
        chain_markets.truncate(chain_length);

        // Create implication chain: A→B→C→...
        for i in 0..chain_markets.len() - 1 {
            let m1 = market_ids[chain_markets[i]];
            let m2 = market_ids[chain_markets[i + 1]];
            constraint_builder = constraint_builder.implies(m1, 0, m2, 0);
        }
    }

    // Add mutual exclusion groups
    for _ in 0..config.exclusion_groups {
        let group_size = rng.gen_range(2..=4);
        let mut group_markets: Vec<usize> = (0..market_ids.len()).collect();
        group_markets.shuffle(&mut rng);
        group_markets.truncate(group_size);

        let outcomes: Vec<(MarketId, u8)> = group_markets
            .iter()
            .map(|&i| (market_ids[i], 0))
            .collect();
        constraint_builder = constraint_builder.mutually_exclusive(outcomes);
    }

    problem.constraints = constraint_builder.build();

    // Add liquidity with severe scarcity
    let avg_order_qty = 50u64;
    let total_demand_estimate = config.num_orders as u64 * avg_order_qty;
    let total_supply = (total_demand_estimate as f64 * config.liquidity_scarcity) as Qty;
    let supply_per_market = total_supply / config.num_markets as Qty;

    // Hot markets get less liquidity (creates more competition)
    for (i, &market) in market_ids.iter().enumerate() {
        let mid_price = market_prices[i];
        let is_hot = hot_markets.contains(&market);
        let market_supply = if is_hot {
            supply_per_market / 3 // Hot markets get 1/3 the supply
        } else {
            supply_per_market
        };

        for outcome in 0..2u8 {
            let outcome_price = if outcome == 0 { mid_price } else { 1.0 - mid_price };

            // Multiple price levels
            for level in 0..4 {
                let offset = 0.08 * (level as f64 + 1.0) / 4.0;
                let level_supply = market_supply / 8;

                let ask_price = (outcome_price + offset).min(0.98);
                problem.liquidity.add_ask(
                    market,
                    outcome,
                    price_to_nanos(ask_price),
                    level_supply.max(5),
                );

                let bid_price = (outcome_price - offset).max(0.02);
                problem.liquidity.add_bid(
                    market,
                    outcome,
                    price_to_nanos(bid_price),
                    level_supply.max(5),
                );
            }
        }
    }

    // Generate orders
    let num_bundles = (config.num_orders as f64 * config.bundle_fraction) as usize;
    let num_aon_single = (config.num_orders as f64 * config.aon_fraction * 0.7) as usize;
    let num_simple = config.num_orders - num_bundles - num_aon_single;

    let mut order_id = 1u64;

    // Simple orders (mix of regular and AON)
    for _ in 0..num_simple {
        let order = generate_milp_killer_simple_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_ids,
            &market_prices,
            &hot_markets,
            false, // not AON
        );
        problem.orders.push(order);
    }

    // All-or-none simple orders (creates binary variables)
    for _ in 0..num_aon_single {
        let order = generate_milp_killer_simple_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_ids,
            &market_prices,
            &hot_markets,
            true, // AON
        );
        problem.orders.push(order);
    }

    // Bundle orders (multi-market)
    for _ in 0..num_bundles {
        let order = generate_milp_killer_bundle_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_ids,
            &market_prices,
            &hot_markets,
        );
        problem.orders.push(order);
    }

    // Shuffle to avoid order-dependent behavior
    problem.orders.shuffle(&mut rng);

    problem
}

fn generate_milp_killer_simple_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    market_ids: &[MarketId],
    market_prices: &[f64],
    hot_markets: &[MarketId],
    is_aon: bool,
) -> Order {
    let id = *order_id;
    *order_id += 1;

    // 70% chance to target hot markets
    let market_idx = if rng.gen_bool(0.7) && !hot_markets.is_empty() {
        let hot_idx = rng.gen_range(0..hot_markets.len());
        market_ids.iter().position(|&m| m == hot_markets[hot_idx]).unwrap_or(0)
    } else {
        rng.gen_range(0..market_ids.len())
    };

    let market = market_ids[market_idx];
    let outcome = rng.gen_range(0..2u8);
    let base_price = if outcome == 0 {
        market_prices[market_idx]
    } else {
        1.0 - market_prices[market_idx]
    };

    let aggressiveness = rng.gen_range(-0.05..0.2);
    let limit = (base_price + aggressiveness).clamp(0.05, 0.95);

    let qty: Qty = if is_aon {
        // AON orders tend to be larger (harder to fill)
        rng.gen_range(30..100)
    } else {
        rng.gen_range(10..60)
    };

    let mut order = outcome_buy(markets, id, market, outcome, price_to_nanos(limit), qty);
    if is_aon {
        order.min_fill = order.max_fill; // All-or-none
    }
    order
}

fn generate_milp_killer_bundle_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    market_ids: &[MarketId],
    market_prices: &[f64],
    hot_markets: &[MarketId],
) -> Order {
    let id = *order_id;
    *order_id += 1;

    // Bundle 2-4 markets (limited to stay under 32 states)
    let num_to_bundle = rng.gen_range(2..=4);
    let mut selected: Vec<usize> = Vec::new();

    // 60% chance each slot includes a hot market
    for _ in 0..num_to_bundle {
        let idx = if rng.gen_bool(0.6) && !hot_markets.is_empty() {
            let hot_idx = rng.gen_range(0..hot_markets.len());
            market_ids.iter().position(|&m| m == hot_markets[hot_idx]).unwrap_or(0)
        } else {
            rng.gen_range(0..market_ids.len())
        };
        if !selected.contains(&idx) {
            selected.push(idx);
        }
    }

    if selected.len() < 2 {
        // Fallback to simple order
        selected = vec![rng.gen_range(0..market_ids.len()), rng.gen_range(0..market_ids.len())];
        selected.dedup();
        if selected.len() < 2 {
            selected.push((selected[0] + 1) % market_ids.len());
        }
    }

    let bundle_markets: Vec<MarketId> = selected.iter().map(|&i| market_ids[i]).collect();
    let combined_prob: f64 = selected.iter().map(|&i| market_prices[i]).product();

    let limit = (combined_prob * rng.gen_range(0.8..1.4)).clamp(0.01, 0.95);
    let qty: Qty = rng.gen_range(10..50);

    let mut order = bundle_yes(markets, id, &bundle_markets, price_to_nanos(limit), qty);

    // 50% of bundles are all-or-none
    if rng.gen_bool(0.5) {
        order.min_fill = order.max_fill;
    }

    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mega_scenario_small() {
        let problem = generate_mega_scenario(MegaScenarioConfig::small());
        assert!(problem.markets.len() >= 20);
        assert!(problem.orders.len() >= 500);
    }

    #[test]
    fn test_mega_scenario_default() {
        let problem = generate_mega_scenario(MegaScenarioConfig::default());
        assert!(problem.markets.len() >= 30);
        assert!(problem.orders.len() >= 1000);
    }

    #[test]
    fn test_combined_scenario() {
        let problem = generate_combined_scenario(42);
        // Should have orders from all sub-scenarios
        assert!(problem.orders.len() > 100);
        assert!(problem.markets.len() > 10);
    }

    #[test]
    fn test_mega_has_bundles() {
        let config = MegaScenarioConfig {
            bundle_fraction: 0.5,
            ..MegaScenarioConfig::small()
        };
        let problem = generate_mega_scenario(config);

        // Check that some orders are bundles (multi-market)
        let bundle_count = problem.orders.iter().filter(|o| o.num_markets > 1).count();
        assert!(bundle_count > 0);
    }

    #[test]
    fn test_milp_killer_test_config() {
        let problem = generate_milp_killer_scenario(MilpKillerConfig::test());
        assert!(problem.markets.len() >= 50);
        assert!(problem.orders.len() >= 3000);
        // Should have significant AON orders
        let aon_count = problem.orders.iter().filter(|o| o.is_all_or_none()).count();
        assert!(aon_count > 1000, "Expected many AON orders for MILP complexity");
        // Should have bundles
        let bundle_count = problem.orders.iter().filter(|o| o.num_markets > 1).count();
        assert!(bundle_count > 500, "Expected many bundle orders");
        // Should have constraints
        assert!(problem.constraints.len() > 0);
    }

    #[test]
    fn test_milp_killer_has_hot_markets() {
        let config = MilpKillerConfig::test();
        let problem = generate_milp_killer_scenario(config);
        // Problem should be generated with scarcity
        let summary = problem.summary();
        assert!(summary.aon_orders > 0);
    }
}
