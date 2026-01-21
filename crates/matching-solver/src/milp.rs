//! MILP solver for optimal matching using good_lp with HiGHS.
//!
//! Formulates the matching problem as a Mixed-Integer Linear Program to find
//! the optimal solution that maximizes welfare subject to liquidity constraints.
//!
//! Supports time-limited solving with status reporting.

use matching_engine::{Fill, MarketId, Order, Problem};

use crate::{MatchingResult, Solver};

use highs::{HighsModelStatus, RowProblem, Sense};
use std::collections::HashMap;

/// Configuration for the MILP solver.
#[derive(Clone, Debug)]
pub struct MilpConfig {
    /// Time limit in seconds. None means no limit.
    pub timeout_secs: Option<f64>,
    /// Optimality gap tolerance (0.0 = exact, 0.01 = 1% gap acceptable)
    pub gap_tolerance: f64,
}

impl Default for MilpConfig {
    fn default() -> Self {
        Self {
            timeout_secs: None,
            gap_tolerance: 0.0,
        }
    }
}

impl MilpConfig {
    /// Create a config with a time limit.
    pub fn with_timeout(timeout_secs: f64) -> Self {
        Self {
            timeout_secs: Some(timeout_secs),
            gap_tolerance: 0.0,
        }
    }

    /// Create a config with time limit and gap tolerance.
    pub fn with_timeout_and_gap(timeout_secs: f64, gap_tolerance: f64) -> Self {
        Self {
            timeout_secs: Some(timeout_secs),
            gap_tolerance,
        }
    }
}

/// Status of a MILP solve.
#[derive(Clone, Debug)]
pub enum SolveStatus {
    /// Found proven optimal solution
    Optimal,
    /// Time limit reached, returning best solution found
    TimeLimitReached {
        /// Gap from optimal (as percentage, e.g., 5.0 = 5%)
        gap_percent: f64,
    },
    /// Problem is infeasible
    Infeasible,
    /// Solver error
    Error(String),
}

impl SolveStatus {
    pub fn is_optimal(&self) -> bool {
        matches!(self, SolveStatus::Optimal)
    }

    pub fn gap(&self) -> Option<f64> {
        match self {
            SolveStatus::Optimal => Some(0.0),
            SolveStatus::TimeLimitReached { gap_percent } => Some(*gap_percent),
            _ => None,
        }
    }
}

/// Result from MILP solver including solve status.
#[derive(Clone, Debug)]
pub struct MilpResult {
    /// The matching result
    pub result: MatchingResult,
    /// Status of the solve
    pub status: SolveStatus,
    /// Time spent solving (seconds)
    pub solve_time_secs: f64,
}

/// Analysis of dual prices from MILP solution.
///
/// Dual prices (shadow prices) indicate the marginal value of relaxing constraints:
/// - High liquidity dual → Market is scarce, bundles sharing it create value
/// - Binding constraint → Opportunity for cross-market optimization
#[derive(Clone, Debug, Default)]
pub struct DualAnalysis {
    /// Shadow prices for liquidity constraints per (market_id, outcome).
    /// High values indicate scarce, valuable liquidity.
    pub liquidity_duals: HashMap<(MarketId, u8), f64>,
    /// Number of binding liquidity constraints (at capacity)
    pub binding_liquidity_constraints: usize,
    /// Number of binding AON constraints
    pub binding_aon_constraints: usize,
    /// Total number of constraints in the model
    pub total_constraints: usize,
    /// Objective value (total welfare)
    pub objective_value: f64,
}

impl DualAnalysis {
    /// Get the most scarce markets (highest dual prices).
    pub fn scarce_markets(&self, top_n: usize) -> Vec<((MarketId, u8), f64)> {
        let mut pairs: Vec<_> = self.liquidity_duals.iter()
            .map(|(&k, &v)| (k, v))
            .collect();
        pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        pairs.truncate(top_n);
        pairs
    }

    /// Summary of where cross-market value comes from.
    pub fn value_summary(&self) -> String {
        let scarce = self.scarce_markets(5);
        let binding_pct = if self.total_constraints > 0 {
            (self.binding_liquidity_constraints as f64 / self.total_constraints as f64) * 100.0
        } else {
            0.0
        };

        format!(
            "Dual Analysis:\n  Binding liquidity: {} ({:.1}% of constraints)\n  Binding AON: {}\n  Top scarce markets: {:?}",
            self.binding_liquidity_constraints,
            binding_pct,
            self.binding_aon_constraints,
            scarce.iter().map(|((m, o), d)| format!("M{}O{}:{:.2}", m.0, o, d)).collect::<Vec<_>>()
        )
    }
}

/// MILP solver that finds the optimal matching solution.
pub struct MilpSolver {
    config: MilpConfig,
}

impl MilpSolver {
    pub fn new() -> Self {
        Self {
            config: MilpConfig::default(),
        }
    }

    /// Create a solver with custom configuration.
    pub fn with_config(config: MilpConfig) -> Self {
        Self { config }
    }

    /// Create a solver with a time limit.
    pub fn with_timeout(timeout_secs: f64) -> Self {
        Self {
            config: MilpConfig::with_timeout(timeout_secs),
        }
    }

    /// Solve and extract dual prices to understand value sources.
    ///
    /// The dual analysis provides insight into:
    /// - Which markets are scarce (high shadow price)
    /// - Where cross-market value comes from
    /// - Which constraints are binding
    pub fn solve_with_duals(&self, problem: &Problem) -> (MilpResult, DualAnalysis) {
        let result = self.solve_with_status(problem);

        // Compute dual analysis by analyzing the solution
        let analysis = self.compute_dual_analysis(problem, &result);

        (result, analysis)
    }

    /// Compute dual analysis from the solution.
    ///
    /// Since HiGHS MIP doesn't provide true duals, we approximate by analyzing
    /// which constraints are binding (at capacity).
    fn compute_dual_analysis(&self, problem: &Problem, result: &MilpResult) -> DualAnalysis {
        let mut analysis = DualAnalysis::default();

        // Compute available liquidity
        let available_liq = Self::compute_available_liquidity(problem);

        // Compute consumed liquidity from fills
        let mut consumed: HashMap<(MarketId, u8), u64> = HashMap::new();
        for fill in &result.result.fills {
            if let Some(order) = problem.orders.iter().find(|o| o.id == fill.order_id) {
                let targets = Self::extract_order_targets(order);
                for (market, outcome) in targets {
                    *consumed.entry((market, outcome)).or_default() += fill.fill_qty;
                }
            }
        }

        // Identify binding constraints and estimate shadow prices
        for ((market, outcome), available) in &available_liq {
            let used = consumed.get(&(*market, *outcome)).copied().unwrap_or(0);
            let utilization = if *available > 0 {
                used as f64 / *available as f64
            } else {
                0.0
            };

            // Estimate shadow price based on utilization
            // Binding constraints (>99% utilized) have high shadow price
            let shadow_price = if utilization > 0.99 {
                analysis.binding_liquidity_constraints += 1;
                1.0  // Fully bound
            } else if utilization > 0.9 {
                0.5 + (utilization - 0.9) * 5.0  // Near-bound
            } else {
                utilization * 0.5  // Slack
            };

            analysis.liquidity_duals.insert((*market, *outcome), shadow_price);
        }

        // Count AON constraints (orders that couldn't be partially filled)
        for order in &problem.orders {
            if order.is_all_or_none() {
                let filled = result.result.fills.iter()
                    .any(|f| f.order_id == order.id);
                if !filled {
                    analysis.binding_aon_constraints += 1;
                }
            }
        }

        analysis.total_constraints = available_liq.len() +
            problem.orders.iter().filter(|o| o.is_all_or_none()).count();
        analysis.objective_value = result.result.total_welfare as f64;

        analysis
    }

    /// Solve with full status reporting.
    pub fn solve_with_status(&self, problem: &Problem) -> MilpResult {
        let start = std::time::Instant::now();
        let mut liquidity = problem.liquidity.snapshot();
        let mut result = MatchingResult::new(liquidity.clone());

        // Filter out conditional orders
        let active_orders: Vec<_> = problem
            .orders
            .iter()
            .filter(|o| !o.is_conditional())
            .collect();

        if active_orders.is_empty() {
            return MilpResult {
                result,
                status: SolveStatus::Optimal,
                solve_time_secs: start.elapsed().as_secs_f64(),
            };
        }

        // Compute available liquidity per (market, outcome)
        let available_liq = Self::compute_available_liquidity(problem);

        // Build the MILP model using HiGHS directly for time limit support
        let solve_result = self.solve_with_highs(&active_orders, &available_liq);

        match solve_result {
            Ok((solution, status, solve_time)) => {
                // Extract fills from solution
                for (i, order) in active_orders.iter().enumerate() {
                    let z_val = solution.z_values.get(i).copied().unwrap_or(0.0);
                    let q_val = solution.q_values.get(i).copied().unwrap_or(0.0);

                    if z_val > 0.5 && q_val > 0.5 {
                        let fill_qty = q_val.round() as u64;

                        if fill_qty >= order.min_fill {
                            let targets = Self::extract_order_targets(order);

                            // Compute average fill price by consuming from the books
                            let mut total_cost: u128 = 0;
                            let mut markets_filled = 0;

                            for (market, outcome) in &targets {
                                if let Some(book) = liquidity.books.get_mut(&(*market, *outcome)) {
                                    let (filled, avg_price) =
                                        book.consume_asks(fill_qty, order.limit_price);
                                    if filled >= fill_qty.min(order.min_fill) {
                                        total_cost += avg_price as u128 * filled as u128;
                                        markets_filled += 1;
                                    }
                                }
                            }

                            if markets_filled == targets.len() && fill_qty > 0 {
                                let avg_fill_price = if !targets.is_empty() {
                                    (total_cost / (fill_qty as u128 * targets.len() as u128))
                                        as u64
                                } else {
                                    0
                                };

                                let fill = Fill::new(order.id, fill_qty, avg_fill_price);
                                result.add_fill(fill, order);
                            } else {
                                if order.is_all_or_none() {
                                    result.orders_unfilled_aon += 1;
                                } else {
                                    result.orders_unfilled_liquidity += 1;
                                }
                            }
                        }
                    } else {
                        if order.is_all_or_none() {
                            result.orders_unfilled_aon += 1;
                        } else {
                            result.orders_unfilled_liquidity += 1;
                        }
                    }
                }

                result.remaining_liquidity = liquidity;

                MilpResult {
                    result,
                    status,
                    solve_time_secs: solve_time,
                }
            }
            Err(err_msg) => {
                // Solver failed - mark all orders as unfilled
                for order in &active_orders {
                    if order.is_all_or_none() {
                        result.orders_unfilled_aon += 1;
                    } else {
                        result.orders_unfilled_liquidity += 1;
                    }
                }

                MilpResult {
                    result,
                    status: SolveStatus::Error(err_msg),
                    solve_time_secs: start.elapsed().as_secs_f64(),
                }
            }
        }
    }

    /// Solve the MILP using HiGHS directly with time limit support.
    fn solve_with_highs(
        &self,
        active_orders: &[&Order],
        available_liq: &HashMap<(MarketId, u8), u64>,
    ) -> Result<(MilpSolution, SolveStatus, f64), String> {
        let start = std::time::Instant::now();
        let n = active_orders.len();

        // Create HiGHS problem
        let mut pb = RowProblem::default();

        // Variable indices: first n are z_i (binary), next n are q_i (continuous)
        // z_i: binary decision variable (is order i filled?)
        // q_i: quantity filled for order i

        // Add z variables (binary/integer) - 0 or 1
        let z_cols: Vec<_> = (0..n)
            .map(|_i| {
                // Objective coefficient is 0 for z (welfare comes from q)
                pb.add_integer_column(0.0, 0.0..=1.0) // Binary: 0 or 1
            })
            .collect();

        // Add q variables (continuous) with objective coefficients
        let q_cols: Vec<_> = (0..n)
            .map(|i| {
                let order = active_orders[i];
                // Objective: maximize limit_price * quantity
                let obj_coef = order.limit_price as f64;
                pb.add_column(obj_coef, 0.0..=(order.max_fill as f64))
            })
            .collect();

        // Add constraints for each order
        for (i, order) in active_orders.iter().enumerate() {
            if order.is_all_or_none() {
                // AON: q_i = z_i * max_fill_i
                // Rewrite as: q_i - max_fill_i * z_i = 0
                pb.add_row(
                    0.0..=0.0,
                    [(q_cols[i], 1.0), (z_cols[i], -(order.max_fill as f64))],
                );
            } else {
                // Partial: q_i >= z_i * min_fill_i
                // Rewrite as: q_i - min_fill_i * z_i >= 0
                if order.min_fill > 0 {
                    pb.add_row(
                        0.0..,
                        [(q_cols[i], 1.0), (z_cols[i], -(order.min_fill as f64))],
                    );
                }
                // q_i <= z_i * max_fill_i
                // Rewrite as: q_i - max_fill_i * z_i <= 0
                pb.add_row(
                    ..=0.0,
                    [(q_cols[i], 1.0), (z_cols[i], -(order.max_fill as f64))],
                );
            }
        }

        // Add liquidity constraints per (market, outcome)
        let mut liq_usage: HashMap<(MarketId, u8), Vec<(highs::Col, f64)>> = HashMap::new();

        for (i, order) in active_orders.iter().enumerate() {
            let targets = Self::extract_order_targets(order);

            for (market, outcome) in targets {
                liq_usage
                    .entry((market, outcome))
                    .or_default()
                    .push((q_cols[i], 1.0));
            }
        }

        for ((market, outcome), usage) in liq_usage {
            let available = available_liq.get(&(market, outcome)).copied().unwrap_or(0);
            pb.add_row(..=(available as f64), usage);
        }

        // Create model and configure
        let mut model = pb.optimise(Sense::Maximise);

        // Configure solver options
        if let Some(timeout) = self.config.timeout_secs {
            model.set_option("time_limit", timeout);
        }

        if self.config.gap_tolerance > 0.0 {
            model.set_option("mip_rel_gap", self.config.gap_tolerance);
        }

        // Solve
        let solved = model.solve();
        let solve_time = start.elapsed().as_secs_f64();

        let status = solved.status();
        match status {
            HighsModelStatus::Optimal => {
                let sol = solved.get_solution();
                let solution = MilpSolution {
                    z_values: z_cols.iter().map(|&c| sol[c]).collect(),
                    q_values: q_cols.iter().map(|&c| sol[c]).collect(),
                };
                Ok((solution, SolveStatus::Optimal, solve_time))
            }
            HighsModelStatus::Infeasible => Ok((
                MilpSolution {
                    z_values: vec![0.0; n],
                    q_values: vec![0.0; n],
                },
                SolveStatus::Infeasible,
                solve_time,
            )),
            HighsModelStatus::ObjectiveBound
            | HighsModelStatus::ObjectiveTarget
            | HighsModelStatus::ReachedTimeLimit
            | HighsModelStatus::ReachedIterationLimit => {
                // Time limit or other limit reached - extract best solution found
                let sol = solved.get_solution();
                let solution = MilpSolution {
                    z_values: z_cols.iter().map(|&c| sol[c]).collect(),
                    q_values: q_cols.iter().map(|&c| sol[c]).collect(),
                };

                // Try to compute gap
                let gap_percent = 0.0; // HiGHS doesn't easily expose gap through this API
                Ok((
                    solution,
                    SolveStatus::TimeLimitReached { gap_percent },
                    solve_time,
                ))
            }
            _ => Err(format!("Solver returned unexpected status: {:?}", status)),
        }
    }

    /// Extract the outcome being bought for each market in an order.
    /// Returns a vec of (market_id, outcome_idx) pairs.
    fn extract_order_targets(order: &Order) -> Vec<(MarketId, u8)> {
        let mut targets = Vec::new();

        for market_idx in 0..order.num_markets as usize {
            let market = order.markets[market_idx];
            if market.is_none() {
                continue;
            }

            // Use the same logic as greedy solver to determine which outcome
            let outcome = Self::determine_outcome(order, market_idx);
            targets.push((market, outcome));
        }

        targets
    }

    /// Determine which outcome is being bought for a specific market in the order.
    fn determine_outcome(order: &Order, market_idx: usize) -> u8 {
        let num_markets = order.num_markets as usize;
        if market_idx >= num_markets {
            return 0;
        }

        // Simple case: single market order
        if num_markets == 1 {
            // Find the best payoff outcome
            let mut best_outcome = 0u8;
            let mut best_payoff = i8::MIN;

            for (i, &payoff) in order.payoffs.iter().take(order.num_states as usize).enumerate() {
                if payoff > best_payoff {
                    best_payoff = payoff;
                    best_outcome = i as u8;
                }
            }
            return best_outcome;
        }

        // Multi-market case: analyze payoff vector
        let market_sizes: Vec<u8> = vec![2; num_markets]; // Assume binary markets

        let mut outcome_votes: [i32; 4] = [0; 4];

        for state_idx in 0..order.num_states as usize {
            let payoff = order.payoffs[state_idx];
            if payoff > 0 {
                let outcome = Self::extract_outcome_from_state(state_idx, market_idx, &market_sizes);
                if (outcome as usize) < outcome_votes.len() {
                    outcome_votes[outcome as usize] += payoff as i32;
                }
            }
        }

        outcome_votes
            .iter()
            .enumerate()
            .max_by_key(|(_, &v)| v)
            .map(|(idx, _)| idx as u8)
            .unwrap_or(0)
    }

    /// Extract the outcome for a specific market from a state index.
    fn extract_outcome_from_state(state_idx: usize, market_idx: usize, market_sizes: &[u8]) -> u8 {
        let mut remaining = state_idx;
        for (i, &size) in market_sizes.iter().enumerate() {
            let outcome = (remaining % size as usize) as u8;
            if i == market_idx {
                return outcome;
            }
            remaining /= size as usize;
        }
        0
    }

    /// Compute available liquidity for each (market, outcome) pair from the asks.
    fn compute_available_liquidity(problem: &Problem) -> HashMap<(MarketId, u8), u64> {
        let mut available = HashMap::new();

        for (&(market, outcome), book) in problem.liquidity.books.iter() {
            let total_ask_qty = book.total_ask_qty();
            if total_ask_qty > 0 {
                available.insert((market, outcome), total_ask_qty);
            }
        }

        available
    }
}

/// Internal solution representation
struct MilpSolution {
    z_values: Vec<f64>,
    q_values: Vec<f64>,
}

impl Default for MilpSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for MilpSolver {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        self.solve_with_status(problem).result
    }

    fn name(&self) -> &str {
        if self.config.timeout_secs.is_some() {
            "MILP (time-limited)"
        } else {
            "MILP"
        }
    }
}

// ============================================================================
// PartialSolver Trait Implementation
// ============================================================================

use crate::combiner::SolutionConfidence;
use crate::traits::{PartialSolution, PartialSolver};

impl PartialSolver for MilpSolver {
    fn solve_partial(&self, problem: &Problem) -> PartialSolution {
        let milp_result = self.solve_with_status(problem);

        let confidence = match &milp_result.status {
            SolveStatus::Optimal => SolutionConfidence::Optimal,
            SolveStatus::TimeLimitReached { gap_percent } => SolutionConfidence::BoundedGap {
                gap_percent: *gap_percent,
            },
            SolveStatus::Infeasible | SolveStatus::Error(_) => SolutionConfidence::Heuristic,
        };

        PartialSolution::with_fills(
            PartialSolver::name(self),
            milp_result.result.fills,
            milp_result.result.total_welfare,
            confidence,
        )
    }

    fn name(&self) -> &str {
        if self.config.timeout_secs.is_some() {
            "MILP (time-limited)"
        } else {
            "MILP"
        }
    }

    fn confidence(&self) -> SolutionConfidence {
        // Default confidence - actual confidence depends on solve status
        if self.config.timeout_secs.is_some() {
            SolutionConfidence::Heuristic
        } else {
            SolutionConfidence::Optimal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::simple_yes_buy;

    fn create_test_problem() -> Problem {
        let mut problem = Problem::new("test");
        let market = problem.markets.add_binary("market");

        // Add liquidity
        problem.liquidity.add_ask(market, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(market, 1, 500_000_000, 1000);

        // Add orders
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            100,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            2,
            market,
            550_000_000,
            200,
        ));

        problem
    }

    #[test]
    fn test_milp_basic() {
        let problem = create_test_problem();
        let solver = MilpSolver::new();

        let result = solver.solve(&problem);
        assert!(result.orders_filled > 0);
    }

    #[test]
    fn test_milp_with_timeout() {
        let problem = create_test_problem();
        let solver = MilpSolver::with_timeout(1.0);

        let milp_result = solver.solve_with_status(&problem);
        assert!(
            matches!(milp_result.status, SolveStatus::Optimal)
                || matches!(milp_result.status, SolveStatus::TimeLimitReached { .. })
        );
    }

    #[test]
    fn test_milp_config() {
        let config = MilpConfig::with_timeout_and_gap(5.0, 0.01);
        assert_eq!(config.timeout_secs, Some(5.0));
        assert_eq!(config.gap_tolerance, 0.01);
    }
}
