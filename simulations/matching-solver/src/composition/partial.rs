//! Partial solution representation for solver composition.
//!
//! Partial solutions allow merging results from multiple solvers
//! while tracking confidence and provenance.

use std::collections::HashMap;

use matching_engine::{Fill, MarketId, Nanos, Qty};

/// Confidence level for a solution.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum SolutionConfidence {
    /// Proven optimal (e.g., MILP with exact solution)
    Optimal,
    /// Bounded optimality gap (e.g., MILP with known gap)
    BoundedGap {
        /// Upper bound on gap from optimal (as percentage)
        gap_percent: f64,
    },
    /// Heuristic solution with unknown gap
    #[default]
    Heuristic,
}

impl SolutionConfidence {
    /// Check if this solution is proven optimal.
    pub fn is_optimal(&self) -> bool {
        matches!(self, SolutionConfidence::Optimal)
    }

    /// Get the optimality gap if known.
    pub fn gap(&self) -> Option<f64> {
        match self {
            SolutionConfidence::Optimal => Some(0.0),
            SolutionConfidence::BoundedGap { gap_percent } => Some(*gap_percent),
            SolutionConfidence::Heuristic => None,
        }
    }

    /// Combine two confidence levels (takes the worse one).
    pub fn combine(self, other: SolutionConfidence) -> SolutionConfidence {
        match (self, other) {
            (SolutionConfidence::Optimal, SolutionConfidence::Optimal) => SolutionConfidence::Optimal,
            (SolutionConfidence::Optimal, other) | (other, SolutionConfidence::Optimal) => other,
            (
                SolutionConfidence::BoundedGap { gap_percent: g1 },
                SolutionConfidence::BoundedGap { gap_percent: g2 },
            ) => SolutionConfidence::BoundedGap {
                gap_percent: g1.max(g2),
            },
            (SolutionConfidence::BoundedGap { gap_percent }, SolutionConfidence::Heuristic)
            | (SolutionConfidence::Heuristic, SolutionConfidence::BoundedGap { gap_percent }) => {
                SolutionConfidence::BoundedGap {
                    gap_percent: gap_percent + 10.0, // Add uncertainty
                }
            }
            (SolutionConfidence::Heuristic, SolutionConfidence::Heuristic) => {
                SolutionConfidence::Heuristic
            }
        }
    }
}


impl std::fmt::Display for SolutionConfidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolutionConfidence::Optimal => write!(f, "Optimal"),
            SolutionConfidence::BoundedGap { gap_percent } => {
                write!(f, "Gap ≤ {:.1}%", gap_percent)
            }
            SolutionConfidence::Heuristic => write!(f, "Heuristic"),
        }
    }
}

/// A partial solution from a solver, tracking provenance and confidence.
#[derive(Clone, Debug)]
pub struct PartialSolution {
    /// Identifier for the sub-problem this solution came from
    pub subproblem_id: usize,
    /// Fills with their original order indices
    pub fills: Vec<(usize, Fill)>,
    /// Total welfare achieved
    pub welfare: i64,
    /// Liquidity consumed by this solution
    pub consumed_liquidity: HashMap<(MarketId, u8), ConsumedLiquidity>,
    /// Confidence level of this solution
    pub confidence: SolutionConfidence,
    /// Name of the solver that produced this solution
    pub solver_name: String,
}

impl PartialSolution {
    /// Create a new empty partial solution.
    pub fn new(subproblem_id: usize, solver_name: impl Into<String>) -> Self {
        Self {
            subproblem_id,
            fills: Vec::new(),
            welfare: 0,
            consumed_liquidity: HashMap::new(),
            confidence: SolutionConfidence::Heuristic,
            solver_name: solver_name.into(),
        }
    }

    /// Add a fill to this partial solution.
    pub fn add_fill(&mut self, original_order_idx: usize, fill: Fill, welfare_delta: i64) {
        self.fills.push((original_order_idx, fill));
        self.welfare += welfare_delta;
    }

    /// Record liquidity consumption.
    pub fn consume_liquidity(&mut self, market: MarketId, outcome: u8, qty: Qty, price: Nanos) {
        let entry = self
            .consumed_liquidity
            .entry((market, outcome))
            .or_default();
        entry.quantity += qty;
        // Track weighted average price
        if entry.quantity > 0 {
            let total_value = entry.avg_price as u128 * (entry.quantity - qty) as u128
                + price as u128 * qty as u128;
            entry.avg_price = (total_value / entry.quantity as u128) as Nanos;
        }
    }

    /// Set the confidence level.
    pub fn set_confidence(&mut self, confidence: SolutionConfidence) {
        self.confidence = confidence;
    }

    /// Get the number of orders filled.
    pub fn num_fills(&self) -> usize {
        self.fills.len()
    }

    /// Get total quantity filled.
    pub fn total_quantity(&self) -> Qty {
        self.fills.iter().map(|(_, f)| f.fill_qty).sum()
    }

    /// Check if a specific order was filled in this partial solution.
    pub fn is_order_filled(&self, original_order_idx: usize) -> bool {
        self.fills.iter().any(|(idx, _)| *idx == original_order_idx)
    }

    /// Get the fill for a specific order, if any.
    pub fn get_fill(&self, original_order_idx: usize) -> Option<&Fill> {
        self.fills
            .iter()
            .find(|(idx, _)| *idx == original_order_idx)
            .map(|(_, fill)| fill)
    }
}

/// Tracks liquidity consumed at a specific (market, outcome).
#[derive(Clone, Debug, Default)]
pub struct ConsumedLiquidity {
    /// Total quantity consumed
    pub quantity: Qty,
    /// Weighted average price
    pub avg_price: Nanos,
}

impl ConsumedLiquidity {
    /// Check if any liquidity was consumed.
    pub fn is_empty(&self) -> bool {
        self.quantity == 0
    }

    /// Total value consumed (quantity * average price).
    pub fn total_value(&self) -> u128 {
        self.quantity as u128 * self.avg_price as u128
    }
}

/// Statistics about a merged solution.
#[derive(Clone, Debug, Default)]
pub struct MergeStats {
    /// Number of partial solutions merged
    pub num_partials: usize,
    /// Number of conflicts detected
    pub num_conflicts: usize,
    /// Number of orders affected by conflicts
    pub orders_with_conflicts: usize,
    /// Total welfare from all partials before conflict resolution
    pub pre_merge_welfare: i64,
    /// Welfare lost to conflict resolution
    pub welfare_lost_to_conflicts: i64,
}

impl MergeStats {
    /// Create new empty stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Final welfare after merging.
    pub fn final_welfare(&self) -> i64 {
        self.pre_merge_welfare - self.welfare_lost_to_conflicts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_combine() {
        assert_eq!(
            SolutionConfidence::Optimal.combine(SolutionConfidence::Optimal),
            SolutionConfidence::Optimal
        );

        assert!(matches!(
            SolutionConfidence::Optimal
                .combine(SolutionConfidence::BoundedGap { gap_percent: 5.0 }),
            SolutionConfidence::BoundedGap { gap_percent } if (gap_percent - 5.0).abs() < 0.01
        ));

        assert!(matches!(
            SolutionConfidence::Optimal.combine(SolutionConfidence::Heuristic),
            SolutionConfidence::Heuristic
        ));
    }

    #[test]
    fn test_partial_solution() {
        let mut partial = PartialSolution::new(0, "test_solver");
        let fill = Fill::new(1, 100, 500_000_000);

        partial.add_fill(0, fill, 1000);
        partial.set_confidence(SolutionConfidence::Optimal);

        assert_eq!(partial.num_fills(), 1);
        assert_eq!(partial.welfare, 1000);
        assert!(partial.is_order_filled(0));
        assert!(!partial.is_order_filled(1));
    }

    #[test]
    fn test_consumed_liquidity() {
        let mut partial = PartialSolution::new(0, "test");
        let market = MarketId::new(1);

        partial.consume_liquidity(market, 0, 100, 500_000_000);
        partial.consume_liquidity(market, 0, 100, 600_000_000);

        let consumed = &partial.consumed_liquidity[&(market, 0)];
        assert_eq!(consumed.quantity, 200);
        assert_eq!(consumed.avg_price, 550_000_000); // Weighted average
    }
}
