//! Result verification for ZK proof integration.
//!
//! Verifies that a matching result is valid and could be accepted by a ZK prover.
//! All invariants that the ZK circuit will check are verified here first.

use std::collections::{HashMap, HashSet};

use matching_engine::{Fill, Order, Problem};

use crate::MatchingResult;

/// Verification result with detailed error information.
#[derive(Clone, Debug)]
pub struct VerificationResult {
    /// Whether verification passed
    pub valid: bool,
    /// List of violations found
    pub violations: Vec<Violation>,
    /// Statistics about the verification
    pub stats: VerificationStats,
}

/// Statistics from verification.
#[derive(Clone, Debug, Default)]
pub struct VerificationStats {
    /// Number of fills verified
    pub fills_checked: usize,
    /// Number of orders verified
    pub orders_checked: usize,
    /// Number of MM constraints verified
    pub mm_constraints_checked: usize,
    /// Computed total welfare
    pub computed_welfare: i64,
    /// Reported total welfare
    pub reported_welfare: i64,
}

/// A specific violation found during verification.
#[derive(Clone, Debug)]
pub struct Violation {
    pub kind: ViolationKind,
    pub details: String,
}

/// Types of violations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViolationKind {
    /// Order referenced in fill doesn't exist
    OrderNotFound,
    /// Fill quantity exceeds max_fill
    QuantityExceedsMax,
    /// Fill quantity below min_fill (and not zero)
    QuantityBelowMin,
    /// Fill price exceeds limit price
    PriceExceedsLimit,
    /// Same order filled multiple times
    DuplicateFill,
    /// Negative welfare for a fill
    NegativeWelfare,
    /// Welfare sum doesn't match reported total
    WelfareMismatch,
    /// MM budget constraint violated
    MmBudgetExceeded,
    /// Fill has zero quantity (wasteful)
    ZeroQuantityFill,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.details)
    }
}

/// Verifier for matching results.
pub struct Verifier {
    /// Whether to allow zero-quantity fills (lenient mode)
    allow_zero_fills: bool,
    /// Tolerance for welfare mismatch (in nanos)
    welfare_tolerance: i64,
}

impl Default for Verifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Verifier {
    /// Create a new verifier with default settings.
    pub fn new() -> Self {
        Self {
            allow_zero_fills: true,
            welfare_tolerance: 1_000, // Allow tiny rounding errors
        }
    }

    /// Create a strict verifier (for ZK proof compatibility).
    pub fn strict() -> Self {
        Self {
            allow_zero_fills: false,
            welfare_tolerance: 0,
        }
    }

    /// Verify a matching result against a problem.
    pub fn verify(&self, problem: &Problem, result: &MatchingResult) -> VerificationResult {
        let mut violations = Vec::new();
        let mut stats = VerificationStats::default();

        // Build order lookup
        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();

        // Track which orders have been filled
        let mut filled_orders: HashSet<u64> = HashSet::new();

        // Verify each fill
        let mut computed_welfare: i64 = 0;
        for fill in &result.fills {
            stats.fills_checked += 1;

            // Check order exists
            let Some(order) = order_map.get(&fill.order_id) else {
                violations.push(Violation {
                    kind: ViolationKind::OrderNotFound,
                    details: format!("Order {} not found in problem", fill.order_id),
                });
                continue;
            };

            // Check for duplicate fills
            if filled_orders.contains(&fill.order_id) {
                violations.push(Violation {
                    kind: ViolationKind::DuplicateFill,
                    details: format!("Order {} filled multiple times", fill.order_id),
                });
            }
            filled_orders.insert(fill.order_id);

            // Check quantity constraints
            if fill.fill_qty > order.max_fill {
                violations.push(Violation {
                    kind: ViolationKind::QuantityExceedsMax,
                    details: format!(
                        "Order {}: fill_qty {} > max_fill {}",
                        fill.order_id, fill.fill_qty, order.max_fill
                    ),
                });
            }

            if fill.fill_qty > 0 && fill.fill_qty < order.min_fill {
                violations.push(Violation {
                    kind: ViolationKind::QuantityBelowMin,
                    details: format!(
                        "Order {}: fill_qty {} < min_fill {} (AON violation)",
                        fill.order_id, fill.fill_qty, order.min_fill
                    ),
                });
            }

            // Check zero fills
            if fill.fill_qty == 0 && !self.allow_zero_fills {
                violations.push(Violation {
                    kind: ViolationKind::ZeroQuantityFill,
                    details: format!("Order {}: zero quantity fill", fill.order_id),
                });
            }

            // Check price constraint
            if fill.fill_price > order.limit_price {
                violations.push(Violation {
                    kind: ViolationKind::PriceExceedsLimit,
                    details: format!(
                        "Order {}: fill_price {} > limit_price {}",
                        fill.order_id, fill.fill_price, order.limit_price
                    ),
                });
            }

            // Compute welfare
            let fill_welfare = fill.welfare(order);
            if fill_welfare < 0 {
                violations.push(Violation {
                    kind: ViolationKind::NegativeWelfare,
                    details: format!(
                        "Order {}: negative welfare {} (limit={}, fill_price={}, qty={})",
                        fill.order_id, fill_welfare, order.limit_price, fill.fill_price, fill.fill_qty
                    ),
                });
            }
            computed_welfare += fill_welfare;
        }

        stats.orders_checked = order_map.len();
        stats.computed_welfare = computed_welfare;
        stats.reported_welfare = result.total_welfare;

        // Check welfare consistency
        let welfare_diff = (computed_welfare - result.total_welfare).abs();
        if welfare_diff > self.welfare_tolerance {
            violations.push(Violation {
                kind: ViolationKind::WelfareMismatch,
                details: format!(
                    "Computed welfare {} != reported welfare {} (diff={})",
                    computed_welfare, result.total_welfare, welfare_diff
                ),
            });
        }

        // Verify MM constraints
        self.verify_mm_constraints(problem, result, &order_map, &mut violations, &mut stats);

        VerificationResult {
            valid: violations.is_empty(),
            violations,
            stats,
        }
    }

    /// Verify market maker budget constraints.
    fn verify_mm_constraints(
        &self,
        problem: &Problem,
        result: &MatchingResult,
        _order_map: &HashMap<u64, &Order>,
        violations: &mut Vec<Violation>,
        stats: &mut VerificationStats,
    ) {
        // Build fill lookup
        let fill_map: HashMap<u64, &Fill> =
            result.fills.iter().map(|f| (f.order_id, f)).collect();

        for mm in &problem.mm_constraints {
            stats.mm_constraints_checked += 1;

            // Compute capital used by this MM
            let mut fills: HashMap<u64, (u64, u64)> = HashMap::new(); // order_id -> (price, qty)

            for &order_id in &mm.order_ids {
                if let Some(fill) = fill_map.get(&order_id) {
                    if fill.fill_qty > 0 {
                        fills.insert(order_id, (fill.fill_price, fill.fill_qty));
                    }
                }
            }

            let capital_used = mm.capital_used(&fills);

            if capital_used > mm.max_capital {
                violations.push(Violation {
                    kind: ViolationKind::MmBudgetExceeded,
                    details: format!(
                        "MM {:?}: capital_used {} > budget {} (overflow by {})",
                        mm.mm_id,
                        capital_used,
                        mm.max_capital,
                        capital_used - mm.max_capital
                    ),
                });
            }
        }
    }
}

/// Convenience function to verify a result.
pub fn verify(problem: &Problem, result: &MatchingResult) -> VerificationResult {
    Verifier::new().verify(problem, result)
}

/// Convenience function for strict verification (ZK-compatible).
pub fn verify_strict(problem: &Problem, result: &MatchingResult) -> VerificationResult {
    Verifier::strict().verify(problem, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{simple_yes_buy, MmConstraint, MmId, MmSide};

    fn create_test_problem() -> Problem {
        let mut problem = Problem::new("test");
        let market = problem.markets.add_binary("m");

        problem.liquidity.add_ask(market, 0, 500_000_000, 1000);

        // Add some orders
        for i in 1..=5 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i,
                market,
                600_000_000, // $0.60 limit
                100,
            ));
        }

        problem
    }

    #[test]
    fn test_valid_result() {
        let problem = create_test_problem();

        let mut result = MatchingResult::new(problem.liquidity.snapshot());
        result.fills.push(Fill::new(1, 50, 500_000_000));
        result.fills.push(Fill::new(2, 100, 550_000_000));

        // Compute welfare
        let order1 = problem.orders.iter().find(|o| o.id == 1).unwrap();
        let order2 = problem.orders.iter().find(|o| o.id == 2).unwrap();
        result.total_welfare = result.fills[0].welfare(order1) + result.fills[1].welfare(order2);

        let verification = verify(&problem, &result);
        assert!(verification.valid, "Violations: {:?}", verification.violations);
    }

    #[test]
    fn test_quantity_exceeds_max() {
        let problem = create_test_problem();

        let mut result = MatchingResult::new(problem.liquidity.snapshot());
        result.fills.push(Fill::new(1, 200, 500_000_000)); // max_fill is 100
        result.total_welfare = 0;

        let verification = verify(&problem, &result);
        assert!(!verification.valid);
        assert!(verification
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::QuantityExceedsMax));
    }

    #[test]
    fn test_price_exceeds_limit() {
        let problem = create_test_problem();

        let mut result = MatchingResult::new(problem.liquidity.snapshot());
        result.fills.push(Fill::new(1, 50, 700_000_000)); // limit is 600_000_000
        result.total_welfare = 0;

        let verification = verify(&problem, &result);
        assert!(!verification.valid);
        assert!(verification
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::PriceExceedsLimit));
    }

    #[test]
    fn test_duplicate_fill() {
        let problem = create_test_problem();

        let mut result = MatchingResult::new(problem.liquidity.snapshot());
        result.fills.push(Fill::new(1, 50, 500_000_000));
        result.fills.push(Fill::new(1, 30, 500_000_000)); // Duplicate
        result.total_welfare = 0;

        let verification = verify(&problem, &result);
        assert!(!verification.valid);
        assert!(verification
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::DuplicateFill));
    }

    #[test]
    fn test_order_not_found() {
        let problem = create_test_problem();

        let mut result = MatchingResult::new(problem.liquidity.snapshot());
        result.fills.push(Fill::new(999, 50, 500_000_000)); // Order doesn't exist
        result.total_welfare = 0;

        let verification = verify(&problem, &result);
        assert!(!verification.valid);
        assert!(verification
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::OrderNotFound));
    }

    #[test]
    fn test_aon_violation() {
        let mut problem = create_test_problem();

        // Make order 1 all-or-none
        problem.orders[0].min_fill = 100;

        let mut result = MatchingResult::new(problem.liquidity.snapshot());
        result.fills.push(Fill::new(1, 50, 500_000_000)); // Partial fill of AON
        result.total_welfare = 0;

        let verification = verify(&problem, &result);
        assert!(!verification.valid);
        assert!(verification
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::QuantityBelowMin));
    }

    #[test]
    fn test_mm_budget_exceeded() {
        let mut problem = create_test_problem();

        // Add MM constraint with small budget
        let mm = MmConstraint::new(MmId(1), 10_000_000_000) // $10 budget
            .with_order(1, MmSide::SellYes)
            .with_order(2, MmSide::SellYes);
        problem.mm_constraints.push(mm);

        let mut result = MatchingResult::new(problem.liquidity.snapshot());
        // Fill both orders - each costs ~$50 capital (100 shares * $0.50)
        result.fills.push(Fill::new(1, 100, 500_000_000));
        result.fills.push(Fill::new(2, 100, 500_000_000));

        let order1 = problem.orders.iter().find(|o| o.id == 1).unwrap();
        let order2 = problem.orders.iter().find(|o| o.id == 2).unwrap();
        result.total_welfare = result.fills[0].welfare(order1) + result.fills[1].welfare(order2);

        let verification = verify(&problem, &result);
        assert!(!verification.valid);
        assert!(verification
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::MmBudgetExceeded));
    }

    #[test]
    fn test_welfare_mismatch() {
        let problem = create_test_problem();

        let mut result = MatchingResult::new(problem.liquidity.snapshot());
        result.fills.push(Fill::new(1, 50, 500_000_000));
        result.total_welfare = 999_999_999_999; // Wrong welfare

        let verification = verify(&problem, &result);
        assert!(!verification.valid);
        assert!(verification
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::WelfareMismatch));
    }
}
