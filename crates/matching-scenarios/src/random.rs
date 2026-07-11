//! Random hard instance generation with binary markets.

use rand::RngExt;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use matching_engine::{
    MarketId, MarketSet, Order, Problem, Qty, outcome_buy, outcome_sell, price_to_nanos,
};

/// Configuration for random hard instance generation
#[derive(Clone, Debug)]
pub struct RandomConfig {
    pub seed: u64,
    pub num_markets: usize,
    pub num_orders: usize,
    pub oversubscription: f64,
    pub base_liquidity_depth: u64,
    pub price_levels: usize,
    pub price_spread: f64,
}

impl Default for RandomConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            num_markets: 5,
            num_orders: 50,
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
            ..Default::default()
        }
    }

    pub fn medium() -> Self {
        Self {
            oversubscription: 1.5,
            ..Default::default()
        }
    }

    pub fn hard() -> Self {
        Self {
            oversubscription: 3.0,
            num_orders: 100,
            ..Default::default()
        }
    }
}

/// Generate a random hard instance with binary markets.
pub fn generate_random_scenario(config: RandomConfig) -> Problem {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
    let mut problem = Problem::new(format!(
        "Random(markets={}, orders={}, oversub={:.1}x)",
        config.num_markets, config.num_orders, config.oversubscription
    ));

    let mut market_ids: Vec<MarketId> = Vec::new();
    let mut market_prices: Vec<f64> = Vec::new(); // YES price for each market

    for i in 0..config.num_markets {
        let market = problem.markets.add_binary(format!("Market{}", i));
        market_ids.push(market);

        let mid_price = rng.random_range(0.3..0.7);
        market_prices.push(mid_price);
    }

    // Calculate supply per market
    let avg_order_qty = 75u64;
    let total_demand_estimate = config.num_orders as u64 * avg_order_qty;
    let total_supply = (total_demand_estimate as f64 / config.oversubscription) as u64;
    let supply_per_market = total_supply / config.num_markets as u64;
    let supply_per_level = supply_per_market / (config.price_levels as u64 * 2);

    // Add supply/demand orders for each market (YES and NO)
    let mut liq_order_id = 5_000_000u64;
    for (i, &market) in market_ids.iter().enumerate() {
        let yes_price = market_prices[i];
        let no_price = 1.0 - yes_price;

        for level in 0..config.price_levels {
            let offset = config.price_spread * (level as f64 + 1.0) / config.price_levels as f64;

            // YES buy orders (bids)
            problem.orders.push(outcome_buy(
                &problem.markets,
                liq_order_id,
                market,
                0,
                price_to_nanos((yes_price - offset).max(0.01)).0,
                supply_per_level.max(10),
            ));
            liq_order_id += 1;
            // YES sell orders (asks)
            problem.orders.push(outcome_sell(
                &problem.markets,
                liq_order_id,
                market,
                0,
                price_to_nanos((yes_price + offset).min(0.99)).0,
                supply_per_level.max(10),
            ));
            liq_order_id += 1;

            // NO buy orders (bids)
            problem.orders.push(outcome_buy(
                &problem.markets,
                liq_order_id,
                market,
                1,
                price_to_nanos((no_price - offset).max(0.01)).0,
                supply_per_level.max(10),
            ));
            liq_order_id += 1;
            // NO sell orders (asks)
            problem.orders.push(outcome_sell(
                &problem.markets,
                liq_order_id,
                market,
                1,
                price_to_nanos((no_price + offset).min(0.99)).0,
                supply_per_level.max(10),
            ));
            liq_order_id += 1;
        }
    }

    // Generate orders
    let mut order_id = 1u64;

    for _ in 0..config.num_orders {
        let order = generate_simple_random_order(
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
    rng: &mut ChaCha8Rng,
    order_id: &mut u64,
    market_ids: &[MarketId],
    market_prices: &[f64],
) -> Order {
    let id = *order_id;
    *order_id += 1;

    let market_idx = rng.random_range(0..market_ids.len());
    let market = market_ids[market_idx];
    let outcome = rng.random_range(0..2u8); // YES or NO

    let base_price = if outcome == 0 {
        market_prices[market_idx]
    } else {
        1.0 - market_prices[market_idx]
    };

    let aggressiveness = rng.random_range(0.0..0.1);
    let limit = (base_price + aggressiveness).min(0.95);

    let qty: u64 = rng.random_range(30..120);

    outcome_buy(markets, id, market, outcome, price_to_nanos(limit).0, qty)
}

fn inject_conflicts(problem: &mut Problem, rng: &mut ChaCha8Rng, market_ids: &[MarketId]) {
    problem.orders.sort_by(|a, b| {
        let a_welfare = a.limit_price.0 as u128 * a.max_fill.0 as u128;
        let b_welfare = b.limit_price.0 as u128 * b.max_fill.0 as u128;
        b_welfare.cmp(&a_welfare)
    });

    let conflict_count = problem.orders.len() / 5;
    if conflict_count > 1 && !market_ids.is_empty() {
        let conflict_market = market_ids[rng.random_range(0..market_ids.len())];

        for order in problem.orders.iter_mut().take(conflict_count) {
            let touches_market = order.active_markets().any(|m| m == conflict_market);
            if touches_market {
                order.max_fill = Qty((order.max_fill.0 as f64 * 1.5) as u64);
            }
        }
    }
}
