//! JIT Validator - the trust boundary.
//!
//! Validates JIT submissions and classifies orders as backrun or displacement.
//! This is the TRUST BOUNDARY - JIT providers are untrusted, all inputs validated here.

use matching_engine::Side;

use super::input::JitInput;
use super::types::{
    JitOrder, JitRejection, JitSubmission, JitType, ValidatedJit, ValidatedJitOrder,
};

/// Validates JIT submissions.
///
/// This is the TRUST BOUNDARY. JIT providers are untrusted - all inputs
/// are validated here before being accepted into the system.
pub trait JitValidator: Send + Sync {
    /// Validate a submission and classify orders as backrun/displacement.
    ///
    /// Returns classified orders or rejection reason.
    fn validate(
        &self,
        input: &JitInput,
        submission: &JitSubmission,
    ) -> Result<ValidatedJit, JitRejection>;
}

/// Default validator implementation.
///
/// Validates orders and classifies them based on whether they fill
/// unfilled demand (backrun) or displace passive liquidity (displacement).
pub struct DefaultValidator {
    /// Minimum welfare improvement required for displacement.
    pub min_welfare_improvement: i64,
    /// Maximum price deviation from clearing (as a fraction).
    pub max_price_deviation: f64,
}

impl Default for DefaultValidator {
    fn default() -> Self {
        Self {
            min_welfare_improvement: 0,
            max_price_deviation: 0.10, // 10% max deviation from clearing
        }
    }
}

impl DefaultValidator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_min_welfare(mut self, min: i64) -> Self {
        self.min_welfare_improvement = min;
        self
    }

    pub fn with_max_price_deviation(mut self, deviation: f64) -> Self {
        self.max_price_deviation = deviation;
        self
    }

    /// Classify a single order as backrun or displacement.
    fn classify_order(&self, input: &JitInput, order: &JitOrder) -> Result<JitType, JitRejection> {
        // Check market exists
        if !input.markets.iter().any(|m| m.id == order.market_id) {
            return Err(JitRejection::InvalidMarket(order.market_id));
        }

        // Check price is valid
        if order.price == 0 {
            return Err(JitRejection::InvalidPrice {
                market: order.market_id,
                reason: "Price cannot be zero".to_string(),
            });
        }

        // Get unfilled demand for this market
        let unfilled = input.unfilled_demand(order.market_id);

        // Determine if this is backrun or displacement
        match order.side {
            Side::Ask => {
                // Selling - fills buy demand
                if let Some(unfilled) = unfilled {
                    if unfilled.buy_qty > 0 && order.quantity <= unfilled.buy_qty {
                        // Filling unfilled buy demand = backrun
                        return Ok(JitType::Backrun);
                    }
                }
                // Otherwise it's displacement
                Ok(JitType::Displacement)
            }
            Side::Bid => {
                // Buying - fills sell demand
                if let Some(unfilled) = unfilled {
                    if unfilled.sell_qty > 0 && order.quantity <= unfilled.sell_qty {
                        // Filling unfilled sell demand = backrun
                        return Ok(JitType::Backrun);
                    }
                }
                // Otherwise it's displacement
                Ok(JitType::Displacement)
            }
        }
    }

    /// Calculate displaced volume for a displacement order.
    fn calculate_displaced_volume(&self, input: &JitInput, order: &JitOrder) -> u64 {
        // For displacement, the entire quantity displaces passive LPs
        // (In a more sophisticated implementation, we'd calculate partial displacement)
        match order.side {
            Side::Ask => {
                // JIT selling displaces passive sellers
                let unfilled_buy = input
                    .unfilled_demand(order.market_id)
                    .map(|u| u.buy_qty)
                    .unwrap_or(0);
                order.quantity.saturating_sub(unfilled_buy)
            }
            Side::Bid => {
                // JIT buying displaces passive buyers
                let unfilled_sell = input
                    .unfilled_demand(order.market_id)
                    .map(|u| u.sell_qty)
                    .unwrap_or(0);
                order.quantity.saturating_sub(unfilled_sell)
            }
        }
    }

    /// Estimate welfare improvement from an order.
    fn estimate_welfare_improvement(&self, input: &JitInput, order: &JitOrder) -> i64 {
        // Simple welfare estimate: fills * price_improvement
        // In a more sophisticated implementation, we'd compute actual welfare delta
        let clearing_price = input.clearing_price(order.market_id).unwrap_or(order.price);

        let price_diff = match order.side {
            Side::Ask => {
                // Selling below clearing = welfare improvement for buyers
                clearing_price.saturating_sub(order.price) as i64
            }
            Side::Bid => {
                // Buying above clearing = welfare improvement for sellers
                order.price.saturating_sub(clearing_price) as i64
            }
        };

        price_diff * order.quantity as i64
    }

    /// Validate price is within acceptable bounds.
    fn validate_price(&self, input: &JitInput, order: &JitOrder) -> Result<(), JitRejection> {
        if let Some(clearing) = input.clearing_price(order.market_id) {
            let max_deviation = (clearing as f64 * self.max_price_deviation) as u64;
            let lower_bound = clearing.saturating_sub(max_deviation);
            let upper_bound = clearing.saturating_add(max_deviation);

            if order.price < lower_bound || order.price > upper_bound {
                return Err(JitRejection::InvalidPrice {
                    market: order.market_id,
                    reason: format!(
                        "Price {} outside acceptable range [{}, {}]",
                        order.price, lower_bound, upper_bound
                    ),
                });
            }
        }

        Ok(())
    }
}

impl JitValidator for DefaultValidator {
    fn validate(
        &self,
        input: &JitInput,
        submission: &JitSubmission,
    ) -> Result<ValidatedJit, JitRejection> {
        // Validate batch ID
        if submission.batch_id != input.batch_id {
            return Err(JitRejection::InvalidBatchId);
        }

        let mut validated = ValidatedJit::new(submission.provider_id);

        for order in &submission.orders {
            // Validate price bounds
            self.validate_price(input, order)?;

            // Classify the order
            let jit_type = self.classify_order(input, order)?;

            // Calculate metrics
            let displaced_volume = match jit_type {
                JitType::Backrun => 0,
                JitType::Displacement => self.calculate_displaced_volume(input, order),
            };

            let welfare_improvement = self.estimate_welfare_improvement(input, order);

            // For displacement, require minimum welfare improvement
            if jit_type == JitType::Displacement && welfare_improvement < self.min_welfare_improvement
            {
                return Err(JitRejection::NoWelfareImprovement);
            }

            let validated_order = ValidatedJitOrder {
                order: order.clone(),
                jit_type,
                displaced_volume,
                welfare_improvement,
            };

            match jit_type {
                JitType::Backrun => validated.backrun_orders.push(validated_order),
                JitType::Displacement => validated.displacement_orders.push(validated_order),
            }
        }

        Ok(validated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::input::{AnonymizedOrderbook, BaseSolutionSummary};
    use crate::jit::types::{BatchId, ProviderId, UnfilledDemand};
    use matching_engine::MarketId;
    use std::collections::HashMap;

    fn create_test_input() -> JitInput {
        let mut unfilled_demand = HashMap::new();
        unfilled_demand.insert(
            MarketId::new(0),
            UnfilledDemand {
                buy_qty: 100,
                buy_price: 500_000_000,
                sell_qty: 0,
                sell_price: 0,
            },
        );

        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(MarketId::new(0), 500_000_000);

        JitInput {
            batch_id: BatchId::new(1),
            orderbook: AnonymizedOrderbook::default(),
            base_solution: BaseSolutionSummary {
                clearing_prices,
                total_welfare: 1000,
                fill_rate: 0.8,
                unfilled_demand,
                total_volume_filled: 500,
                orders_filled: 10,
            },
            markets: vec![super::super::input::MarketInfo {
                id: MarketId::new(0),
                name: "Test Market".to_string(),
                num_outcomes: 2,
            }],
        }
    }

    #[test]
    fn test_backrun_classification() {
        let validator = DefaultValidator::new();
        let input = create_test_input();

        let mut submission = JitSubmission::new(BatchId::new(1), ProviderId::new(1));
        // Selling into unfilled buy demand = backrun
        submission.add_order(JitOrder::sell(MarketId::new(0), 500_000_000, 50));

        let result = validator.validate(&input, &submission).unwrap();
        assert_eq!(result.backrun_orders.len(), 1);
        assert_eq!(result.displacement_orders.len(), 0);
        assert_eq!(result.backrun_orders[0].jit_type, JitType::Backrun);
    }

    #[test]
    fn test_displacement_classification() {
        let validator = DefaultValidator::new();
        let input = create_test_input();

        let mut submission = JitSubmission::new(BatchId::new(1), ProviderId::new(1));
        // Selling more than unfilled demand = displacement
        submission.add_order(JitOrder::sell(MarketId::new(0), 500_000_000, 150));

        let result = validator.validate(&input, &submission).unwrap();
        assert_eq!(result.backrun_orders.len(), 0);
        assert_eq!(result.displacement_orders.len(), 1);
        assert_eq!(result.displacement_orders[0].jit_type, JitType::Displacement);
        assert_eq!(result.displacement_orders[0].displaced_volume, 50); // 150 - 100 unfilled
    }

    #[test]
    fn test_invalid_batch_id() {
        let validator = DefaultValidator::new();
        let input = create_test_input();

        let submission = JitSubmission::new(BatchId::new(999), ProviderId::new(1));

        let result = validator.validate(&input, &submission);
        assert!(matches!(result, Err(JitRejection::InvalidBatchId)));
    }

    #[test]
    fn test_invalid_market() {
        let validator = DefaultValidator::new();
        let input = create_test_input();

        let mut submission = JitSubmission::new(BatchId::new(1), ProviderId::new(1));
        submission.add_order(JitOrder::sell(MarketId::new(999), 500_000_000, 50));

        let result = validator.validate(&input, &submission);
        assert!(matches!(result, Err(JitRejection::InvalidMarket(_))));
    }

    #[test]
    fn test_price_validation() {
        let validator = DefaultValidator::new().with_max_price_deviation(0.05); // 5%
        let input = create_test_input();

        let mut submission = JitSubmission::new(BatchId::new(1), ProviderId::new(1));
        // Price way outside 5% of clearing (500_000_000)
        submission.add_order(JitOrder::sell(MarketId::new(0), 300_000_000, 50));

        let result = validator.validate(&input, &submission);
        assert!(matches!(result, Err(JitRejection::InvalidPrice { .. })));
    }
}
