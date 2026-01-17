//! Agent-based displacement tax simulation
//!
//! This module implements a simulation framework to study the equilibrium
//! between passive LPs and JIT market makers under different tax structures.

pub mod agents;
pub mod tax;
pub mod metrics;
pub mod market_structure;

use rand::Rng;
use rand_chacha::ChaCha8Rng;
use std::collections::HashMap;

use crate::simulation::agents::{AgentId, PassiveMM, JitMM, NoiseTrader};
use crate::simulation::tax::TaxCalculator;
use crate::simulation::metrics::{MetricsCollector, RoundMetrics};

/// Price/value representation in basis points (10000 = 1.0 = 100%)
pub type Bps = u64;

/// Quantity representation (units)
pub type Qty = u64;

/// Side of an order
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

/// An order in the simulation
#[derive(Debug, Clone)]
pub struct SimOrder {
    pub id: u64,
    pub agent_id: AgentId,
    pub side: Side,
    pub quantity: Qty,
    pub limit_price: Bps,
    pub is_jit: bool,
}

/// A fill (execution) in the simulation
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SimFill {
    pub order_id: u64,
    pub agent_id: AgentId,
    pub quantity: Qty,
    pub price: Bps,
}

/// Solution to a batch auction
#[derive(Debug, Clone)]
pub struct SimSolution {
    pub clearing_price: Bps,
    pub fills: Vec<SimFill>,
    pub total_volume: Qty,
}

impl SimSolution {
    pub fn empty() -> Self {
        SimSolution {
            clearing_price: 0,
            fills: vec![],
            total_volume: 0,
        }
    }
}

/// Configuration for the simulation
#[derive(Debug, Clone)]
pub struct SimulationConfig {
    pub num_rounds: u64,
    pub num_passive_lps: usize,
    pub num_jit_mms: usize,
    pub num_noise_traders: usize,

    /// True value starts at this price (in basis points)
    pub true_value_mean: Bps,
    /// Volatility per round (in basis points)
    pub true_value_volatility: Bps,

    /// LP spread around true value (in basis points)
    pub lp_spread_bps: Bps,
    /// LP order size
    pub lp_order_size: Qty,

    /// Minimum profit threshold for JIT MM to participate (in basis points)
    pub jit_profit_threshold_bps: Bps,

    /// Mean noise trader order size
    pub noise_size_mean: Qty,
    /// Std dev of noise trader order size
    pub noise_size_stddev: Qty,

    /// Seed for reproducibility
    pub seed: u64,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        SimulationConfig {
            num_rounds: 10_000,
            num_passive_lps: 5,
            num_jit_mms: 1,
            num_noise_traders: 10,
            true_value_mean: 5000,       // 0.50
            true_value_volatility: 100,  // ±1% per round
            lp_spread_bps: 100,          // 1% spread
            lp_order_size: 100,
            jit_profit_threshold_bps: 10, // Min 10 bps profit
            noise_size_mean: 100,
            noise_size_stddev: 50,
            seed: 42,
        }
    }
}


/// The main simulation engine
pub struct Simulation<T: TaxCalculator> {
    pub config: SimulationConfig,
    pub tax_calculator: T,
    pub rng: ChaCha8Rng,
    pub metrics: MetricsCollector,

    // Agents
    pub passive_mms: Vec<PassiveMM>,
    pub jit_mms: Vec<JitMM>,
    pub noise_traders: Vec<NoiseTrader>,

    // State
    pub current_round: u64,
    pub current_true_value: Bps,
    next_order_id: u64,
}

impl<T: TaxCalculator> Simulation<T> {
    pub fn new(config: SimulationConfig, tax_calculator: T) -> Self {
        use rand::SeedableRng;

        let rng = ChaCha8Rng::seed_from_u64(config.seed);

        // Create agents
        let passive_mms: Vec<PassiveMM> = (0..config.num_passive_lps)
            .map(|i| PassiveMM::new(
                AgentId(i as u64),
                config.lp_spread_bps,
                config.lp_order_size,
            ))
            .collect();

        let jit_mms: Vec<JitMM> = (0..config.num_jit_mms)
            .map(|i| JitMM::new(
                AgentId((config.num_passive_lps + i) as u64),
                config.jit_profit_threshold_bps,
            ))
            .collect();

        let noise_traders: Vec<NoiseTrader> = (0..config.num_noise_traders)
            .map(|i| NoiseTrader::new(
                AgentId((config.num_passive_lps + config.num_jit_mms + i) as u64),
                config.noise_size_mean,
                config.noise_size_stddev,
            ))
            .collect();

        Simulation {
            config: config.clone(),
            tax_calculator,
            rng,
            metrics: MetricsCollector::new(),
            passive_mms,
            jit_mms,
            noise_traders,
            current_round: 0,
            current_true_value: config.true_value_mean,
            next_order_id: 0,
        }
    }

    fn next_order_id(&mut self) -> u64 {
        let id = self.next_order_id;
        self.next_order_id += 1;
        id
    }

    /// Update true value with random walk
    fn update_true_value(&mut self) {
        let vol = self.config.true_value_volatility as i64;
        let change: i64 = self.rng.gen_range(-vol..=vol);
        let new_value = (self.current_true_value as i64 + change).max(1000).min(9000) as Bps;
        self.current_true_value = new_value;
    }

    /// Run a single round of the simulation
    pub fn run_round(&mut self) -> RoundMetrics {
        self.current_round += 1;

        // 1. Update true value
        self.update_true_value();
        let true_value = self.current_true_value;

        // 2. Collect passive MM orders
        let mut orders: Vec<SimOrder> = Vec::new();

        // Pre-calculate order IDs to avoid borrow issues
        let mm_count = self.passive_mms.len();
        let mut mm_order_ids: Vec<(u64, u64)> = Vec::with_capacity(mm_count);
        for _ in 0..mm_count {
            let id1 = self.next_order_id();
            let id2 = self.next_order_id();
            mm_order_ids.push((id1, id2));
        }

        for (i, mm) in self.passive_mms.iter().enumerate() {
            let (id1, id2) = mm_order_ids[i];
            let mut id_iter = [id1, id2].into_iter();
            let mm_orders = mm.generate_orders(true_value, &mut || id_iter.next().unwrap());
            orders.extend(mm_orders);
        }

        // 3. Collect noise trader orders
        let noise_count = self.noise_traders.len();
        let mut noise_order_ids: Vec<u64> = Vec::with_capacity(noise_count);
        for _ in 0..noise_count {
            noise_order_ids.push(self.next_order_id());
        }

        for (i, trader) in self.noise_traders.iter().enumerate() {
            let order_id = noise_order_ids[i];
            if let Some(order) = trader.generate_order(true_value, &mut self.rng, &mut || order_id) {
                orders.push(order);
            }
        }

        // 4. Solve base batch (without JIT)
        let base_solution = solve_batch_u64(&orders);

        // 5. JIT MMs observe and decide whether to participate
        let mut jit_orders: Vec<SimOrder> = Vec::new();

        // Pre-allocate JIT order IDs
        let jit_count = self.jit_mms.len();
        let mut jit_order_ids: Vec<u64> = Vec::with_capacity(jit_count);
        for _ in 0..jit_count {
            jit_order_ids.push(self.next_order_id());
        }

        for (i, mm) in self.jit_mms.iter().enumerate() {
            let order_id = jit_order_ids[i];
            if let Some(jit_order) = mm.decide_jit(
                true_value,
                &orders,
                &base_solution,
                &self.tax_calculator,
                &mut || order_id,
            ) {
                jit_orders.push(jit_order);
            }
        }

        // 6. Solve final batch with JIT
        let mut all_orders = orders.clone();
        all_orders.extend(jit_orders.clone());
        let final_solution = solve_batch_u64(&all_orders);

        // 7. Calculate displacement
        let displacement = calculate_displacement(&base_solution, &final_solution, &orders);

        // 8. Apply tax
        let tax_result = self.tax_calculator.calculate_tax(
            &displacement,
            &final_solution,
            true_value,
        );

        // 9. Calculate agent P&Ls
        let passive_lp_pnl = calculate_passive_lp_pnl(&final_solution, &orders, true_value);
        let jit_mm_pnl = calculate_jit_pnl(&final_solution, &jit_orders, true_value) - tax_result.total_tax as i64;

        // 10. Calculate welfare metrics
        let price_impact_bps = if true_value > 0 {
            (final_solution.clearing_price as i64 - true_value as i64).abs() as f64
        } else {
            0.0
        };

        // Track user orders (noise traders act as users for now)
        // In the future, this can track actual User agent orders
        let user_orders: Vec<&SimOrder> = orders.iter()
            .filter(|o| !o.is_jit)
            .collect();
        let user_orders_submitted = user_orders.len() as u64;
        let user_order_qty: Qty = user_orders.iter().map(|o| o.quantity).sum();

        let user_orders_filled = user_orders.iter()
            .filter(|o| final_solution.fills.iter().any(|f| f.order_id == o.id))
            .count() as u64;
        let user_qty_filled: Qty = user_orders.iter()
            .filter_map(|o| final_solution.fills.iter().find(|f| f.order_id == o.id))
            .map(|f| f.quantity)
            .sum();

        // 11. Record metrics
        let metrics = RoundMetrics {
            round: self.current_round,
            true_value,
            clearing_price: final_solution.clearing_price,
            total_volume: final_solution.total_volume,
            jit_participated: !jit_orders.is_empty(),
            jit_volume: jit_orders.iter()
                .filter_map(|o| final_solution.fills.iter().find(|f| f.order_id == o.id))
                .map(|f| f.quantity)
                .sum(),
            displacement_qty: displacement.values().sum(),
            tax_collected: tax_result.total_tax,
            tax_rate_bps: tax_result.effective_rate_bps,
            passive_lp_pnl,
            jit_mm_pnl,
            price_impact_bps,
            user_orders_submitted,
            user_orders_filled,
            user_order_qty,
            user_qty_filled,
        };

        // Update tax calculator state (for dynamic tax)
        self.tax_calculator.update(
            !jit_orders.is_empty(),
            displacement.values().sum(),
        );

        self.metrics.record_round(metrics.clone());

        metrics
    }

    /// Run the full simulation
    pub fn run(&mut self) {
        for _ in 0..self.config.num_rounds {
            self.run_round();
        }
    }
}

/// Solve batch auction with u64 arithmetic
pub fn solve_batch_u64(orders: &[SimOrder]) -> SimSolution {
    if orders.is_empty() {
        return SimSolution::empty();
    }

    // Collect all limit prices as candidates
    let mut candidate_prices: Vec<Bps> = orders.iter().map(|o| o.limit_price).collect();
    candidate_prices.sort();
    candidate_prices.dedup();

    if candidate_prices.is_empty() {
        return SimSolution::empty();
    }

    // Check if any crossing is possible
    let max_bid = orders.iter()
        .filter(|o| o.side == Side::Buy)
        .map(|o| o.limit_price)
        .max();
    let min_ask = orders.iter()
        .filter(|o| o.side == Side::Sell)
        .map(|o| o.limit_price)
        .min();

    if let (Some(mb), Some(ma)) = (max_bid, min_ask) {
        if mb < ma {
            return SimSolution::empty();
        }
    }

    // Find price that maximizes volume
    let mut best_volume: Qty = 0;
    let mut best_prices: Vec<Bps> = vec![];

    for &price in &candidate_prices {
        let (demand, supply) = calculate_demand_supply(orders, price);
        let volume = demand.min(supply);

        if volume > best_volume {
            best_volume = volume;
            best_prices.clear();
            best_prices.push(price);
        } else if volume == best_volume && best_volume > 0 {
            best_prices.push(price);
        }
    }

    if best_prices.is_empty() || best_volume == 0 {
        return SimSolution::empty();
    }

    // Use midpoint of valid range
    let min_valid = *best_prices.first().unwrap();
    let max_valid = *best_prices.last().unwrap();
    let clearing_price = (min_valid + max_valid) / 2;

    // Clear at the chosen price
    clear_at_price_u64(orders, clearing_price)
}

fn calculate_demand_supply(orders: &[SimOrder], price: Bps) -> (Qty, Qty) {
    let demand: Qty = orders.iter()
        .filter(|o| o.side == Side::Buy && o.limit_price >= price)
        .map(|o| o.quantity)
        .sum();

    let supply: Qty = orders.iter()
        .filter(|o| o.side == Side::Sell && o.limit_price <= price)
        .map(|o| o.quantity)
        .sum();

    (demand, supply)
}

fn clear_at_price_u64(orders: &[SimOrder], price: Bps) -> SimSolution {
    let (demand, supply) = calculate_demand_supply(orders, price);
    let volume = demand.min(supply);

    if volume == 0 {
        return SimSolution {
            clearing_price: price,
            fills: vec![],
            total_volume: 0,
        };
    }

    let mut fills = vec![];

    // Fill buys (pro-rata if needed)
    let buy_orders: Vec<&SimOrder> = orders.iter()
        .filter(|o| o.side == Side::Buy && o.limit_price >= price)
        .collect();

    for order in &buy_orders {
        let fill_qty = if demand > volume {
            // Pro-rata: order.quantity * volume / demand
            (order.quantity as u128 * volume as u128 / demand as u128) as Qty
        } else {
            order.quantity
        };

        if fill_qty > 0 {
            fills.push(SimFill {
                order_id: order.id,
                agent_id: order.agent_id,
                quantity: fill_qty,
                price,
            });
        }
    }

    // Fill sells (pro-rata if needed)
    let sell_orders: Vec<&SimOrder> = orders.iter()
        .filter(|o| o.side == Side::Sell && o.limit_price <= price)
        .collect();

    for order in &sell_orders {
        let fill_qty = if supply > volume {
            (order.quantity as u128 * volume as u128 / supply as u128) as Qty
        } else {
            order.quantity
        };

        if fill_qty > 0 {
            fills.push(SimFill {
                order_id: order.id,
                agent_id: order.agent_id,
                quantity: fill_qty,
                price,
            });
        }
    }

    SimSolution {
        clearing_price: price,
        fills,
        total_volume: volume,
    }
}

/// Calculate how much each passive LP was displaced by JIT
fn calculate_displacement(
    base: &SimSolution,
    final_sol: &SimSolution,
    passive_orders: &[SimOrder],
) -> HashMap<AgentId, Qty> {
    let mut displacement: HashMap<AgentId, Qty> = HashMap::new();

    for order in passive_orders {
        if order.is_jit {
            continue; // Skip JIT orders
        }

        let base_fill = base.fills.iter()
            .find(|f| f.order_id == order.id)
            .map(|f| f.quantity)
            .unwrap_or(0);

        let final_fill = final_sol.fills.iter()
            .find(|f| f.order_id == order.id)
            .map(|f| f.quantity)
            .unwrap_or(0);

        if final_fill < base_fill {
            let displaced = base_fill - final_fill;
            *displacement.entry(order.agent_id).or_insert(0) += displaced;
        }
    }

    displacement
}

/// Calculate P&L for passive LPs
fn calculate_passive_lp_pnl(
    solution: &SimSolution,
    orders: &[SimOrder],
    true_value: Bps,
) -> i64 {
    let mut total_pnl: i64 = 0;

    for order in orders {
        if order.is_jit {
            continue;
        }

        if let Some(fill) = solution.fills.iter().find(|f| f.order_id == order.id) {
            // P&L = (exec_price - true_value) * qty for sells
            // P&L = (true_value - exec_price) * qty for buys
            let pnl = match order.side {
                Side::Sell => (fill.price as i64 - true_value as i64) * fill.quantity as i64,
                Side::Buy => (true_value as i64 - fill.price as i64) * fill.quantity as i64,
            };
            total_pnl += pnl;
        }
    }

    total_pnl
}

/// Calculate P&L for JIT orders
fn calculate_jit_pnl(
    solution: &SimSolution,
    jit_orders: &[SimOrder],
    true_value: Bps,
) -> i64 {
    let mut total_pnl: i64 = 0;

    for order in jit_orders {
        if let Some(fill) = solution.fills.iter().find(|f| f.order_id == order.id) {
            let pnl = match order.side {
                Side::Sell => (fill.price as i64 - true_value as i64) * fill.quantity as i64,
                Side::Buy => (true_value as i64 - fill.price as i64) * fill.quantity as i64,
            };
            total_pnl += pnl;
        }
    }

    total_pnl
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_batch_clearing() {
        let orders = vec![
            SimOrder {
                id: 1,
                agent_id: AgentId(0),
                side: Side::Buy,
                quantity: 100,
                limit_price: 6000, // 0.60
                is_jit: false,
            },
            SimOrder {
                id: 2,
                agent_id: AgentId(1),
                side: Side::Sell,
                quantity: 100,
                limit_price: 4000, // 0.40
                is_jit: false,
            },
        ];

        let solution = solve_batch_u64(&orders);

        assert_eq!(solution.clearing_price, 5000); // Midpoint
        assert_eq!(solution.total_volume, 100);
        assert_eq!(solution.fills.len(), 2);
    }

    #[test]
    fn test_no_crossing() {
        let orders = vec![
            SimOrder {
                id: 1,
                agent_id: AgentId(0),
                side: Side::Buy,
                quantity: 100,
                limit_price: 4000, // 0.40
                is_jit: false,
            },
            SimOrder {
                id: 2,
                agent_id: AgentId(1),
                side: Side::Sell,
                quantity: 100,
                limit_price: 6000, // 0.60
                is_jit: false,
            },
        ];

        let solution = solve_batch_u64(&orders);

        assert_eq!(solution.total_volume, 0);
    }

    #[test]
    fn test_pro_rata_fill() {
        let orders = vec![
            SimOrder {
                id: 1,
                agent_id: AgentId(0),
                side: Side::Buy,
                quantity: 100,
                limit_price: 5500,
                is_jit: false,
            },
            SimOrder {
                id: 2,
                agent_id: AgentId(1),
                side: Side::Buy,
                quantity: 100,
                limit_price: 5500,
                is_jit: false,
            },
            SimOrder {
                id: 3,
                agent_id: AgentId(2),
                side: Side::Sell,
                quantity: 100,
                limit_price: 4500,
                is_jit: false,
            },
        ];

        let solution = solve_batch_u64(&orders);

        assert_eq!(solution.total_volume, 100);

        let fill1 = solution.fills.iter().find(|f| f.order_id == 1).unwrap();
        let fill2 = solution.fills.iter().find(|f| f.order_id == 2).unwrap();

        // Pro-rata: each buyer gets 50
        assert_eq!(fill1.quantity, 50);
        assert_eq!(fill2.quantity, 50);
    }
}
