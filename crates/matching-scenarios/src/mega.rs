//! Comprehensive mega scenario for internal solver validation.
//!
//! This module provides a single comprehensive scenario generator that creates
//! realistic test problems with:
//! - Multiple binary markets
//! - Market maker constraints with different strategies
//! - Configurable order distributions
//!
//! # Example
//!
//! ```ignore
//! let config = MegaScenarioConfig::default();
//! let problem = generate_mega_scenario_v2(config);
//! ```

use std::collections::HashMap;
use std::ops::Range;

use rand::prelude::*;
use rand_chacha::ChaCha8Rng;

use matching_engine::{
    bundle_yes, outcome_buy, outcome_sell, price_to_nanos, MarketId, MmConstraint, MmId, MmSide,
    Nanos, Order, Problem, Qty, NANOS_PER_DOLLAR,
};

/// Market maker strategy types.
#[derive(Clone, Debug)]
pub enum MmStrategy {
    /// Tight spreads around fair price (10-50 bps)
    TightSpreads { spread_bps: u32 },
    /// Wide spreads (100-300 bps)
    WideSpreads { spread_bps: u32 },
    /// Focus on few markets
    FewMarkets { count: usize },
    /// Cover many markets
    ManyMarkets { count: usize },
    /// Heavy positions in top N markets by volume
    Concentrated { top_n: usize },
    /// Even spread across markets
    Diversified,
}

impl Default for MmStrategy {
    fn default() -> Self {
        Self::Diversified
    }
}

/// Price distribution for order generation.
#[derive(Clone, Debug)]
pub enum PriceDistribution {
    /// Most orders cluster near fair price
    Normal { std_dev: f64 },
    /// Even distribution around fair price
    Uniform { spread: f64 },
    /// Two clusters (aggressive + passive)
    Bimodal { peaks: (f64, f64) },
}

impl Default for PriceDistribution {
    fn default() -> Self {
        Self::Normal { std_dev: 0.1 }
    }
}

/// Configuration for mega scenarios.
#[derive(Clone, Debug)]
pub struct MegaScenarioConfigV2 {
    /// Random seed for reproducibility
    pub seed: u64,
    /// Number of markets to generate (all binary)
    pub num_markets: usize,
    /// Range of orders per market
    pub orders_per_market: Range<usize>,
    /// Fraction of orders that get matched (affects liquidity)
    pub matching_fraction: Range<f64>,

    // MM configuration
    /// Number of market makers
    pub num_mms: usize,
    /// MM leverage range (capital used / budget)
    pub mm_leverage: Range<f64>,
    /// MM budget range in dollars
    pub mm_budget_dollars: Range<u64>,
    /// MM strategies to use
    pub mm_strategies: Vec<MmStrategy>,

    // Order configuration
    /// Fraction of orders that are bundles (multi-market)
    pub bundle_fraction: f64,
    /// Price distribution for orders
    pub price_distribution: PriceDistribution,
    /// Range of order sizes
    pub order_size: Range<Qty>,
}

impl Default for MegaScenarioConfigV2 {
    fn default() -> Self {
        Self {
            seed: 42,
            num_markets: 50,
            orders_per_market: 50..200,
            matching_fraction: 0.3..0.7,

            num_mms: 3,
            mm_leverage: 3.0..8.0,
            mm_budget_dollars: 10_000..100_000,
            mm_strategies: vec![
                MmStrategy::TightSpreads { spread_bps: 30 },
                MmStrategy::WideSpreads { spread_bps: 150 },
                MmStrategy::Diversified,
            ],

            bundle_fraction: 0.15,
            price_distribution: PriceDistribution::Normal { std_dev: 0.12 },
            order_size: 10..500,
        }
    }
}

impl MegaScenarioConfigV2 {
    /// Small configuration for quick tests
    pub fn small() -> Self {
        Self {
            num_markets: 10,
            orders_per_market: 20..50,
            num_mms: 1,
            ..Default::default()
        }
    }

    /// Medium configuration for moderate testing
    pub fn medium() -> Self {
        Self {
            num_markets: 30,
            orders_per_market: 50..150,
            num_mms: 2,
            ..Default::default()
        }
    }

    /// Large configuration for stress testing
    pub fn large() -> Self {
        Self {
            num_markets: 100,
            orders_per_market: 100..300,
            num_mms: 5,
            mm_strategies: vec![
                MmStrategy::TightSpreads { spread_bps: 20 },
                MmStrategy::TightSpreads { spread_bps: 50 },
                MmStrategy::WideSpreads { spread_bps: 200 },
                MmStrategy::FewMarkets { count: 10 },
                MmStrategy::Diversified,
            ],
            ..Default::default()
        }
    }

    /// Extreme configuration for maximum stress
    pub fn extreme() -> Self {
        Self {
            num_markets: 200,
            orders_per_market: 200..500,
            num_mms: 8,
            bundle_fraction: 0.25,
            ..Default::default()
        }
    }
}

/// Generate a comprehensive mega scenario with binary markets.
pub fn generate_mega_scenario_v2(config: MegaScenarioConfigV2) -> Problem {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
    let mut problem = Problem::new(format!(
        "MegaV2(markets={}, mms={})",
        config.num_markets, config.num_mms
    ));

    // Generate binary markets with fair prices
    let mut market_info: Vec<(MarketId, f64)> = Vec::new(); // (id, fair_price_yes)

    for i in 0..config.num_markets {
        let market_id = problem.markets.add_binary(&format!("market_{}", i));

        // Generate fair price for YES (NO is 1 - YES)
        let fair_price_yes = rng.gen_range(0.2..0.8);

        // Add liquidity for YES and NO
        let liquidity_qty = rng.gen_range(1000..10000);
        let yes_ask_price = (fair_price_yes * NANOS_PER_DOLLAR as f64 * 1.02) as Nanos;
        let no_ask_price = ((1.0 - fair_price_yes) * NANOS_PER_DOLLAR as f64 * 1.02) as Nanos;

        problem.liquidity.add_ask(market_id, 0, yes_ask_price, liquidity_qty); // YES
        problem.liquidity.add_ask(market_id, 1, no_ask_price, liquidity_qty); // NO

        market_info.push((market_id, fair_price_yes));
    }

    // Generate regular orders
    let mut order_id: u64 = 1;
    let mut market_orders: HashMap<MarketId, Vec<u64>> = HashMap::new();

    for (market_id, _) in &market_info {
        let num_orders = rng.gen_range(config.orders_per_market.clone());

        for _ in 0..num_orders {
            let outcome = rng.gen_range(0..2u8); // YES or NO
            let size = rng.gen_range(config.order_size.clone());
            let price = generate_order_price(&mut rng, &config.price_distribution);

            let order = outcome_buy(
                &problem.markets,
                order_id,
                *market_id,
                outcome,
                price_to_nanos(price),
                size,
            );

            market_orders.entry(*market_id).or_default().push(order_id);
            problem.orders.push(order);
            order_id += 1;
        }
    }

    // Generate bundle orders (across multiple binary markets)
    let num_bundles = (config.bundle_fraction * problem.orders.len() as f64) as usize;
    for _ in 0..num_bundles {
        if market_info.len() < 2 {
            break;
        }

        // Pick 2-3 random markets for the bundle
        let num_markets = rng.gen_range(2..=3.min(market_info.len()));
        let mut selected: Vec<usize> = (0..market_info.len()).collect();
        selected.shuffle(&mut rng);
        selected.truncate(num_markets);

        let market_ids: Vec<MarketId> = selected.iter().map(|&i| market_info[i].0).collect();
        let price = generate_order_price(&mut rng, &config.price_distribution);
        let size = rng.gen_range(config.order_size.clone());

        // Create bundle (all YES outcomes)
        if let Some(order) = create_bundle_order(
            &problem.markets,
            order_id,
            &market_ids,
            price_to_nanos(price),
            size,
        ) {
            problem.orders.push(order);
            order_id += 1;
        }
    }

    // Compute anchor prices from order flow before generating MM orders
    let anchor_prices = compute_anchor_prices(&problem, &market_info);

    // Build market_info with anchor prices
    let market_info_with_anchors: Vec<(MarketId, f64)> = market_info
        .iter()
        .map(|(id, initial)| {
            let anchor = anchor_prices.get(id).copied().unwrap_or(*initial);
            (*id, anchor)
        })
        .collect();

    // Generate MM constraints using anchor prices
    for mm_idx in 0..config.num_mms {
        let strategy = if mm_idx < config.mm_strategies.len() {
            config.mm_strategies[mm_idx].clone()
        } else {
            MmStrategy::Diversified
        };

        let budget_dollars = rng.gen_range(config.mm_budget_dollars.clone());
        let leverage = rng.gen_range(config.mm_leverage.clone());

        let mm_constraint = create_mm_constraint(
            &mut rng,
            MmId::new(mm_idx as u64 + 1),
            budget_dollars,
            leverage,
            &strategy,
            &market_info_with_anchors,
            &market_orders,
            &mut order_id,
            &mut problem,
        );

        problem.mm_constraints.push(mm_constraint);
    }

    problem
}

/// Compute anchor prices by simulating price discovery from order flow.
fn compute_anchor_prices(
    problem: &Problem,
    market_info: &[(MarketId, f64)],
) -> HashMap<MarketId, f64> {
    let mut anchor_prices: HashMap<MarketId, f64> = HashMap::new();

    for (market_id, initial_fair_price) in market_info {
        // Collect buy orders for YES outcome
        let mut buy_prices: Vec<(Nanos, Qty)> = problem
            .orders
            .iter()
            .filter(|o| {
                o.num_markets == 1
                    && o.markets[0] == *market_id
                    && o.payoffs[0] > 0 // YES outcome
            })
            .map(|o| (o.limit_price, o.max_fill))
            .collect();

        buy_prices.sort_by(|a, b| b.0.cmp(&a.0));

        let liquidity_supply: Qty = problem
            .liquidity
            .books
            .get(&(*market_id, 0))
            .map(|book| book.asks().iter().map(|l| l.available_qty).sum())
            .unwrap_or(0);

        let liquidity_price: Nanos = problem
            .liquidity
            .books
            .get(&(*market_id, 0))
            .and_then(|book| book.asks().first().map(|l| l.price))
            .unwrap_or((*initial_fair_price * NANOS_PER_DOLLAR as f64) as Nanos);

        let mut cumulative_demand: Qty = 0;
        let mut clearing_price = liquidity_price;

        for (price, qty) in &buy_prices {
            cumulative_demand += qty;
            if cumulative_demand >= liquidity_supply {
                clearing_price = *price;
                break;
            }
        }

        if cumulative_demand < liquidity_supply {
            clearing_price = buy_prices
                .last()
                .map(|(p, _)| *p)
                .unwrap_or(liquidity_price);
        }

        let price = (clearing_price as f64 / NANOS_PER_DOLLAR as f64).clamp(0.05, 0.95);
        anchor_prices.insert(*market_id, price);
    }

    anchor_prices
}

/// Generate an order price based on distribution
fn generate_order_price(rng: &mut ChaCha8Rng, dist: &PriceDistribution) -> f64 {
    match dist {
        PriceDistribution::Normal { std_dev } => {
            let base = 0.5;
            let deviation: f64 = rng.gen::<f64>() * std_dev - std_dev / 2.0;
            (base + deviation).clamp(0.05, 0.95)
        }
        PriceDistribution::Uniform { spread } => {
            let base = 0.5;
            let offset = rng.gen::<f64>() * spread - spread / 2.0;
            (base + offset).clamp(0.05, 0.95)
        }
        PriceDistribution::Bimodal { peaks } => if rng.gen::<bool>() {
            peaks.0 + rng.gen::<f64>() * 0.1 - 0.05
        } else {
            peaks.1 + rng.gen::<f64>() * 0.1 - 0.05
        }
        .clamp(0.05, 0.95),
    }
}

/// Create a bundle order across multiple markets
fn create_bundle_order(
    markets: &matching_engine::MarketSet,
    id: u64,
    market_ids: &[MarketId],
    price: Nanos,
    qty: Qty,
) -> Option<Order> {
    if market_ids.len() < 2 {
        return None;
    }
    Some(bundle_yes(markets, id, market_ids, price, qty))
}

/// Create MM constraint with orders based on strategy
fn create_mm_constraint(
    rng: &mut ChaCha8Rng,
    mm_id: MmId,
    budget_dollars: u64,
    leverage: f64,
    strategy: &MmStrategy,
    market_info: &[(MarketId, f64)],
    market_orders: &HashMap<MarketId, Vec<u64>>,
    order_id: &mut u64,
    problem: &mut Problem,
) -> MmConstraint {
    let budget_nanos = budget_dollars as Nanos * NANOS_PER_DOLLAR;
    let notional_budget = (budget_dollars as f64 * leverage) as u64;

    let mut constraint = MmConstraint::new(mm_id, budget_nanos);

    let fair_price_map: HashMap<MarketId, f64> = market_info
        .iter()
        .map(|(id, price)| (*id, *price))
        .collect();

    // Select markets based on strategy
    let selected_markets: Vec<MarketId> = match strategy {
        MmStrategy::FewMarkets { count } => {
            let mut markets: Vec<_> = market_info.iter().map(|(id, _)| *id).collect();
            markets.shuffle(rng);
            markets.truncate(*count);
            markets
        }
        MmStrategy::ManyMarkets { count } => {
            let mut markets: Vec<_> = market_info.iter().map(|(id, _)| *id).collect();
            markets.shuffle(rng);
            markets.truncate(*count);
            markets
        }
        MmStrategy::Concentrated { top_n } => {
            let mut by_order_count: Vec<_> = market_info
                .iter()
                .map(|(id, _)| {
                    let count = market_orders.get(id).map(|v| v.len()).unwrap_or(0);
                    (*id, count)
                })
                .collect();
            by_order_count.sort_by(|a, b| b.1.cmp(&a.1));
            by_order_count.truncate(*top_n);
            by_order_count.into_iter().map(|(id, _)| id).collect()
        }
        _ => market_info.iter().map(|(id, _)| *id).collect(),
    };

    let notional_per_market = if selected_markets.is_empty() {
        0
    } else {
        notional_budget / selected_markets.len() as u64
    };

    for market_id in selected_markets {
        let spread_bps = match strategy {
            MmStrategy::TightSpreads { spread_bps } => *spread_bps,
            MmStrategy::WideSpreads { spread_bps } => *spread_bps,
            _ => 50,
        };

        let fair_price = fair_price_map.get(&market_id).copied().unwrap_or(0.50);
        let spread_frac = spread_bps as f64 / 10000.0;

        let bid_price = fair_price - spread_frac / 2.0;
        let ask_price = fair_price + spread_frac / 2.0;

        let qty_per_side = (notional_per_market as f64 / 2.0 / fair_price) as Qty;
        if qty_per_side == 0 {
            continue;
        }

        // MM sell order (provides liquidity at ask)
        let sell_order = outcome_sell(
            &problem.markets,
            *order_id,
            market_id,
            0, // YES outcome
            price_to_nanos(ask_price),
            qty_per_side,
        );
        constraint.add_order(*order_id, MmSide::SellYes);
        problem.orders.push(sell_order);
        *order_id += 1;

        // MM buy order (provides liquidity at bid)
        let buy_order = outcome_buy(
            &problem.markets,
            *order_id,
            market_id,
            0, // YES outcome
            price_to_nanos(bid_price),
            qty_per_side,
        );
        constraint.add_order(*order_id, MmSide::BuyYes);
        problem.orders.push(buy_order);
        *order_id += 1;
    }

    constraint
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mega_v2_small() {
        let config = MegaScenarioConfigV2::small();
        let problem = generate_mega_scenario_v2(config);

        assert!(problem.num_markets() >= 5);
        assert!(problem.num_orders() > 0);
        assert!(!problem.mm_constraints.is_empty());
    }

    #[test]
    fn test_mega_v2_default() {
        let config = MegaScenarioConfigV2::default();
        let problem = generate_mega_scenario_v2(config);

        assert_eq!(problem.num_markets(), 50);
        assert!(problem.num_orders() > 100);
        assert_eq!(problem.mm_constraints.len(), 3);
    }

    #[test]
    fn test_mega_v2_all_binary() {
        let config = MegaScenarioConfigV2::default();
        let problem = generate_mega_scenario_v2(config);

        // All markets should be binary
        for market in problem.markets.iter() {
            assert!(market.is_binary());
            assert_eq!(market.num_outcomes(), 2);
        }
    }

    #[test]
    fn test_mega_v2_mm_constraints_have_orders() {
        let config = MegaScenarioConfigV2::medium();
        let problem = generate_mega_scenario_v2(config);

        for mm in &problem.mm_constraints {
            assert!(mm.num_orders() > 0, "MM {} should have orders", mm.mm_id.0);
        }
    }
}
