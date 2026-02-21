//! Unified scenario generator for testing and benchmarking.
//!
//! This module provides a single comprehensive scenario generator with configurable
//! parameters for all testing needs: quick tests, stress tests, MILP-killer scenarios, etc.

use std::collections::HashMap;

use rand::prelude::*;
use rand_chacha::ChaCha8Rng;

use matching_engine::{
    bundle_sell, bundle_yes, outcome_buy, outcome_sell, price_to_nanos, spread, spread_sell,
    JointOutcome, MarketGroup, MarketId, MmConstraint, MmId, MmSide, Nanos, Order, Problem, Qty,
    NANOS_PER_DOLLAR, YES,
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
    /// Fraction of orders that are bundles (multi-market)
    pub bundle_fraction: f64,
    /// Fraction of orders that are spreads (A - B)
    pub spread_fraction: f64,
    /// Fraction of bundle orders that are sells (counterparties to bundle_yes)
    pub bundle_sell_fraction: f64,
    /// Fraction of spread orders that are sells (counterparties to spread)
    pub spread_sell_fraction: f64,
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
            bundle_fraction: 0.15,
            spread_fraction: 0.05,
            bundle_sell_fraction: 0.4,
            spread_sell_fraction: 0.4,
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
    /// Quick test scenario (~300 orders, fast to run)
    pub fn quick() -> Self {
        Self {
            num_markets: 5,
            num_orders: 50,
            bundle_fraction: 0.1,
            spread_fraction: 0.0,

            num_mms: 0,
            liquidity_scarcity: 0.8,
            ..Default::default()
        }
    }

    /// Small scenario for unit tests (~500 orders)
    pub fn small() -> Self {
        Self {
            num_markets: 10,
            num_orders: 300,
            bundle_fraction: 0.15,
            spread_fraction: 0.05,

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
            bundle_fraction: 0.15,
            spread_fraction: 0.05,

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
            bundle_fraction: 0.2,
            spread_fraction: 0.05,

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
            bundle_fraction: 0.2,
            spread_fraction: 0.05,

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
            bundle_fraction: 0.45, // High bundle fraction = many binary variables
            spread_fraction: 0.0,
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
        "Scenario(m={},o={},b={:.0}%,s={:.0}%)",
        config.num_markets,
        config.num_orders,
        config.bundle_fraction * 100.0,
        config.spread_fraction * 100.0,
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

    // Build market → group_id map (for avoiding impossible cross-group bundles)
    let mut market_group_map: HashMap<MarketId, usize> = HashMap::new();
    for (gid, group) in problem.market_groups.iter().enumerate() {
        for &mid in &group.markets {
            market_group_map.insert(mid, gid);
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
    let num_bundles = (config.num_orders as f64 * config.bundle_fraction) as usize;
    let num_bundle_sells = (num_bundles as f64 * config.bundle_sell_fraction) as usize;
    let num_bundle_buys = num_bundles - num_bundle_sells;
    let num_spreads = (config.num_orders as f64 * config.spread_fraction) as usize;
    let num_spread_sells = (num_spreads as f64 * config.spread_sell_fraction) as usize;
    let num_spread_buys = num_spreads - num_spread_sells;
    let num_simple = config.num_orders - num_bundles - num_spreads;

    // Simple orders
    for _ in 0..num_simple {
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

    // Bundle buy orders - track joint outcomes for liquidity
    let mut bundle_outcomes: HashMap<JointOutcome, (Qty, Nanos)> = HashMap::new();
    for _ in 0..num_bundle_buys {
        let (order, joint_outcome, limit_price) = generate_bundle_order_with_outcome(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_info,
            &market_group_map,
            &config,
        );
        // Track demand for this joint outcome
        let entry = bundle_outcomes.entry(joint_outcome).or_insert((0, 0));
        entry.0 += order.max_fill;
        entry.1 = entry.1.max(limit_price);
        problem.orders.push(order);
    }

    // Bundle sell orders
    for _ in 0..num_bundle_sells {
        let order = generate_bundle_sell_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_info,
            &market_group_map,
            &config,
        );
        problem.orders.push(order);
    }

    // Add joint liquidity for bundle outcomes
    add_joint_liquidity(&mut problem, &bundle_outcomes, &config, &mut rng);

    // Spread buy orders
    for _ in 0..num_spread_buys {
        let order = generate_spread_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_info,
            &config,
        );
        problem.orders.push(order);
    }

    // Spread sell orders
    for _ in 0..num_spread_sells {
        let order = generate_spread_sell_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_info,
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

fn generate_bundle_order_with_outcome(
    markets: &matching_engine::MarketSet,
    rng: &mut ChaCha8Rng,
    order_id: &mut u64,
    market_info: &[(MarketId, f64)],
    group_map: &HashMap<MarketId, usize>,
    config: &ScenarioConfig,
) -> (Order, JointOutcome, Nanos) {
    let id = *order_id;
    *order_id += 1;

    let max_bundle = market_info.len().min(4);
    if max_bundle < 2 {
        // Fallback to simple order
        let (market, price) = market_info[0];
        let limit_nanos = price_to_nanos(price);
        let order = outcome_buy(markets, id, market, 0, limit_nanos, 50);
        let joint_outcome = JointOutcome::new(vec![(market, YES)]);
        return (order, joint_outcome, limit_nanos);
    }

    let num_to_bundle = rng.random_range(2..=max_bundle);
    let selected = select_cross_group_markets(market_info, group_map, num_to_bundle, rng);

    let bundle_markets: Vec<MarketId> = selected.iter().map(|&i| market_info[i].0).collect();
    let combined_prob: f64 = selected.iter().map(|&i| market_info[i].1).product();

    let limit = (combined_prob * rng.random_range(0.8..1.3)).clamp(0.01, 0.95);
    let limit_nanos = price_to_nanos(limit);
    let qty = rng.random_range(config.order_size_min..config.order_size_max);

    let order = bundle_yes(markets, id, &bundle_markets, limit_nanos, qty);

    // Create joint outcome for all YES
    let joint_outcome = JointOutcome::new(bundle_markets.iter().map(|&m| (m, YES)).collect());

    (order, joint_outcome, limit_nanos)
}

/// Joint liquidity for bundles is no longer supported.
/// Bundle matching is order-vs-order only.
fn add_joint_liquidity(
    _problem: &mut Problem,
    _bundle_outcomes: &HashMap<JointOutcome, (Qty, Nanos)>,
    _config: &ScenarioConfig,
    _rng: &mut ChaCha8Rng,
) {
    // No-op: platform joint liquidity has been removed.
    // Bundle orders are matched against each other through the solver.
}

fn generate_spread_order(
    markets: &matching_engine::MarketSet,
    rng: &mut ChaCha8Rng,
    order_id: &mut u64,
    market_info: &[(MarketId, f64)],
    config: &ScenarioConfig,
) -> Order {
    let id = *order_id;
    *order_id += 1;

    if market_info.len() < 2 {
        let (market, price) = market_info[0];
        return outcome_buy(markets, id, market, 0, price_to_nanos(price), 50);
    }

    let m1_idx = rng.random_range(0..market_info.len());
    let mut m2_idx = rng.random_range(0..market_info.len());
    while m2_idx == m1_idx {
        m2_idx = rng.random_range(0..market_info.len());
    }

    // Ensure spread direction matches pricing: long the more expensive market
    let (market_a, price_a, market_b, price_b) = if market_info[m1_idx].1 >= market_info[m2_idx].1
    {
        (
            market_info[m1_idx].0,
            market_info[m1_idx].1,
            market_info[m2_idx].0,
            market_info[m2_idx].1,
        )
    } else {
        (
            market_info[m2_idx].0,
            market_info[m2_idx].1,
            market_info[m1_idx].0,
            market_info[m1_idx].1,
        )
    };

    let price_diff = price_a - price_b; // Always >= 0 now
    let limit = (price_diff + rng.random_range(-0.05..0.1)).clamp(0.01, 0.5);
    let qty = rng.random_range(config.order_size_min..config.order_size_max);

    spread(markets, id, market_a, market_b, price_to_nanos(limit), qty)
}

fn generate_bundle_sell_order(
    markets: &matching_engine::MarketSet,
    rng: &mut ChaCha8Rng,
    order_id: &mut u64,
    market_info: &[(MarketId, f64)],
    group_map: &HashMap<MarketId, usize>,
    config: &ScenarioConfig,
) -> Order {
    let id = *order_id;
    *order_id += 1;

    let max_bundle = market_info.len().min(4);
    if max_bundle < 2 {
        let (market, price) = market_info[0];
        return outcome_sell(markets, id, market, 0, price_to_nanos(price), 50);
    }

    let num_to_bundle = rng.random_range(2..=max_bundle);
    let selected = select_cross_group_markets(market_info, group_map, num_to_bundle, rng);

    let bundle_markets: Vec<MarketId> = selected.iter().map(|&i| market_info[i].0).collect();
    let combined_prob: f64 = selected.iter().map(|&i| market_info[i].1).product();

    // Seller wants to receive at least this much (lower limit = more aggressive seller)
    let limit = (combined_prob * rng.random_range(0.7..1.1)).clamp(0.01, 0.95);
    let limit_nanos = price_to_nanos(limit);
    let qty = rng.random_range(config.order_size_min..config.order_size_max);

    bundle_sell(markets, id, &bundle_markets, limit_nanos, qty)
}

fn generate_spread_sell_order(
    markets: &matching_engine::MarketSet,
    rng: &mut ChaCha8Rng,
    order_id: &mut u64,
    market_info: &[(MarketId, f64)],
    config: &ScenarioConfig,
) -> Order {
    let id = *order_id;
    *order_id += 1;

    if market_info.len() < 2 {
        let (market, price) = market_info[0];
        return outcome_sell(markets, id, market, 0, price_to_nanos(price), 50);
    }

    let m1_idx = rng.random_range(0..market_info.len());
    let mut m2_idx = rng.random_range(0..market_info.len());
    while m2_idx == m1_idx {
        m2_idx = rng.random_range(0..market_info.len());
    }

    // Spread sell is counterparty to spread buy: short A, long B
    // Ensure direction matches pricing: short the more expensive market
    let (market_a, price_a, market_b, price_b) = if market_info[m1_idx].1 >= market_info[m2_idx].1
    {
        (
            market_info[m1_idx].0,
            market_info[m1_idx].1,
            market_info[m2_idx].0,
            market_info[m2_idx].1,
        )
    } else {
        (
            market_info[m2_idx].0,
            market_info[m2_idx].1,
            market_info[m1_idx].0,
            market_info[m1_idx].1,
        )
    };

    let price_diff = price_a - price_b; // Always >= 0
    let limit = (price_diff + rng.random_range(-0.05..0.1)).clamp(0.01, 0.5);
    let qty = rng.random_range(config.order_size_min..config.order_size_max);

    spread_sell(markets, id, market_a, market_b, price_to_nanos(limit), qty)
}

/// Select `n` market indices ensuring no two markets from the same group.
/// Falls back to allowing same-group if not enough cross-group markets available.
fn select_cross_group_markets(
    market_info: &[(MarketId, f64)],
    group_map: &HashMap<MarketId, usize>,
    n: usize,
    rng: &mut ChaCha8Rng,
) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..market_info.len()).collect();
    indices.shuffle(rng);

    let mut selected = Vec::new();
    let mut used_groups = std::collections::HashSet::new();

    for &idx in &indices {
        if selected.len() >= n {
            break;
        }
        let mid = market_info[idx].0;
        if let Some(&gid) = group_map.get(&mid) {
            if used_groups.contains(&gid) {
                continue; // Skip markets in already-used groups
            }
            used_groups.insert(gid);
        }
        selected.push(idx);
    }

    // If we couldn't find enough cross-group markets, fill with any remaining
    if selected.len() < n {
        for &idx in &indices {
            if selected.len() >= n {
                break;
            }
            if !selected.contains(&idx) {
                selected.push(idx);
            }
        }
    }

    selected
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
