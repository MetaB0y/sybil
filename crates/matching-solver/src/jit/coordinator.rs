//! JIT Coordinator - orchestrates the JIT phase.
//!
//! The coordinator manages the flow:
//! 1. Build JitInput from base solution
//! 2. Collect submissions from providers
//! 3. Validate all submissions
//! 4. Select orders via price-priority
//! 5. Calculate tax and rebates
//! 6. Return JIT phase result

use std::cmp::Ordering;

use matching_engine::{Problem, Side};

use super::input::JitInput;
use super::provider::JitProvider;
use super::tax::{BatchTaxSummary, JitTaxCalculator};
use super::types::{
    BatchId, JitFill, JitPhaseResult, JitStats, JitType, ProviderId, ValidatedJit,
    ValidatedJitOrder,
};
use super::validator::JitValidator;
use crate::MatchingResult;

/// Configuration for the JIT coordinator.
#[derive(Clone, Debug)]
pub struct JitConfig {
    /// Whether JIT is enabled.
    pub enabled: bool,
    /// Maximum number of JIT orders to accept per batch.
    pub max_orders_per_batch: usize,
    /// Maximum total JIT volume as fraction of base solution volume.
    pub max_jit_volume_fraction: f64,
}

impl Default for JitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_orders_per_batch: 100,
            max_jit_volume_fraction: 0.50, // Max 50% of volume from JIT
        }
    }
}

impl JitConfig {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

/// Coordinates the JIT phase.
///
/// Decoupled from SolverPlatform - can be used independently or integrated.
pub struct JitCoordinator {
    config: JitConfig,
    validator: Box<dyn JitValidator>,
    tax_calculator: Box<dyn JitTaxCalculator>,
    providers: Vec<Box<dyn JitProvider>>,
}

impl JitCoordinator {
    /// Create a new coordinator with default components.
    pub fn new() -> Self {
        Self {
            config: JitConfig::default(),
            validator: Box::new(super::validator::DefaultValidator::new()),
            tax_calculator: Box::new(super::tax::FlatRateTaxCalculator::new()),
            providers: Vec::new(),
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: JitConfig) -> Self {
        Self {
            config,
            ..Self::new()
        }
    }

    /// Set the validator.
    pub fn with_validator(mut self, validator: Box<dyn JitValidator>) -> Self {
        self.validator = validator;
        self
    }

    /// Set the tax calculator.
    pub fn with_tax_calculator(mut self, calculator: Box<dyn JitTaxCalculator>) -> Self {
        self.tax_calculator = calculator;
        self
    }

    /// Add a JIT provider.
    pub fn add_provider(&mut self, provider: Box<dyn JitProvider>) {
        self.providers.push(provider);
    }

    /// Add a provider (builder pattern).
    pub fn with_provider(mut self, provider: Box<dyn JitProvider>) -> Self {
        self.providers.push(provider);
        self
    }

    /// Run the JIT phase.
    ///
    /// Takes the problem and base solution, returns improved result.
    pub fn run_jit_phase(
        &self,
        batch_id: BatchId,
        problem: &Problem,
        base_result: &MatchingResult,
    ) -> JitPhaseResult {
        if !self.config.enabled || self.providers.is_empty() {
            return JitPhaseResult::empty();
        }

        // 1. Build JitInput (anonymized)
        let input = JitInput::from_problem_and_solution(batch_id, problem, base_result);

        // 2. Collect submissions from all providers
        let submissions: Vec<_> = self
            .providers
            .iter()
            .map(|p| p.provide(&input))
            .collect();

        // 3. Validate all submissions
        let mut stats = JitStats {
            providers_submitted: submissions.len(),
            ..Default::default()
        };

        let mut validated: Vec<ValidatedJit> = Vec::new();
        for submission in &submissions {
            stats.orders_submitted += submission.num_orders();

            match self.validator.validate(&input, submission) {
                Ok(v) => validated.push(v),
                Err(rejection) => {
                    stats.record_rejection(&rejection);
                }
            }
        }

        // 4. Flatten all validated orders
        let all_orders: Vec<(ProviderId, ValidatedJitOrder)> = validated
            .iter()
            .flat_map(|v| {
                v.all_orders()
                    .map(|o| (v.provider_id, o.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();

        // 5. Select orders via price-priority
        let selected = self.select_orders_price_priority(&input, all_orders, &mut stats);

        // 6. Calculate tax and create fills
        let mut tax_summary = BatchTaxSummary::default();
        let mut fills = Vec::new();
        let rebates = Vec::new();

        for (provider_id, order) in &selected {
            let fill_price = order.order.price;
            let tax_result = self.tax_calculator.calculate_tax(order, fill_price);
            tax_summary.add(&tax_result, order);

            fills.push(JitFill {
                provider_id: *provider_id,
                market_id: order.order.market_id,
                side: order.order.side,
                fill_qty: order.order.quantity,
                fill_price,
                jit_type: order.jit_type,
                tax_paid: tax_result.tax_amount,
            });

            // Create rebates for displacement
            if order.jit_type == JitType::Displacement && tax_result.rebate_pool > 0 {
                // In a full implementation, we'd track which specific orders
                // were displaced and distribute rebates proportionally.
                // For now, we just record the rebate pool.
                // This would require additional tracking in the validator.
            }
        }

        // Update stats
        stats.backrun_orders_accepted = tax_summary.backrun_count;
        stats.displacement_orders_accepted = tax_summary.displacement_count;
        stats.backrun_volume = tax_summary.backrun_volume;
        stats.displacement_volume = tax_summary.displacement_volume;

        let welfare_improvement: i64 = selected.iter().map(|(_, o)| o.welfare_improvement).sum();

        JitPhaseResult {
            jit_fills: fills,
            welfare_improvement,
            total_tax: tax_summary.total_tax,
            rebates,
            stats,
        }
    }

    /// Select orders using price-priority matching.
    ///
    /// For each market, sort orders by price (best first) and select
    /// up to the unfilled demand.
    fn select_orders_price_priority(
        &self,
        input: &JitInput,
        mut orders: Vec<(ProviderId, ValidatedJitOrder)>,
        _stats: &mut JitStats,
    ) -> Vec<(ProviderId, ValidatedJitOrder)> {
        // Sort by price priority:
        // - Asks: lowest price first (better for buyers)
        // - Bids: highest price first (better for sellers)
        orders.sort_by(|a, b| {
            let order_a = &a.1.order;
            let order_b = &b.1.order;

            // First sort by market
            match order_a.market_id.0.cmp(&order_b.market_id.0) {
                Ordering::Equal => {}
                other => return other,
            }

            // Then by side
            match (&order_a.side, &order_b.side) {
                (Side::Ask, Side::Bid) => return Ordering::Less,
                (Side::Bid, Side::Ask) => return Ordering::Greater,
                _ => {}
            }

            // Then by price (best first)
            match order_a.side {
                Side::Ask => order_a.price.cmp(&order_b.price), // Lower is better
                Side::Bid => order_b.price.cmp(&order_a.price), // Higher is better
            }
        });

        let mut selected = Vec::new();
        let mut remaining_demand: std::collections::HashMap<_, _> = input
            .base_solution
            .unfilled_demand
            .iter()
            .map(|(k, v)| (*k, (v.buy_qty, v.sell_qty)))
            .collect();

        let max_total_volume =
            (input.base_solution.total_volume_filled as f64 * self.config.max_jit_volume_fraction)
                as u64;
        let mut total_volume = 0u64;

        for (provider_id, order) in orders {
            // Check limits
            if selected.len() >= self.config.max_orders_per_batch {
                break;
            }
            if total_volume >= max_total_volume {
                break;
            }

            let market_demand = remaining_demand
                .entry(order.order.market_id)
                .or_insert((0, 0));

            // Check if there's demand to fill
            let available = match order.order.side {
                Side::Ask => market_demand.0, // Fill buy demand
                Side::Bid => market_demand.1, // Fill sell demand
            };

            // For backrun, only fill up to unfilled demand
            // For displacement, allow more (already validated)
            let fill_qty = if order.jit_type == JitType::Backrun {
                order.order.quantity.min(available)
            } else {
                order.order.quantity
            };

            if fill_qty == 0 {
                continue;
            }

            // Update remaining demand
            match order.order.side {
                Side::Ask => market_demand.0 = market_demand.0.saturating_sub(fill_qty),
                Side::Bid => market_demand.1 = market_demand.1.saturating_sub(fill_qty),
            }

            total_volume += fill_qty;

            // Create adjusted order with actual fill quantity
            let mut filled_order = order.clone();
            filled_order.order.quantity = fill_qty;

            selected.push((provider_id, filled_order));
        }

        selected
    }
}

impl Default for JitCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::simple::SimpleJitProvider;
    use crate::jit::tax::FlatRateTaxCalculator;
    use crate::jit::validator::DefaultValidator;
    use matching_engine::{MarketId, LiquidityPool};

    fn create_test_problem() -> Problem {
        let mut problem = Problem::new("test");
        let market = problem.markets.add_binary("TestMarket");

        // Add some liquidity
        problem.liquidity.add_ask(market, 0, 500_000_000, 1000);

        // Add some orders
        for i in 0..10 {
            let mut order = matching_engine::Order::new(i + 1);
            order.markets[0] = market;
            order.num_markets = 1;
            order.limit_price = 500_000_000 + i * 10_000_000;
            order.max_fill = 50;
            problem.orders.push(order);
        }

        problem
    }

    fn create_test_result() -> MatchingResult {
        MatchingResult {
            fills: vec![],
            total_welfare: 1000,
            orders_filled: 5,
            orders_unfilled_liquidity: 3,
            orders_unfilled_aon: 2,
            total_quantity_filled: 200,
            remaining_liquidity: LiquidityPool::new(),
        }
    }

    #[test]
    fn test_coordinator_creation() {
        let coordinator = JitCoordinator::new();
        assert!(coordinator.config.enabled);
        assert!(coordinator.providers.is_empty());
    }

    #[test]
    fn test_coordinator_disabled() {
        let coordinator = JitCoordinator::with_config(JitConfig::disabled());
        let problem = create_test_problem();
        let result = create_test_result();

        let jit_result = coordinator.run_jit_phase(BatchId::new(1), &problem, &result);
        assert!(jit_result.jit_fills.is_empty());
    }

    #[test]
    fn test_coordinator_no_providers() {
        let coordinator = JitCoordinator::new();
        let problem = create_test_problem();
        let result = create_test_result();

        let jit_result = coordinator.run_jit_phase(BatchId::new(1), &problem, &result);
        assert!(jit_result.jit_fills.is_empty());
    }

    #[test]
    fn test_coordinator_with_provider() {
        let coordinator = JitCoordinator::new()
            .with_provider(Box::new(SimpleJitProvider::new(ProviderId::new(1))));

        let problem = create_test_problem();
        let result = create_test_result();

        let jit_result = coordinator.run_jit_phase(BatchId::new(1), &problem, &result);
        assert_eq!(jit_result.stats.providers_submitted, 1);
    }

    #[test]
    fn test_coordinator_custom_components() {
        let coordinator = JitCoordinator::new()
            .with_validator(Box::new(DefaultValidator::new().with_max_price_deviation(0.20)))
            .with_tax_calculator(Box::new(FlatRateTaxCalculator::with_rate(0.01)))
            .with_provider(Box::new(SimpleJitProvider::new(ProviderId::new(1))));

        let problem = create_test_problem();
        let result = create_test_result();

        let jit_result = coordinator.run_jit_phase(BatchId::new(1), &problem, &result);
        assert_eq!(jit_result.stats.providers_submitted, 1);
    }
}
