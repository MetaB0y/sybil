//! JIT Tax Calculator - pluggable taxation.
//!
//! Calculates tax on JIT orders. Designed to be pluggable so different
//! tax strategies can be easily swapped.
//!
//! Key design decisions:
//! - Backrun: No tax (pure value add)
//! - Displacement: Taxed based on strategy
//!
//! V1: Simple flat rate on notional value
//! Future: Dynamic rate (EIP-1559 style) based on JIT utilization

use matching_engine::Nanos;

use super::types::{JitType, ValidatedJitOrder};

/// Result of tax calculation for a single order.
#[derive(Clone, Debug)]
pub struct TaxResult {
    pub tax_amount: Nanos,
    pub rebate_pool: Nanos,
    pub protocol_revenue: Nanos,
}

impl TaxResult {
    pub fn zero() -> Self {
        Self {
            tax_amount: 0,
            rebate_pool: 0,
            protocol_revenue: 0,
        }
    }
}

/// Trait for JIT tax calculators.
///
/// Implement this trait to create custom tax strategies.
pub trait JitTaxCalculator: Send + Sync {
    /// Calculate tax for a JIT order.
    fn calculate_tax(&self, order: &ValidatedJitOrder, fill_price: Nanos) -> TaxResult;

    /// Name of this tax strategy (for logging).
    fn name(&self) -> &str {
        "JitTaxCalculator"
    }
}

/// Simple flat-rate tax calculator.
///
/// - Backrun: 0% tax
/// - Displacement: Fixed percentage of notional value
pub struct FlatRateTaxCalculator {
    /// Tax rate for displacement orders (e.g., 0.005 = 0.5%).
    pub displacement_rate: f64,
    /// Fraction of tax that goes to rebate pool (rest is protocol revenue).
    pub rebate_fraction: f64,
}

impl Default for FlatRateTaxCalculator {
    fn default() -> Self {
        Self {
            displacement_rate: 0.005, // 0.5% of notional
            rebate_fraction: 0.70,    // 70% to rebates, 30% to protocol
        }
    }
}

impl FlatRateTaxCalculator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with custom displacement rate.
    pub fn with_rate(displacement_rate: f64) -> Self {
        Self {
            displacement_rate,
            ..Default::default()
        }
    }

    /// Create with custom displacement rate and rebate fraction.
    pub fn with_params(displacement_rate: f64, rebate_fraction: f64) -> Self {
        Self {
            displacement_rate,
            rebate_fraction,
        }
    }
}

impl JitTaxCalculator for FlatRateTaxCalculator {
    fn calculate_tax(&self, order: &ValidatedJitOrder, fill_price: Nanos) -> TaxResult {
        match order.jit_type {
            JitType::Backrun => {
                // No tax on backrun - pure value add
                TaxResult::zero()
            }
            JitType::Displacement => {
                // Tax based on notional value of displacement
                let notional = fill_price as u128 * order.displaced_volume as u128;
                let tax = (notional as f64 * self.displacement_rate) as Nanos;
                let rebate_pool = (tax as f64 * self.rebate_fraction) as Nanos;
                let protocol_revenue = tax - rebate_pool;

                TaxResult {
                    tax_amount: tax,
                    rebate_pool,
                    protocol_revenue,
                }
            }
        }
    }

    fn name(&self) -> &str {
        "FlatRateTax"
    }
}

/// Welfare-based tax calculator.
///
/// Tax is based on the welfare improvement, not notional value.
/// More economically aligned but harder to verify.
pub struct WelfareTaxCalculator {
    /// Tax rate on welfare improvement (e.g., 0.20 = 20%).
    pub welfare_rate: f64,
    /// Fraction of tax that goes to rebate pool.
    pub rebate_fraction: f64,
}

impl Default for WelfareTaxCalculator {
    fn default() -> Self {
        Self {
            welfare_rate: 0.20,    // 20% of welfare improvement
            rebate_fraction: 0.70, // 70% to rebates
        }
    }
}

impl WelfareTaxCalculator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_rate(welfare_rate: f64) -> Self {
        Self {
            welfare_rate,
            ..Default::default()
        }
    }
}

impl JitTaxCalculator for WelfareTaxCalculator {
    fn calculate_tax(&self, order: &ValidatedJitOrder, _fill_price: Nanos) -> TaxResult {
        match order.jit_type {
            JitType::Backrun => TaxResult::zero(),
            JitType::Displacement => {
                // Tax based on welfare improvement
                let welfare = order.welfare_improvement.max(0) as u64;
                let tax = (welfare as f64 * self.welfare_rate) as Nanos;
                let rebate_pool = (tax as f64 * self.rebate_fraction) as Nanos;
                let protocol_revenue = tax - rebate_pool;

                TaxResult {
                    tax_amount: tax,
                    rebate_pool,
                    protocol_revenue,
                }
            }
        }
    }

    fn name(&self) -> &str {
        "WelfareTax"
    }
}

/// Dynamic tax calculator (EIP-1559 style).
///
/// Tax rate adjusts based on JIT utilization target.
/// When JIT volume exceeds target, rate increases; when below, rate decreases.
pub struct DynamicTaxCalculator {
    /// Current base rate.
    current_rate: f64,
    /// Target JIT volume as fraction of total (e.g., 0.30 = 30%).
    pub target_jit_ratio: f64,
    /// Minimum rate.
    pub min_rate: f64,
    /// Maximum rate.
    pub max_rate: f64,
    /// Rate adjustment factor per batch.
    pub adjustment_factor: f64,
    /// Rebate fraction.
    pub rebate_fraction: f64,
}

impl Default for DynamicTaxCalculator {
    fn default() -> Self {
        Self {
            current_rate: 0.005,     // Start at 0.5%
            target_jit_ratio: 0.30,  // Target 30% JIT volume
            min_rate: 0.001,         // Min 0.1%
            max_rate: 0.05,          // Max 5%
            adjustment_factor: 0.125, // +/- 12.5% per batch
            rebate_fraction: 0.70,
        }
    }
}

impl DynamicTaxCalculator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update rate based on actual JIT ratio.
    pub fn update_rate(&mut self, actual_jit_ratio: f64) {
        if actual_jit_ratio > self.target_jit_ratio {
            // Too much JIT, increase rate
            self.current_rate *= 1.0 + self.adjustment_factor;
        } else {
            // Room for more JIT, decrease rate
            self.current_rate *= 1.0 - self.adjustment_factor;
        }

        // Clamp to bounds
        self.current_rate = self.current_rate.clamp(self.min_rate, self.max_rate);
    }

    /// Get current rate.
    pub fn current_rate(&self) -> f64 {
        self.current_rate
    }
}

impl JitTaxCalculator for DynamicTaxCalculator {
    fn calculate_tax(&self, order: &ValidatedJitOrder, fill_price: Nanos) -> TaxResult {
        match order.jit_type {
            JitType::Backrun => TaxResult::zero(),
            JitType::Displacement => {
                let notional = fill_price as u128 * order.displaced_volume as u128;
                let tax = (notional as f64 * self.current_rate) as Nanos;
                let rebate_pool = (tax as f64 * self.rebate_fraction) as Nanos;
                let protocol_revenue = tax - rebate_pool;

                TaxResult {
                    tax_amount: tax,
                    rebate_pool,
                    protocol_revenue,
                }
            }
        }
    }

    fn name(&self) -> &str {
        "DynamicTax"
    }
}

/// Zero tax calculator (for testing or special cases).
pub struct ZeroTaxCalculator;

impl JitTaxCalculator for ZeroTaxCalculator {
    fn calculate_tax(&self, _order: &ValidatedJitOrder, _fill_price: Nanos) -> TaxResult {
        TaxResult::zero()
    }

    fn name(&self) -> &str {
        "ZeroTax"
    }
}

/// Aggregate tax results for a batch.
#[derive(Clone, Debug, Default)]
pub struct BatchTaxSummary {
    pub total_tax: Nanos,
    pub total_rebate_pool: Nanos,
    pub total_protocol_revenue: Nanos,
    pub backrun_count: usize,
    pub displacement_count: usize,
    pub backrun_volume: u64,
    pub displacement_volume: u64,
}

impl BatchTaxSummary {
    pub fn add(&mut self, result: &TaxResult, order: &ValidatedJitOrder) {
        self.total_tax += result.tax_amount;
        self.total_rebate_pool += result.rebate_pool;
        self.total_protocol_revenue += result.protocol_revenue;

        match order.jit_type {
            JitType::Backrun => {
                self.backrun_count += 1;
                self.backrun_volume += order.order.quantity;
            }
            JitType::Displacement => {
                self.displacement_count += 1;
                self.displacement_volume += order.displaced_volume;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::types::JitOrder;
    use matching_engine::MarketId;

    fn create_backrun_order() -> ValidatedJitOrder {
        ValidatedJitOrder {
            order: JitOrder::sell(MarketId::new(0), 500_000_000, 100),
            jit_type: JitType::Backrun,
            displaced_volume: 0,
            welfare_improvement: 1000,
        }
    }

    fn create_displacement_order() -> ValidatedJitOrder {
        ValidatedJitOrder {
            order: JitOrder::sell(MarketId::new(0), 500_000_000, 100),
            jit_type: JitType::Displacement,
            displaced_volume: 50,
            welfare_improvement: 500,
        }
    }

    #[test]
    fn test_flat_rate_backrun_no_tax() {
        let calculator = FlatRateTaxCalculator::new();
        let order = create_backrun_order();

        let result = calculator.calculate_tax(&order, 500_000_000);
        assert_eq!(result.tax_amount, 0);
    }

    #[test]
    fn test_flat_rate_displacement_tax() {
        let calculator = FlatRateTaxCalculator::with_rate(0.01); // 1%
        let order = create_displacement_order();

        // Notional = 500_000_000 * 50 = 25_000_000_000
        // Tax = 1% = 250_000_000
        let result = calculator.calculate_tax(&order, 500_000_000);
        assert_eq!(result.tax_amount, 250_000_000);
        assert_eq!(result.rebate_pool, 175_000_000); // 70%
        assert_eq!(result.protocol_revenue, 75_000_000); // 30%
    }

    #[test]
    fn test_welfare_tax() {
        let calculator = WelfareTaxCalculator::with_rate(0.20);
        let order = create_displacement_order();

        // Welfare improvement = 500
        // Tax = 20% = 100
        let result = calculator.calculate_tax(&order, 500_000_000);
        assert_eq!(result.tax_amount, 100);
    }

    #[test]
    fn test_dynamic_rate_adjustment() {
        let mut calculator = DynamicTaxCalculator::new();
        let initial_rate = calculator.current_rate();

        // High JIT ratio should increase rate
        calculator.update_rate(0.50); // 50% > 30% target
        assert!(calculator.current_rate() > initial_rate);

        // Low JIT ratio should decrease rate
        let high_rate = calculator.current_rate();
        calculator.update_rate(0.10); // 10% < 30% target
        assert!(calculator.current_rate() < high_rate);
    }

    #[test]
    fn test_zero_tax() {
        let calculator = ZeroTaxCalculator;
        let order = create_displacement_order();

        let result = calculator.calculate_tax(&order, 500_000_000);
        assert_eq!(result.tax_amount, 0);
    }

    #[test]
    fn test_batch_summary() {
        let calculator = FlatRateTaxCalculator::with_rate(0.01);
        let mut summary = BatchTaxSummary::default();

        let backrun = create_backrun_order();
        let displacement = create_displacement_order();

        let result1 = calculator.calculate_tax(&backrun, 500_000_000);
        summary.add(&result1, &backrun);

        let result2 = calculator.calculate_tax(&displacement, 500_000_000);
        summary.add(&result2, &displacement);

        assert_eq!(summary.backrun_count, 1);
        assert_eq!(summary.displacement_count, 1);
        assert_eq!(summary.total_tax, result2.tax_amount); // Only displacement taxed
    }
}
