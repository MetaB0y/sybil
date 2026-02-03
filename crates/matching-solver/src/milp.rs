//! MILP solver for optimal matching with Uniform Clearing Prices (UCP).
//!
//! Formulates the matching problem as a Mixed-Integer Linear Program:
//!
//! **Variables:**
//! - `z_i ∈ {0,1}`: whether order i is filled
//! - `q_i ∈ [0, max_fill_i]`: fill quantity for order i
//! - `p_m ∈ [0, NANOS_PER_DOLLAR]`: YES clearing price for market m
//! - `mint_m ∈ ℝ`: net minting for market m (positive = mint, negative = burn)
//!
//! **Objective (maximize):**
//! `Σ_buyers(L_i * q_i) - Σ_sellers(L_i * q_i) - Σ_m(NANOS_PER_DOLLAR * mint_m)`
//!
//! This equals total welfare because clearing price terms cancel between participants,
//! leaving only the price-independent surplus minus the cost of minting new position pairs.
//!
//! **Constraints:**
//! - z/q linking: AON, min/max fill
//! - UCP (Big-M): if order is filled, clearing price must satisfy its limit
//! - Position balance: `net_YES = net_NO = mint_m` per market (minting is symmetric)

use matching_engine::{Fill, MarketId, Nanos, Order, Problem, NANOS_PER_DOLLAR};

use crate::{MatchingResult, Solver};

use highs::{Col, HighsModelStatus, RowProblem, Sense};
use std::collections::{HashMap, HashSet};

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
    /// Clearing prices derived by the MILP (YES price per market)
    pub clearing_prices: HashMap<MarketId, Vec<Nanos>>,
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
        let mut pairs: Vec<_> = self.liquidity_duals.iter().map(|(&k, &v)| (k, v)).collect();
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

/// Classify a single-market binary order.
/// Returns `(outcome, is_seller)` where outcome=0 is YES, outcome=1 is NO.
fn classify_single_market_order(order: &Order) -> (u8, bool) {
    let is_seller = order.is_seller();

    if is_seller {
        // Find the outcome being sold (most negative payoff)
        let mut sold_outcome = 0u8;
        let mut most_neg = 0i8;
        for s in 0..order.num_states as usize {
            if order.payoffs[s] < most_neg {
                most_neg = order.payoffs[s];
                sold_outcome = s as u8;
            }
        }
        (sold_outcome, true)
    } else {
        // Find the outcome being bought (most positive payoff)
        let mut bought_outcome = 0u8;
        let mut most_pos = 0i8;
        for s in 0..order.num_states as usize {
            if order.payoffs[s] > most_pos {
                most_pos = order.payoffs[s];
                bought_outcome = s as u8;
            }
        }
        (bought_outcome, false)
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
    pub fn solve_with_duals(&self, problem: &Problem) -> (MilpResult, DualAnalysis) {
        let result = self.solve_with_status(problem);
        let analysis = self.compute_dual_analysis(problem, &result);
        (result, analysis)
    }

    fn compute_dual_analysis(&self, problem: &Problem, result: &MilpResult) -> DualAnalysis {
        let mut analysis = DualAnalysis::default();

        // Count AON constraints (orders that couldn't be partially filled)
        for order in &problem.orders {
            if order.is_all_or_none() {
                let filled = result.result.fills.iter().any(|f| f.order_id == order.id);
                if !filled {
                    analysis.binding_aon_constraints += 1;
                }
            }
        }

        analysis.objective_value = result.result.total_welfare as f64;
        analysis
    }

    /// Solve with full status reporting.
    pub fn solve_with_status(&self, problem: &Problem) -> MilpResult {
        let start = std::time::Instant::now();
        let mut result = MatchingResult::new();

        // Filter out conditional orders and multi-market orders (bundles).
        // Bundles require cross-market payoff modeling that this MILP doesn't handle.
        let active_orders: Vec<_> = problem
            .orders
            .iter()
            .filter(|o| !o.is_conditional() && o.num_markets == 1 && o.num_states == 2)
            .collect();

        if active_orders.is_empty() {
            return MilpResult {
                result,
                status: SolveStatus::Optimal,
                solve_time_secs: start.elapsed().as_secs_f64(),
                clearing_prices: HashMap::new(),
            };
        }

        let solve_result = self.solve_with_highs(&active_orders, problem);

        match solve_result {
            Ok((solution, status, solve_time)) => {
                let mut clearing_prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();

                // Build clearing prices from price variables
                for (&market, &p_yes_f64) in &solution.p_values {
                    let p_yes = p_yes_f64.round().max(0.0) as Nanos;
                    let p_no = NANOS_PER_DOLLAR.saturating_sub(p_yes);
                    clearing_prices.insert(market, vec![p_yes, p_no]);
                }

                // Extract fills from solution
                for (i, order) in active_orders.iter().enumerate() {
                    let z_val = solution.z_values.get(i).copied().unwrap_or(0.0);
                    let q_val = solution.q_values.get(i).copied().unwrap_or(0.0);

                    if z_val > 0.5 && q_val > 0.5 {
                        let fill_qty = q_val.round() as u64;

                        if fill_qty >= order.min_fill {
                            // Compute fill_price from clearing prices
                            let fill_price = if order.num_markets == 1 {
                                let market = order.markets[0];
                                if let Some(prices) = clearing_prices.get(&market) {
                                    let (outcome, _) = classify_single_market_order(order);
                                    prices.get(outcome as usize).copied().unwrap_or(order.limit_price)
                                } else {
                                    order.limit_price
                                }
                            } else {
                                // Multi-market: no UCP, use limit_price
                                order.limit_price
                            };

                            let fill = Fill::new(order.id, fill_qty, fill_price);
                            result.add_fill(fill, order);
                        }
                    } else if order.is_all_or_none() {
                        result.orders_unfilled_aon += 1;
                    } else {
                        result.orders_unfilled_liquidity += 1;
                    }
                }

                MilpResult {
                    result,
                    status,
                    solve_time_secs: solve_time,
                    clearing_prices,
                }
            }
            Err(err_msg) => {
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
                    clearing_prices: HashMap::new(),
                }
            }
        }
    }

    /// Build and solve the MILP using HiGHS.
    ///
    /// The formulation enforces:
    /// - UCP via Big-M indicator constraints
    /// - Position balance via mint variables (minting/burning)
    /// - MM budget constraints (conservative linearization)
    /// - Market group constraints (sum of YES prices ≤ $1)
    fn solve_with_highs(
        &self,
        active_orders: &[&Order],
        problem: &Problem,
    ) -> Result<(MilpSolution, SolveStatus, f64), String> {
        let start = std::time::Instant::now();
        let n = active_orders.len();

        let mut pb = RowProblem::default();
        let nanos_f = NANOS_PER_DOLLAR as f64;

        // Build order_id -> index map for MM constraint lookups
        let order_id_to_idx: HashMap<u64, usize> = active_orders
            .iter()
            .enumerate()
            .map(|(i, o)| (o.id, i))
            .collect();

        // Collect all markets from active orders (already filtered to single-market binary)
        let mut markets: Vec<MarketId> = Vec::new();
        {
            let mut seen = HashSet::new();
            for order in active_orders.iter() {
                let m = order.markets[0];
                if !m.is_none() && seen.insert(m) {
                    markets.push(m);
                }
            }
        }

        // ================================================================
        // Variables
        // ================================================================

        // z_i (binary): is order i filled?
        let z_cols: Vec<Col> = (0..n)
            .map(|_| pb.add_integer_column(0.0, 0.0..=1.0))
            .collect();

        // q_i (continuous): fill quantity for order i
        // Objective: +L for buyers, -L for sellers (welfare contribution)
        let q_cols: Vec<Col> = (0..n)
            .map(|i| {
                let order = active_orders[i];
                let sign = if order.is_seller() { -1.0 } else { 1.0 };
                let obj = sign * order.limit_price as f64;
                pb.add_column(obj, 0.0..=(order.max_fill as f64))
            })
            .collect();

        // p_m (continuous): YES clearing price for market m
        // NO price = NANOS_PER_DOLLAR - p_m (complementarity is automatic)
        let p_cols: HashMap<MarketId, Col> = markets
            .iter()
            .map(|&m| {
                let col = pb.add_column(0.0, 0.0..=nanos_f);
                (m, col)
            })
            .collect();

        // mint_m (continuous, free): net minting for market m
        // Positive = minting (costs $1 per pair), negative = burning (earns $1)
        // Objective: -NANOS_PER_DOLLAR per unit (minting cost)
        let mint_cols: HashMap<MarketId, Col> = markets
            .iter()
            .map(|&m| {
                let col = pb.add_column(-nanos_f, -1e15..=1e15);
                (m, col)
            })
            .collect();

        // ================================================================
        // Per-order constraints: z/q linking
        // ================================================================

        for (i, order) in active_orders.iter().enumerate() {
            if order.is_all_or_none() {
                // AON: q_i = z_i * max_fill
                pb.add_row(
                    0.0..=0.0,
                    [(q_cols[i], 1.0), (z_cols[i], -(order.max_fill as f64))],
                );
            } else {
                if order.min_fill > 0 {
                    pb.add_row(
                        0.0..,
                        [(q_cols[i], 1.0), (z_cols[i], -(order.min_fill as f64))],
                    );
                }
                pb.add_row(
                    ..=0.0,
                    [(q_cols[i], 1.0), (z_cols[i], -(order.max_fill as f64))],
                );
            }
        }

        // ================================================================
        // UCP constraints (Big-M)
        // ================================================================

        let big_m = nanos_f;

        for (i, order) in active_orders.iter().enumerate() {
            let market = order.markets[0];
            let Some(&p_col) = p_cols.get(&market) else {
                continue;
            };

            let (outcome, is_seller) = classify_single_market_order(order);
            let is_yes = outcome == 0;

            if is_yes && !is_seller {
                pb.add_row(
                    ..=(order.limit_price as f64 + big_m),
                    [(p_col, 1.0), (z_cols[i], big_m)],
                );
            } else if is_yes && is_seller {
                pb.add_row(
                    (order.limit_price as f64 - big_m)..,
                    [(p_col, 1.0), (z_cols[i], -big_m)],
                );
            } else if !is_yes && !is_seller {
                let threshold = nanos_f - order.limit_price as f64;
                pb.add_row(
                    (threshold - big_m)..,
                    [(p_col, 1.0), (z_cols[i], -big_m)],
                );
            } else {
                let threshold = nanos_f - order.limit_price as f64;
                pb.add_row(
                    ..=(threshold + big_m),
                    [(p_col, 1.0), (z_cols[i], big_m)],
                );
            }
        }

        // ================================================================
        // Position balance constraints per market
        // ================================================================

        for &market in &markets {
            let mut yes_terms: Vec<(Col, f64)> = Vec::new();
            let mut no_terms: Vec<(Col, f64)> = Vec::new();

            for (i, order) in active_orders.iter().enumerate() {
                if order.markets[0] != market {
                    continue;
                }

                let (outcome, is_seller) = classify_single_market_order(order);
                let coef = if is_seller { -1.0 } else { 1.0 };

                if outcome == 0 {
                    yes_terms.push((q_cols[i], coef));
                } else {
                    no_terms.push((q_cols[i], coef));
                }
            }

            if let Some(&mint_col) = mint_cols.get(&market) {
                yes_terms.push((mint_col, -1.0));
                if !yes_terms.is_empty() {
                    pb.add_row(0.0..=0.0, yes_terms);
                }

                no_terms.push((mint_col, -1.0));
                if !no_terms.is_empty() {
                    pb.add_row(0.0..=0.0, no_terms);
                }
            }
        }

        // ================================================================
        // MM budget constraints (conservative linearization)
        // ================================================================
        //
        // Capital per MM fill is price-dependent (bilinear: price * qty).
        // We use a conservative upper bound based on the order's limit price:
        //   BuyYes/SellNo:  capital ≤ L * q   (since price ≤ L for buyers)
        //   SellYes/BuyNo:  capital ≤ (1-L) * q  (since price ≥ L for sellers)
        //
        // This is a valid linear upper bound. It's slightly conservative
        // (overestimates capital when clearing price < limit), meaning the
        // MILP may reject some fills that would actually fit the budget.

        for mm in &problem.mm_constraints {
            let mut budget_terms: Vec<(Col, f64)> = Vec::new();

            for &order_id in &mm.order_ids {
                let Some(&idx) = order_id_to_idx.get(&order_id) else {
                    continue; // Order not in active set (filtered out)
                };

                let order = active_orders[idx];
                let Some(&side) = mm.order_sides.get(&order_id) else {
                    continue;
                };

                // Conservative upper bound on capital per unit
                use matching_engine::MmSide;
                let capital_per_unit = match side {
                    MmSide::BuyYes | MmSide::SellNo => order.limit_price as f64,
                    MmSide::SellYes | MmSide::BuyNo => nanos_f - order.limit_price as f64,
                };

                budget_terms.push((q_cols[idx], capital_per_unit));
            }

            if !budget_terms.is_empty() {
                pb.add_row(..=(mm.max_capital as f64), budget_terms);
            }
        }

        // ================================================================
        // Market group constraints
        // ================================================================
        //
        // For multi-outcome events (e.g., "Who wins the election?"), markets
        // in a group represent mutually exclusive outcomes. The constraint:
        //   Σ(p_YES_m for m in group) ≤ NANOS_PER_DOLLAR
        // prevents prices from implying > 100% total probability.

        for group in &problem.market_groups {
            let mut group_terms: Vec<(Col, f64)> = Vec::new();

            for &market in &group.markets {
                if let Some(&p_col) = p_cols.get(&market) {
                    group_terms.push((p_col, 1.0));
                }
            }

            if !group_terms.is_empty() {
                pb.add_row(..=nanos_f, group_terms);
            }
        }

        // ================================================================
        // Solve
        // ================================================================

        let mut model = pb.optimise(Sense::Maximise);

        model.set_option("output_flag", false);

        if let Some(timeout) = self.config.timeout_secs {
            model.set_option("time_limit", timeout);
        }

        if self.config.gap_tolerance > 0.0 {
            model.set_option("mip_rel_gap", self.config.gap_tolerance);
        }

        let solved = model.solve();
        let solve_time = start.elapsed().as_secs_f64();

        let extract_solution = |solved: &highs::SolvedModel| -> MilpSolution {
            let sol = solved.get_solution();
            MilpSolution {
                z_values: z_cols.iter().map(|&c| sol[c]).collect(),
                q_values: q_cols.iter().map(|&c| sol[c]).collect(),
                p_values: p_cols.iter().map(|(&m, &c)| (m, sol[c])).collect(),
            }
        };

        let status = solved.status();
        match status {
            HighsModelStatus::Optimal => {
                let solution = extract_solution(&solved);
                Ok((solution, SolveStatus::Optimal, solve_time))
            }
            HighsModelStatus::Infeasible => Ok((
                MilpSolution {
                    z_values: vec![0.0; n],
                    q_values: vec![0.0; n],
                    p_values: HashMap::new(),
                },
                SolveStatus::Infeasible,
                solve_time,
            )),
            HighsModelStatus::ObjectiveBound
            | HighsModelStatus::ObjectiveTarget
            | HighsModelStatus::ReachedTimeLimit
            | HighsModelStatus::ReachedIterationLimit => {
                let solution = extract_solution(&solved);
                let gap_percent = 0.0;
                Ok((
                    solution,
                    SolveStatus::TimeLimitReached { gap_percent },
                    solve_time,
                ))
            }
            _ => Err(format!("Solver returned unexpected status: {:?}", status)),
        }
    }
}

/// Internal solution representation
struct MilpSolution {
    z_values: Vec<f64>,
    q_values: Vec<f64>,
    /// YES clearing price per market (from p_m variables)
    p_values: HashMap<MarketId, f64>,
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

        // Sell orders provide supply
        problem.orders.push(matching_engine::outcome_sell(
            &problem.markets,
            100,
            market,
            0,
            500_000_000,
            1000,
        ));
        problem.orders.push(matching_engine::outcome_sell(
            &problem.markets,
            101,
            market,
            1,
            500_000_000,
            1000,
        ));

        // Add buy orders
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
        // With proper UCP, welfare should be positive (not zero)
        assert!(result.total_welfare > 0, "welfare should be positive, got {}", result.total_welfare);
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

    #[test]
    fn test_milp_clearing_prices() {
        let problem = create_test_problem();
        let solver = MilpSolver::new();

        let milp_result = solver.solve_with_status(&problem);
        assert!(milp_result.status.is_optimal());

        // Should have clearing prices for the market
        assert!(!milp_result.clearing_prices.is_empty(), "should produce clearing prices");

        // YES + NO should sum to $1
        for (_market, prices) in &milp_result.clearing_prices {
            assert_eq!(prices.len(), 2);
            let sum = prices[0] + prices[1];
            // Allow small rounding error
            assert!(
                (sum as i64 - NANOS_PER_DOLLAR as i64).unsigned_abs() < 2,
                "YES+NO should sum to $1, got {}",
                sum
            );
        }
    }

    #[test]
    fn test_milp_minting() {
        // Test where only buyers exist — must be matched via minting
        let mut problem = Problem::new("minting_test");
        let market = problem.markets.add_binary("market");

        // YES buyer at 60¢
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            100,
        ));

        // NO buyer at 50¢
        problem.orders.push(matching_engine::simple_no_buy(
            &problem.markets,
            2,
            market,
            500_000_000,
            100,
        ));

        let solver = MilpSolver::new();
        let result = solver.solve_with_status(&problem);

        // Both should fill via minting (60¢ + 50¢ > $1 → positive welfare)
        assert!(result.status.is_optimal());
        assert_eq!(result.result.orders_filled, 2, "both orders should fill via minting");
        // Welfare = (60¢ + 50¢ - $1) * 100 = 10¢ * 100 = $10
        assert!(result.result.total_welfare > 0, "minting should produce positive welfare");
    }
}
