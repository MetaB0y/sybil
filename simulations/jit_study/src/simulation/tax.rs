//! Tax mechanisms for the displacement tax simulation
//!
//! Three tax mechanisms:
//! 1. FixedRate: `tax = displaced_volume * rate_bps / 10000`
//! 2. Dynamic (EIP-1559): Adjust rate to target X% JIT participation
//! 3. ProportionalHarm: `tax = harm_to_passive_lps * coefficient`

use std::collections::HashMap;

use super::{Bps, Qty, SimSolution};
use super::agents::AgentId;

/// Result of tax calculation
#[derive(Debug, Clone)]
pub struct TaxResult {
    /// Total tax to collect (in value units: bps * qty)
    pub total_tax: u64,
    /// Per-agent breakdown (for future use)
    #[allow(dead_code)]
    pub per_agent_tax: HashMap<AgentId, u64>,
    /// Effective tax rate in basis points
    pub effective_rate_bps: Bps,
}

impl TaxResult {
    pub fn zero() -> Self {
        TaxResult {
            total_tax: 0,
            per_agent_tax: HashMap::new(),
            effective_rate_bps: 0,
        }
    }
}

/// Trait for tax calculators
pub trait TaxCalculator: Clone {
    /// Calculate tax based on displacement and solution
    fn calculate_tax(
        &self,
        displacement: &HashMap<AgentId, Qty>,
        solution: &SimSolution,
        true_value: Bps,
    ) -> TaxResult;

    /// Update state after a round (for dynamic tax)
    fn update(&mut self, jit_participated: bool, displacement_qty: Qty);

    /// Get current tax rate in basis points
    fn current_rate_bps(&self) -> Bps;

    /// Get a descriptive name
    fn name(&self) -> String;
}

/// Fixed rate tax: tax = displaced_volume * rate_bps / 10000
#[derive(Debug, Clone)]
pub struct FixedRateTax {
    /// Tax rate in basis points (e.g., 100 = 1%)
    pub rate_bps: Bps,
}

impl FixedRateTax {
    pub fn new(rate_bps: Bps) -> Self {
        FixedRateTax { rate_bps }
    }
}

impl TaxCalculator for FixedRateTax {
    fn calculate_tax(
        &self,
        displacement: &HashMap<AgentId, Qty>,
        _solution: &SimSolution,
        _true_value: Bps,
    ) -> TaxResult {
        let total_displacement: Qty = displacement.values().sum();

        if total_displacement == 0 {
            return TaxResult::zero();
        }

        // Tax = displaced_qty * rate_bps
        // This is in the same units as profit (bps * qty)
        // rate_bps = 100 means 100 bps per unit displaced
        // So displacing 50 units at 100 bps rate = 5000 tax
        // This makes the tax comparable to the profit calculation
        let total_tax = (total_displacement as u64).saturating_mul(self.rate_bps);

        let per_agent_tax = HashMap::new();

        TaxResult {
            total_tax,
            per_agent_tax,
            effective_rate_bps: self.rate_bps,
        }
    }

    fn update(&mut self, _jit_participated: bool, _displacement_qty: Qty) {
        // Fixed rate doesn't change
    }

    fn current_rate_bps(&self) -> Bps {
        self.rate_bps
    }

    fn name(&self) -> String {
        format!("FixedRate({}bps)", self.rate_bps)
    }
}

/// Dynamic tax (EIP-1559 style): Adjust rate to target X% JIT participation
///
/// The tax rate adjusts up when JIT participation exceeds target,
/// and down when it's below target.
#[derive(Debug, Clone)]
pub struct DynamicTax {
    /// Current tax rate in basis points
    pub current_rate_bps: Bps,
    /// Target JIT participation rate (0-100%)
    pub target_participation_pct: u8,
    /// Adjustment speed (basis points per round)
    pub adjustment_speed_bps: Bps,
    /// Minimum rate
    pub min_rate_bps: Bps,
    /// Maximum rate
    pub max_rate_bps: Bps,

    // State for averaging
    recent_participation: Vec<bool>,
    window_size: usize,
}

impl DynamicTax {
    pub fn new(
        initial_rate_bps: Bps,
        target_participation_pct: u8,
        adjustment_speed_bps: Bps,
    ) -> Self {
        DynamicTax {
            current_rate_bps: initial_rate_bps,
            target_participation_pct,
            adjustment_speed_bps,
            min_rate_bps: 0,
            max_rate_bps: 10000, // Max 100% of spread - allows full range
            recent_participation: Vec::new(),
            window_size: 50, // Shorter window for faster adjustment
        }
    }

    fn current_participation_pct(&self) -> u8 {
        if self.recent_participation.is_empty() {
            return 50; // Default assumption
        }

        let participated = self.recent_participation.iter().filter(|&&p| p).count();
        ((participated * 100) / self.recent_participation.len()) as u8
    }
}

impl TaxCalculator for DynamicTax {
    fn calculate_tax(
        &self,
        displacement: &HashMap<AgentId, Qty>,
        _solution: &SimSolution,
        _true_value: Bps,
    ) -> TaxResult {
        let total_displacement: Qty = displacement.values().sum();

        if total_displacement == 0 {
            return TaxResult {
                total_tax: 0,
                per_agent_tax: HashMap::new(),
                effective_rate_bps: self.current_rate_bps,
            };
        }

        // Tax = displaced_qty * current_rate_bps (same units as profit)
        let total_tax = (total_displacement as u64).saturating_mul(self.current_rate_bps);

        TaxResult {
            total_tax,
            per_agent_tax: HashMap::new(),
            effective_rate_bps: self.current_rate_bps,
        }
    }

    fn update(&mut self, jit_participated: bool, _displacement_qty: Qty) {
        // Track participation
        self.recent_participation.push(jit_participated);
        if self.recent_participation.len() > self.window_size {
            self.recent_participation.remove(0);
        }

        // Adjust rate based on recent participation
        let current_pct = self.current_participation_pct();

        if current_pct > self.target_participation_pct {
            // Too much JIT - increase tax
            self.current_rate_bps = (self.current_rate_bps + self.adjustment_speed_bps)
                .min(self.max_rate_bps);
        } else if current_pct < self.target_participation_pct {
            // Too little JIT - decrease tax
            self.current_rate_bps = self.current_rate_bps
                .saturating_sub(self.adjustment_speed_bps)
                .max(self.min_rate_bps);
        }
    }

    fn current_rate_bps(&self) -> Bps {
        self.current_rate_bps
    }

    fn name(&self) -> String {
        format!("Dynamic(target={}%)", self.target_participation_pct)
    }
}

/// Proportional harm tax: tax = harm_to_passive_lps * coefficient
///
/// The harm is calculated as the P&L lost by passive LPs due to displacement.
#[derive(Debug, Clone)]
pub struct ProportionalHarmTax {
    /// Coefficient multiplied by harm (1.0 = 10000)
    pub coefficient: u64,
}

impl ProportionalHarmTax {
    pub fn new(coefficient_x100: u64) -> Self {
        // coefficient_x100 = 100 means 1.0x, 150 means 1.5x
        ProportionalHarmTax {
            coefficient: coefficient_x100,
        }
    }
}

impl TaxCalculator for ProportionalHarmTax {
    fn calculate_tax(
        &self,
        displacement: &HashMap<AgentId, Qty>,
        solution: &SimSolution,
        true_value: Bps,
    ) -> TaxResult {
        let total_displacement: Qty = displacement.values().sum();

        if total_displacement == 0 {
            return TaxResult::zero();
        }

        // Harm = price difference * displaced volume
        // This is the profit passive LPs would have made
        let price_diff = if solution.clearing_price > true_value {
            solution.clearing_price - true_value
        } else {
            true_value - solution.clearing_price
        };

        // harm = price_diff * displaced_qty (same units as profit)
        let harm = (price_diff as u64).saturating_mul(total_displacement as u64);

        if harm == 0 {
            // Use a minimum harm based on displacement
            // Even if price is at true value, displacement has a cost
            let min_harm = total_displacement as u64 * 10; // 10 bps per unit minimum
            let total_tax = (min_harm as u128 * self.coefficient as u128 / 100) as u64;
            return TaxResult {
                total_tax,
                per_agent_tax: HashMap::new(),
                effective_rate_bps: (total_tax / total_displacement as u64) as Bps,
            };
        }

        // Tax = harm * coefficient / 100 (coefficient 100 = 1x)
        let total_tax = (harm as u128 * self.coefficient as u128 / 100) as u64;

        // Calculate effective rate for comparison
        let effective_rate_bps = if total_displacement > 0 {
            (total_tax / total_displacement as u64) as Bps
        } else {
            0
        };

        TaxResult {
            total_tax,
            per_agent_tax: HashMap::new(),
            effective_rate_bps,
        }
    }

    fn update(&mut self, _jit_participated: bool, _displacement_qty: Qty) {
        // Proportional harm doesn't change
    }

    fn current_rate_bps(&self) -> Bps {
        // This is variable, return coefficient as proxy
        self.coefficient as Bps
    }

    fn name(&self) -> String {
        format!("ProportionalHarm({}x)", self.coefficient as f64 / 100.0)
    }
}

/// No tax (baseline)
#[derive(Debug, Clone)]
pub struct NoTax;

impl TaxCalculator for NoTax {
    fn calculate_tax(
        &self,
        _displacement: &HashMap<AgentId, Qty>,
        _solution: &SimSolution,
        _true_value: Bps,
    ) -> TaxResult {
        TaxResult::zero()
    }

    fn update(&mut self, _jit_participated: bool, _displacement_qty: Qty) {}

    fn current_rate_bps(&self) -> Bps {
        0
    }

    fn name(&self) -> String {
        "NoTax".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_rate_tax() {
        let tax = FixedRateTax::new(100); // 100 bps per unit displaced

        let mut displacement = HashMap::new();
        displacement.insert(AgentId(0), 50);

        let solution = SimSolution {
            clearing_price: 5000,
            fills: vec![],
            total_volume: 1000,
        };

        let result = tax.calculate_tax(&displacement, &solution, 5000);

        // Tax = 50 * 100 = 5000
        assert_eq!(result.total_tax, 5000);
        assert_eq!(result.effective_rate_bps, 100);
    }

    #[test]
    fn test_dynamic_tax_adjustment() {
        let mut tax = DynamicTax::new(100, 25, 5); // Target 25% participation

        // Simulate high participation (should increase rate)
        for _ in 0..50 {
            tax.update(true, 100);
        }

        assert!(tax.current_rate_bps > 100);

        // Simulate low participation (should decrease rate)
        let mut tax = DynamicTax::new(100, 75, 5); // Target 75%

        for _ in 0..50 {
            tax.update(false, 0);
        }

        assert!(tax.current_rate_bps < 100);
    }

    #[test]
    fn test_no_displacement_no_tax() {
        let tax = FixedRateTax::new(500);

        let displacement = HashMap::new();
        let solution = SimSolution {
            clearing_price: 5000,
            fills: vec![],
            total_volume: 0,
        };

        let result = tax.calculate_tax(&displacement, &solution, 5000);

        assert_eq!(result.total_tax, 0);
    }
}
