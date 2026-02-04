//! MIQCQP solver for optimal matching with Uniform Clearing Prices (UCP).
//!
//! Formulates the matching problem as a Mixed-Integer Quadratically Constrained
//! Quadratic Program using SCIP. Handles ALL order types (single-market, bundles,
//! spreads, multi-market) via per-market marginal contribution coefficients.
//!
//! **Variables:**
//! - `z_i ∈ {0,1}`: whether order i is filled
//! - `q_i ∈ [0, max_fill_i]`: fill quantity for order i
//! - `p_m ∈ [0, NANOS_PER_DOLLAR]`: YES clearing price for market m
//! - `mint_m ∈ ℝ`: net minting for market m (positive = mint, negative = burn)
//!
//! **Objective (maximize):**
//! `Σ sign_i × L_i × q_i - Σ_m NANOS_PER_DOLLAR × mint_m`
//!
//! **Constraints:**
//! - z/q linking: AON, min/max fill
//! - UCP (Big-M): generalized effective price via alpha/beta coefficients
//! - Position balance: per-market c_YES/c_NO coefficients from payoff decomposition
//! - MM budget (quadratic): exact bilinear `price × quantity` via SCIP MIQCQP
//! - Market groups: `Σ p_m ≤ NANOS_PER_DOLLAR` per group

use matching_engine::{Fill, MarketId, Nanos, Order, Problem, NANOS_PER_DOLLAR};

use crate::{MatchingResult, Solver};

use russcip::prelude::*;
use russcip::Variable;
use std::collections::{HashMap, HashSet};

/// How to handle MM budget constraints.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum MmBudgetMode {
    /// Exact bilinear `price × quantity` via SCIP MIQCQP.
    /// Most accurate but makes the problem harder for the solver.
    #[default]
    Exact,
    /// McCormick linearization: replaces bilinear terms with linear envelopes.
    /// Underestimates capital usage → valid relaxation for upper bound.
    /// Keeps the problem as pure MILP → much faster solving.
    McCormick,
    /// Ignore MM budget constraints entirely.
    /// Loosest relaxation — valid upper bound but potentially very loose.
    Ignore,
}

/// Configuration for the MILP solver.
#[derive(Clone, Debug)]
pub struct MilpConfig {
    /// Time limit in seconds. None means no limit.
    pub timeout_secs: Option<f64>,
    /// Optimality gap tolerance (0.0 = exact, 0.01 = 1% gap acceptable)
    pub gap_tolerance: f64,
    /// How to handle MM budget constraints.
    pub mm_budget_mode: MmBudgetMode,
}

impl Default for MilpConfig {
    fn default() -> Self {
        Self {
            timeout_secs: None,
            gap_tolerance: 0.0,
            mm_budget_mode: MmBudgetMode::default(),
        }
    }
}

impl MilpConfig {
    /// Create a config with a time limit.
    pub fn with_timeout(timeout_secs: f64) -> Self {
        Self {
            timeout_secs: Some(timeout_secs),
            gap_tolerance: 0.0,
            mm_budget_mode: MmBudgetMode::default(),
        }
    }

    /// Create a config with time limit and gap tolerance.
    pub fn with_timeout_and_gap(timeout_secs: f64, gap_tolerance: f64) -> Self {
        Self {
            timeout_secs: Some(timeout_secs),
            gap_tolerance,
            mm_budget_mode: MmBudgetMode::default(),
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
/// - High liquidity dual -> Market is scarce, bundles sharing it create value
/// - Binding constraint -> Opportunity for cross-market optimization
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

// ============================================================================
// Generalized order coefficient computation
// ============================================================================

/// Per-market marginal contribution coefficients for an order.
///
/// Decomposition mirrors `settle_generic`: for each market m, we compute
/// the average payoff when m=YES vs m=NO across all states.
pub struct OrderCoefficients {
    /// Average payoff when market m outcome = YES (outcome 0)
    pub c_yes: HashMap<MarketId, f64>,
    /// Average payoff when market m outcome = NO (outcome 1)
    pub c_no: HashMap<MarketId, f64>,
    /// alpha_m = c_YES_m - c_NO_m (price sensitivity per market)
    pub alpha: HashMap<MarketId, f64>,
    /// beta = NANOS_PER_DOLLAR * sum(c_NO_m) (price-independent offset)
    pub beta: f64,
}

/// Compute per-market marginal contribution coefficients from payoff vector.
///
/// For each market m (at index m_idx in the order's market list):
/// - stride = 1 << m_idx (binary markets)
/// - For each state s, `(s / stride) % 2` gives the outcome for market m
/// - c_YES_m = average of payoffs where market m = YES (outcome 0)
/// - c_NO_m = average of payoffs where market m = NO (outcome 1)
pub fn precompute_coefficients(order: &Order) -> OrderCoefficients {
    let num_markets = order.num_markets as usize;
    let num_states = order.num_states as usize;
    let nanos_f = NANOS_PER_DOLLAR as f64;

    let mut c_yes = HashMap::new();
    let mut c_no = HashMap::new();
    let mut alpha = HashMap::new();
    let mut beta_sum = 0.0;

    for m_idx in 0..num_markets {
        let market = order.markets[m_idx];
        if market.is_none() {
            continue;
        }

        let stride = 1usize << m_idx;

        let mut yes_sum: f64 = 0.0;
        let mut yes_count: usize = 0;
        let mut no_sum: f64 = 0.0;
        let mut no_count: usize = 0;

        for s in 0..num_states {
            let outcome_for_market = (s / stride) % 2;
            let payoff = order.payoffs[s] as f64;
            if outcome_for_market == 0 {
                yes_sum += payoff;
                yes_count += 1;
            } else {
                no_sum += payoff;
                no_count += 1;
            }
        }

        let c_y = if yes_count > 0 {
            yes_sum / yes_count as f64
        } else {
            0.0
        };
        let c_n = if no_count > 0 {
            no_sum / no_count as f64
        } else {
            0.0
        };

        c_yes.insert(market, c_y);
        c_no.insert(market, c_n);
        alpha.insert(market, c_y - c_n);
        beta_sum += c_n;
    }

    OrderCoefficients {
        c_yes,
        c_no,
        alpha,
        beta: nanos_f * beta_sum,
    }
}

/// Determine the sign for an order in the welfare objective.
///
/// Uses `is_seller()` for consistency with the verifier and welfare calculation.
/// - Buyer (no negative payoffs) -> +1.0
/// - Seller (any negative payoff) -> -1.0
pub fn milp_sign(order: &Order) -> f64 {
    if order.is_seller() {
        -1.0
    } else {
        1.0
    }
}

// ============================================================================
// MILP Solver
// ============================================================================

/// MIQCQP solver that finds the optimal matching solution via SCIP.
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

        // Filter out conditional orders only — we now handle all market structures.
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
                clearing_prices: HashMap::new(),
            };
        }

        let solve_result = self.solve_with_scip(&active_orders, problem);

        match solve_result {
            Ok((solution, status, solve_time)) => {
                let mut clearing_prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();

                // Build clearing prices from price variables
                for (&market, &p_yes_f64) in &solution.p_values {
                    let p_yes = p_yes_f64.round().max(0.0) as Nanos;
                    let p_no = NANOS_PER_DOLLAR.saturating_sub(p_yes);
                    clearing_prices.insert(market, vec![p_yes, p_no]);
                }

                // Precompute coefficients for fill price extraction
                let coeffs: Vec<_> = active_orders
                    .iter()
                    .map(|o| precompute_coefficients(o))
                    .collect();

                // Extract fills from solution
                for (i, order) in active_orders.iter().enumerate() {
                    let z_val = solution.z_values.get(i).copied().unwrap_or(0.0);
                    let q_val = solution.q_values.get(i).copied().unwrap_or(0.0);

                    if z_val > 0.5 && q_val > 0.5 {
                        let fill_qty = q_val.round() as u64;

                        if fill_qty >= order.min_fill {
                            // Compute fill_price using alpha/beta formula:
                            // eff_price = |sum_m alpha_m * p_m + beta|
                            let eff_price: f64 = coeffs[i]
                                .alpha
                                .iter()
                                .map(|(m, &a)| {
                                    a * solution
                                        .p_values
                                        .get(m)
                                        .copied()
                                        .unwrap_or(0.0)
                                })
                                .sum::<f64>()
                                + coeffs[i].beta;
                            let fill_price =
                                eff_price.abs().round().max(0.0) as Nanos;

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

    /// Build and solve the MIQCQP using SCIP.
    ///
    /// The formulation enforces:
    /// - UCP via Big-M indicator constraints (generalized alpha/beta)
    /// - Position balance via per-market c_YES/c_NO coefficients
    /// - MM budget constraints (exact bilinear price*qty via quadratic constraints)
    /// - Market group constraints (sum of YES prices <= $1)
    fn solve_with_scip(
        &self,
        active_orders: &[&Order],
        problem: &Problem,
    ) -> Result<(ScipSolution, SolveStatus, f64), String> {
        let start = std::time::Instant::now();
        let n = active_orders.len();
        let nanos_f = NANOS_PER_DOLLAR as f64;

        // Precompute coefficients for all orders
        let coeffs: Vec<_> = active_orders
            .iter()
            .map(|o| precompute_coefficients(o))
            .collect();

        // Build order_id -> index map for MM constraint lookups
        let order_id_to_idx: HashMap<u64, usize> = active_orders
            .iter()
            .enumerate()
            .map(|(i, o)| (o.id, i))
            .collect();

        // Collect all markets from active orders (including multi-market orders)
        let mut markets: Vec<MarketId> = Vec::new();
        {
            let mut seen = HashSet::new();
            for order in active_orders.iter() {
                for m_idx in 0..order.num_markets as usize {
                    let m = order.markets[m_idx];
                    if !m.is_none() && seen.insert(m) {
                        markets.push(m);
                    }
                }
            }
        }

        // ================================================================
        // Create SCIP model
        // ================================================================

        let mut model = Model::new()
            .include_default_plugins()
            .create_prob("milp_matching")
            .maximize()
            .hide_output();

        if let Some(timeout) = self.config.timeout_secs {
            model = model.set_time_limit(timeout.ceil() as usize);
        }

        if self.config.gap_tolerance > 0.0 {
            model = model
                .set_real_param("limits/gap", self.config.gap_tolerance)
                .map_err(|e| format!("Failed to set gap tolerance: {:?}", e))?;
        }

        // ================================================================
        // Variables
        // ================================================================

        // z_i (binary): is order i filled? (no objective contribution)
        let z_vars: Vec<Variable> = (0..n)
            .map(|_| model.add(var().bin().obj(0.0)))
            .collect();

        // q_i (continuous): fill quantity for order i
        // Objective: sign_i * L_i (welfare contribution)
        let q_vars: Vec<Variable> = (0..n)
            .map(|i| {
                let order = active_orders[i];
                let sign = milp_sign(order);
                let obj = sign * order.limit_price as f64;
                model.add(var().cont(0.0..=(order.max_fill as f64)).obj(obj))
            })
            .collect();

        // p_m (continuous): YES clearing price for market m
        let p_vars: HashMap<MarketId, Variable> = markets
            .iter()
            .map(|&m| {
                let v = model.add(var().cont(0.0..=nanos_f).obj(0.0));
                (m, v)
            })
            .collect();

        // mint_m (continuous, free): net minting for market m
        // Objective: -NANOS_PER_DOLLAR per unit (minting cost)
        let mint_vars: HashMap<MarketId, Variable> = markets
            .iter()
            .map(|&m| {
                let v = model.add(var().cont(-1e15..=1e15).obj(-nanos_f));
                (m, v)
            })
            .collect();

        // group_mint_g (continuous, free): group-level minting per market group.
        //
        // In a group of N mutually exclusive markets, minting 1 YES share in
        // every market costs only $1 total (guaranteed $1 payoff since exactly
        // one resolves YES). This is N times cheaper than per-market minting.
        //
        // Positive group_mint = negrisk arbitrage (buy YES in all markets)
        // Negative group_mint = posrisk arbitrage (sell YES in all markets)
        //
        // Objective: -NANOS_PER_DOLLAR per unit (same $1 cost as a single mint)
        let market_to_group: HashMap<MarketId, usize> = problem
            .market_groups
            .iter()
            .enumerate()
            .flat_map(|(g_idx, group)| group.markets.iter().map(move |&m| (m, g_idx)))
            .collect();

        let group_mint_vars: Vec<Variable> = (0..problem.market_groups.len())
            .map(|_| model.add(var().cont(-1e15..=1e15).obj(-nanos_f)))
            .collect();

        // ================================================================
        // Per-order constraints: z/q linking
        // ================================================================

        for (i, order) in active_orders.iter().enumerate() {
            if order.is_all_or_none() {
                // AON: q_i = z_i * max_fill  =>  q_i - max_fill * z_i = 0
                model.add(
                    cons()
                        .coef(&q_vars[i], 1.0)
                        .coef(&z_vars[i], -(order.max_fill as f64))
                        .eq(0.0),
                );
            } else {
                if order.min_fill > 0 {
                    // q_i >= min_fill * z_i  =>  q_i - min_fill * z_i >= 0
                    model.add(
                        cons()
                            .coef(&q_vars[i], 1.0)
                            .coef(&z_vars[i], -(order.min_fill as f64))
                            .ge(0.0),
                    );
                }
                // q_i <= max_fill * z_i  =>  q_i - max_fill * z_i <= 0
                model.add(
                    cons()
                        .coef(&q_vars[i], 1.0)
                        .coef(&z_vars[i], -(order.max_fill as f64))
                        .le(0.0),
                );
            }
        }

        // ================================================================
        // UCP constraints (Big-M) using alpha/beta formulation
        // ================================================================
        //
        // Effective price: eff_price_i = Σ_m alpha_i_m * p_m + beta_i
        //
        // For buyers (sign=+1): eff_price <= L when z=1
        //   => Σ_m alpha_m * p_m + M * z_i <= L + M - beta
        //
        // For sellers (sign=-1): eff_price >= L when z=1
        //   => -Σ_m alpha_m * p_m + M * z_i <= -L + M + beta

        let max_markets = active_orders
            .iter()
            .map(|o| o.num_markets as usize)
            .max()
            .unwrap_or(1);
        let big_m = nanos_f * max_markets as f64;

        for (i, order) in active_orders.iter().enumerate() {
            let sign = milp_sign(order);
            let limit = order.limit_price as f64;
            let beta = coeffs[i].beta;

            // Unified UCP constraint for both buyers and sellers:
            //   Buyer (sign=+1): eff_price <= L when z=1
            //   Seller (sign=-1): eff_price <= -L when z=1 (i.e., |eff_price| >= L)
            //
            // Big-M relaxation: alpha*p + M*z <= sign*L + M - beta
            let mut c = cons().coef(&z_vars[i], big_m);
            for (&market, &a) in &coeffs[i].alpha {
                if let Some(p_var) = p_vars.get(&market) {
                    c = c.coef(p_var, a);
                }
            }

            let rhs = sign * limit + big_m - beta;
            model.add(c.le(rhs));
        }

        // ================================================================
        // Position balance constraints per market
        // ================================================================
        //
        // For each market m:
        //   YES: Σ_i c_i_m_YES * q_i = mint_m [+ group_mint_g]
        //   NO:  Σ_i c_i_m_NO  * q_i = mint_m
        //
        // If market m belongs to group g, group_mint_g contributes +1 YES
        // share per unit to market m. This models negrisk/posrisk arbitrage:
        // minting 1 YES in every market of a mutually exclusive group costs
        // only $1 total (vs $N for N separate per-market mints).

        for &market in &markets {
            let mut yes_cons = cons();
            let mut no_cons = cons();

            for i in 0..active_orders.len() {
                // Raw c_yes/c_no already encode direction correctly:
                // Buy YES (payoffs=[+1,0]): c_YES=+1 (demands YES shares)
                // Sell YES (payoffs=[-1,0]): c_YES=-1 (supplies YES shares)
                if let Some(&c_y) = coeffs[i].c_yes.get(&market) {
                    if c_y.abs() > 1e-12 {
                        yes_cons = yes_cons.coef(&q_vars[i], c_y);
                    }
                }

                if let Some(&c_n) = coeffs[i].c_no.get(&market) {
                    if c_n.abs() > 1e-12 {
                        no_cons = no_cons.coef(&q_vars[i], c_n);
                    }
                }
            }

            if let Some(mint_var) = mint_vars.get(&market) {
                // YES balance: demand = per-market mint + group mint (if in group)
                yes_cons = yes_cons.coef(mint_var, -1.0);
                if let Some(&g_idx) = market_to_group.get(&market) {
                    yes_cons = yes_cons.coef(&group_mint_vars[g_idx], -1.0);
                }
                model.add(yes_cons.eq(0.0));

                // NO balance: demand = per-market mint only
                // (group minting creates YES shares, not NO shares)
                no_cons = no_cons.coef(mint_var, -1.0);
                model.add(no_cons.eq(0.0));
            }
        }

        // ================================================================
        // MM budget constraints
        // ================================================================
        //
        // For each MM constraint with orders {i} and budget B:
        //   Σ_i capital_i(p, q) <= B
        //
        // Where capital depends on the MmSide:
        //   BuyYes/SellNo:  capital = p_m * q_i
        //   SellYes/BuyNo:  capital = (NANOS - p_m) * q_i

        if self.config.mm_budget_mode != MmBudgetMode::Ignore {
            for mm in &problem.mm_constraints {
                match self.config.mm_budget_mode {
                    MmBudgetMode::Exact => {
                        // Exact bilinear via SCIP MIQCQP
                        let mut lin_vars_vec: Vec<&Variable> = Vec::new();
                        let mut lin_coefs_vec: Vec<f64> = Vec::new();
                        let mut quad_vars1: Vec<&Variable> = Vec::new();
                        let mut quad_vars2: Vec<&Variable> = Vec::new();
                        let mut quad_coefs_vec: Vec<f64> = Vec::new();
                        let mut has_terms = false;

                        for &order_id in &mm.order_ids {
                            let Some(&idx) = order_id_to_idx.get(&order_id) else {
                                continue;
                            };
                            let order = active_orders[idx];
                            let Some(&side) = mm.order_sides.get(&order_id) else {
                                continue;
                            };
                            let market = order.markets[0];
                            let Some(p_var) = p_vars.get(&market) else {
                                continue;
                            };

                            use matching_engine::MmSide;
                            match side {
                                MmSide::BuyYes | MmSide::SellNo => {
                                    quad_vars1.push(p_var);
                                    quad_vars2.push(&q_vars[idx]);
                                    quad_coefs_vec.push(1.0);
                                }
                                MmSide::SellYes | MmSide::BuyNo => {
                                    lin_vars_vec.push(&q_vars[idx]);
                                    lin_coefs_vec.push(nanos_f);
                                    quad_vars1.push(p_var);
                                    quad_vars2.push(&q_vars[idx]);
                                    quad_coefs_vec.push(-1.0);
                                }
                            }
                            has_terms = true;
                        }

                        if has_terms {
                            model.add_cons_quadratic(
                                lin_vars_vec,
                                &mut lin_coefs_vec,
                                quad_vars1,
                                quad_vars2,
                                &mut quad_coefs_vec,
                                f64::NEG_INFINITY,
                                mm.max_capital as f64,
                                &format!("mm_budget_{}", mm.mm_id.0),
                            );
                        }
                    }
                    MmBudgetMode::McCormick => {
                        // McCormick linearization of bilinear p_m * q_i.
                        //
                        // For w = p * q with p ∈ [0, P], q ∈ [0, Q]:
                        //   McCormick lower bounds: w ≥ 0, w ≥ Pq + Qp - PQ
                        //   McCormick upper bounds: w ≤ Pq, w ≤ Qp
                        //
                        // For the budget constraint Σ capital_i ≤ B, using
                        // McCormick LOWER bounds on capital underestimates
                        // capital usage → relaxes the constraint → valid upper bound.
                        //
                        // Auxiliary w_i variables for each bilinear term.
                        // Collected up-front so they outlive the budget constraint.

                        // First pass: create all auxiliary variables and their
                        // McCormick envelope constraints.
                        struct McCormickTerm {
                            // Index into w_aux_vars
                            w_idx: usize,
                            // Index into q_vars for the linear NANOS*q part (SellYes/BuyNo only)
                            q_idx: Option<usize>,
                            // +1 for BuyYes/SellNo (budget += w), -1 for SellYes/BuyNo (budget += NANOS*q - w)
                            w_sign: f64,
                        }

                        let mut w_aux_vars: Vec<Variable> = Vec::new();
                        let mut terms: Vec<McCormickTerm> = Vec::new();

                        for &order_id in &mm.order_ids {
                            let Some(&idx) = order_id_to_idx.get(&order_id) else {
                                continue;
                            };
                            let order = active_orders[idx];
                            let Some(&side) = mm.order_sides.get(&order_id) else {
                                continue;
                            };
                            let market = order.markets[0];
                            let Some(p_var) = p_vars.get(&market) else {
                                continue;
                            };

                            let q_ub = order.max_fill as f64;
                            let p_ub = nanos_f;

                            // Create auxiliary variable w_i ∈ [0, P*Q]
                            let w = model.add(
                                var().cont(0.0..=(p_ub * q_ub)).obj(0.0),
                            );
                            let w_idx = w_aux_vars.len();
                            w_aux_vars.push(w);
                            let w_ref = &w_aux_vars[w_idx];

                            // McCormick envelope constraints
                            // Lower: w ≥ P*q + Q*p - P*Q
                            model.add(
                                cons()
                                    .coef(w_ref, 1.0)
                                    .coef(&q_vars[idx], -p_ub)
                                    .coef(p_var, -q_ub)
                                    .ge(-p_ub * q_ub),
                            );
                            // Upper: w ≤ P*q
                            model.add(
                                cons()
                                    .coef(w_ref, 1.0)
                                    .coef(&q_vars[idx], -p_ub)
                                    .le(0.0),
                            );
                            // Upper: w ≤ Q*p
                            model.add(
                                cons()
                                    .coef(w_ref, 1.0)
                                    .coef(p_var, -q_ub)
                                    .le(0.0),
                            );

                            use matching_engine::MmSide;
                            match side {
                                MmSide::BuyYes | MmSide::SellNo => {
                                    // capital = p * q = w
                                    terms.push(McCormickTerm {
                                        w_idx,
                                        q_idx: None,
                                        w_sign: 1.0,
                                    });
                                }
                                MmSide::SellYes | MmSide::BuyNo => {
                                    // capital = NANOS*q - p*q = NANOS*q - w
                                    terms.push(McCormickTerm {
                                        w_idx,
                                        q_idx: Some(idx),
                                        w_sign: -1.0,
                                    });
                                }
                            }
                        }

                        if !terms.is_empty() {
                            // Build the budget constraint using collected terms
                            let mut budget_cons = cons();
                            for term in &terms {
                                budget_cons =
                                    budget_cons.coef(&w_aux_vars[term.w_idx], term.w_sign);
                                if let Some(q_idx) = term.q_idx {
                                    budget_cons =
                                        budget_cons.coef(&q_vars[q_idx], nanos_f);
                                }
                            }
                            model.add(budget_cons.le(mm.max_capital as f64));
                        }
                    }
                    MmBudgetMode::Ignore => unreachable!(),
                }
            }
        }

        // ================================================================
        // Market group constraints
        // ================================================================

        for group in &problem.market_groups {
            let mut c = cons();
            let mut has_terms = false;

            for &market in &group.markets {
                if let Some(p_var) = p_vars.get(&market) {
                    c = c.coef(p_var, 1.0);
                    has_terms = true;
                }
            }

            if has_terms {
                model.add(c.le(nanos_f));
            }
        }

        // ================================================================
        // Solve
        // ================================================================

        let solved = model.solve();
        let solve_time = start.elapsed().as_secs_f64();

        let scip_status = solved.status();

        match scip_status {
            Status::Optimal | Status::GapLimit | Status::SolutionLimit
            | Status::BestSolutionLimit => {
                let sol = solved
                    .best_sol()
                    .ok_or_else(|| "No solution found despite optimal status".to_string())?;

                let solution = ScipSolution {
                    z_values: z_vars.iter().map(|v| sol.val(v)).collect(),
                    q_values: q_vars.iter().map(|v| sol.val(v)).collect(),
                    p_values: p_vars.iter().map(|(&m, v)| (m, sol.val(v))).collect(),
                };

                let status = if scip_status == Status::Optimal {
                    SolveStatus::Optimal
                } else {
                    // Compute gap from SCIP
                    let obj = solved.obj_val();
                    let bound = solved.best_bound();
                    let gap_percent = if obj.abs() > 1e-10 {
                        ((bound - obj) / obj.abs() * 100.0).abs()
                    } else {
                        0.0
                    };
                    SolveStatus::TimeLimitReached { gap_percent }
                };

                Ok((solution, status, solve_time))
            }
            Status::TimeLimit | Status::NodeLimit | Status::TotalNodeLimit
            | Status::StallNodeLimit | Status::MemoryLimit | Status::RestartLimit => {
                // Time/resource limit — return best solution if available
                if let Some(sol) = solved.best_sol() {
                    let solution = ScipSolution {
                        z_values: z_vars.iter().map(|v| sol.val(v)).collect(),
                        q_values: q_vars.iter().map(|v| sol.val(v)).collect(),
                        p_values: p_vars.iter().map(|(&m, v)| (m, sol.val(v))).collect(),
                    };

                    let obj = solved.obj_val();
                    let bound = solved.best_bound();
                    let gap_percent = if obj.abs() > 1e-10 {
                        ((bound - obj) / obj.abs() * 100.0).abs()
                    } else {
                        0.0
                    };

                    Ok((
                        solution,
                        SolveStatus::TimeLimitReached { gap_percent },
                        solve_time,
                    ))
                } else {
                    Ok((
                        ScipSolution {
                            z_values: vec![0.0; n],
                            q_values: vec![0.0; n],
                            p_values: HashMap::new(),
                        },
                        SolveStatus::TimeLimitReached {
                            gap_percent: 100.0,
                        },
                        solve_time,
                    ))
                }
            }
            Status::Infeasible | Status::Inforunbd => Ok((
                ScipSolution {
                    z_values: vec![0.0; n],
                    q_values: vec![0.0; n],
                    p_values: HashMap::new(),
                },
                SolveStatus::Infeasible,
                solve_time,
            )),
            Status::Unbounded => Err("Problem is unbounded".to_string()),
            _ => Err(format!("Solver returned unexpected status: {:?}", scip_status)),
        }
    }
}

/// Internal solution representation
struct ScipSolution {
    z_values: Vec<f64>,
    q_values: Vec<f64>,
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
        assert!(
            result.total_welfare > 0,
            "welfare should be positive, got {}",
            result.total_welfare
        );
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

        assert!(
            !milp_result.clearing_prices.is_empty(),
            "should produce clearing prices"
        );

        // YES + NO should sum to $1
        for (_market, prices) in &milp_result.clearing_prices {
            assert_eq!(prices.len(), 2);
            let sum = prices[0] + prices[1];
            assert!(
                (sum as i64 - NANOS_PER_DOLLAR as i64).unsigned_abs() < 2,
                "YES+NO should sum to $1, got {}",
                sum
            );
        }
    }

    #[test]
    fn test_milp_minting() {
        let mut problem = Problem::new("minting_test");
        let market = problem.markets.add_binary("market");

        // YES buyer at 60c
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            100,
        ));

        // NO buyer at 50c
        problem.orders.push(matching_engine::simple_no_buy(
            &problem.markets,
            2,
            market,
            500_000_000,
            100,
        ));

        let solver = MilpSolver::new();
        let result = solver.solve_with_status(&problem);

        assert!(result.status.is_optimal());
        assert_eq!(
            result.result.orders_filled, 2,
            "both orders should fill via minting"
        );
        assert!(
            result.result.total_welfare > 0,
            "minting should produce positive welfare"
        );
    }

    // =====================================================================
    // New tests for generalized MILP
    // =====================================================================

    #[test]
    fn test_milp_coefficients() {
        let mut problem = Problem::new("coeff_test");
        let market_a = problem.markets.add_binary("A");
        let market_b = problem.markets.add_binary("B");

        // YES buyer: payoffs = [+1, 0] on single market
        let yes_buy = simple_yes_buy(&problem.markets, 1, market_a, 600_000_000, 100);
        let coeffs = precompute_coefficients(&yes_buy);
        assert!((coeffs.c_yes[&market_a] - 1.0).abs() < 1e-9, "YES buyer c_YES should be 1.0");
        assert!(coeffs.c_no[&market_a].abs() < 1e-9, "YES buyer c_NO should be 0.0");
        assert!((coeffs.alpha[&market_a] - 1.0).abs() < 1e-9, "YES buyer alpha should be 1.0");
        assert!(coeffs.beta.abs() < 1e-9, "YES buyer beta should be 0.0");

        // NO buyer: payoffs = [0, +1] on single market
        let no_buy = matching_engine::simple_no_buy(&problem.markets, 2, market_a, 500_000_000, 100);
        let coeffs = precompute_coefficients(&no_buy);
        assert!(coeffs.c_yes[&market_a].abs() < 1e-9, "NO buyer c_YES should be 0.0");
        assert!((coeffs.c_no[&market_a] - 1.0).abs() < 1e-9, "NO buyer c_NO should be 1.0");
        assert!((coeffs.alpha[&market_a] - (-1.0)).abs() < 1e-9, "NO buyer alpha should be -1.0");
        assert!((coeffs.beta - NANOS_PER_DOLLAR as f64).abs() < 1e-3, "NO buyer beta should be NANOS");

        // Bundle YES-YES: payoffs = [+1, 0, 0, 0] on two markets
        let bundle = matching_engine::bundle_yes(
            &problem.markets,
            3,
            &[market_a, market_b],
            400_000_000,
            50,
        );
        let coeffs = precompute_coefficients(&bundle);
        // For all-YES bundle: c_YES_A = 0.5 (payoff 1 in state 0 out of 2 YES states),
        // c_NO_A = 0.0, c_YES_B = 0.5, c_NO_B = 0.0
        assert!(
            (coeffs.c_yes[&market_a] - 0.5).abs() < 1e-9,
            "bundle c_YES_A should be 0.5, got {}",
            coeffs.c_yes[&market_a]
        );
        assert!(
            coeffs.c_no[&market_a].abs() < 1e-9,
            "bundle c_NO_A should be 0.0"
        );
        assert!(
            (coeffs.c_yes[&market_b] - 0.5).abs() < 1e-9,
            "bundle c_YES_B should be 0.5, got {}",
            coeffs.c_yes[&market_b]
        );

        // Spread: payoffs = [0, -1, +1, 0] on two markets
        let sp = matching_engine::spread(
            &problem.markets,
            4,
            market_a,
            market_b,
            200_000_000,
            100,
        );
        let coeffs = precompute_coefficients(&sp);
        // c_YES_A = avg(payoffs where A=YES) = avg(state0=0, state2=+1) = 0.5
        // c_NO_A  = avg(payoffs where A=NO)  = avg(state1=-1, state3=0) = -0.5
        // alpha_A = 0.5 - (-0.5) = 1.0
        assert!(
            (coeffs.c_yes[&market_a] - 0.5).abs() < 1e-9,
            "spread c_YES_A should be 0.5, got {}",
            coeffs.c_yes[&market_a]
        );
        assert!(
            (coeffs.c_no[&market_a] - (-0.5)).abs() < 1e-9,
            "spread c_NO_A should be -0.5, got {}",
            coeffs.c_no[&market_a]
        );
        assert!(
            (coeffs.alpha[&market_a] - 1.0).abs() < 1e-9,
            "spread alpha_A should be 1.0, got {}",
            coeffs.alpha[&market_a]
        );
    }

    #[test]
    fn test_milp_sign() {
        let mut problem = Problem::new("sign_test");
        let market_a = problem.markets.add_binary("A");
        let market_b = problem.markets.add_binary("B");

        // YES buyer: payoffs = [+1, 0] -> no negative -> buyer
        let yes_buy = simple_yes_buy(&problem.markets, 1, market_a, 600_000_000, 100);
        assert!(milp_sign(&yes_buy) > 0.0, "YES buyer should be +1");

        // YES seller: payoffs = [-1, 0] -> has negative -> seller
        let yes_sell =
            matching_engine::outcome_sell(&problem.markets, 2, market_a, 0, 600_000_000, 100);
        assert!(milp_sign(&yes_sell) < 0.0, "YES seller should be -1");

        // Spread: payoffs = [0, -1, +1, 0] -> has negative -> seller
        let sp = matching_engine::spread(
            &problem.markets,
            3,
            market_a,
            market_b,
            200_000_000,
            100,
        );
        assert!(milp_sign(&sp) < 0.0, "spread has negative payoff -> seller");

        // Bundle YES: payoffs = [+1, 0, 0, 0] -> no negative -> buyer
        let bundle = matching_engine::bundle_yes(
            &problem.markets,
            4,
            &[market_a, market_b],
            400_000_000,
            50,
        );
        assert!(milp_sign(&bundle) > 0.0, "bundle YES should be buyer");
    }

    #[test]
    fn test_milp_bundle_matching() {
        // Two-market bundle buyer + individual sellers -> positive welfare
        let mut problem = Problem::new("bundle_test");
        let market_a = problem.markets.add_binary("A");
        let market_b = problem.markets.add_binary("B");

        // Bundle buyer: wants all YES at 40c (limit=400M nanos)
        problem.orders.push(matching_engine::bundle_yes(
            &problem.markets,
            1,
            &[market_a, market_b],
            400_000_000,
            100,
        ));

        // Individual YES sellers on each market
        // Seller A: sell YES A at 15c
        problem.orders.push(matching_engine::outcome_sell(
            &problem.markets,
            10,
            market_a,
            0,
            150_000_000,
            200,
        ));
        // Seller B: sell YES B at 15c
        problem.orders.push(matching_engine::outcome_sell(
            &problem.markets,
            11,
            market_b,
            0,
            150_000_000,
            200,
        ));

        let solver = MilpSolver::new();
        let result = solver.solve_with_status(&problem);

        assert!(
            matches!(result.status, SolveStatus::Optimal),
            "should find optimal solution, got {:?}",
            result.status
        );
        assert!(
            result.result.orders_filled > 0,
            "should fill some orders"
        );
        assert!(
            result.result.total_welfare > 0,
            "should produce positive welfare, got {}",
            result.result.total_welfare
        );
    }

    #[test]
    fn test_milp_spread_matching() {
        // Spread buyer + counterparties -> fills
        let mut problem = Problem::new("spread_test");
        let market_a = problem.markets.add_binary("A");
        let market_b = problem.markets.add_binary("B");

        // Spread: long A YES, short B YES, at 10c
        problem.orders.push(matching_engine::spread(
            &problem.markets,
            1,
            market_a,
            market_b,
            100_000_000,
            100,
        ));

        // Counterparty: sell A YES at 40c
        problem.orders.push(matching_engine::outcome_sell(
            &problem.markets,
            10,
            market_a,
            0,
            400_000_000,
            200,
        ));

        // Counterparty: buy B YES at 60c
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            11,
            market_b,
            600_000_000,
            200,
        ));

        let solver = MilpSolver::new();
        let result = solver.solve_with_status(&problem);

        assert!(
            matches!(result.status, SolveStatus::Optimal),
            "should find optimal solution, got {:?}",
            result.status
        );
        // The spread + counterparties should produce some welfare
        assert!(
            result.result.total_welfare >= 0,
            "should produce non-negative welfare, got {}",
            result.result.total_welfare
        );
    }

    #[test]
    fn test_milp_group_minting() {
        // Test that group-level minting enables negrisk-style arbitrage.
        //
        // Setup: 3 mutually exclusive markets (group), YES buyers in each.
        // Without group minting: can't fill because per-market minting creates
        // equal YES+NO, but no NO demand → mint_m forced to 0.
        // With group minting: can fill all by minting 1 YES per market at $1 total.
        use matching_engine::MarketGroup;

        let mut problem = Problem::new("group_mint_test");
        let m0 = problem.markets.add_binary("Candidate A");
        let m1 = problem.markets.add_binary("Candidate B");
        let m2 = problem.markets.add_binary("Candidate C");

        let mut group = MarketGroup::new("Election");
        group.add_market(m0);
        group.add_market(m1);
        group.add_market(m2);
        problem.add_market_group(group);

        // YES buyers at prices that sum to > $1 (profitable negrisk)
        // A at 40c, B at 35c, C at 30c → sum = $1.05
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            m0,
            400_000_000,
            100,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            2,
            m1,
            350_000_000,
            100,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            3,
            m2,
            300_000_000,
            100,
        ));

        let solver = MilpSolver::new();
        let result = solver.solve_with_status(&problem);

        assert!(
            matches!(result.status, SolveStatus::Optimal),
            "should find optimal, got {:?}",
            result.status
        );
        assert!(
            result.result.orders_filled >= 3,
            "should fill all 3 YES buyers via group minting, filled {}",
            result.result.orders_filled
        );
        // Welfare = sum(limit * qty) - group_mint_cost
        // = (0.40 + 0.35 + 0.30) * 100 - 1.00 * 100 = $5 = 5_000_000_000 nanos
        assert!(
            result.result.total_welfare > 0,
            "group minting should produce positive welfare, got {}",
            result.result.total_welfare
        );
    }

    #[test]
    fn test_milp_upper_bound() {
        // Verify MILP welfare >= pipeline welfare on a realistic scenario
        use matching_scenarios::{generate_scenario, ScenarioConfig};

        let mut config = ScenarioConfig::quick();
        config.seed = 42;
        let problem = generate_scenario(config);

        let solver = MilpSolver::new();
        let milp_result = solver.solve_with_status(&problem);

        // Run pipeline for comparison
        let pipeline = crate::Pipeline::current();
        let pipeline_result = pipeline.solve(&problem);

        let milp_welfare = milp_result.result.total_welfare;
        let pipeline_welfare = pipeline_result.result.total_welfare;

        assert!(
            milp_welfare >= pipeline_welfare,
            "MILP welfare ({}) should be >= pipeline welfare ({})",
            milp_welfare,
            pipeline_welfare
        );
    }
}
