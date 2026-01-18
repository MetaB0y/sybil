//! 2024 US Presidential Election scenario.

use rand::Rng;
use rand::rngs::StdRng;
use rand::SeedableRng;

use matching_engine::{
    LiquidityPool, ConstraintBuilder, MarketSet, Order, MarketId, Qty,
    price_to_nanos, OrderBuilder, StateSpace, ConditionDir,
    outcome_buy, bundle_yes, conditional_buy,
};

use matching_engine::Problem;

/// Configuration for the presidential scenario
#[derive(Clone, Debug)]
pub struct PresidentialConfig {
    pub seed: u64,
    pub num_simple_orders: usize,
    pub num_bundle_orders: usize,
    pub num_conditional_orders: usize,
    pub liquidity_multiplier: f64,
    pub price_noise: f64,
}

impl Default for PresidentialConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            num_simple_orders: 30,
            num_bundle_orders: 10,
            num_conditional_orders: 5,
            liquidity_multiplier: 0.5,
            price_noise: 0.03,
        }
    }
}

/// Generate the presidential election scenario.
pub fn generate_presidential_scenario(config: PresidentialConfig) -> Problem {
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut problem = Problem::new("2024 US Presidential Election");

    let president = problem.markets.add(
        "President",
        vec!["Trump".into(), "Harris".into(), "Other".into()],
    );
    let party = problem.markets.add(
        "Winning Party",
        vec!["Republican".into(), "Democrat".into()],
    );
    let senate = problem.markets.add(
        "Senate Control",
        vec!["Rep_Senate".into(), "Dem_Senate".into()],
    );

    problem.constraints = ConstraintBuilder::new()
        .implies(president, 0, party, 0)
        .implies(president, 1, party, 1)
        .build();

    let trump_price = 0.52;
    let harris_price = 0.45;
    let other_price = 0.03;
    let rep_senate_price = 0.55;

    let base_depth = (100.0 * config.liquidity_multiplier) as Qty;

    setup_presidential_liquidity(
        &mut problem.liquidity,
        president,
        party,
        senate,
        trump_price,
        harris_price,
        other_price,
        rep_senate_price,
        base_depth,
    );

    let mut order_id = 1u64;

    for _ in 0..config.num_simple_orders {
        let order = generate_simple_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            president,
            party,
            senate,
            trump_price,
            harris_price,
            rep_senate_price,
            config.price_noise,
        );
        problem.orders.push(order);
    }

    for _ in 0..config.num_bundle_orders {
        let order = generate_bundle_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            president,
            party,
            senate,
            trump_price,
            harris_price,
            rep_senate_price,
        );
        problem.orders.push(order);
    }

    for _ in 0..config.num_conditional_orders {
        let order = generate_conditional_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            president,
            party,
            senate,
            trump_price,
            harris_price,
        );
        problem.orders.push(order);
    }

    problem
}

fn setup_presidential_liquidity(
    liquidity: &mut LiquidityPool,
    president: MarketId,
    party: MarketId,
    senate: MarketId,
    trump_price: f64,
    harris_price: f64,
    other_price: f64,
    rep_senate_price: f64,
    base_depth: Qty,
) {
    liquidity.add_bid(president, 0, price_to_nanos(trump_price - 0.01), base_depth);
    liquidity.add_bid(president, 0, price_to_nanos(trump_price - 0.02), base_depth * 2);
    liquidity.add_ask(president, 0, price_to_nanos(trump_price + 0.01), base_depth);
    liquidity.add_ask(president, 0, price_to_nanos(trump_price + 0.02), base_depth * 2);
    liquidity.add_ask(president, 0, price_to_nanos(trump_price + 0.03), base_depth * 3);

    liquidity.add_bid(president, 1, price_to_nanos(harris_price - 0.01), base_depth);
    liquidity.add_bid(president, 1, price_to_nanos(harris_price - 0.02), base_depth * 2);
    liquidity.add_ask(president, 1, price_to_nanos(harris_price + 0.01), base_depth);
    liquidity.add_ask(president, 1, price_to_nanos(harris_price + 0.02), base_depth * 2);

    liquidity.add_bid(president, 2, price_to_nanos(other_price - 0.005), base_depth / 2);
    liquidity.add_ask(president, 2, price_to_nanos(other_price + 0.01), base_depth / 2);

    liquidity.add_bid(party, 0, price_to_nanos(trump_price - 0.02), base_depth / 2);
    liquidity.add_ask(party, 0, price_to_nanos(trump_price + 0.02), base_depth / 2);
    liquidity.add_bid(party, 1, price_to_nanos(harris_price - 0.02), base_depth / 2);
    liquidity.add_ask(party, 1, price_to_nanos(harris_price + 0.02), base_depth / 2);

    liquidity.add_bid(senate, 0, price_to_nanos(rep_senate_price - 0.02), base_depth);
    liquidity.add_bid(senate, 0, price_to_nanos(rep_senate_price - 0.03), base_depth * 2);
    liquidity.add_ask(senate, 0, price_to_nanos(rep_senate_price + 0.02), base_depth);
    liquidity.add_ask(senate, 0, price_to_nanos(rep_senate_price + 0.03), base_depth * 2);

    liquidity.add_bid(senate, 1, price_to_nanos(1.0 - rep_senate_price - 0.02), base_depth);
    liquidity.add_ask(senate, 1, price_to_nanos(1.0 - rep_senate_price + 0.02), base_depth);
}

fn generate_simple_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    president: MarketId,
    party: MarketId,
    senate: MarketId,
    trump_price: f64,
    harris_price: f64,
    rep_senate_price: f64,
    noise: f64,
) -> Order {
    let id = *order_id;
    *order_id += 1;

    let choice = rng.gen_range(0..5);

    let (market, outcome, base_price) = match choice {
        0 => (president, 0, trump_price),
        1 => (president, 1, harris_price),
        2 => (party, 0, trump_price),
        3 => (senate, 0, rep_senate_price),
        _ => (senate, 1, 1.0 - rep_senate_price),
    };

    let price_adjustment: f64 = rng.gen_range(-noise..noise * 2.0);
    let limit = (base_price + price_adjustment).clamp(0.01, 0.99);

    let qty: Qty = rng.gen_range(50..200);

    outcome_buy(markets, id, market, outcome, price_to_nanos(limit), qty)
}

fn generate_bundle_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    president: MarketId,
    _party: MarketId,
    senate: MarketId,
    trump_price: f64,
    harris_price: f64,
    rep_senate_price: f64,
) -> Order {
    let id = *order_id;
    *order_id += 1;

    let bundle_type = rng.gen_range(0..2);

    let (market_ids, limit, _desc) = match bundle_type {
        0 => {
            let combined_price = trump_price * rep_senate_price;
            let limit = combined_price * rng.gen_range(0.9..1.1);
            (vec![president, senate], limit, "Trump + Rep Senate")
        }
        _ => {
            let dem_senate_price = 1.0 - rep_senate_price;
            let combined_price = harris_price * dem_senate_price;
            let limit = combined_price * rng.gen_range(0.9..1.1);
            (vec![president, senate], limit, "Harris + Dem Senate")
        }
    };

    let qty: Qty = rng.gen_range(30..100);

    if bundle_type == 0 {
        bundle_yes(markets, id, &market_ids, price_to_nanos(limit), qty)
    } else {
        let mut builder = OrderBuilder::new(markets, id)
            .spanning(&market_ids)
            .limit(price_to_nanos(limit))
            .all_or_none(qty);

        let sizes: Vec<u8> = market_ids.iter().map(|m| markets.num_outcomes(*m)).collect();
        let space = StateSpace::new(&sizes);

        let winning_state = space.state_index(&[1, 1]);
        builder = builder.payoff_at(winning_state, 1);

        builder.build()
    }
}

fn generate_conditional_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    president: MarketId,
    _party: MarketId,
    senate: MarketId,
    trump_price: f64,
    harris_price: f64,
) -> Order {
    let id = *order_id;
    *order_id += 1;

    let cond_type = rng.gen_range(0..2);

    let (target_market, _target_outcome, limit, cond_market, threshold, direction) = match cond_type {
        0 => {
            (
                senate,
                0,
                0.60,
                president,
                price_to_nanos(trump_price + 0.03),
                ConditionDir::Above,
            )
        }
        _ => {
            (
                senate,
                1,
                0.50,
                president,
                price_to_nanos(harris_price + 0.03),
                ConditionDir::Above,
            )
        }
    };

    let qty: Qty = rng.gen_range(20..80);

    conditional_buy(
        markets,
        id,
        target_market,
        price_to_nanos(limit),
        qty,
        cond_market,
        threshold,
        direction,
    )
}
