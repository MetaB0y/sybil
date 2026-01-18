//! Agent implementations for the displacement tax simulation
//!
//! Agent types:
//! - User: End users who want to trade (simpler than NoiseTrader)
//! - PassiveMM: Traditional market maker, posts quotes pre-batch
//! - JitMM: Just-In-Time market maker, sees orderbook after seal
//! - NoiseTrader: Random market orders (legacy, for compatibility)

use rand::Rng;
use std::collections::HashMap;

use super::{Bps, Qty, Side, SimOrder, SimSolution};
use super::tax::TaxCalculator;

/// Unique identifier for an agent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentId(pub u64);

/// JIT strategy determines how the JIT MM participates
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JitStrategy {
    /// Only fill unfilled demand (backrun) - cannot displace passive orders
    BackrunOnly,
    /// Can displace passive orders but pays a tax
    DisplacementAllowed,
}

/// Passive MM: Traditional market maker, posts limit orders at fair_value +/- spread
///
/// Strategy: Always participates, posts both bid and ask
/// at (true_value - spread/2) and (true_value + spread/2)
#[derive(Debug, Clone)]
pub struct PassiveMM {
    pub id: AgentId,
    /// Spread around true value (half-spread on each side)
    pub spread_bps: Bps,
    /// Size of each order
    pub order_size: Qty,
}

impl PassiveMM {
    pub fn new(id: AgentId, spread_bps: Bps, order_size: Qty) -> Self {
        PassiveMM {
            id,
            spread_bps,
            order_size,
        }
    }

    /// Generate bid and ask orders around the true value
    pub fn generate_orders<F>(&self, true_value: Bps, mut next_id: F) -> Vec<SimOrder>
    where
        F: FnMut() -> u64,
    {
        let half_spread = self.spread_bps / 2;

        // Bid: willing to buy at true_value - half_spread
        let bid_price = true_value.saturating_sub(half_spread);
        // Ask: willing to sell at true_value + half_spread
        let ask_price = true_value + half_spread;

        vec![
            SimOrder {
                id: next_id(),
                agent_id: self.id,
                side: Side::Buy,
                quantity: self.order_size,
                limit_price: bid_price,
                is_jit: false,
            },
            SimOrder {
                id: next_id(),
                agent_id: self.id,
                side: Side::Sell,
                quantity: self.order_size,
                limit_price: ask_price,
                is_jit: false,
            },
        ]
    }
}


/// Noise Trader: Submits random market orders
///
/// Strategy: With 50% probability buy, 50% sell
/// Size drawn from normal distribution
#[derive(Debug, Clone)]
pub struct NoiseTrader {
    pub id: AgentId,
    /// Mean order size
    pub size_mean: Qty,
    /// Standard deviation of order size
    pub size_stddev: Qty,
}

impl NoiseTrader {
    pub fn new(id: AgentId, size_mean: Qty, size_stddev: Qty) -> Self {
        NoiseTrader {
            id,
            size_mean,
            size_stddev,
        }
    }

    /// Generate a random market order
    pub fn generate_order<R, F>(&self, true_value: Bps, rng: &mut R, mut next_id: F) -> Option<SimOrder>
    where
        R: Rng,
        F: FnMut() -> u64,
    {
        // 50% probability of participating
        if !rng.gen_bool(0.5) {
            return None;
        }

        // Random side
        let side = if rng.gen_bool(0.5) { Side::Buy } else { Side::Sell };

        // Random size (normal distribution, truncated at 1)
        let stddev = self.size_stddev as f64;
        let mean = self.size_mean as f64;
        let size: f64 = mean + stddev * rng.gen_range(-2.0..2.0);
        let size = size.max(1.0) as Qty;

        // Market order: aggressive price to ensure fill
        let limit_price = match side {
            Side::Buy => true_value + 1000,  // Very aggressive buy
            Side::Sell => true_value.saturating_sub(1000), // Very aggressive sell
        };

        Some(SimOrder {
            id: next_id(),
            agent_id: self.id,
            side,
            quantity: size,
            limit_price,
            is_jit: false,
        })
    }
}


/// JIT Market Maker: Observes the orderbook and decides to participate
///
/// Strategy determined by JitStrategy:
/// - BackrunOnly: Can only fill unfilled demand, no displacement
/// - DisplacementAllowed: Can displace passive orders (pays tax)
///
#[derive(Debug, Clone)]
pub struct JitMM {
    pub id: AgentId,
    /// Which strategy this JIT MM uses
    pub strategy: JitStrategy,
    /// Minimum profit threshold to participate (in basis points)
    pub profit_threshold_bps: Bps,
}

impl JitMM {
    pub fn new(id: AgentId, profit_threshold_bps: Bps) -> Self {
        JitMM {
            id,
            strategy: JitStrategy::DisplacementAllowed,
            profit_threshold_bps,
        }
    }

    pub fn with_strategy(id: AgentId, strategy: JitStrategy, profit_threshold_bps: Bps) -> Self {
        JitMM {
            id,
            strategy,
            profit_threshold_bps,
        }
    }

    /// Analyze the opportunity and decide whether to submit JIT order
    ///
    /// Behavior depends on strategy:
    /// - BackrunOnly: Only fills unfilled demand/supply
    /// - DisplacementAllowed: Tries aggressive displacement first, then backrun
    pub fn decide_jit<T, F>(
        &self,
        true_value: Bps,
        passive_orders: &[SimOrder],
        base_solution: &SimSolution,
        tax_calculator: &T,
        mut next_id: F,
    ) -> Option<SimOrder>
    where
        T: TaxCalculator,
        F: FnMut() -> u64,
    {
        // No clearing happened, can't JIT
        if base_solution.total_volume == 0 {
            return None;
        }

        match self.strategy {
            JitStrategy::BackrunOnly => {
                // Can only fill unfilled demand - no displacement allowed
                self.try_backrun(true_value, passive_orders, base_solution, tax_calculator, next_id)
            }
            JitStrategy::DisplacementAllowed => {
                // Try aggressive displacement first (max profit), fall back to backrun
                if let Some(order) = self.try_aggressive_displacement(
                    true_value,
                    passive_orders,
                    base_solution,
                    tax_calculator,
                    &mut next_id,
                ) {
                    return Some(order);
                }
                // Fall back to backrun
                self.try_backrun(true_value, passive_orders, base_solution, tax_calculator, next_id)
            }
        }
    }

    /// Try to fill unfilled demand/supply (backrun strategy)
    fn try_backrun<T, F>(
        &self,
        true_value: Bps,
        passive_orders: &[SimOrder],
        base_solution: &SimSolution,
        tax_calculator: &T,
        mut next_id: F,
    ) -> Option<SimOrder>
    where
        T: TaxCalculator,
        F: FnMut() -> u64,
    {
        let clearing_price = base_solution.clearing_price;
        let (unfilled_buy, unfilled_sell) = self.analyze_unfilled(passive_orders, base_solution);

        // Decide which side to take for backrun
        let (side, jit_qty) = if unfilled_buy > unfilled_sell {
            (Side::Sell, unfilled_buy)
        } else if unfilled_sell > unfilled_buy {
            (Side::Buy, unfilled_sell)
        } else if unfilled_buy > 0 {
            (Side::Sell, unfilled_buy)
        } else {
            return None;
        };

        if jit_qty == 0 {
            return None;
        }

        // Calculate expected profit for backrun
        let expected_profit = match side {
            Side::Sell => (clearing_price as i64 - true_value as i64) * jit_qty as i64,
            Side::Buy => (true_value as i64 - clearing_price as i64) * jit_qty as i64,
        };

        // No displacement for backrun
        let estimated_displacement: HashMap<AgentId, Qty> = HashMap::new();
        let tax_result = tax_calculator.calculate_tax(
            &estimated_displacement,
            base_solution,
            true_value,
        );

        let profit_after_tax = expected_profit - tax_result.total_tax as i64;

        if profit_after_tax > (self.profit_threshold_bps * jit_qty) as i64 {
            Some(SimOrder {
                id: next_id(),
                agent_id: self.id,
                side,
                quantity: jit_qty,
                limit_price: clearing_price,
                is_jit: true,
            })
        } else {
            None
        }
    }

    /// Analyze unfilled demand/supply in the base solution
    fn analyze_unfilled(
        &self,
        orders: &[SimOrder],
        solution: &SimSolution,
    ) -> (Qty, Qty) {
        let price = solution.clearing_price;

        let mut unfilled_buy: Qty = 0;
        let mut unfilled_sell: Qty = 0;

        for order in orders {
            if order.is_jit {
                continue;
            }

            let filled = solution.fills.iter()
                .find(|f| f.order_id == order.id)
                .map(|f| f.quantity)
                .unwrap_or(0);

            let unfilled = order.quantity.saturating_sub(filled);

            match order.side {
                Side::Buy if order.limit_price >= price => {
                    unfilled_buy += unfilled;
                }
                Side::Sell if order.limit_price <= price => {
                    unfilled_sell += unfilled;
                }
                _ => {}
            }
        }

        (unfilled_buy, unfilled_sell)
    }

    /// Try aggressive displacement strategy
    fn try_aggressive_displacement<T, F>(
        &self,
        true_value: Bps,
        passive_orders: &[SimOrder],
        base_solution: &SimSolution,
        tax_calculator: &T,
        mut next_id: F,
    ) -> Option<SimOrder>
    where
        T: TaxCalculator,
        F: FnMut() -> u64,
    {
        let clearing_price = base_solution.clearing_price;

        // Find passive volume on each side
        let passive_sell_volume: Qty = passive_orders.iter()
            .filter(|o| !o.is_jit && o.side == Side::Sell && o.limit_price <= clearing_price)
            .map(|o| o.quantity)
            .sum();

        let passive_buy_volume: Qty = passive_orders.iter()
            .filter(|o| !o.is_jit && o.side == Side::Buy && o.limit_price >= clearing_price)
            .map(|o| o.quantity)
            .sum();

        // Try to capture half of passive volume
        let (side, target_qty) = if passive_sell_volume >= passive_buy_volume && passive_sell_volume > 0 {
            (Side::Sell, passive_sell_volume / 2)
        } else if passive_buy_volume > 0 {
            (Side::Buy, passive_buy_volume / 2)
        } else {
            return None;
        };

        if target_qty == 0 {
            return None;
        }

        // Calculate expected profit
        let expected_profit = match side {
            Side::Sell => (clearing_price as i64 - true_value as i64) * target_qty as i64,
            Side::Buy => (true_value as i64 - clearing_price as i64) * target_qty as i64,
        };

        // Estimate displacement
        let mut estimated_displacement: HashMap<AgentId, Qty> = HashMap::new();
        for order in passive_orders.iter().filter(|o| !o.is_jit && o.side == side) {
            *estimated_displacement.entry(order.agent_id).or_insert(0) += order.quantity / 2;
        }

        // Calculate tax
        let tax_result = tax_calculator.calculate_tax(
            &estimated_displacement,
            base_solution,
            true_value,
        );

        let profit_after_tax = expected_profit - tax_result.total_tax as i64;

        // Only participate if profitable enough
        if profit_after_tax > (self.profit_threshold_bps * target_qty) as i64 {
            Some(SimOrder {
                id: next_id(),
                agent_id: self.id,
                side,
                quantity: target_qty,
                limit_price: clearing_price,
                is_jit: true,
            })
        } else {
            None
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_passive_mm_orders() {
        let mm = PassiveMM::new(AgentId(1), 100, 50);
        let mut id_counter = 0u64;

        let orders = mm.generate_orders(5000, || {
            let id = id_counter;
            id_counter += 1;
            id
        });

        assert_eq!(orders.len(), 2);

        // Buy order at 4950 (5000 - 50)
        let buy = orders.iter().find(|o| o.side == Side::Buy).unwrap();
        assert_eq!(buy.limit_price, 4950);
        assert_eq!(buy.quantity, 50);

        // Sell order at 5050 (5000 + 50)
        let sell = orders.iter().find(|o| o.side == Side::Sell).unwrap();
        assert_eq!(sell.limit_price, 5050);
        assert_eq!(sell.quantity, 50);
    }

    #[test]
    fn test_noise_trader_order() {
        use rand::SeedableRng;
        use rand_chacha::ChaCha8Rng;

        let trader = NoiseTrader::new(AgentId(2), 100, 20);
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        let mut id_counter = 0u64;

        // Generate multiple orders to test randomness
        let mut orders_generated = 0;
        for _ in 0..100 {
            if trader.generate_order(5000, &mut rng, || {
                let id = id_counter;
                id_counter += 1;
                id
            }).is_some() {
                orders_generated += 1;
            }
        }

        // Should generate roughly 50% of the time
        assert!(orders_generated > 30 && orders_generated < 70);
    }

    #[test]
    fn test_jit_strategy_backrun_only() {
        let jit = JitMM::with_strategy(AgentId(1), JitStrategy::BackrunOnly, 10);
        assert_eq!(jit.strategy, JitStrategy::BackrunOnly);
    }

    #[test]
    fn test_jit_strategy_displacement() {
        let jit = JitMM::with_strategy(AgentId(1), JitStrategy::DisplacementAllowed, 10);
        assert_eq!(jit.strategy, JitStrategy::DisplacementAllowed);
    }
}
