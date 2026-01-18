//! Realistic scenario generator demonstrating cross-market matching value.
//!
//! This module generates scenarios that showcase why cross-market matching is valuable:
//! - Bundle liquidity sharing (orders sharing markets can split scarce liquidity)
//! - Constraint arbitrage (implications create cheaper exposure paths)
//! - Complementary sets (bundles covering all states = guaranteed profit)
//! - Synergy groups (orders that enable each other)
//!
//! Unlike MilpKillerConfig which aims to force MILP timeout, this generator
//! creates realistic market structures with deliberate arbitrage opportunities.

use rand::Rng;
use rand::rngs::StdRng;
use rand::SeedableRng;
use rand::seq::SliceRandom;

use matching_engine::{
    ConstraintBuilder, MarketSet, Order, MarketId, Qty,
    price_to_nanos, outcome_buy, bundle_yes, spread, butterfly,
    OrderBuilder, ConditionDir,
};
use matching_engine::Problem;

/// Information about a market including its fair prices and cluster membership.
#[derive(Clone, Debug)]
pub struct MarketInfo {
    pub id: MarketId,
    pub num_outcomes: u8,
    pub fair_prices: Vec<f64>,      // Fair price for each outcome
    pub cluster_id: Option<usize>,  // Which cluster this market belongs to
    pub cluster_depth: u8,          // Depth in cluster hierarchy (0 = root)
}

/// A cluster of related markets with implication constraints.
#[derive(Clone, Debug)]
pub struct MarketCluster {
    pub name: String,
    pub markets: Vec<MarketId>,
    pub cluster_type: ClusterType,
}

/// Type of market cluster determining the constraint structure.
#[derive(Clone, Debug)]
pub enum ClusterType {
    /// Election: Winner(4) → Party(2) → Chamber(2)
    Election,
    /// Tournament: Champion(4) → Finalist(2) → Semi(2)
    Tournament,
    /// Economic: Related 3-way markets (GDP, Rate, Inflation)
    Economic,
    /// Generic chain of implications
    Chain(usize),
}

/// Configuration for realistic scenario generation.
#[derive(Clone, Debug)]
pub struct RealisticConfig {
    // Scale
    pub seed: u64,
    pub num_orders: usize,
    pub num_markets: usize,

    // Market structure (binary, 3-way, 4-way, 5-way fractions)
    pub market_type_distribution: (f64, f64, f64, f64),
    pub num_market_clusters: usize,

    // Order type distribution
    pub simple_fraction: f64,
    pub bundle_fraction: f64,
    pub spread_fraction: f64,
    pub butterfly_fraction: f64,
    pub conditional_fraction: f64,

    // Cross-cluster orders (fraction of bundles that span multiple clusters)
    // These create "bridging" orders that complicate decomposition solvers
    pub cross_cluster_bundle_fraction: f64,

    // Constraints
    pub aon_fraction: f64,
    pub liquidity_scarcity: f64,

    // Planted opportunities
    pub planted_bundle_arbitrages: usize,
    pub planted_chain_arbitrages: usize,
    pub planted_complement_sets: usize,
    pub planted_synergy_groups: usize,
}

impl Default for RealisticConfig {
    fn default() -> Self {
        Self::standard()
    }
}

impl RealisticConfig {
    /// Test configuration: 10k orders, 100 markets (~30s)
    pub fn test() -> Self {
        Self {
            seed: 42,
            num_orders: 10_000,
            num_markets: 100,
            market_type_distribution: (0.60, 0.25, 0.10, 0.05),
            num_market_clusters: 15,
            simple_fraction: 0.40,
            bundle_fraction: 0.35,
            spread_fraction: 0.10,
            butterfly_fraction: 0.05,
            conditional_fraction: 0.10,
            cross_cluster_bundle_fraction: 0.30,  // 30% of bundles cross clusters
            aon_fraction: 0.15,
            liquidity_scarcity: 0.25,
            planted_bundle_arbitrages: 25,
            planted_chain_arbitrages: 15,
            planted_complement_sets: 10,
            planted_synergy_groups: 20,
        }
    }

    /// Standard configuration: 50k orders, 200 markets
    pub fn standard() -> Self {
        Self {
            seed: 42,
            num_orders: 50_000,
            num_markets: 200,
            market_type_distribution: (0.60, 0.25, 0.10, 0.05),
            num_market_clusters: 30,
            simple_fraction: 0.40,
            bundle_fraction: 0.35,
            spread_fraction: 0.10,
            butterfly_fraction: 0.05,
            conditional_fraction: 0.10,
            cross_cluster_bundle_fraction: 0.30,
            aon_fraction: 0.15,
            liquidity_scarcity: 0.25,
            planted_bundle_arbitrages: 50,
            planted_chain_arbitrages: 30,
            planted_complement_sets: 20,
            planted_synergy_groups: 40,
        }
    }

    /// Extreme configuration: 100k orders, 400 markets
    pub fn extreme() -> Self {
        Self {
            seed: 42,
            num_orders: 100_000,
            num_markets: 400,
            market_type_distribution: (0.60, 0.25, 0.10, 0.05),
            num_market_clusters: 60,
            simple_fraction: 0.40,
            bundle_fraction: 0.35,
            spread_fraction: 0.10,
            butterfly_fraction: 0.05,
            conditional_fraction: 0.10,
            cross_cluster_bundle_fraction: 0.30,
            aon_fraction: 0.15,
            liquidity_scarcity: 0.20,
            planted_bundle_arbitrages: 100,
            planted_chain_arbitrages: 60,
            planted_complement_sets: 40,
            planted_synergy_groups: 80,
        }
    }

    /// Cross-market demo: High bundle fraction + high cross-cluster to showcase value
    pub fn cross_market_demo() -> Self {
        Self {
            bundle_fraction: 0.50,
            simple_fraction: 0.25,
            cross_cluster_bundle_fraction: 0.50,  // Half of bundles cross clusters!
            planted_bundle_arbitrages: 100,
            planted_chain_arbitrages: 50,
            planted_complement_sets: 30,
            planted_synergy_groups: 60,
            ..Self::standard()
        }
    }

    /// Small configuration for quick testing
    pub fn small() -> Self {
        Self {
            seed: 42,
            num_orders: 3_000,
            num_markets: 50,
            market_type_distribution: (0.60, 0.25, 0.10, 0.05),
            num_market_clusters: 8,
            simple_fraction: 0.40,
            bundle_fraction: 0.35,
            spread_fraction: 0.10,
            butterfly_fraction: 0.05,
            conditional_fraction: 0.10,
            cross_cluster_bundle_fraction: 0.30,
            aon_fraction: 0.15,
            liquidity_scarcity: 0.30,
            planted_bundle_arbitrages: 10,
            planted_chain_arbitrages: 8,
            planted_complement_sets: 5,
            planted_synergy_groups: 10,
        }
    }
}

/// Generate a realistic scenario demonstrating cross-market matching value.
pub fn generate_realistic_scenario(config: RealisticConfig) -> Problem {
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut problem = Problem::new(format!(
        "Realistic(markets={}, orders={}, bundles={}%, aon={}%)",
        config.num_markets,
        config.num_orders,
        (config.bundle_fraction * 100.0) as i32,
        (config.aon_fraction * 100.0) as i32,
    ));

    // Step 1: Create markets with varying outcomes
    let (market_infos, clusters) = create_markets_and_clusters(&mut problem, &config, &mut rng);

    // Step 2: Add constraints from clusters
    let mut constraint_builder = ConstraintBuilder::new();
    for cluster in &clusters {
        constraint_builder = add_cluster_constraints(constraint_builder, cluster);
    }
    problem.constraints = constraint_builder.build();

    // Step 3: Add liquidity
    add_liquidity(&mut problem, &market_infos, &config, &mut rng);

    // Step 4: Calculate order counts
    let num_simple = (config.num_orders as f64 * config.simple_fraction) as usize;
    let num_bundles = (config.num_orders as f64 * config.bundle_fraction) as usize;
    let num_spreads = (config.num_orders as f64 * config.spread_fraction) as usize;
    let num_butterflies = (config.num_orders as f64 * config.butterfly_fraction) as usize;
    let num_conditionals = config.num_orders - num_simple - num_bundles - num_spreads - num_butterflies;

    let mut order_id = 1u64;

    // Step 5: Generate simple orders
    for _ in 0..num_simple {
        let order = generate_simple_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_infos,
            config.aon_fraction,
        );
        problem.orders.push(order);
    }

    // Step 6: Generate bundle orders (some cross-cluster)
    for _ in 0..num_bundles {
        let order = generate_bundle_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_infos,
            config.aon_fraction,
            config.cross_cluster_bundle_fraction,
        );
        problem.orders.push(order);
    }

    // Step 7: Generate spread orders
    for _ in 0..num_spreads {
        let order = generate_spread_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_infos,
        );
        problem.orders.push(order);
    }

    // Step 8: Generate butterfly orders (on 3+ outcome markets)
    let multi_outcome_markets: Vec<&MarketInfo> = market_infos
        .iter()
        .filter(|m| m.num_outcomes >= 3)
        .collect();

    for _ in 0..num_butterflies {
        if let Some(order) = generate_butterfly_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &multi_outcome_markets,
        ) {
            problem.orders.push(order);
        }
    }

    // Step 9: Generate conditional orders
    for _ in 0..num_conditionals {
        let order = generate_conditional_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_infos,
        );
        problem.orders.push(order);
    }

    // Step 10: Plant arbitrage opportunities
    plant_bundle_arbitrages(&mut problem, &mut order_id, &market_infos, &config, &mut rng);
    plant_chain_arbitrages(&mut problem, &mut order_id, &clusters, &market_infos, &config, &mut rng);
    plant_complement_sets(&mut problem, &mut order_id, &market_infos, &config, &mut rng);
    plant_synergy_groups(&mut problem, &mut order_id, &market_infos, &config, &mut rng);

    // Shuffle orders to avoid order-dependent behavior
    problem.orders.shuffle(&mut rng);

    problem
}

/// Create markets with varying outcomes and organize into clusters.
fn create_markets_and_clusters(
    problem: &mut Problem,
    config: &RealisticConfig,
    rng: &mut StdRng,
) -> (Vec<MarketInfo>, Vec<MarketCluster>) {
    let mut market_infos = Vec::new();
    let mut clusters = Vec::new();

    let (binary_frac, three_way_frac, four_way_frac, _five_way_frac) = config.market_type_distribution;

    // Create clusters first
    for cluster_idx in 0..config.num_market_clusters {
        let cluster_type = match cluster_idx % 4 {
            0 => ClusterType::Election,
            1 => ClusterType::Tournament,
            2 => ClusterType::Economic,
            _ => ClusterType::Chain(rng.gen_range(3..=5)),
        };

        let cluster = create_cluster(problem, &mut market_infos, cluster_idx, &cluster_type, rng);
        clusters.push(cluster);
    }

    // Fill remaining markets as independent
    let markets_in_clusters = market_infos.len();
    let remaining = config.num_markets.saturating_sub(markets_in_clusters);

    for i in 0..remaining {
        let roll = rng.gen::<f64>();
        let num_outcomes = if roll < binary_frac {
            2
        } else if roll < binary_frac + three_way_frac {
            3
        } else if roll < binary_frac + three_way_frac + four_way_frac {
            4
        } else {
            5
        };

        let outcome_names: Vec<String> = (0..num_outcomes).map(|j| format!("O{}", j)).collect();
        let market_id = problem.markets.add(format!("Ind{}", i), outcome_names);

        // Generate fair prices
        let fair_prices = generate_fair_prices(num_outcomes, rng);

        market_infos.push(MarketInfo {
            id: market_id,
            num_outcomes,
            fair_prices,
            cluster_id: None,
            cluster_depth: 0,
        });
    }

    (market_infos, clusters)
}

/// Create a market cluster based on type.
fn create_cluster(
    problem: &mut Problem,
    market_infos: &mut Vec<MarketInfo>,
    cluster_idx: usize,
    cluster_type: &ClusterType,
    rng: &mut StdRng,
) -> MarketCluster {
    let mut cluster_market_ids = Vec::new();

    match cluster_type {
        ClusterType::Election => {
            // Winner(4) → Party(2) → Chamber(2)
            let winner = problem.markets.add(
                format!("Elect{}_Winner", cluster_idx),
                vec!["CandA".into(), "CandB".into(), "CandC".into(), "CandD".into()],
            );
            let party = problem.markets.add(
                format!("Elect{}_Party", cluster_idx),
                vec!["PartyX".into(), "PartyY".into()],
            );
            let chamber = problem.markets.add(
                format!("Elect{}_Chamber", cluster_idx),
                vec!["Majority".into(), "Minority".into()],
            );

            cluster_market_ids.extend([winner, party, chamber]);

            market_infos.push(MarketInfo {
                id: winner,
                num_outcomes: 4,
                fair_prices: generate_fair_prices(4, rng),
                cluster_id: Some(cluster_idx),
                cluster_depth: 0,
            });
            market_infos.push(MarketInfo {
                id: party,
                num_outcomes: 2,
                fair_prices: generate_fair_prices(2, rng),
                cluster_id: Some(cluster_idx),
                cluster_depth: 1,
            });
            market_infos.push(MarketInfo {
                id: chamber,
                num_outcomes: 2,
                fair_prices: generate_fair_prices(2, rng),
                cluster_id: Some(cluster_idx),
                cluster_depth: 2,
            });
        }
        ClusterType::Tournament => {
            // Champion(4) → Finalist(2) → Semi(2)
            let champion = problem.markets.add(
                format!("Tourn{}_Champion", cluster_idx),
                vec!["TeamA".into(), "TeamB".into(), "TeamC".into(), "TeamD".into()],
            );
            let finalist = problem.markets.add(
                format!("Tourn{}_Finalist", cluster_idx),
                vec!["Yes".into(), "No".into()],
            );
            let semi = problem.markets.add(
                format!("Tourn{}_Semi", cluster_idx),
                vec!["Yes".into(), "No".into()],
            );

            cluster_market_ids.extend([champion, finalist, semi]);

            market_infos.push(MarketInfo {
                id: champion,
                num_outcomes: 4,
                fair_prices: generate_fair_prices(4, rng),
                cluster_id: Some(cluster_idx),
                cluster_depth: 0,
            });
            market_infos.push(MarketInfo {
                id: finalist,
                num_outcomes: 2,
                fair_prices: generate_fair_prices(2, rng),
                cluster_id: Some(cluster_idx),
                cluster_depth: 1,
            });
            market_infos.push(MarketInfo {
                id: semi,
                num_outcomes: 2,
                fair_prices: generate_fair_prices(2, rng),
                cluster_id: Some(cluster_idx),
                cluster_depth: 2,
            });
        }
        ClusterType::Economic => {
            // GDP(3), FedRate(3), Inflation(3) - related but not strictly implied
            let gdp = problem.markets.add(
                format!("Econ{}_GDP", cluster_idx),
                vec!["Growth".into(), "Stable".into(), "Decline".into()],
            );
            let rate = problem.markets.add(
                format!("Econ{}_FedRate", cluster_idx),
                vec!["Raise".into(), "Hold".into(), "Cut".into()],
            );
            let inflation = problem.markets.add(
                format!("Econ{}_Inflation", cluster_idx),
                vec!["High".into(), "Medium".into(), "Low".into()],
            );

            cluster_market_ids.extend([gdp, rate, inflation]);

            market_infos.push(MarketInfo {
                id: gdp,
                num_outcomes: 3,
                fair_prices: generate_fair_prices(3, rng),
                cluster_id: Some(cluster_idx),
                cluster_depth: 0,
            });
            market_infos.push(MarketInfo {
                id: rate,
                num_outcomes: 3,
                fair_prices: generate_fair_prices(3, rng),
                cluster_id: Some(cluster_idx),
                cluster_depth: 1,
            });
            market_infos.push(MarketInfo {
                id: inflation,
                num_outcomes: 3,
                fair_prices: generate_fair_prices(3, rng),
                cluster_id: Some(cluster_idx),
                cluster_depth: 2,
            });
        }
        ClusterType::Chain(len) => {
            // Generic chain: M0 → M1 → M2 → ...
            for depth in 0..*len {
                let market = problem.markets.add(
                    format!("Chain{}_L{}", cluster_idx, depth),
                    vec!["Yes".into(), "No".into()],
                );
                cluster_market_ids.push(market);

                market_infos.push(MarketInfo {
                    id: market,
                    num_outcomes: 2,
                    fair_prices: generate_fair_prices(2, rng),
                    cluster_id: Some(cluster_idx),
                    cluster_depth: depth as u8,
                });
            }
        }
    }

    MarketCluster {
        name: format!("Cluster{}", cluster_idx),
        markets: cluster_market_ids,
        cluster_type: cluster_type.clone(),
    }
}

/// Add constraints based on cluster type.
fn add_cluster_constraints(
    mut builder: ConstraintBuilder,
    cluster: &MarketCluster,
) -> ConstraintBuilder {
    if cluster.markets.len() < 2 {
        return builder;
    }

    match &cluster.cluster_type {
        ClusterType::Election | ClusterType::Tournament => {
            // Winner outcome 0 or 1 → Party outcome 0
            // This creates: if specific candidate wins → their party wins
            if cluster.markets.len() >= 2 {
                let winner = cluster.markets[0];
                let party = cluster.markets[1];
                // CandA or CandB wins → PartyX wins
                builder = builder.implies(winner, 0, party, 0);
                builder = builder.implies(winner, 1, party, 0);
            }
            if cluster.markets.len() >= 3 {
                // Party winning → has chamber control (soft implication)
                let party = cluster.markets[1];
                let chamber = cluster.markets[2];
                builder = builder.implies(party, 0, chamber, 0);
            }
        }
        ClusterType::Economic => {
            // GDP growth → Fed likely to raise rates (soft correlation via mutual exclusion)
            if cluster.markets.len() >= 2 {
                let gdp = cluster.markets[0];
                let rate = cluster.markets[1];
                // If GDP is declining, rate cut is more likely (but not strict implication)
                // Instead, use mutual exclusion: GDP decline + rate raise is unlikely
                builder = builder.mutually_exclusive(vec![(gdp, 2), (rate, 0)]);
            }
        }
        ClusterType::Chain(len) => {
            // Simple chain: M0 → M1 → M2 → ...
            for i in 0..(*len - 1) {
                if i + 1 < cluster.markets.len() {
                    builder = builder.implies(cluster.markets[i], 0, cluster.markets[i + 1], 0);
                }
            }
        }
    }

    builder
}

/// Generate fair prices that sum to approximately 1.0.
fn generate_fair_prices(num_outcomes: u8, rng: &mut StdRng) -> Vec<f64> {
    let mut prices: Vec<f64> = (0..num_outcomes)
        .map(|_| rng.gen_range(0.1..0.9))
        .collect();

    // Normalize to sum to 1.0
    let sum: f64 = prices.iter().sum();
    for p in &mut prices {
        *p /= sum;
    }

    prices
}

/// Add liquidity to all markets.
fn add_liquidity(
    problem: &mut Problem,
    market_infos: &[MarketInfo],
    config: &RealisticConfig,
    _rng: &mut StdRng,  // Reserved for future randomization
) {
    let avg_order_qty = 50u64;
    let total_demand_estimate = config.num_orders as u64 * avg_order_qty;
    let total_supply = (total_demand_estimate as f64 * config.liquidity_scarcity) as Qty;
    let supply_per_market = total_supply / market_infos.len() as Qty;

    for market_info in market_infos {
        let market = market_info.id;
        let num_outcomes = market_info.num_outcomes;
        let market_supply = supply_per_market / num_outcomes as Qty;

        for (outcome_idx, &fair_price) in market_info.fair_prices.iter().enumerate() {
            let outcome = outcome_idx as u8;

            // Add multiple price levels around fair price
            for level in 0..4 {
                let offset = 0.06 * (level as f64 + 1.0) / 4.0;
                let level_supply = market_supply / 8;

                // Asks (sellers)
                let ask_price = (fair_price + offset).min(0.98);
                problem.liquidity.add_ask(
                    market,
                    outcome,
                    price_to_nanos(ask_price),
                    level_supply.max(5),
                );

                // Bids (buyers)
                let bid_price = (fair_price - offset).max(0.02);
                problem.liquidity.add_bid(
                    market,
                    outcome,
                    price_to_nanos(bid_price),
                    level_supply.max(5),
                );
            }
        }
    }
}

/// Generate a simple single-market order.
fn generate_simple_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    market_infos: &[MarketInfo],
    aon_fraction: f64,
) -> Order {
    let id = *order_id;
    *order_id += 1;

    let market_info = &market_infos[rng.gen_range(0..market_infos.len())];
    let outcome = rng.gen_range(0..market_info.num_outcomes);
    let fair_price = market_info.fair_prices[outcome as usize];

    // Price with some aggressiveness
    let aggressiveness = rng.gen_range(-0.05..0.15);
    let limit = (fair_price + aggressiveness).clamp(0.05, 0.95);

    let qty: Qty = rng.gen_range(10..80);
    let is_aon = rng.gen_bool(aon_fraction);

    let mut order = outcome_buy(markets, id, market_info.id, outcome, price_to_nanos(limit), qty);
    if is_aon {
        order.min_fill = order.max_fill;
    }
    order
}

/// Generate a bundle order spanning 2-5 binary markets.
/// With cross_cluster_fraction probability, deliberately picks markets from different clusters.
fn generate_bundle_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    market_infos: &[MarketInfo],
    aon_fraction: f64,
    cross_cluster_fraction: f64,
) -> Order {
    let id = *order_id;
    *order_id += 1;

    // Only select binary markets for bundling (to stay under 32 states)
    let binary_markets: Vec<&MarketInfo> = market_infos
        .iter()
        .filter(|m| m.num_outcomes == 2)
        .collect();

    if binary_markets.len() < 2 {
        // Fallback to simple order
        return generate_simple_order(markets, rng, &mut (*order_id - 1), market_infos, aon_fraction);
    }

    // Decide if this should be a cross-cluster bundle
    let is_cross_cluster = rng.gen_bool(cross_cluster_fraction);

    let selected: Vec<&MarketInfo> = if is_cross_cluster {
        // Deliberately pick markets from different clusters
        generate_cross_cluster_selection(&binary_markets, rng)
    } else {
        // Random selection (may or may not cross clusters)
        let max_bundle = binary_markets.len().min(5);
        let num_to_bundle = rng.gen_range(2..=max_bundle);
        let mut sel: Vec<&MarketInfo> = binary_markets.clone();
        sel.shuffle(rng);
        sel.truncate(num_to_bundle);
        sel
    };

    if selected.len() < 2 {
        // Fallback
        return generate_simple_order(markets, rng, &mut (*order_id - 1), market_infos, aon_fraction);
    }

    let bundle_market_ids: Vec<MarketId> = selected.iter().map(|m| m.id).collect();
    let combined_prob: f64 = selected.iter().map(|m| m.fair_prices[0]).product();

    let limit = (combined_prob * rng.gen_range(0.8..1.3)).clamp(0.01, 0.95);
    let qty: Qty = rng.gen_range(10..50);

    let mut order = bundle_yes(markets, id, &bundle_market_ids, price_to_nanos(limit), qty);

    // Override AON setting (bundle_yes defaults to AON)
    if !rng.gen_bool(aon_fraction * 1.5) {
        // Bundles are more likely to be AON, but not always
        order.min_fill = 0;
    }

    order
}

/// Select markets from different clusters for cross-cluster bundles.
/// These "bridging" orders complicate decomposition-based solvers.
fn generate_cross_cluster_selection<'a>(
    binary_markets: &[&'a MarketInfo],
    rng: &mut StdRng,
) -> Vec<&'a MarketInfo> {
    // Group markets by cluster
    let mut by_cluster: std::collections::HashMap<Option<usize>, Vec<&MarketInfo>> =
        std::collections::HashMap::new();
    for m in binary_markets {
        by_cluster.entry(m.cluster_id).or_default().push(*m);
    }

    // Need at least 2 clusters
    if by_cluster.len() < 2 {
        let mut all: Vec<&MarketInfo> = binary_markets.to_vec();
        all.shuffle(rng);
        all.truncate(rng.gen_range(2..=5.min(all.len())));
        return all;
    }

    // Pick 2-4 different clusters
    let mut cluster_ids: Vec<Option<usize>> = by_cluster.keys().cloned().collect();
    cluster_ids.shuffle(rng);
    let num_clusters = rng.gen_range(2..=4.min(cluster_ids.len()));
    cluster_ids.truncate(num_clusters);

    // Pick one market from each cluster
    let mut selected = Vec::new();
    for cluster_id in cluster_ids {
        if let Some(cluster_markets) = by_cluster.get(&cluster_id) {
            if !cluster_markets.is_empty() {
                let idx = rng.gen_range(0..cluster_markets.len());
                selected.push(cluster_markets[idx]);
            }
        }
    }

    // Limit to 5 markets max (32 states)
    if selected.len() > 5 {
        selected.truncate(5);
    }

    selected
}

/// Generate a spread order (A - B).
fn generate_spread_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    market_infos: &[MarketInfo],
) -> Order {
    let id = *order_id;
    *order_id += 1;

    // Only select binary markets for spreads
    let binary_markets: Vec<&MarketInfo> = market_infos
        .iter()
        .filter(|m| m.num_outcomes == 2)
        .collect();

    if binary_markets.len() < 2 {
        // Fallback
        let m = &market_infos[rng.gen_range(0..market_infos.len())];
        return outcome_buy(markets, id, m.id, 0, price_to_nanos(0.5), 50);
    }

    let mut selected: Vec<&MarketInfo> = binary_markets.clone();
    selected.shuffle(rng);

    let market_a = selected[0];
    let market_b = selected[1];

    let price_diff = (market_a.fair_prices[0] - market_b.fair_prices[0]).abs();
    let limit = (price_diff + rng.gen_range(-0.05..0.10)).clamp(0.01, 0.5);
    let qty: Qty = rng.gen_range(20..60);

    spread(markets, id, market_a.id, market_b.id, price_to_nanos(limit), qty)
}

/// Generate a butterfly order on a 3+ outcome market.
fn generate_butterfly_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    multi_outcome_markets: &[&MarketInfo],
) -> Option<Order> {
    if multi_outcome_markets.is_empty() {
        return None;
    }

    let id = *order_id;
    *order_id += 1;

    // Filter to 3-outcome markets for standard butterfly
    let three_outcome: Vec<&&MarketInfo> = multi_outcome_markets
        .iter()
        .filter(|m| m.num_outcomes == 3)
        .collect();

    if three_outcome.is_empty() {
        // Use any multi-outcome market with custom payoff
        let market = multi_outcome_markets[rng.gen_range(0..multi_outcome_markets.len())];
        let qty: Qty = rng.gen_range(10..40);

        // Create iron condor style: +1, -1, -1, +1 (or subset)
        let mut builder = OrderBuilder::new(markets, id)
            .spanning(&[market.id])
            .limit(price_to_nanos(rng.gen_range(0.02..0.15)))
            .quantity(0, qty);

        // Set payoffs: profit from middle outcomes
        let num = market.num_outcomes as usize;
        for i in 0..num {
            let payoff = if i == 0 || i == num - 1 { 1 } else { -1 };
            builder = builder.payoff_at(i, payoff);
        }

        return Some(builder.build());
    }

    let market = three_outcome[rng.gen_range(0..three_outcome.len())];
    let qty: Qty = rng.gen_range(10..40);

    // Standard butterfly price based on middle outcome probability
    let mid_prob = market.fair_prices[1];
    let limit = (mid_prob * 2.0 * rng.gen_range(0.6..1.2)).clamp(0.01, 0.30);

    Some(butterfly(markets, id, market.id, price_to_nanos(limit), qty))
}

/// Generate a conditional order.
fn generate_conditional_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    market_infos: &[MarketInfo],
) -> Order {
    let id = *order_id;
    *order_id += 1;

    if market_infos.len() < 2 {
        // Fallback to simple order
        let m = &market_infos[0];
        return outcome_buy(markets, id, m.id, 0, price_to_nanos(m.fair_prices[0]), 50);
    }

    // Pick two different markets
    let mut indices: Vec<usize> = (0..market_infos.len()).collect();
    indices.shuffle(rng);

    let order_market = &market_infos[indices[0]];
    let condition_market = &market_infos[indices[1]];

    let outcome = rng.gen_range(0..order_market.num_outcomes);
    let fair_price = order_market.fair_prices[outcome as usize];
    let limit = (fair_price * rng.gen_range(0.9..1.2)).clamp(0.05, 0.95);
    let qty: Qty = rng.gen_range(15..50);

    // Condition: trigger when condition market's outcome 0 price crosses threshold
    let threshold = price_to_nanos(condition_market.fair_prices[0] * rng.gen_range(0.8..1.2));
    let direction = if rng.gen_bool(0.5) {
        ConditionDir::Above
    } else {
        ConditionDir::Below
    };

    OrderBuilder::new(markets, id)
        .spanning(&[order_market.id])
        .limit(price_to_nanos(limit))
        .quantity(0, qty)
        .payoff_at(outcome as usize, 1)
        .condition(condition_market.id, threshold, direction)
        .build()
}

/// Plant bundle liquidity sharing arbitrage opportunities.
///
/// Bundle1: A+B at $0.40
/// Bundle2: B+C at $0.35
/// Shared market B creates opportunity for optimal matching.
fn plant_bundle_arbitrages(
    problem: &mut Problem,
    order_id: &mut u64,
    market_infos: &[MarketInfo],
    config: &RealisticConfig,
    rng: &mut StdRng,
) {
    let binary_markets: Vec<&MarketInfo> = market_infos
        .iter()
        .filter(|m| m.num_outcomes == 2)
        .collect();

    if binary_markets.len() < 3 {
        return;
    }

    for _ in 0..config.planted_bundle_arbitrages {
        let mut selected: Vec<&MarketInfo> = binary_markets.clone();
        selected.shuffle(rng);

        if selected.len() < 3 {
            continue;
        }

        let market_a = selected[0].id;
        let market_b = selected[1].id; // Shared market
        let market_c = selected[2].id;

        // Bundle1: A+B at aggressive price
        let id1 = *order_id;
        *order_id += 1;
        let order1 = bundle_yes(
            &problem.markets,
            id1,
            &[market_a, market_b],
            price_to_nanos(rng.gen_range(0.35..0.50)),
            rng.gen_range(30..60),
        );
        problem.orders.push(order1);

        // Bundle2: B+C at aggressive price (shares market B)
        let id2 = *order_id;
        *order_id += 1;
        let order2 = bundle_yes(
            &problem.markets,
            id2,
            &[market_b, market_c],
            price_to_nanos(rng.gen_range(0.30..0.45)),
            rng.gen_range(30..60),
        );
        problem.orders.push(order2);
    }
}

/// Plant chain arbitrage opportunities using market clusters.
///
/// In an implication chain A→B→C with prices $0.15, $0.30, $0.45,
/// buying A gives exposure to all levels cheaper than buying individually.
fn plant_chain_arbitrages(
    problem: &mut Problem,
    order_id: &mut u64,
    clusters: &[MarketCluster],
    _market_infos: &[MarketInfo],  // Reserved for enhanced pricing
    config: &RealisticConfig,
    rng: &mut StdRng,
) {
    let chain_clusters: Vec<&MarketCluster> = clusters
        .iter()
        .filter(|c| c.markets.len() >= 3)
        .collect();

    if chain_clusters.is_empty() {
        return;
    }

    for _ in 0..config.planted_chain_arbitrages {
        let cluster = chain_clusters[rng.gen_range(0..chain_clusters.len())];

        // Create orders at different levels with prices that create arbitrage
        // Root level (deepest implication) has lowest price but provides exposure to all
        for (depth, &market_id) in cluster.markets.iter().enumerate() {
            let id = *order_id;
            *order_id += 1;

            // Price increases with depth (but root gives most value)
            let base_price = 0.10 + (depth as f64 * 0.12);
            let price = (base_price * rng.gen_range(0.9..1.3)).clamp(0.05, 0.70);
            let qty = rng.gen_range(20..50);

            let order = outcome_buy(&problem.markets, id, market_id, 0, price_to_nanos(price), qty);
            problem.orders.push(order);
        }
    }
}

/// Plant complementary set arbitrage opportunities.
///
/// Create 4 bundles covering all states of 2 markets where total price > $1.00
/// (guaranteed profit opportunity).
///
/// Strategy: Concentrate complement sets on a smaller number of "hot" market pairs
/// so the BundleDecomposer can find patterns more easily.
fn plant_complement_sets(
    problem: &mut Problem,
    order_id: &mut u64,
    market_infos: &[MarketInfo],
    config: &RealisticConfig,
    rng: &mut StdRng,
) {
    let binary_markets: Vec<&MarketInfo> = market_infos
        .iter()
        .filter(|m| m.num_outcomes == 2)
        .collect();

    if binary_markets.len() < 2 {
        return;
    }

    // Select a smaller set of "complement hot" market pairs
    // This concentrates complement sets so BundleDecomposer can find them
    let num_hot_pairs = (config.planted_complement_sets / 3).clamp(5, 20);
    let mut hot_pairs: Vec<(MarketId, MarketId)> = Vec::new();

    for _ in 0..num_hot_pairs {
        let mut selected: Vec<&MarketInfo> = binary_markets.clone();
        selected.shuffle(rng);
        if selected.len() >= 2 {
            hot_pairs.push((selected[0].id, selected[1].id));
        }
    }

    for i in 0..config.planted_complement_sets {
        // Cycle through hot pairs to concentrate complement sets
        let (ma, mb) = hot_pairs[i % hot_pairs.len()];

        // IMPORTANT: Sort market IDs so payoffs match BundleDecomposer's grouping
        // (BundleDecomposer groups by sorted market keys)
        let (market_a, market_b) = if ma.0 <= mb.0 { (ma, mb) } else { (mb, ma) };

        // Create 4 orders covering all states, pricing to create slight arbitrage
        // Total should be just over $1.00 (like $1.03)
        let states = [
            (&[0u8, 0u8], 0.26),  // A=Yes, B=Yes
            (&[1u8, 0u8], 0.26),  // A=No, B=Yes
            (&[0u8, 1u8], 0.26),  // A=Yes, B=No
            (&[1u8, 1u8], 0.25),  // A=No, B=No
        ];

        for (outcomes, base_price) in states {
            let id = *order_id;
            *order_id += 1;

            // Higher prices and quantities to make complement sets competitive
            let price = base_price * rng.gen_range(1.0..1.15);  // More aggressive pricing
            let qty = rng.gen_range(50..100);  // Larger quantities

            // Create order that pays 1 for specific state combination
            let order = OrderBuilder::new(&problem.markets, id)
                .spanning(&[market_a, market_b])
                .limit(price_to_nanos(price))
                .quantity(0, qty)
                .payoff_when(outcomes, 1)
                .build();

            problem.orders.push(order);
        }
    }
}

/// Plant synergy groups - orders that are bad alone but great together.
///
/// One "trap" high-value order vs 3 medium orders that synergize.
fn plant_synergy_groups(
    problem: &mut Problem,
    order_id: &mut u64,
    market_infos: &[MarketInfo],
    config: &RealisticConfig,
    rng: &mut StdRng,
) {
    let binary_markets: Vec<&MarketInfo> = market_infos
        .iter()
        .filter(|m| m.num_outcomes == 2)
        .collect();

    if binary_markets.len() < 4 {
        return;
    }

    for _ in 0..config.planted_synergy_groups {
        let mut selected: Vec<&MarketInfo> = binary_markets.clone();
        selected.shuffle(rng);

        // Trap order: high value but consumes scarce liquidity on one market
        let trap_market = selected[0].id;
        let id_trap = *order_id;
        *order_id += 1;

        let trap_order = outcome_buy(
            &problem.markets,
            id_trap,
            trap_market,
            0,
            price_to_nanos(0.85),  // Very aggressive
            rng.gen_range(80..120), // Large quantity
        );
        problem.orders.push(trap_order);

        // Synergy group: 3 smaller orders that together beat the trap
        for i in 1..=3 {
            let id = *order_id;
            *order_id += 1;

            let market = selected[i.min(selected.len() - 1)].id;
            let order = outcome_buy(
                &problem.markets,
                id,
                market,
                0,
                price_to_nanos(rng.gen_range(0.55..0.70)),
                rng.gen_range(30..50),
            );
            problem.orders.push(order);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_realistic_small() {
        let problem = generate_realistic_scenario(RealisticConfig::small());
        assert!(problem.markets.len() >= 50);
        assert!(problem.orders.len() >= 3000);

        // Should have various order types
        let bundle_count = problem.orders.iter().filter(|o| o.num_markets > 1).count();
        assert!(bundle_count > 500, "Expected bundles, got {}", bundle_count);

        // Should have constraints
        assert!(problem.constraints.len() > 0);
    }

    #[test]
    fn test_realistic_test() {
        let problem = generate_realistic_scenario(RealisticConfig::test());
        assert!(problem.markets.len() >= 100);
        assert!(problem.orders.len() >= 10000);

        // Check AON fraction is lower than MILP killer
        let aon_count = problem.orders.iter().filter(|o| o.is_all_or_none()).count();
        let aon_fraction = aon_count as f64 / problem.orders.len() as f64;
        assert!(aon_fraction < 0.25, "AON fraction {} too high", aon_fraction);
    }

    #[test]
    fn test_market_type_distribution() {
        let config = RealisticConfig::small();
        let problem = generate_realistic_scenario(config);

        let binary = problem.markets.iter().filter(|m| m.outcomes.len() == 2).count();
        let total = problem.markets.len();

        // Should have mix of market types
        assert!(binary > 0);
        assert!(binary < total, "Should have multi-outcome markets");
    }

    #[test]
    fn test_cross_market_demo() {
        let problem = generate_realistic_scenario(RealisticConfig::cross_market_demo());

        // Should have high bundle fraction
        let bundle_count = problem.orders.iter().filter(|o| o.num_markets > 1).count();
        let bundle_fraction = bundle_count as f64 / problem.orders.len() as f64;
        assert!(bundle_fraction > 0.40, "Bundle fraction {} too low", bundle_fraction);
    }

    #[test]
    fn test_has_conditionals() {
        let problem = generate_realistic_scenario(RealisticConfig::small());

        let conditional_count = problem.orders.iter().filter(|o| o.is_conditional()).count();
        assert!(conditional_count > 100, "Expected conditionals, got {}", conditional_count);
    }
}
