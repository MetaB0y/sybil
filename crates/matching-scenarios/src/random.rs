//! Random hard instance generation with binary markets.

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::Rng;
use rand::SeedableRng;

use matching_engine::{
    bundle_yes, outcome_buy, price_to_nanos, spread, MarketId, MarketSet, Order, Problem, Qty,
};

/// Configuration for random hard instance generation
#[derive(Clone, Debug)]
pub struct RandomConfig {
    pub seed: u64,
    pub num_markets: usize,
    pub num_orders: usize,
    pub bundle_fraction: f64,
    pub spread_fraction: f64,
    pub oversubscription: f64,
    pub base_liquidity_depth: Qty,
    pub price_levels: usize,
    pub price_spread: f64,
}

impl Default for RandomConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            num_markets: 5,
            num_orders: 50,
            bundle_fraction: 0.2,
            spread_fraction: 0.1,
            oversubscription: 2.0,
            base_liquidity_depth: 100,
            price_levels: 3,
            price_spread: 0.05,
        }
    }
}

impl RandomConfig {
    pub fn easy() -> Self {
        Self {
            oversubscription: 0.5,
            bundle_fraction: 0.0,
            spread_fraction: 0.0,
            ..Default::default()
        }
    }

    pub fn medium() -> Self {
        Self {
            oversubscription: 1.5,
            bundle_fraction: 0.1,
            spread_fraction: 0.1,
            ..Default::default()
        }
    }

    pub fn hard() -> Self {
        Self {
            oversubscription: 3.0,
            bundle_fraction: 0.3,
            spread_fraction: 0.2,
            num_orders: 100,
            ..Default::default()
        }
    }
}

/// Generate a random hard instance with binary markets.
pub fn generate_random_scenario(config: RandomConfig) -> Problem {
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut problem = Problem::new(format!(
        "Random(markets={}, orders={}, oversub={:.1}x)",
        config.num_markets, config.num_orders, config.oversubscription
    ));

    let mut market_ids: Vec<MarketId> = Vec::new();
    let mut market_prices: Vec<f64> = Vec::new(); // YES price for each market

    for i in 0..config.num_markets {
        let market = problem.markets.add_binary(format!("Market{}", i));
        market_ids.push(market);

        let mid_price = rng.gen_range(0.3..0.7);
        market_prices.push(mid_price);
    }

    // Calculate supply per market
    let avg_order_qty = 75u64;
    let total_demand_estimate = config.num_orders as u64 * avg_order_qty;
    let total_supply = (total_demand_estimate as f64 / config.oversubscription) as Qty;
    let supply_per_market = total_supply / config.num_markets as Qty;
    let supply_per_level = supply_per_market / (config.price_levels as Qty * 2);

    // Add liquidity for each market (YES and NO)
    for (i, &market) in market_ids.iter().enumerate() {
        let yes_price = market_prices[i];
        let no_price = 1.0 - yes_price;

        for level in 0..config.price_levels {
            let offset = config.price_spread * (level as f64 + 1.0) / config.price_levels as f64;

            // YES liquidity
            problem.liquidity.add_bid(
                market,
                0,
                price_to_nanos((yes_price - offset).max(0.01)),
                supply_per_level.max(10),
            );
            problem.liquidity.add_ask(
                market,
                0,
                price_to_nanos((yes_price + offset).min(0.99)),
                supply_per_level.max(10),
            );

            // NO liquidity
            problem.liquidity.add_bid(
                market,
                1,
                price_to_nanos((no_price - offset).max(0.01)),
                supply_per_level.max(10),
            );
            problem.liquidity.add_ask(
                market,
                1,
                price_to_nanos((no_price + offset).min(0.99)),
                supply_per_level.max(10),
            );
        }
    }

    // Generate orders
    let num_bundles = (config.num_orders as f64 * config.bundle_fraction) as usize;
    let num_spreads = (config.num_orders as f64 * config.spread_fraction) as usize;
    let num_simple = config.num_orders - num_bundles - num_spreads;

    let mut order_id = 1u64;

    for _ in 0..num_simple {
        let order = generate_simple_random_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_ids,
            &market_prices,
        );
        problem.orders.push(order);
    }

    for _ in 0..num_bundles {
        let order = generate_bundle_random_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_ids,
            &market_prices,
        );
        problem.orders.push(order);
    }

    for _ in 0..num_spreads {
        let order = generate_spread_random_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &market_ids,
            &market_prices,
        );
        problem.orders.push(order);
    }

    inject_conflicts(&mut problem, &mut rng, &market_ids);

    problem
}

fn generate_simple_random_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    market_ids: &[MarketId],
    market_prices: &[f64],
) -> Order {
    let id = *order_id;
    *order_id += 1;

    let market_idx = rng.gen_range(0..market_ids.len());
    let market = market_ids[market_idx];
    let outcome = rng.gen_range(0..2u8); // YES or NO

    let base_price = if outcome == 0 {
        market_prices[market_idx]
    } else {
        1.0 - market_prices[market_idx]
    };

    let aggressiveness = rng.gen_range(0.0..0.1);
    let limit = (base_price + aggressiveness).min(0.95);

    let qty: Qty = rng.gen_range(30..120);

    outcome_buy(markets, id, market, outcome, price_to_nanos(limit), qty)
}

fn generate_bundle_random_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    market_ids: &[MarketId],
    market_prices: &[f64],
) -> Order {
    let id = *order_id;
    *order_id += 1;

    let num_to_bundle = rng.gen_range(2..=market_ids.len().min(3));
    let mut selected: Vec<usize> = (0..market_ids.len()).collect();
    selected.shuffle(rng);
    selected.truncate(num_to_bundle);

    let bundle_markets: Vec<MarketId> = selected.iter().map(|&i| market_ids[i]).collect();

    let combined_prob: f64 = selected.iter().map(|&i| market_prices[i]).product();

    let limit = (combined_prob * rng.gen_range(0.9..1.2)).clamp(0.01, 0.95);
    let qty: Qty = rng.gen_range(20..80);

    bundle_yes(markets, id, &bundle_markets, price_to_nanos(limit), qty)
}

fn generate_spread_random_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    market_ids: &[MarketId],
    market_prices: &[f64],
) -> Order {
    let id = *order_id;
    *order_id += 1;

    if market_ids.len() < 2 {
        return outcome_buy(markets, id, market_ids[0], 0, price_to_nanos(0.5), 50);
    }

    let m1_idx = rng.gen_range(0..market_ids.len());
    let mut m2_idx = rng.gen_range(0..market_ids.len());
    while m2_idx == m1_idx {
        m2_idx = rng.gen_range(0..market_ids.len());
    }

    let market_a = market_ids[m1_idx];
    let market_b = market_ids[m2_idx];

    let price_diff = (market_prices[m1_idx] - market_prices[m2_idx]).abs();
    let limit = (price_diff + rng.gen_range(-0.05..0.1)).clamp(0.01, 0.5);
    let qty: Qty = rng.gen_range(30..100);

    spread(markets, id, market_a, market_b, price_to_nanos(limit), qty)
}

fn inject_conflicts(problem: &mut Problem, rng: &mut StdRng, market_ids: &[MarketId]) {
    problem.orders.sort_by(|a, b| {
        let a_welfare = a.limit_price as u128 * a.max_fill as u128;
        let b_welfare = b.limit_price as u128 * b.max_fill as u128;
        b_welfare.cmp(&a_welfare)
    });

    let conflict_count = problem.orders.len() / 5;
    if conflict_count > 1 && !market_ids.is_empty() {
        let conflict_market = market_ids[rng.gen_range(0..market_ids.len())];

        for order in problem.orders.iter_mut().take(conflict_count) {
            let touches_market = order.active_markets().any(|m| m == conflict_market);
            if touches_market {
                order.max_fill = (order.max_fill as f64 * 1.5) as Qty;
            }
        }
    }
}
