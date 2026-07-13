//! Structured shared-capital stress books for solver evaluation.
//!
//! Opportunities alternate between an MM YES bid paired with a retail NO bid
//! and an MM YES ask paired with a retail YES bid. The former mints a complete
//! set; the latter exercises the paper's sell-to-complementary-buy reduction.
//! Both directions share one MM budget and have a declared ladder of returns.
//! A plain LP accepts every positive-surplus pair; retained-cash clearing
//! applies one pacing cutoff across all markets covered by the same MM. This
//! makes the theorem's intended regime visible without selecting favorable
//! random seeds after a benchmark is run.

use rand::prelude::*;
use rand_chacha::ChaCha8Rng;

use matching_engine::{
    MmConstraint, MmId, MmSide, NANOS_PER_DOLLAR, Nanos, Problem, outcome_buy, outcome_sell,
    price_to_nanos, shares_to_qty,
};

#[derive(Clone, Debug)]
pub struct FlashLiquidityConfig {
    pub seed: u64,
    pub num_markets: usize,
    pub opportunities_per_market: usize,
    pub num_mms: usize,
    pub quantity_min_shares: u64,
    pub quantity_max_shares: u64,
    /// Initial placeholder budget. Preregistered experiments normally replace
    /// it with a declared fraction of unconstrained LP limit-value.
    pub initial_budget_dollars: u64,
}

impl Default for FlashLiquidityConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            num_markets: 20,
            opportunities_per_market: 4,
            num_mms: 1,
            quantity_min_shares: 25,
            quantity_max_shares: 100,
            initial_budget_dollars: 1_000,
        }
    }
}

pub fn generate_flash_liquidity_scenario(config: FlashLiquidityConfig) -> Problem {
    assert!(config.num_markets > 0);
    assert!(config.opportunities_per_market > 0);
    assert!(config.num_mms > 0);
    assert!(config.quantity_min_shares > 0);
    assert!(config.quantity_max_shares >= config.quantity_min_shares);

    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
    let mut problem = Problem::new(format!(
        "FlashLiquidity(m={},levels={},mms={})",
        config.num_markets, config.opportunities_per_market, config.num_mms
    ));
    let markets: Vec<_> = (0..config.num_markets)
        .map(|index| problem.markets.add_binary(format!("flash-{index}")))
        .collect();
    let mut constraints: Vec<_> = (0..config.num_mms)
        .map(|index| {
            MmConstraint::new(
                MmId::new(index as u64 + 1),
                Nanos(config.initial_budget_dollars * NANOS_PER_DOLLAR),
            )
        })
        .collect();

    let total_levels = config
        .num_markets
        .saturating_mul(config.opportunities_per_market)
        .max(1);
    let mut order_id = 1u64;
    for (market_index, &market) in markets.iter().enumerate() {
        for level in 0..config.opportunities_per_market {
            let rank = market_index * config.opportunities_per_market + level;
            let fraction = if total_levels == 1 {
                0.5
            } else {
                rank as f64 / (total_levels - 1) as f64
            };
            // All pairs have positive LP surplus. The range creates a strict
            // return ordering for the retained-cash pacing cutoff.
            let mm_limit = 0.50 + 0.38 * fraction;
            let retail_limit = 0.56 + rng.random_range(-0.005..0.005);
            let quantity_shares = if config.quantity_max_shares == config.quantity_min_shares {
                config.quantity_min_shares
            } else {
                rng.random_range(config.quantity_min_shares..=config.quantity_max_shares)
            };
            let quantity = shares_to_qty(quantity_shares).0;

            let mm_order_id = order_id;
            let mm_index = market_index % config.num_mms;
            if rank.is_multiple_of(2) {
                problem.orders.push(outcome_buy(
                    &problem.markets,
                    mm_order_id,
                    market,
                    0,
                    price_to_nanos(mm_limit).0,
                    quantity,
                ));
                constraints[mm_index].add_order(mm_order_id, MmSide::BuyYes);
            } else {
                problem.orders.push(outcome_sell(
                    &problem.markets,
                    mm_order_id,
                    market,
                    0,
                    price_to_nanos(1.0 - mm_limit).0,
                    quantity,
                ));
                constraints[mm_index].add_order(mm_order_id, MmSide::SellYes);
            }
            order_id += 1;

            problem.orders.push(outcome_buy(
                &problem.markets,
                order_id,
                market,
                if rank.is_multiple_of(2) { 1 } else { 0 },
                price_to_nanos(retail_limit).0,
                quantity,
            ));
            order_id += 1;
        }
    }

    problem.mm_constraints = constraints;
    problem.orders.shuffle(&mut rng);
    problem
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::SHARE_SCALE;

    #[test]
    fn generated_book_has_declared_shared_budget_shape() {
        let config = FlashLiquidityConfig {
            num_markets: 8,
            opportunities_per_market: 3,
            num_mms: 2,
            ..Default::default()
        };
        let problem = generate_flash_liquidity_scenario(config);

        assert_eq!(problem.orders.len(), 8 * 3 * 2);
        assert_eq!(problem.mm_constraints.len(), 2);
        assert!(
            problem
                .orders
                .iter()
                .all(|order| order.max_fill.0 % SHARE_SCALE == 0)
        );
        assert_eq!(
            problem
                .mm_constraints
                .iter()
                .map(|mm| mm.order_ids.len())
                .sum::<usize>(),
            8 * 3
        );
        let sides: Vec<_> = problem
            .mm_constraints
            .iter()
            .flat_map(|mm| mm.order_sides.values().copied())
            .collect();
        assert!(sides.contains(&MmSide::BuyYes));
        assert!(sides.contains(&MmSide::SellYes));
    }

    #[test]
    fn seed_is_reproducible() {
        let config = FlashLiquidityConfig::default();
        let left = generate_flash_liquidity_scenario(config.clone());
        let right = generate_flash_liquidity_scenario(config);
        assert_eq!(
            left.orders
                .iter()
                .map(|order| (order.id, order.limit_price, order.max_fill))
                .collect::<Vec<_>>(),
            right
                .orders
                .iter()
                .map(|order| (order.id, order.limit_price, order.max_fill))
                .collect::<Vec<_>>()
        );
    }
}
