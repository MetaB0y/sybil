//! LP-based solver for prediction market matching.
//!
//! Formulates the welfare-maximizing matching problem as a Linear Program:
//! - Variables: fill quantities, per-market minting, group minting
//! - Constraints: YES/NO minting epigraph per market, quantity bounds
//! - Objective: maximize total welfare (limit_price × quantity for buyers, minus for sellers)
//!   minus minting cost ($1 per mint)
//!
//! Prices emerge from LP duality: the dual of the YES epigraph constraint for market m
//! gives p_YES_m, and the dual of the NO constraint gives p_NO_m. When minting is active,
//! p_YES + p_NO = $1 automatically. When group minting is active, Σ p_YES = $1.
//!
//! MM budget constraints (bilinear: price × quantity) are handled iteratively by
//! re-solving the LP with tightened order limits until budgets are satisfied.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use highs::{Col, HighsModelStatus, Model, RowProblem, Sense};

use matching_engine::{
    Fill, MarketId, MmSide, NANOS_PER_DOLLAR, Nanos, Order, Problem, Qty, minting_cost_from_fills,
};

use crate::MatchingResult;
use crate::result::{
    PipelineResult, PipelineTimings, PriceDiscoveryResult, SolverDiagnostics, TerminationStatus,
};
use crate::solver::order_sign;

/// Configuration for the LP solver.
#[derive(Clone, Debug)]
pub struct LpConfig {
    /// Max SLP iterations for MM budget linearization (0 = LP only, no MM handling).
    pub max_mm_iterations: usize,
}

impl Default for LpConfig {
    fn default() -> Self {
        Self {
            max_mm_iterations: 1,
        }
    }
}

/// LP-based solver that handles the convex core exactly via HiGHS,
/// then uses SLP (sequential LP) for MM budget constraints.
pub struct LpSolver {
    config: LpConfig,
}

impl LpSolver {
    pub fn new() -> Self {
        Self {
            config: LpConfig::default(),
        }
    }

    #[cfg(feature = "lp")]
    pub fn with_config(config: LpConfig) -> Self {
        Self { config }
    }

    /// Solve a matching problem using LP + SLP for MM budgets.
    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();

        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        let supported = crate::solver::filter_supported_problem(problem, "LP");
        let rejected_orders = supported.rejected_orders;
        let problem = supported.problem.as_ref();
        if problem.orders.is_empty() {
            return PipelineResult::failure(
                "lp",
                TerminationStatus::UnsupportedInput,
                format!("rejected {rejected_orders} unsupported orders"),
                start.elapsed().as_secs_f64(),
            );
        }

        let ctx = build_solver_context(problem);
        let mut oracle_orders = problem.orders.clone();
        disable_zero_budget_mm_orders(problem, &mut oracle_orders);

        // Pre-group MM orders by constraint for efficient iteration
        let mm_constraint_orders = mm_constraint_order_indices(problem, &ctx);

        // Sequential LP: solve without budgets, then add linearized budget
        // constraints and re-solve until budgets are satisfied.
        let mut budget_rows: Vec<(Vec<(usize, f64)>, f64)> = Vec::new();
        let mut best_solution: Option<LpSolution> = None;
        let mut lp_solves = 0usize;
        let mut budget_converged = problem.mm_constraints.is_empty();

        for slp_iter in 0..=self.config.max_mm_iterations {
            lp_solves += 1;
            let solution = self.solve_lp(
                &oracle_orders,
                &ctx.markets,
                &ctx.market_to_group,
                ctx.num_groups,
                &budget_rows,
            );

            let Some(sol) = solution else {
                break;
            };
            if problem.mm_constraints.is_empty() {
                best_solution = Some(sol);
                break;
            }

            // Check MM budget violations at current prices
            let prices = normalized_yes_prices(&sol, &ctx.markets);
            let violated = has_mm_budget_violations(
                &sol,
                &oracle_orders,
                &problem.mm_constraints,
                &mm_constraint_orders,
                &prices,
            );

            if !violated {
                budget_converged = true;
                best_solution = Some(sol);
                break;
            }

            // Keep the final capped iterate. Integer post-processing still
            // trims it to a verifier-valid budget, but the diagnostic must not
            // call the SLP fixed point converged.
            if slp_iter == self.config.max_mm_iterations {
                best_solution = Some(sol);
                break;
            }

            // Linearize budget constraints at current prices and re-solve.
            // For each MM: Σ capital_per_unit_i(p) × q_i ≤ Budget_k
            budget_rows = linearize_mm_budgets(
                &oracle_orders,
                &problem.mm_constraints,
                &mm_constraint_orders,
                &prices,
            );

            best_solution = Some(sol);
        }

        let Some(solution) = best_solution else {
            return PipelineResult::failure(
                "lp",
                TerminationStatus::NumericalFailure,
                "HiGHS did not return an LP solution",
                start.elapsed().as_secs_f64(),
            );
        };

        let mut result = finalize_result(&solution, problem, &ctx, start);
        result.diagnostics = SolverDiagnostics {
            algorithm: "lp".to_string(),
            status: if budget_converged {
                TerminationStatus::Converged
            } else {
                TerminationStatus::IterationLimit
            },
            iterations: Some(lp_solves),
            message: (!budget_converged).then(|| {
                "MM-budget SLP reached its configured cap; integer trimming was applied".to_string()
            }),
            ..Default::default()
        };
        result
    }

    /// Build and solve the core LP using HiGHS.
    ///
    /// Returns the raw LP solution (primal + dual values) or None if infeasible.
    /// `budget_rows` contains linearized MM budget constraints: each entry is
    /// (terms: [(order_index, capital_per_unit)], budget_nanos_f64).
    fn solve_lp(
        &self,
        orders: &[Order],
        markets: &[MarketId],
        market_to_group: &HashMap<MarketId, usize>,
        num_groups: usize,
        budget_rows: &[(Vec<(usize, f64)>, f64)],
    ) -> Option<LpSolution> {
        // Default welfare objective: sign_i * limit_price_i
        let objective_coeffs = welfare_weights(orders);
        build_and_solve_lp(
            orders,
            markets,
            market_to_group,
            num_groups,
            &objective_coeffs,
            budget_rows,
        )
    }
}

fn disable_zero_budget_mm_orders(problem: &Problem, orders: &mut [Order]) {
    let disabled: HashSet<u64> = problem
        .mm_constraints
        .iter()
        .filter(|mm| mm.max_capital == Nanos(0))
        .flat_map(|mm| mm.order_ids.iter().copied())
        .collect();
    for order in orders {
        if disabled.contains(&order.id) {
            order.max_fill = Qty::ZERO;
        }
    }
}

impl Default for LpSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::Solver for LpSolver {
    /// Forwards to the inherent `LpSolver::solve` method.
    /// Explicit path needed to disambiguate from this trait method.
    fn solve(&self, problem: &Problem) -> PipelineResult {
        LpSolver::solve(self, problem)
    }
    fn name(&self) -> &str {
        "LP"
    }
}

/// Raw solution from the LP solver: primal fill quantities + dual prices.
pub(crate) struct LpSolution {
    pub(crate) q_values: Vec<f64>,
    pub(crate) dual_yes: HashMap<MarketId, f64>,
    pub(crate) dual_no: HashMap<MarketId, f64>,
    /// A Lagrangian upper bound for zero-RHS matching LPs, in HiGHS objective
    /// units. Unlike the returned primal objective, this remains a valid
    /// oracle bound when HiGHS stops within floating-point tolerances.
    pub(crate) objective_upper_bound_dollars: Option<f64>,
    pub(crate) objective_value_dollars: f64,
}

/// A fixed matching LP whose objective can be changed between solves.
///
/// Retained-cash clearing calls the same linear oracle repeatedly with new
/// pacing coefficients. Keeping the HiGHS model alive avoids rebuilding the
/// sparse matrix and, after the first solve, lets HiGHS re-optimize from the
/// previous basis.
pub(crate) struct ReusableLpOracle {
    model: Option<Model>,
    q_cols: Vec<Col>,
    mint_cols: Vec<Col>,
    gmint_cols: Vec<Col>,
    markets: Vec<MarketId>,
    yes_row_indices: HashMap<MarketId, usize>,
    no_row_indices: HashMap<MarketId, usize>,
    column_bounds: Vec<(f64, f64)>,
    certifiable_zero_rhs: bool,
}

impl ReusableLpOracle {
    pub(crate) fn new(
        orders: &[Order],
        markets: &[MarketId],
        market_to_group: &HashMap<MarketId, usize>,
        num_groups: usize,
        budget_rows: &[(Vec<(usize, f64)>, f64)],
    ) -> Option<Self> {
        let n = orders.len();
        let mut pb = RowProblem::default();

        // The objective is installed immediately before each solve.
        let q_cols: Vec<_> = (0..n)
            .map(|i| pb.add_column(0.0, 0.0..=orders[i].max_fill.0 as f64))
            .collect();

        // Every balance variable is a signed sum of order fills, so total
        // available quantity is a finite analytical bound. Finite bounds also
        // let any returned row-dual vector produce a conservative Lagrangian
        // upper bound, even when its reduced costs have numerical residuals.
        let flow_bound = orders
            .iter()
            .map(|order| order.max_fill.0 as f64)
            .sum::<f64>()
            .max(1.0);
        let mint_cols_by_market: HashMap<MarketId, _> = markets
            .iter()
            .map(|&market| (market, pb.add_column(-1.0, -flow_bound..=flow_bound)))
            .collect();
        let mint_cols: Vec<_> = markets
            .iter()
            .filter_map(|market| mint_cols_by_market.get(market).copied())
            .collect();
        let gmint_cols: Vec<_> = (0..num_groups)
            .map(|_| pb.add_column(-1.0, 0.0..=flow_bound))
            .collect();
        let mut column_bounds: Vec<_> = orders
            .iter()
            .map(|order| (0.0, order.max_fill.0 as f64))
            .collect();
        column_bounds.extend(markets.iter().map(|_| (-flow_bound, flow_bound)));
        column_bounds.extend((0..num_groups).map(|_| (0.0, flow_bound)));

        let mut yes_row_indices = HashMap::new();
        let mut no_row_indices = HashMap::new();
        let mut row_count = 0usize;

        // Index orders once. The former market-by-order scan made model setup
        // O(markets * orders), which was especially visible before reuse.
        let mut orders_by_market: HashMap<MarketId, Vec<usize>> = HashMap::new();
        for (index, order) in orders.iter().enumerate() {
            orders_by_market
                .entry(order.markets[0])
                .or_default()
                .push(index);
        }

        for &market in markets {
            let market_orders = orders_by_market
                .get(&market)
                .map(Vec::as_slice)
                .unwrap_or(&[]);

            let mut yes_terms = Vec::with_capacity(market_orders.len() + 2);
            let mut no_terms = Vec::with_capacity(market_orders.len() + 1);
            for &i in market_orders {
                let c_yes = orders[i].payoffs[0] as f64;
                if c_yes.abs() > 1e-12 {
                    yes_terms.push((q_cols[i], c_yes));
                }
                let c_no = orders[i].payoffs[1] as f64;
                if c_no.abs() > 1e-12 {
                    no_terms.push((q_cols[i], c_no));
                }
            }
            let &mint_col = mint_cols_by_market.get(&market)?;
            yes_terms.push((mint_col, -1.0));
            if let Some(&group) = market_to_group.get(&market) {
                yes_terms.push((gmint_cols[group], -1.0));
            }
            no_terms.push((mint_col, -1.0));

            // Zero-temperature minting is an epigraph: net demand for every
            // outcome is bounded above by the amount minted. Equality would
            // incorrectly require balanced demand before the minting sector
            // acts and is stricter than the paper's `max_omega D_omega` cost.
            pb.add_row(..=0.0, &yes_terms);
            yes_row_indices.insert(market, row_count);
            row_count += 1;
            pb.add_row(..=0.0, &no_terms);
            no_row_indices.insert(market, row_count);
            row_count += 1;
        }

        for (terms, budget) in budget_rows {
            let row_terms: Vec<_> = terms
                .iter()
                .map(|&(order_index, coefficient)| (q_cols[order_index], coefficient))
                .collect();
            pb.add_row(..=*budget, &row_terms);
        }

        let mut model = pb.try_optimise(Sense::Maximise).ok()?;
        model.make_quiet();
        // Solver results feed consensus-adjacent integer landing and retained
        // benchmark artifacts. Pin HiGHS' execution and tie-breaking so the
        // same ordered model does not choose different degenerate bases across
        // processes or machines with different core counts.
        model.set_option("parallel", "off");
        model.set_option("threads", 1);
        model.set_option("random_seed", 0);
        Some(Self {
            model: Some(model),
            q_cols,
            mint_cols,
            gmint_cols,
            markets: markets.to_vec(),
            yes_row_indices,
            no_row_indices,
            column_bounds,
            certifiable_zero_rhs: budget_rows.is_empty(),
        })
    }

    pub(crate) fn solve(&mut self, objective_coeffs: &[f64]) -> Option<LpSolution> {
        if objective_coeffs.len() != self.q_cols.len() {
            return None;
        }

        let nanos_f = NANOS_PER_DOLLAR as f64;
        let mut model = self.model.take()?;
        for (&column, &coefficient) in self.q_cols.iter().zip(objective_coeffs) {
            model.change_column_cost(column, coefficient / nanos_f);
        }

        let solved = model.solve();
        let status = solved.status();
        let solution = solved.get_solution();
        let objective_value_dollars = solved.objective_value();
        let primal = solution.columns();
        let dual_rows = solution.dual_rows();
        let objective_upper_bound_dollars = self.certifiable_zero_rhs.then(|| {
            solution
                .dual_columns()
                .iter()
                .zip(&self.column_bounds)
                .map(|(&reduced_cost, &(lower, upper))| {
                    if reduced_cost >= 0.0 {
                        reduced_cost * upper
                    } else {
                        reduced_cost * lower
                    }
                })
                .sum()
        });

        let q_values = primal[..self.q_cols.len()].to_vec();
        let mut dual_yes = HashMap::new();
        let mut dual_no = HashMap::new();
        for &market in &self.markets {
            if let Some(&row) = self.yes_row_indices.get(&market) {
                dual_yes.insert(market, dual_rows[row] * nanos_f);
            }
            if let Some(&row) = self.no_row_indices.get(&market) {
                dual_no.insert(market, dual_rows[row] * nanos_f);
            }
        }

        // Converting the solved model back preserves HiGHS' current basis for
        // the next objective update.
        self.model = Some(Model::from(solved));

        match status {
            HighsModelStatus::Optimal | HighsModelStatus::ObjectiveBound => Some(LpSolution {
                q_values,
                dual_yes,
                dual_no,
                objective_upper_bound_dollars,
                objective_value_dollars,
            }),
            _ => None,
        }
    }

    /// Select the point on the current objective's optimal face that is
    /// closest in L1 distance to `target`.
    ///
    /// The market prices must still come from the preceding primary solve.
    /// The extra face and distance rows are only a primal selector; using
    /// their duals as market prices would repeat the utility-band bug where
    /// omitted auxiliary shadow prices broke order-limit support.
    pub(crate) fn solve_nearest_on_current_face(
        &mut self,
        target: &[f64],
        primary_objective: &[f64],
        primary_optimum: f64,
        face_tolerance: f64,
    ) -> Option<Vec<f64>> {
        if target.len() != self.q_cols.len() || primary_objective.len() != self.q_cols.len() {
            return None;
        }

        let mut model = self.model.take()?;
        let nanos_f = NANOS_PER_DOLLAR as f64;
        let mut face_terms: Vec<_> = self
            .q_cols
            .iter()
            .zip(primary_objective)
            .map(|(&column, &coefficient)| (column, coefficient / nanos_f))
            .collect();
        face_terms.extend(self.mint_cols.iter().map(|&column| (column, -1.0)));
        face_terms.extend(self.gmint_cols.iter().map(|&column| (column, -1.0)));
        model.add_row((primary_optimum - face_tolerance).., face_terms);

        for &column in self
            .q_cols
            .iter()
            .chain(&self.mint_cols)
            .chain(&self.gmint_cols)
        {
            model.change_column_cost(column, 0.0);
        }
        for (&quantity_col, &quantity) in self.q_cols.iter().zip(target) {
            let distance_col = model.add_col(-1.0, 0.0.., []);
            model.add_row(..=quantity, [(quantity_col, 1.0), (distance_col, -1.0)]);
            model.add_row(..=-quantity, [(quantity_col, -1.0), (distance_col, -1.0)]);
        }

        let solved = model.solve();
        if !matches!(
            solved.status(),
            HighsModelStatus::Optimal | HighsModelStatus::ObjectiveBound
        ) {
            return None;
        }
        let solution = solved.get_solution();
        let face_activity = self
            .q_cols
            .iter()
            .zip(primary_objective)
            .map(|(&column, &coefficient)| solution[column] * coefficient / nanos_f)
            .sum::<f64>()
            - self
                .mint_cols
                .iter()
                .chain(&self.gmint_cols)
                .map(|&column| solution[column])
                .sum::<f64>();
        let validation_tolerance = 1e-7_f64.max(primary_optimum.abs() * 1e-10);
        if face_tolerance == 0.0 && face_activity < primary_optimum - validation_tolerance {
            return None;
        }
        Some(self.q_cols.iter().map(|&column| solution[column]).collect())
    }
}

/// Build and solve an LP with custom objective coefficients.
///
/// This is the LP oracle used by both the LP solver (linear welfare) and the
/// retained-cash solver (Frank--Wolfe gradient). The constraints (minting epigraph,
/// quantity bounds, minting) are the same; only the objective varies.
///
/// All orders must be single-market binary orders.
///
/// `objective_coeffs[i]` is the objective coefficient for order i's fill variable.
/// `budget_rows` contains linearized MM budget constraints (empty for the
/// retained-cash oracle).
pub(crate) fn build_and_solve_lp(
    orders: &[Order],
    markets: &[MarketId],
    market_to_group: &HashMap<MarketId, usize>,
    num_groups: usize,
    objective_coeffs: &[f64],
    budget_rows: &[(Vec<(usize, f64)>, f64)],
) -> Option<LpSolution> {
    let mut oracle =
        ReusableLpOracle::new(orders, markets, market_to_group, num_groups, budget_rows)?;
    oracle.solve(objective_coeffs)
}

fn solve_primary_and_nearest_face(
    orders: &[Order],
    markets: &[MarketId],
    market_to_group: &HashMap<MarketId, usize>,
    num_groups: usize,
    objective_coeffs: &[f64],
    budget_rows: &[(Vec<(usize, f64)>, f64)],
    target: &[f64],
) -> Option<(LpSolution, Vec<f64>)> {
    let mut oracle =
        ReusableLpOracle::new(orders, markets, market_to_group, num_groups, budget_rows)?;
    let primary = oracle.solve(objective_coeffs)?;
    const MAX_WELL_SCALED_EXACT_FACE_QTY: u64 = 100_000_000;
    let exact_face_is_well_scaled = orders
        .iter()
        .all(|order| order.max_fill.0 <= MAX_WELL_SCALED_EXACT_FACE_QTY);

    if exact_face_is_well_scaled {
        if let Some(nearest) = oracle.solve_nearest_on_current_face(
            target,
            objective_coeffs,
            primary.objective_value_dollars,
            0.0,
        ) {
            return Some((primary, nearest));
        }
    } else {
        // Billion-unit order bounds make an exact auxiliary face row poorly
        // scaled relative to HiGHS' primal tolerances. On those deliberate
        // wide-range books the backend can nondeterministically accept a
        // distant point even when the row-activity check passes. Use the same
        // narrow, explicit near-face band without first consuming the warm
        // primary model.
        let face_tolerance = 1e-7_f64.max(primary.objective_value_dollars.abs() * 1e-8);
        let nearest = oracle.solve_nearest_on_current_face(
            target,
            objective_coeffs,
            primary.objective_value_dollars,
            face_tolerance,
        )?;
        return Some((primary, nearest));
    }

    // HiGHS' reported optimum can occasionally be a few ulps outside the
    // feasible face once thousands of L1-distance rows are added. Rebuild the
    // primary model and use a narrow near-face band only when the exact
    // lexicographic solve is numerically infeasible. Making the relaxed band
    // the default can move substantial quantity along a nearly flat objective
    // and leave the published primary prices unable to support the landing.
    let mut relaxed_oracle =
        ReusableLpOracle::new(orders, markets, market_to_group, num_groups, budget_rows)?;
    let relaxed_primary = relaxed_oracle.solve(objective_coeffs)?;
    let face_tolerance = 1e-7_f64.max(relaxed_primary.objective_value_dollars.abs() * 1e-8);
    let nearest = relaxed_oracle.solve_nearest_on_current_face(
        target,
        objective_coeffs,
        relaxed_primary.objective_value_dollars,
        face_tolerance,
    )?;
    Some((relaxed_primary, nearest))
}

/// Derive normalized YES clearing prices from LP dual variables.
///
/// For each market, takes |dual_yes| and |dual_no|, normalizes so they sum to $1.
/// Returns p_YES per market (in nanos). p_NO = NANOS_PER_DOLLAR - p_YES.
pub(crate) fn normalized_yes_prices(
    solution: &LpSolution,
    markets: &[MarketId],
) -> HashMap<MarketId, Nanos> {
    let nanos_f = NANOS_PER_DOLLAR as f64;
    let mut prices = HashMap::new();

    for &market in markets {
        let dual_y = solution.dual_yes.get(&market).copied().unwrap_or(0.0);
        let dual_n = solution.dual_no.get(&market).copied().unwrap_or(0.0);

        let p_yes_raw = dual_y.abs().round().clamp(0.0, nanos_f) as u64;
        let p_no_raw = dual_n.abs().round().clamp(0.0, nanos_f) as u64;

        let sum = p_yes_raw + p_no_raw;
        let p_yes = if sum > 0 {
            let scale = nanos_f / sum as f64;
            Nanos((p_yes_raw as f64 * scale).round() as u64)
        } else {
            // No price signal — use 50/50 as neutral default.
            // Only happens when no orders touch the market.
            Nanos(NANOS_PER_DOLLAR / 2)
        };

        prices.insert(market, p_yes);
    }

    prices
}

/// Collect all unique markets from active orders.
pub(crate) fn collect_markets(orders: &[Order]) -> Vec<MarketId> {
    let mut seen = HashSet::new();
    orders
        .iter()
        .flat_map(|o| &o.markets[..o.num_markets as usize])
        .filter(|m| !m.is_none() && seen.insert(**m))
        .copied()
        .collect()
}

/// Extract real order fills and clearing prices from the LP solution.
///
/// Rounds continuous q_i to integer fills and derives clearing prices from
/// duals. Minting/group-minting variables are settled later by the sequencer's
/// MINT account; they are never represented as synthetic fills.
pub(crate) fn extract_result(
    solution: &LpSolution,
    orders: &[Order],
    markets: &[MarketId],
) -> (MatchingResult, HashMap<MarketId, Vec<Nanos>>) {
    let mut result = MatchingResult::new();

    // Derive clearing prices from LP duals (YES and NO per market)
    let yes_prices = normalized_yes_prices(solution, markets);
    let clearing_prices: HashMap<MarketId, Vec<Nanos>> = yes_prices
        .iter()
        .map(|(&m, &p_yes)| {
            (
                m,
                vec![p_yes, Nanos(NANOS_PER_DOLLAR.saturating_sub(p_yes.0))],
            )
        })
        .collect();

    // Extract fills from primal solution
    for (i, order) in orders.iter().enumerate() {
        let q_val = solution.q_values[i];
        if q_val < 0.5 {
            result.orders_unfilled_liquidity += 1;
            continue;
        }

        let fill_qty = Qty(q_val.round() as u64);
        if fill_qty == Qty::ZERO {
            result.orders_unfilled_liquidity += 1;
            continue;
        }

        // For single-market binary orders, fill price is simply:
        // - YES side (payoffs[0] != 0): p_yes
        // - NO side (payoffs[1] != 0, payoffs[0] == 0): NANOS - p_yes
        let market = order.markets[0];
        let p_yes = clearing_prices
            .get(&market)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Nanos(0));
        let fill_price = if order.payoffs[0] != 0 {
            p_yes
        } else {
            Nanos(NANOS_PER_DOLLAR.saturating_sub(p_yes.0))
        };

        let fill = Fill::new(order.id, fill_qty, fill_price);
        result.add_fill(fill, order);
    }

    // Welfare is recomputed from scratch after all post-processing.
    (result, clearing_prices)
}

/// Check whether any MM budget constraint is violated at current LP solution prices.
pub(crate) fn has_mm_budget_violations(
    solution: &LpSolution,
    orders: &[Order],
    mm_constraints: &[matching_engine::MmConstraint],
    mm_constraint_orders: &[Vec<(usize, MmSide)>],
    prices: &HashMap<MarketId, Nanos>,
) -> bool {
    for (mm_idx, mm) in mm_constraints.iter().enumerate() {
        let total_capital: u128 = mm_constraint_orders[mm_idx]
            .iter()
            .map(|&(i, side)| {
                let q = Qty(solution.q_values[i].round() as u64);
                if q == Qty::ZERO {
                    return 0;
                }
                let p_yes = prices
                    .get(&orders[i].markets[0])
                    .copied()
                    .unwrap_or(Nanos(NANOS_PER_DOLLAR / 2));
                let fill_price = if orders[i].payoffs[0] != 0 {
                    p_yes
                } else {
                    Nanos(NANOS_PER_DOLLAR.saturating_sub(p_yes.0))
                };
                side.capital_needed(fill_price, q).0 as u128
            })
            .sum();

        if total_capital > mm.max_capital.0 as u128 {
            return true;
        }
    }

    false
}

/// Build linearized MM budget constraints from current clearing prices.
///
/// For each MM constraint, produces a row: Σ capital_per_unit_i × q_i ≤ Budget.
/// The capital_per_unit is computed at the given prices (fixed for this LP iteration).
/// This linearizes the bilinear p×q constraint, enabling the LP to enforce budgets directly.
pub(crate) fn linearize_mm_budgets(
    orders: &[Order],
    mm_constraints: &[matching_engine::MmConstraint],
    mm_constraint_orders: &[Vec<(usize, MmSide)>],
    prices: &HashMap<MarketId, Nanos>,
) -> Vec<(Vec<(usize, f64)>, f64)> {
    mm_constraints
        .iter()
        .enumerate()
        .map(|(mm_idx, mm)| {
            let terms: Vec<(usize, f64)> = mm_constraint_orders[mm_idx]
                .iter()
                .filter_map(|&(i, side)| {
                    let p_yes = prices
                        .get(&orders[i].markets[0])
                        .copied()
                        .unwrap_or(Nanos(NANOS_PER_DOLLAR / 2));
                    let fill_price = if orders[i].payoffs[0] != 0 {
                        p_yes
                    } else {
                        Nanos(NANOS_PER_DOLLAR.saturating_sub(p_yes.0))
                    };
                    let cpu = side.capital_needed(fill_price, Qty(1)).0 as f64;
                    (cpu > 0.0).then_some((i, cpu))
                })
                .collect();
            (terms, mm.max_capital.0 as f64)
        })
        .collect()
}

/// Trim MM fills to fix tiny budget overflows from integer rounding.
///
/// The SLP enforces budgets at linearized prices, but rounding continuous q_i
/// to integers can push capital usage slightly over budget. Trims the minimum
/// number of fill units to satisfy all budgets. Welfare is recomputed separately.
pub(crate) fn trim_mm_budget_overflows(
    result: &mut MatchingResult,
    mm_constraints: &[matching_engine::MmConstraint],
    mm_order_info: &HashMap<u64, (usize, MmSide)>,
) {
    for (mm_idx, mm) in mm_constraints.iter().enumerate() {
        let mut mm_fills: Vec<(usize, u64)> = Vec::new(); // (fill_index, capital)

        for (fi, fill) in result.fills.iter().enumerate() {
            let Some(&(oi_mm_idx, side)) = mm_order_info.get(&fill.order_id) else {
                continue;
            };
            if oi_mm_idx != mm_idx || fill.fill_qty == Qty::ZERO {
                continue;
            }
            mm_fills.push((fi, side.capital_needed(fill.fill_price, fill.fill_qty).0));
        }

        let total_capital: u128 = mm_fills.iter().map(|&(_, c)| c as u128).sum();
        if total_capital <= mm.max_capital.0 as u128 {
            continue;
        }

        // Over budget — trim smallest fills first (least disruptive)
        mm_fills.sort_by_key(|&(_, cap)| cap);

        let mut remaining = total_capital;
        for &(fi, _) in &mm_fills {
            if remaining <= mm.max_capital.0 as u128 {
                break;
            }
            let fill = &result.fills[fi];
            let Some(&(_, side)) = mm_order_info.get(&fill.order_id) else {
                continue;
            };
            let trim = trim_qty_to_fit_budget(
                side,
                fill.fill_price,
                fill.fill_qty.0,
                remaining,
                mm.max_capital.0 as u128,
            );
            if trim == 0 {
                continue;
            }

            let fill_price = fill.fill_price;
            let old_qty = result.fills[fi].fill_qty;
            let old_capital = side.capital_needed(fill_price, old_qty).0 as u128;
            result.fills[fi].fill_qty.0 -= trim;
            let new_capital = side.capital_needed(fill_price, result.fills[fi].fill_qty).0 as u128;
            remaining = remaining - old_capital + new_capital;
        }
    }

    result.fills.retain(|f| f.fill_qty.0 > 0);
}

fn trim_qty_to_fit_budget(
    side: MmSide,
    fill_price: Nanos,
    fill_qty: u64,
    remaining_capital: u128,
    budget: u128,
) -> u64 {
    if remaining_capital <= budget || fill_qty == 0 {
        return 0;
    }

    let old_capital = side.capital_needed(fill_price, Qty(fill_qty)).0 as u128;
    if old_capital == 0 {
        return 0;
    }

    if remaining_capital - old_capital > budget {
        return fill_qty;
    }

    let mut lo = 1;
    let mut hi = fill_qty;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let new_qty = fill_qty - mid;
        let new_capital = side.capital_needed(fill_price, Qty(new_qty)).0 as u128;
        let after_trim = remaining_capital - old_capital + new_capital;
        if after_trim <= budget {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }

    lo
}

pub(crate) fn trim_zero_price_minting(
    result: &mut MatchingResult,
    order_map: &HashMap<u64, &Order>,
    clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
) {
    let mut diff_by_market: HashMap<MarketId, i128> = HashMap::new();
    for fill in &result.fills {
        let Some(&order) = order_map.get(&fill.order_id) else {
            continue;
        };
        let diff_coeff = outcome_diff_coeff(order);
        if diff_coeff == 0 {
            continue;
        }
        *diff_by_market.entry(order.markets[0]).or_insert(0) +=
            diff_coeff as i128 * fill.fill_qty.0 as i128;
    }

    for (market, diff) in diff_by_market {
        let Some(trim_direction) = zero_price_mint_direction(market, diff, clearing_prices) else {
            continue;
        };

        let mut remaining = diff.unsigned_abs();
        let mut candidates: Vec<(usize, u64)> = result
            .fills
            .iter()
            .enumerate()
            .filter_map(|(fill_idx, fill)| {
                let &order = order_map.get(&fill.order_id)?;
                if order.markets[0] != market || outcome_diff_coeff(order) != trim_direction {
                    return None;
                }
                Some((fill_idx, fill.fill_qty.0))
            })
            .collect();
        candidates.sort_by_key(|&(_, qty)| qty);

        for (fill_idx, qty) in candidates {
            if remaining == 0 {
                break;
            }
            let trim = if remaining > qty as u128 {
                qty
            } else {
                remaining as u64
            };
            result.fills[fill_idx].fill_qty.0 -= trim;
            remaining -= trim as u128;
        }
    }

    result.fills.retain(|fill| fill.fill_qty.0 > 0);
}

fn zero_price_mint_direction(
    market: MarketId,
    diff: i128,
    clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
) -> Option<i8> {
    if diff == 0 {
        return None;
    }

    let prices = clearing_prices.get(&market);
    let missing_or_zero = |outcome: usize| {
        prices
            .and_then(|market_prices| market_prices.get(outcome))
            .copied()
            .unwrap_or(Nanos(0))
            == Nanos(0)
    };

    if diff > 0 && missing_or_zero(0) {
        Some(1)
    } else if diff < 0 && missing_or_zero(1) {
        Some(-1)
    } else {
        None
    }
}

fn outcome_diff_coeff(order: &Order) -> i8 {
    order.payoffs[0].saturating_sub(order.payoffs[1])
}

/// Per-order welfare weight in the objective: sign × limit price.
///
/// Buyers contribute `+limit_price`, sellers `-limit_price`. This is the
/// linear welfare coefficient shared by every LP-family objective.
pub(crate) fn welfare_weight(order: &Order) -> f64 {
    order_sign(order) * order.limit_price.0 as f64
}

/// Per-order welfare weights (`sign × limit price`) for all orders, in order.
pub(crate) fn welfare_weights(orders: &[Order]) -> Vec<f64> {
    orders.iter().map(welfare_weight).collect()
}

/// Build the MM order map `order_id → (mm_constraint_index, MmSide)`.
///
/// Shared by [`build_solver_context`] and the decomposed solver's global
/// budget-trimming pass.
pub(crate) fn build_mm_order_info(problem: &Problem) -> HashMap<u64, (usize, MmSide)> {
    problem
        .mm_constraints
        .iter()
        .enumerate()
        .flat_map(|(mm_idx, mm)| {
            mm.order_ids
                .iter()
                .filter_map(move |&oid| mm.order_sides.get(&oid).map(|&side| (oid, (mm_idx, side))))
        })
        .collect()
}

fn mm_constraint_order_indices(
    problem: &Problem,
    ctx: &SolverContext,
) -> Vec<Vec<(usize, MmSide)>> {
    let mut by_mm = vec![Vec::new(); problem.mm_constraints.len()];
    for (index, order) in problem.orders.iter().enumerate() {
        if let Some(&(mm_index, side)) = ctx.mm_order_info.get(&order.id) {
            by_mm[mm_index].push((index, side));
        }
    }
    by_mm
}

/// Common setup shared across all LP-family solvers: collect markets,
/// build market-to-group mapping, build MM order info.
pub(crate) struct SolverContext {
    pub markets: Vec<MarketId>,
    pub market_to_group: HashMap<MarketId, usize>,
    pub num_groups: usize,
    pub mm_order_info: HashMap<u64, (usize, MmSide)>,
}

impl SolverContext {
    /// Per-order MM info keyed by order *index*: `order_index → (mm_idx, side)`.
    ///
    /// Convenience view over [`Self::mm_order_info`] for solvers that iterate
    /// orders positionally (retained-cash and Conic).
    pub(crate) fn mm_order_index_map(&self, orders: &[Order]) -> HashMap<usize, (usize, MmSide)> {
        orders
            .iter()
            .enumerate()
            .filter_map(|(i, o)| self.mm_order_info.get(&o.id).map(|&info| (i, info)))
            .collect()
    }
}

/// Build the common context from a Problem.
pub(crate) fn build_solver_context(problem: &Problem) -> SolverContext {
    let markets = collect_markets(&problem.orders);
    let market_to_group: HashMap<MarketId, usize> = problem
        .market_groups
        .iter()
        .enumerate()
        .flat_map(|(g_idx, group)| group.markets.iter().map(move |&m| (m, g_idx)))
        .collect();
    SolverContext {
        markets,
        market_to_group,
        num_groups: problem.market_groups.len(),
        mm_order_info: build_mm_order_info(problem),
    }
}

/// Common post-processing shared across all LP-family solvers.
///
/// After the core solving phase (LP, Frank--Wolfe, or conic),
/// all solvers share this finalization: extract real order fills from the LP
/// solution, trim MM budget overflows, recompute welfare, and gate on
/// non-negative welfare.
pub(crate) fn finalize_result(
    solution: &LpSolution,
    problem: &Problem,
    ctx: &SolverContext,
    start: Instant,
) -> PipelineResult {
    let orders = &problem.orders;
    let order_map: HashMap<u64, &Order> = orders.iter().map(|o| (o.id, o)).collect();
    let (mut result, prices) = extract_result(solution, orders, &ctx.markets);

    trim_mm_budget_overflows(&mut result, &problem.mm_constraints, &ctx.mm_order_info);
    trim_zero_price_minting(&mut result, &order_map, &prices);
    recompute_welfare(&mut result, &order_map);

    let mut pipeline_result = PipelineResult::empty();
    pipeline_result.result = result;
    pipeline_result.price_discovery = Some(PriceDiscoveryResult {
        prices,
        total_fills: pipeline_result.result.fills.len(),
        total_welfare: pipeline_result.result.total_welfare(),
    });
    pipeline_result.total_time_secs = start.elapsed().as_secs_f64();
    pipeline_result.phase_times = PipelineTimings {
        price_discovery_secs: start.elapsed().as_secs_f64(),
        ..Default::default()
    };
    pipeline_result.diagnostics = SolverDiagnostics {
        algorithm: "lp-core".to_string(),
        status: TerminationStatus::Converged,
        iterations: Some(1),
        ..Default::default()
    };

    pipeline_result
}

/// Shared projection-LP epilogue for continuous solvers.
///
/// Their core phase (Frank--Wolfe or conic interior point)
/// produces a continuous allocation whose duals don't yield valid clearing
/// prices. This caps each order's `max_fill` at the ceiled core allocation,
/// re-solves the standard welfare LP for exact prices, and finalizes.
///
/// `allocation[i]` is the core-phase fill for order `i` (in the same order as
/// `problem.orders`); it is ceiled as an integer upper bound and clamped to
/// `[0, max_fill]`.
pub(crate) fn project_and_finalize(
    allocation: &[f64],
    problem: &Problem,
    ctx: &SolverContext,
    start: Instant,
) -> PipelineResult {
    let projection_obj = welfare_weights(&problem.orders);
    project_and_finalize_with_objective(allocation, problem, ctx, &projection_obj, start)
}

/// Project a continuous allocation using a caller-supplied supporting LP
/// objective. Retained-cash clearing uses its final pacing-weighted objective
/// so the projection prices support the same first-order system as the core
/// solve; legacy solvers use [`project_and_finalize`] and linear welfare.
pub(crate) fn project_and_finalize_with_objective(
    allocation: &[f64],
    problem: &Problem,
    ctx: &SolverContext,
    projection_obj: &[f64],
    start: Instant,
) -> PipelineResult {
    let orders = &problem.orders;

    let mut projected_orders: Vec<Order> = orders.to_vec();
    for (i, order) in projected_orders.iter_mut().enumerate() {
        let core_fill = if allocation[i] <= 1e-9 {
            0
        } else {
            allocation[i].ceil() as u64
        };
        order.max_fill = Qty(core_fill.min(orders[i].max_fill.0));
    }

    let mm_constraint_orders = mm_constraint_order_indices(problem, ctx);
    let mut budget_rows = Vec::new();
    const MAX_BUDGET_PROJECTION_STEPS: usize = 8;

    for iteration in 0..=MAX_BUDGET_PROJECTION_STEPS {
        let Some(final_sol) = build_and_solve_lp(
            &projected_orders,
            &ctx.markets,
            &ctx.market_to_group,
            ctx.num_groups,
            projection_obj,
            &budget_rows,
        ) else {
            return PipelineResult::failure(
                "projection-lp",
                TerminationStatus::PostProcessingFailure,
                format!(
                    "projection LP did not return a solution at budget step {iteration} with {} rows",
                    budget_rows.len()
                ),
                start.elapsed().as_secs_f64(),
            );
        };

        let prices = normalized_yes_prices(&final_sol, &ctx.markets);
        if !has_mm_budget_violations(
            &final_sol,
            &projected_orders,
            &problem.mm_constraints,
            &mm_constraint_orders,
            &prices,
        ) {
            return finalize_result(&final_sol, problem, ctx, start);
        }
        if iteration == MAX_BUDGET_PROJECTION_STEPS {
            return PipelineResult::failure(
                "projection-lp",
                TerminationStatus::PostProcessingFailure,
                "integer landing did not reach an MM-budget fixed point in 8 projection steps",
                start.elapsed().as_secs_f64(),
            );
        }

        budget_rows.clear();
        budget_rows.extend(linearize_mm_budgets(
            &projected_orders,
            &problem.mm_constraints,
            &mm_constraint_orders,
            &prices,
        ));
    }

    unreachable!("bounded projection loop always returns")
}

/// Land a certified continuous target while taking prices from its supporting
/// matching LP.
///
/// This is stronger than [`project_and_finalize_with_objective`]: the caller
/// must know that `allocation` maximizes `projection_obj` up to its declared
/// certificate. A linear supporting objective can expose a large optimal
/// face, so a second lexicographic LP selects the primary-optimal point nearest
/// the certified target. Published market prices still come from the original
/// primary LP; auxiliary nearest-face duals are never mistaken for prices.
pub(crate) fn support_and_finalize_target_with_objective(
    allocation: &[f64],
    problem: &Problem,
    ctx: &SolverContext,
    projection_obj: &[f64],
    start: Instant,
) -> PipelineResult {
    let orders = &problem.orders;
    let order_map: HashMap<u64, &Order> = orders.iter().map(|order| (order.id, order)).collect();
    let mut capped_orders = orders.to_vec();
    for (index, order) in capped_orders.iter_mut().enumerate() {
        let target_cap = if allocation[index] <= 1e-9 {
            0
        } else {
            allocation[index].ceil() as u64
        };
        order.max_fill = Qty(target_cap.min(orders[index].max_fill.0));
    }

    let mm_constraint_orders = mm_constraint_order_indices(problem, ctx);
    let mut budget_rows = Vec::new();
    const MAX_PRICE_STEPS: usize = 8;

    for iteration in 0..=MAX_PRICE_STEPS {
        let Some((mut price_solution, nearest_allocation)) = solve_primary_and_nearest_face(
            &capped_orders,
            &ctx.markets,
            &ctx.market_to_group,
            ctx.num_groups,
            projection_obj,
            &budget_rows,
            allocation,
        ) else {
            return PipelineResult::failure(
                "target-support-lp",
                TerminationStatus::PostProcessingFailure,
                format!(
                    "supporting-price or nearest-face LP failed at budget step {iteration} with {} rows",
                    budget_rows.len()
                ),
                start.elapsed().as_secs_f64(),
            );
        };

        let prices = normalized_yes_prices(&price_solution, &ctx.markets);
        let primary_allocation = price_solution.q_values.clone();
        let round_at_primary_prices = |allocation: &[f64]| {
            allocation
                .iter()
                .zip(orders)
                .map(|(&quantity, order)| {
                    let rounded = quantity.round().clamp(0.0, order.max_fill.0 as f64);
                    let p_yes = prices
                        .get(&order.markets[0])
                        .copied()
                        .unwrap_or(Nanos(NANOS_PER_DOLLAR / 2));
                    let fill_price = if order.payoffs[0] != 0 {
                        p_yes
                    } else {
                        Nanos(NANOS_PER_DOLLAR.saturating_sub(p_yes.0))
                    };
                    let within_limit = if order.is_seller() {
                        fill_price >= order.limit_price
                    } else {
                        fill_price <= order.limit_price
                    };
                    if within_limit { rounded } else { 0.0 }
                })
                .collect::<Vec<_>>()
        };
        // Ill-scaled faces can make either the L1 selector or the primary basis
        // a poor integer representative of the same continuous price system.
        // Evaluate the three in-solver candidates already available: nearest
        // face, primary basis, and certified target. Select by the economic
        // minting-duality residual before checking hard budgets. No other
        // solver is called, and target movement remains measured.
        let candidate_allocations = [
            round_at_primary_prices(&nearest_allocation),
            round_at_primary_prices(&primary_allocation),
            round_at_primary_prices(allocation),
        ];
        const MAX_NEAREST_FACE_MINTING_GAP_NANOS: f64 = 50_000_000.0;
        let mut best_candidate = None;
        let mut candidate_gaps = Vec::with_capacity(candidate_allocations.len());
        let mut raw_candidate_gaps = Vec::with_capacity(candidate_allocations.len());
        for candidate in candidate_allocations {
            price_solution.q_values = candidate;
            // Test price support before hard-budget projection. Calling the
            // ordinary finalizer here would silently trim an over-budget MM
            // candidate, measure the mutated fill vector, and reject it before
            // the fixed-point loop had a chance to add its budget row. The
            // zero-price cleanup is retained because it is independent of MM
            // budgets and is also applied to the eventual landed result.
            let (mut preview, clearing_prices) =
                extract_result(&price_solution, orders, &ctx.markets);
            recompute_welfare(&mut preview, &order_map);
            let raw_zero_temperature =
                crate::retained_cash_solver::zero_temperature_minting_cost_for_fills(
                    problem,
                    &preview.fills,
                );
            raw_candidate_gaps.push(
                (raw_zero_temperature - preview.minting_cost as f64).abs()
                    / NANOS_PER_DOLLAR as f64,
            );
            trim_zero_price_minting(&mut preview, &order_map, &clearing_prices);
            recompute_welfare(&mut preview, &order_map);
            let zero_temperature =
                crate::retained_cash_solver::zero_temperature_minting_cost_for_fills(
                    problem,
                    &preview.fills,
                );
            let gap = (zero_temperature - preview.minting_cost as f64).abs();
            candidate_gaps.push(gap / NANOS_PER_DOLLAR as f64);
            if best_candidate
                .as_ref()
                .is_none_or(|(_, best_gap, _, _)| gap < *best_gap)
            {
                best_candidate = Some((
                    price_solution.q_values.clone(),
                    gap,
                    zero_temperature,
                    preview.minting_cost,
                ));
            }
        }
        let (candidate, minting_gap, zero_temperature, settlement_minting) =
            best_candidate.expect("three landing candidates");
        if minting_gap > MAX_NEAREST_FACE_MINTING_GAP_NANOS {
            return PipelineResult::failure(
                "target-support-lp",
                TerminationStatus::PostProcessingFailure,
                format!(
                    "no integer candidate was supported by primary minting prices at budget step {iteration}: best gap=${:.9}, C0=${:.9}, price cash=${:.9}, raw gaps=${raw_candidate_gaps:?}, cleaned gaps=${candidate_gaps:?}",
                    minting_gap / NANOS_PER_DOLLAR as f64,
                    zero_temperature / NANOS_PER_DOLLAR as f64,
                    settlement_minting as f64 / NANOS_PER_DOLLAR as f64,
                ),
                start.elapsed().as_secs_f64(),
            );
        }
        price_solution.q_values = candidate;

        if !has_mm_budget_violations(
            &price_solution,
            &capped_orders,
            &problem.mm_constraints,
            &mm_constraint_orders,
            &prices,
        ) {
            let mut result = finalize_result(&price_solution, problem, ctx, start);
            result.diagnostics.integer_landing_budget_trimmed = Some(false);
            return result;
        }
        if iteration == MAX_PRICE_STEPS {
            // This is an explicit, measured integer repair inside the same
            // solver—not a substitution of a different core allocation.
            // The L1/objective/duality diagnostics expose its effect.
            let mut result = finalize_result(&price_solution, problem, ctx, start);
            result.diagnostics.integer_landing_budget_trimmed = Some(true);
            return result;
        }

        budget_rows = linearize_mm_budgets(
            &capped_orders,
            &problem.mm_constraints,
            &mm_constraint_orders,
            &prices,
        );
    }

    unreachable!("bounded supporting-price loop always returns")
}

/// Recompute welfare, volume, and fill count from scratch.
pub(crate) fn recompute_welfare(result: &mut MatchingResult, order_map: &HashMap<u64, &Order>) {
    result.gross_welfare = 0;
    result.total_quantity_filled = 0;
    result.orders_filled = 0;
    for fill in &result.fills {
        if let Some(&order) = order_map.get(&fill.order_id) {
            result.gross_welfare += order.gross_welfare_contribution(fill.fill_qty);
        }
        result.total_quantity_filled += fill.fill_qty.0;
        result.orders_filled += 1;
    }
    result.minting_cost = minting_cost_from_fills(order_map.values().copied(), &result.fills);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{
        group_minting_problem, minting_problem, no_profitable_trades_problem, single_market_problem,
    };

    #[test]
    fn reusable_oracle_matches_cold_objective_after_cost_updates() {
        let problem = group_minting_problem();
        let ctx = build_solver_context(&problem);
        let first = welfare_weights(&problem.orders);
        let second: Vec<_> = first
            .iter()
            .enumerate()
            .map(|(index, value)| value * (0.25 + 0.1 * index as f64))
            .collect();
        let mut reusable = ReusableLpOracle::new(
            &problem.orders,
            &ctx.markets,
            &ctx.market_to_group,
            ctx.num_groups,
            &[],
        )
        .expect("valid oracle");

        for objective in [&first, &second, &first] {
            let warm = reusable.solve(objective).expect("warm solve");
            let cold = build_and_solve_lp(
                &problem.orders,
                &ctx.markets,
                &ctx.market_to_group,
                ctx.num_groups,
                objective,
                &[],
            )
            .expect("cold solve");
            assert!(
                (warm.objective_value_dollars - cold.objective_value_dollars).abs() <= 1e-7,
                "warm={} cold={}",
                warm.objective_value_dollars,
                cold.objective_value_dollars,
            );
            let upper = warm
                .objective_upper_bound_dollars
                .expect("zero-RHS oracle has a dual bound");
            assert!(
                upper + 1e-7 >= warm.objective_value_dollars,
                "dual upper bound {upper} below primal {}",
                warm.objective_value_dollars,
            );
            assert!(
                upper - warm.objective_value_dollars <= 1e-5,
                "unexpectedly loose dual bound: upper={upper}, primal={}",
                warm.objective_value_dollars,
            );
        }
    }

    #[test]
    fn test_lp_single_market() {
        let result = LpSolver::new().solve(&single_market_problem());

        assert!(
            result.result.total_welfare() > 0,
            "should produce positive welfare, got {}",
            result.result.total_welfare()
        );
        assert!(result.result.orders_filled > 0, "should fill some orders");
    }

    #[test]
    fn test_lp_minting() {
        let result = LpSolver::new().solve(&minting_problem());

        assert!(
            result.result.orders_filled == 2,
            "both orders should fill via minting, got {}",
            result.result.orders_filled
        );
        assert!(
            result.result.total_welfare() > 0,
            "minting should produce positive welfare"
        );
        let zero_temperature = crate::zero_temperature_minting_cost_for_fills(
            &minting_problem(),
            &result.result.fills,
        );
        assert!(
            (zero_temperature - result.result.minting_cost as f64).abs() <= 1.0,
            "landed prices must support the complete-set cost: C0={zero_temperature}, cash={}",
            result.result.minting_cost,
        );
    }

    #[test]
    fn one_sided_demand_pays_the_complete_set_epigraph_cost() {
        let mut problem = Problem::new("one_sided_minting");
        let market = problem.markets.add_binary("market");
        problem.orders.push(matching_engine::simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            100,
        ));

        let result = LpSolver::new().solve(&problem);
        assert_eq!(
            result.result.orders_filled, 0,
            "a lone 60c YES bid cannot receive newly minted supply for free"
        );
        assert!(result.result.total_welfare() >= 0);
    }

    #[test]
    fn zero_budget_mm_orders_cannot_suppress_retail_crossing() {
        let mut problem = Problem::new("zero_budget_mm_with_retail_cross");
        let market = problem.markets.add_binary("market");
        problem.orders.push(matching_engine::simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            1,
        ));
        problem.orders.push(matching_engine::outcome_sell(
            &problem.markets,
            2,
            market,
            0,
            400_000_000,
            1,
        ));
        problem.orders.push(matching_engine::outcome_sell(
            &problem.markets,
            3,
            market,
            0,
            0,
            1_000,
        ));
        let mut mm = matching_engine::MmConstraint::new(matching_engine::MmId::new(1), Nanos(0));
        mm.add_order(3, MmSide::SellYes);
        problem.mm_constraints.push(mm);

        let result = LpSolver::new().solve(&problem);
        let filled: HashSet<_> = result
            .result
            .fills
            .iter()
            .map(|fill| fill.order_id)
            .collect();
        assert!(filled.contains(&1));
        assert!(filled.contains(&2));
        assert!(!filled.contains(&3));
    }

    #[test]
    fn test_lp_group_minting() {
        let problem = group_minting_problem();
        let result = LpSolver::new().solve(&problem);

        assert!(
            result.result.orders_filled >= 3,
            "should fill all 3 via group minting, filled {}",
            result.result.orders_filled
        );
        assert!(
            result
                .result
                .fills
                .iter()
                .all(|fill| problem.orders.iter().any(|order| order.id == fill.order_id)),
            "LP finalizer must not leak synthetic minting/arb fills into block output"
        );
        assert!(
            result.result.total_welfare() > 0,
            "group minting should produce positive welfare, got {}",
            result.result.total_welfare()
        );
    }

    #[test]
    fn test_lp_empty_problem() {
        let problem = Problem::new("empty");
        let solver = LpSolver::new();
        let result = solver.solve(&problem);
        assert_eq!(result.result.orders_filled, 0);
    }

    #[test]
    fn test_lp_no_profitable_trades() {
        // Should not fill because minting costs $1 but only recovers $0.60.
        let result = LpSolver::new().solve(&no_profitable_trades_problem());

        assert_eq!(
            result.result.orders_filled, 0,
            "should not fill unprofitable minting"
        );
    }
}
