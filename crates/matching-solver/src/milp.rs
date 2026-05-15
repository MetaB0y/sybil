//! MIQCQP solver for optimal matching with Uniform Clearing Prices (UCP).
//!
//! Formulates the matching problem as a Mixed-Integer Quadratically Constrained
//! Quadratic Program using SCIP for single-market binary orders.
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
//! - UCP (Big-M): effective price via payoff-derived alpha/beta
//! - Position balance: per-market YES/NO payoffs
//! - MM budget (quadratic): exact bilinear `price × quantity` via SCIP MIQCQP
//! - Market groups: `Σ p_m ≤ NANOS_PER_DOLLAR` per group

use matching_engine::{Fill, MarketId, Nanos, Order, Problem, NANOS_PER_DOLLAR};

use crate::MatchingResult;

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
    /// True SCIP objective value (welfare with minting costs deducted).
    /// This equals real-fill welfare minus minting costs.
    pub objective_welfare: i64,
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
            "Dual Analysis:\n  Binding liquidity: {} ({:.1}% of constraints)\n  Top scarce markets: {:?}",
            self.binding_liquidity_constraints,
            binding_pct,
            scarce.iter().map(|((m, o), d)| format!("M{}O{}:{:.2}", m.0, o, d)).collect::<Vec<_>>()
        )
    }
}

use crate::lp_solver::order_sign;

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

    fn compute_dual_analysis(&self, _problem: &Problem, result: &MilpResult) -> DualAnalysis {
        DualAnalysis {
            objective_value: result.result.total_welfare as f64,
            ..Default::default()
        }
    }

    /// Solve with full status reporting.
    pub fn solve_with_status(&self, problem: &Problem) -> MilpResult {
        let start = std::time::Instant::now();
        let mut result = MatchingResult::new();

        let active_orders: Vec<_> = problem.orders.iter().collect();

        if active_orders.is_empty() {
            return MilpResult {
                result,
                status: SolveStatus::Optimal,
                solve_time_secs: start.elapsed().as_secs_f64(),
                clearing_prices: HashMap::new(),
                objective_welfare: 0,
            };
        }

        let solve_result = self.solve_with_scip(&active_orders, problem);

        match solve_result {
            Ok((solution, status, solve_time, scip_objective)) => {
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

                        // Fill price from clearing price for single-market binary orders
                        let market = order.markets[0];
                        let p_yes = solution
                            .p_values
                            .get(&market)
                            .copied()
                            .unwrap_or(0.0)
                            .round()
                            .max(0.0) as Nanos;
                        let fill_price = if order.payoffs[0] != 0 {
                            p_yes
                        } else {
                            NANOS_PER_DOLLAR.saturating_sub(p_yes)
                        };

                        let fill = Fill::new(order.id, fill_qty, fill_price);
                        result.add_fill(fill, order);
                    } else {
                        result.orders_unfilled_liquidity += 1;
                    }
                }

                let objective_welfare = scip_objective.round() as i64;

                // The MILP objective correctly deducts minting cost
                // (per-market: $1/pair, group: $1/set). The fill-level welfare
                // records only real orders, while minting is represented by
                // solver variables and by the sequencer/verifier MINT account.
                let fill_welfare = result.total_welfare;
                result.minting_cost = fill_welfare - objective_welfare;
                result.total_welfare = objective_welfare;

                MilpResult {
                    result,
                    status,
                    solve_time_secs: solve_time,
                    clearing_prices,
                    objective_welfare,
                }
            }
            Err(err_msg) => {
                for _order in &active_orders {
                    result.orders_unfilled_liquidity += 1;
                }

                MilpResult {
                    result,
                    status: SolveStatus::Error(err_msg),
                    solve_time_secs: start.elapsed().as_secs_f64(),
                    clearing_prices: HashMap::new(),
                    objective_welfare: 0,
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
    ///
    /// Returns (solution, status, solve_time, objective_value).
    fn solve_with_scip(
        &self,
        active_orders: &[&Order],
        problem: &Problem,
    ) -> Result<(ScipSolution, SolveStatus, f64, f64), String> {
        let start = std::time::Instant::now();
        let n = active_orders.len();
        let nanos_f = NANOS_PER_DOLLAR as f64;

        debug_assert!(
            active_orders.iter().all(|o| o.num_markets == 1),
            "MILP solver only supports single-market binary orders"
        );

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

        let num_markets = markets.len();
        let num_states = 1usize << num_markets;
        debug_assert!(
            active_orders
                .iter()
                .all(|o| o.num_states as usize <= num_states),
            "MILP assumes binary markets: expected max {} states, found order with {} states",
            num_states,
            active_orders
                .iter()
                .map(|o| o.num_states)
                .max()
                .unwrap_or(0)
        );

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
        let z_vars: Vec<Variable> = (0..n).map(|_| model.add(var().bin().obj(0.0))).collect();

        // q_i (continuous): fill quantity for order i
        // Objective: sign_i * L_i (welfare contribution)
        let q_vars: Vec<Variable> = (0..n)
            .map(|i| {
                let order = active_orders[i];
                let sign = order_sign(order);
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
        // Positive group_mint = arbitrage (buy YES in all markets)
        // Negative group_mint = reverse arbitrage (sell YES in all markets)
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
            // q_i <= max_fill * z_i  =>  q_i - max_fill * z_i <= 0
            model.add(
                cons()
                    .coef(&q_vars[i], 1.0)
                    .coef(&z_vars[i], -(order.max_fill as f64))
                    .le(0.0),
            );
        }

        // ================================================================
        // UCP constraints (Big-M) for single-market binary orders
        // ================================================================
        //
        // For single-market order on market m:
        //   alpha = payoffs[0] - payoffs[1]  (YES coefficient - NO coefficient)
        //   beta  = payoffs[1] * NANOS       (NO coefficient * price scale)
        //   eff_price = alpha * p_m + beta
        //
        // Big-M relaxation: alpha*p + M*z <= sign*L + M - beta

        let big_m = nanos_f;

        for (i, order) in active_orders.iter().enumerate() {
            let sign = order_sign(order);
            let limit = order.limit_price as f64;
            let alpha = (order.payoffs[0] - order.payoffs[1]) as f64;
            let beta = order.payoffs[1] as f64 * nanos_f;

            let market = order.markets[0];
            let mut c = cons().coef(&z_vars[i], big_m);
            if let Some(p_var) = p_vars.get(&market) {
                if alpha.abs() > 1e-12 {
                    c = c.coef(p_var, alpha);
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
        // share per unit to market m. This models group-level arbitrage:
        // minting 1 YES in every market of a mutually exclusive group costs
        // only $1 total (vs $N for N separate per-market mints).

        for &market in &markets {
            let mut yes_cons = cons();
            let mut no_cons = cons();

            for (i, order) in active_orders.iter().enumerate() {
                // Direct payoff lookup for single-market binary orders
                if order.markets[0] == market {
                    let c_y = order.payoffs[0] as f64;
                    if c_y.abs() > 1e-12 {
                        yes_cons = yes_cons.coef(&q_vars[i], c_y);
                    }

                    let c_n = order.payoffs[1] as f64;
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
                            let w = model.add(var().cont(0.0..=(p_ub * q_ub)).obj(0.0));
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
                            model.add(cons().coef(w_ref, 1.0).coef(&q_vars[idx], -p_ub).le(0.0));
                            // Upper: w ≤ Q*p
                            model.add(cons().coef(w_ref, 1.0).coef(p_var, -q_ub).le(0.0));

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
                                    budget_cons = budget_cons.coef(&q_vars[q_idx], nanos_f);
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
                // Σp = $1: mutually exclusive markets must have prices summing
                // to exactly $1. This ensures group minting is always zero-cost
                // (sound) — no protocol subsidy needed.
                model.add(c.eq(nanos_f));
            }
        }

        // ================================================================
        // Solve
        // ================================================================

        let solved = model.solve();
        let solve_time = start.elapsed().as_secs_f64();

        let scip_status = solved.status();

        match scip_status {
            Status::Optimal
            | Status::GapLimit
            | Status::SolutionLimit
            | Status::BestSolutionLimit => {
                let sol = solved
                    .best_sol()
                    .ok_or_else(|| "No solution found despite optimal status".to_string())?;

                let solution = ScipSolution {
                    z_values: z_vars.iter().map(|v| sol.val(v)).collect(),
                    q_values: q_vars.iter().map(|v| sol.val(v)).collect(),
                    p_values: p_vars.iter().map(|(&m, v)| (m, sol.val(v))).collect(),
                };

                let obj = solved.obj_val();
                let status = if scip_status == Status::Optimal {
                    SolveStatus::Optimal
                } else {
                    let bound = solved.best_bound();
                    let gap_percent = if obj.abs() > 1e-10 {
                        ((bound - obj) / obj.abs() * 100.0).abs()
                    } else {
                        0.0
                    };
                    SolveStatus::TimeLimitReached { gap_percent }
                };

                Ok((solution, status, solve_time, obj))
            }
            Status::TimeLimit
            | Status::NodeLimit
            | Status::TotalNodeLimit
            | Status::StallNodeLimit
            | Status::MemoryLimit
            | Status::RestartLimit => {
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
                        obj,
                    ))
                } else {
                    Ok((
                        ScipSolution {
                            z_values: vec![0.0; n],
                            q_values: vec![0.0; n],
                            p_values: HashMap::new(),
                        },
                        SolveStatus::TimeLimitReached { gap_percent: 100.0 },
                        solve_time,
                        0.0,
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
                0.0,
            )),
            Status::Unbounded => Err("Problem is unbounded".to_string()),
            _ => Err(format!(
                "Solver returned unexpected status: {:?}",
                scip_status
            )),
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

impl crate::Solver for MilpSolver {
    /// Wraps `solve_with_status()` into the common `PipelineResult`.
    /// Discards MILP-specific fields (status, gap%, objective_welfare).
    /// Use `solve_with_status()` on the concrete type when you need those.
    fn solve(&self, problem: &Problem) -> crate::PipelineResult {
        let milp_result = self.solve_with_status(problem);

        let mut pr = crate::PipelineResult::empty();
        pr.result = milp_result.result;
        pr.price_discovery = Some(crate::PriceDiscoveryResult {
            prices: milp_result.clearing_prices,
            total_fills: pr.result.fills.len(),
            total_welfare: pr.result.total_welfare,
        });
        pr.total_time_secs = milp_result.solve_time_secs;

        if pr.result.total_welfare < 0 {
            pr.result = crate::MatchingResult::new();
        }

        pr
    }

    fn name(&self) -> &str {
        "MILP"
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

        let result = solver.solve_with_status(&problem).result;
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
    fn test_order_sign() {
        let mut problem = Problem::new("sign_test");
        let market_a = problem.markets.add_binary("A");

        // YES buyer: payoffs = [+1, 0] -> no negative -> buyer
        let yes_buy = simple_yes_buy(&problem.markets, 1, market_a, 600_000_000, 100);
        assert!(order_sign(&yes_buy) > 0.0, "YES buyer should be +1");

        // YES seller: payoffs = [-1, 0] -> has negative -> seller
        let yes_sell =
            matching_engine::outcome_sell(&problem.markets, 2, market_a, 0, 600_000_000, 100);
        assert!(order_sign(&yes_sell) < 0.0, "YES seller should be -1");
    }

    #[test]
    fn test_milp_group_minting() {
        // Test that group-level minting enables cross-market arbitrage.
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

        // YES buyers at prices that sum to > $1 (profitable arbitrage)
        // A at 40c, B at 35c, C at 30c → sum = $1.05
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, m0, 400_000_000, 100));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 2, m1, 350_000_000, 100));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 3, m2, 300_000_000, 100));

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
}
