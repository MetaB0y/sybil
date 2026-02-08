//! Result verification for ZK proof integration.
//!
//! Verifies that a matching result is valid and could be accepted by a ZK prover.
//! All invariants that the ZK circuit will check are verified here first.

use std::collections::{HashMap, HashSet};

use matching_engine::{Fill, MarketId, Order, Problem};

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
    /// Number of markets checked for position balance
    pub markets_checked: usize,
    /// Computed total volume (sum of fill quantities)
    pub computed_volume: u64,
    /// Computed number of orders filled
    pub computed_orders_filled: usize,
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
    /// Net YES positions ≠ net NO positions for a market (money creation)
    PositionImbalance,
    /// Reported volume/count totals don't match computed values
    VolumeCountMismatch,
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

            // Check price constraint (seller-aware)
            // Buyers: fill_price must be <= limit (pay no more than willing)
            // Sellers: fill_price must be >= limit (receive at least minimum)
            let price_violated = if order.is_seller() {
                fill.fill_price < order.limit_price
            } else {
                fill.fill_price > order.limit_price
            };
            if price_violated {
                let dir = if order.is_seller() { "<" } else { ">" };
                violations.push(Violation {
                    kind: ViolationKind::PriceExceedsLimit,
                    details: format!(
                        "Order {} ({}): fill_price {} {} limit_price {}",
                        fill.order_id,
                        if order.is_seller() { "sell" } else { "buy" },
                        fill.fill_price,
                        dir,
                        order.limit_price
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

        // Verify position balance (catches money creation)
        self.verify_position_balance(problem, result, &order_map, &mut violations, &mut stats);

        // Verify reported totals match computed values
        self.verify_reported_totals(result, &mut violations, &mut stats);

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

    /// Verify position balance: for each binary market, net YES == net NO positions.
    ///
    /// Minting creates 1 YES + 1 NO share. If fills create an imbalance (e.g.,
    /// BuyNo fill without a corresponding BuyYes), positions are created from
    /// thin air — money at resolution.
    fn verify_position_balance(
        &self,
        _problem: &Problem,
        result: &MatchingResult,
        order_map: &HashMap<u64, &Order>,
        violations: &mut Vec<Violation>,
        stats: &mut VerificationStats,
    ) {
        // For each market, accumulate the net marginal payoff across all fills.
        // The marginal payoff of an order for market m_idx is:
        //   Σ_s (payoff[s] * indicator[m_idx outcome=YES in s]) - Σ_s (payoff[s] * indicator[m_idx outcome=NO in s])
        // divided by the number of states of all OTHER markets (to get per-share contribution).
        //
        // For a single-market binary order:
        //   marginal = payoff[0] - payoff[1]  (state 0 = YES, state 1 = NO)
        //   BuyYes [1,0] → +1, BuyNo [0,1] → -1
        //
        // The sum of marginal * fill_qty across all fills must be 0 for each market.
        let mut net_position: HashMap<MarketId, i64> = HashMap::new();

        for fill in &result.fills {
            if fill.fill_qty == 0 {
                continue;
            }
            let Some(order) = order_map.get(&fill.order_id) else {
                continue; // Already flagged by OrderNotFound
            };

            let num_markets = order.num_markets as usize;
            let num_states = order.num_states as usize;

            for m_idx in 0..num_markets {
                let market_id = order.markets[m_idx];
                if market_id.is_none() {
                    continue;
                }

                // Compute marginal payoff for this market:
                // Sum payoffs over all states where this market = YES (outcome 0),
                // minus sum over all states where this market = NO (outcome 1).
                // For binary markets, stride = 1 << m_idx.
                let stride = 1usize << m_idx;
                let mut marginal: i64 = 0;

                for s in 0..num_states {
                    let outcome_for_m = (s / stride) % 2;
                    let payoff = order.payoffs[s] as i64;
                    if outcome_for_m == 0 {
                        // YES state for this market
                        marginal += payoff;
                    } else {
                        // NO state for this market
                        marginal -= payoff;
                    }
                }

                // marginal is scaled by the number of "other" states (2^(num_markets-1)),
                // so normalize. All markets are binary, so other_states = num_states / 2.
                // Actually we want the per-share marginal, which for binary markets
                // with the stride decomposition above sums over all 2^(N-1) pairs,
                // giving marginal * 2^(N-1). Divide to get per-share.
                let other_states = (num_states / 2) as i64;
                let normalized = marginal / other_states;

                *net_position.entry(market_id).or_insert(0) += normalized * fill.fill_qty as i64;
            }
        }

        stats.markets_checked = net_position.len();

        for (market_id, net) in &net_position {
            if *net != 0 {
                violations.push(Violation {
                    kind: ViolationKind::PositionImbalance,
                    details: format!(
                        "Market {}: net position delta = {} (expected 0). \
                         Positions created from thin air.",
                        market_id, net
                    ),
                });
            }
        }
    }

    /// Verify that reported aggregate totals match computed values.
    fn verify_reported_totals(
        &self,
        result: &MatchingResult,
        violations: &mut Vec<Violation>,
        stats: &mut VerificationStats,
    ) {
        let computed_volume: u64 = result.fills.iter().map(|f| f.fill_qty).sum();
        let computed_orders_filled = result.fills.iter().filter(|f| f.fill_qty > 0).count();

        stats.computed_volume = computed_volume;
        stats.computed_orders_filled = computed_orders_filled;

        if computed_volume != result.total_quantity_filled {
            violations.push(Violation {
                kind: ViolationKind::VolumeCountMismatch,
                details: format!(
                    "total_quantity_filled: reported {} != computed {}",
                    result.total_quantity_filled, computed_volume
                ),
            });
        }

        if computed_orders_filled != result.orders_filled {
            violations.push(Violation {
                kind: ViolationKind::VolumeCountMismatch,
                details: format!(
                    "orders_filled: reported {} != computed {}",
                    result.orders_filled, computed_orders_filled
                ),
            });
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
    use matching_engine::{simple_no_buy, simple_yes_buy, MmConstraint, MmId, MmSide};

    fn create_test_problem() -> Problem {
        let mut problem = Problem::new("test");
        let market = problem.markets.add_binary("m");

        // Add BuyYes orders (ids 1-5)
        for i in 1..=5 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i,
                market,
                600_000_000, // $0.60 limit
                100,
            ));
        }

        // Add BuyNo orders (ids 11-15) for position balance
        for i in 11..=15 {
            problem.orders.push(simple_no_buy(
                &problem.markets,
                i,
                market,
                500_000_000, // $0.50 limit
                100,
            ));
        }

        problem
    }

    /// Helper to build a valid MatchingResult with correct totals.
    fn build_result(fills: Vec<Fill>, problem: &Problem) -> MatchingResult {
        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();
        let mut result = MatchingResult::new();
        for fill in fills {
            let order = order_map[&fill.order_id];
            result.add_fill(fill, order);
        }
        result
    }

    #[test]
    fn test_valid_result() {
        let problem = create_test_problem();

        // BuyYes + BuyNo at same qty → positions balance
        let result = build_result(
            vec![
                Fill::new(1, 50, 500_000_000),   // BuyYes 50
                Fill::new(2, 100, 550_000_000),   // BuyYes 100
                Fill::new(11, 50, 500_000_000),   // BuyNo 50
                Fill::new(12, 100, 500_000_000),  // BuyNo 100
            ],
            &problem,
        );

        let verification = verify(&problem, &result);
        assert!(verification.valid, "Violations: {:?}", verification.violations);
    }

    #[test]
    fn test_quantity_exceeds_max() {
        let problem = create_test_problem();

        let mut result = MatchingResult::new();
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

        let mut result = MatchingResult::new();
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

        let mut result = MatchingResult::new();
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

        let mut result = MatchingResult::new();
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

        let mut result = MatchingResult::new();
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

        let mut result = MatchingResult::new();
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

        let mut result = MatchingResult::new();
        result.fills.push(Fill::new(1, 50, 500_000_000));
        result.total_welfare = 999_999_999_999; // Wrong welfare

        let verification = verify(&problem, &result);
        assert!(!verification.valid);
        assert!(verification
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::WelfareMismatch));
    }

    // ========== Position balance tests ==========

    #[test]
    fn test_position_balance_valid() {
        // Equal BuyYes + BuyNo fills → net zero → passes
        let problem = create_test_problem();

        let result = build_result(
            vec![
                Fill::new(1, 100, 500_000_000),  // BuyYes 100
                Fill::new(11, 100, 500_000_000), // BuyNo 100
            ],
            &problem,
        );

        let verification = verify(&problem, &result);
        assert!(verification.valid, "Violations: {:?}", verification.violations);
        assert!(verification.stats.markets_checked > 0);
    }

    #[test]
    fn test_position_imbalance() {
        // BuyNo fill without corresponding BuyYes → position imbalance
        let problem = create_test_problem();

        let result = build_result(
            vec![
                Fill::new(11, 100, 500_000_000), // BuyNo only, no BuyYes
            ],
            &problem,
        );

        let verification = verify(&problem, &result);
        assert!(!verification.valid);
        assert!(
            verification
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::PositionImbalance),
            "Expected PositionImbalance, got: {:?}",
            verification.violations
        );
    }

    #[test]
    fn test_position_imbalance_unequal_qty() {
        // BuyYes(100) + BuyNo(50) → net imbalance of 50
        let problem = create_test_problem();

        let result = build_result(
            vec![
                Fill::new(1, 100, 500_000_000),  // BuyYes 100
                Fill::new(11, 50, 500_000_000),  // BuyNo 50
            ],
            &problem,
        );

        let verification = verify(&problem, &result);
        assert!(!verification.valid);
        assert!(verification
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::PositionImbalance));
    }

    // ========== Volume/count consistency tests ==========

    #[test]
    fn test_volume_mismatch() {
        let problem = create_test_problem();

        let mut result = build_result(
            vec![
                Fill::new(1, 100, 500_000_000),
                Fill::new(11, 100, 500_000_000),
            ],
            &problem,
        );
        // Corrupt the reported total
        result.total_quantity_filled = 999;

        let verification = verify(&problem, &result);
        assert!(!verification.valid);
        assert!(
            verification
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::VolumeCountMismatch),
            "Expected VolumeCountMismatch, got: {:?}",
            verification.violations
        );
    }

    #[test]
    fn test_orders_filled_mismatch() {
        let problem = create_test_problem();

        let mut result = build_result(
            vec![
                Fill::new(1, 100, 500_000_000),
                Fill::new(11, 100, 500_000_000),
            ],
            &problem,
        );
        // Corrupt the reported count
        result.orders_filled = 999;

        let verification = verify(&problem, &result);
        assert!(!verification.valid);
        assert!(
            verification
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::VolumeCountMismatch),
            "Expected VolumeCountMismatch, got: {:?}",
            verification.violations
        );
    }
}
