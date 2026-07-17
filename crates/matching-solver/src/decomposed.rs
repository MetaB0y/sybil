//! Per-market-group decomposition for convex solvers.
//!
//! Partitions the problem into independent components (one per `MarketGroup` +
//! standalone markets), solves each with an inner solver, and coordinates
//! MM budgets via **proportional response on deployed value** when MMs span
//! multiple groups.
//!
//! **Theorem** (the decomposition companion note, proportional response /
//! equal scarcity, Theorem 1): When no order spans two components, allocate
//! each spanning MM's budget across components in proportion to the *deployed
//! value* it earns there, `V_k^m = U_k^m + s_k^m` (weighted fill value plus
//! retained cash). The fixed points are the *equal-scarcity* allocations
//! (`B_k^m / V_k^m` equal across the MM's active components), which are exactly
//! the componentwise restrictions of the monolithic optimum. Cross-group
//! orders are dropped.
//!
//! Coordinating instead on per-component EG *objective values* (equalizing
//! utilities `U_k^m`, as an earlier draft did) is unsound: EG optimal values
//! are convex in budget, so ascending their sum rewards piling budget onto
//! saturated components and its interior stationary point (equal utility) is a
//! welfare *minimizer* along allocation lines. See the companion note's
//! "Surrogate Trap" section. This module implements the corrected rule.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use matching_engine::{MarketGroup, MarketId, MarketSet, MmConstraint, Nanos, Order, Problem};

use crate::result::{PipelineResult, SolverDiagnostics, TerminationStatus};

// ============================================================================
// DecomposedSolver
// ============================================================================

/// Decomposes the problem by market group and coordinates MM budgets.
pub struct DecomposedSolver<S: crate::Solver> {
    inner: S,
    max_budget_iters: usize,
    convergence_eps: f64,
}

struct ComponentSolveOutcome {
    results: Vec<PipelineResult>,
    iterations: usize,
    converged: bool,
    convergence_metric: Option<f64>,
    component_failures: usize,
    component_caps: usize,
}

impl<S: crate::Solver> DecomposedSolver<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            max_budget_iters: 20,
            convergence_eps: 1e-4,
        }
    }

    pub fn with_config(inner: S, max_budget_iters: usize, convergence_eps: f64) -> Self {
        Self {
            inner,
            max_budget_iters,
            convergence_eps,
        }
    }

    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();

        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        let supported = crate::solver::filter_supported_problem(problem, "Decomposed");
        let _rejected_orders = supported.rejected_orders;
        let problem = supported.problem.as_ref();
        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        // Step 1: Partition markets into components
        let (market_to_component, num_components) = partition_markets(problem);

        // Single component → delegate directly (zero overhead)
        if num_components <= 1 {
            return self.inner.solve(problem);
        }

        // Step 2: Assign orders to components (drop cross-component)
        let order_components = assign_orders(&problem.orders, &market_to_component);

        // Step 3: Assign MMs to components
        let mm_components = assign_mms(problem, &order_components);

        // Step 4: Classify MMs into local vs spanning
        let (local_mms, spanning_mms) = classify_mms(&mm_components);

        let dropped = order_components.iter().filter(|c| c.is_none()).count();
        tracing::debug!(
            num_components,
            local_mms = local_mms.len(),
            spanning_mms = spanning_mms.len(),
            dropped_orders = dropped,
            "decomposed: partitioned problem"
        );

        // Step 5-7: Solve
        let outcome = if spanning_mms.is_empty() {
            // No spanning MMs → independent solves
            let results = self.solve_independent(
                problem,
                &market_to_component,
                num_components,
                &order_components,
                &mm_components,
                &local_mms,
            );
            ComponentSolveOutcome {
                component_failures: count_component_failures(&results),
                component_caps: count_component_caps(&results),
                results,
                iterations: 1,
                converged: true,
                convergence_metric: Some(0.0),
            }
        } else {
            // Proportional-response budget coordination for spanning MMs
            self.solve_with_proportional_response(
                problem,
                &market_to_component,
                num_components,
                &order_components,
                &mm_components,
                &local_mms,
                &spanning_mms,
            )
        };

        // Step 8: Aggregate + post-process (global budget enforcement, welfare recompute).
        let mut diagnostic_parts = Vec::new();
        if dropped > 0 {
            diagnostic_parts.push(format!("dropped {dropped} cross-component orders"));
        }
        if outcome.component_failures > 0 {
            diagnostic_parts.push(format!(
                "{} component solves failed",
                outcome.component_failures
            ));
        }
        if outcome.component_caps > 0 {
            diagnostic_parts.push(format!(
                "{} component solves reached a cap",
                outcome.component_caps
            ));
        }

        let mut result =
            crate::component_assembly::assemble_component_results(problem, outcome.results);
        result.total_time_secs = start.elapsed().as_secs_f64();
        result.diagnostics = SolverDiagnostics {
            algorithm: format!("decomposed-{}", self.inner.name().to_lowercase()),
            status: if outcome.component_failures > 0 {
                TerminationStatus::NumericalFailure
            } else if outcome.converged && outcome.component_caps == 0 {
                TerminationStatus::Converged
            } else {
                TerminationStatus::IterationLimit
            },
            iterations: Some(outcome.iterations),
            convergence_metric: outcome.convergence_metric,
            message: (!diagnostic_parts.is_empty()).then(|| diagnostic_parts.join("; ")),
            ..Default::default()
        };
        result
    }

    /// Solve all components, using rayon parallelism when the `parallel` feature is enabled.
    fn solves_parallel(
        &self,
        problem: &Problem,
        market_to_component: &HashMap<MarketId, usize>,
        num_components: usize,
        order_components: &[Option<usize>],
        mm_budgets: &HashMap<usize, HashMap<usize, u64>>,
    ) -> Vec<PipelineResult> {
        let solve_one = |comp: usize| {
            let sub = build_sub_problem(
                problem,
                market_to_component,
                comp,
                order_components,
                mm_budgets,
            );
            if sub.orders.is_empty() {
                PipelineResult::empty()
            } else {
                self.inner.solve(&sub)
            }
        };

        #[cfg(feature = "parallel")]
        {
            (0..num_components).into_par_iter().map(solve_one).collect()
        }

        #[cfg(not(feature = "parallel"))]
        {
            (0..num_components).map(solve_one).collect()
        }
    }

    /// Solve each component independently (no spanning MMs).
    fn solve_independent(
        &self,
        problem: &Problem,
        market_to_component: &HashMap<MarketId, usize>,
        num_components: usize,
        order_components: &[Option<usize>],
        _mm_components: &[(HashSet<usize>, HashSet<usize>)],
        local_mms: &[(usize, usize)], // (mm_idx, component)
    ) -> Vec<PipelineResult> {
        // Build budget map: mm_idx -> component -> budget
        let mut mm_budgets: HashMap<usize, HashMap<usize, u64>> = HashMap::new();
        for &(mm_idx, comp) in local_mms {
            mm_budgets
                .entry(mm_idx)
                .or_default()
                .insert(comp, problem.mm_constraints[mm_idx].max_capital.0);
        }

        self.solves_parallel(
            problem,
            market_to_component,
            num_components,
            order_components,
            &mm_budgets,
        )
    }

    /// Solve with proportional response to coordinate spanning MM budgets.
    ///
    /// Each round: solve all components with the current budget split, measure
    /// each spanning MM's deployed value `V_k^m = U_k^m + s_k^m` per component,
    /// then reallocate `B_k^m ← pos_budget · V_k^m / Σ_{m'} V_k^{m'}` over the
    /// MM's positive-weight components (seller-only components keep their fixed
    /// reserved share). Fixed points are equal-scarcity allocations, i.e. the
    /// exact monolithic decomposition (companion note, Theorem 1).
    #[allow(clippy::too_many_arguments)]
    fn solve_with_proportional_response(
        &self,
        problem: &Problem,
        market_to_component: &HashMap<MarketId, usize>,
        num_components: usize,
        order_components: &[Option<usize>],
        _mm_components: &[(HashSet<usize>, HashSet<usize>)],
        local_mms: &[(usize, usize)],
        spanning_mms: &[(usize, Vec<usize>)], // (mm_idx, positive-weight components)
    ) -> ComponentSolveOutcome {
        // Initialize budgets
        // Local MMs: full budget to their component
        // Spanning MMs: proportional split across ALL active components (including
        // seller-only ones). Proportional response only rebalances among
        // positive-weight components; seller-only components keep a fixed share.
        let mut mm_budgets: HashMap<usize, HashMap<usize, u64>> = HashMap::new();

        for &(mm_idx, comp) in local_mms {
            mm_budgets
                .entry(mm_idx)
                .or_default()
                .insert(comp, problem.mm_constraints[mm_idx].max_capital.0);
        }

        // Build order lookup for initialization
        let order_id_to_idx: HashMap<u64, usize> = problem
            .orders
            .iter()
            .enumerate()
            .map(|(i, o)| (o.id, i))
            .collect();

        // For spanning MMs, allocate across ALL components (pos + seller-only)
        // proportional to order count, but weight positive-weight orders higher.
        for &(mm_idx, ref _pos_comps) in spanning_mms {
            let mm = &problem.mm_constraints[mm_idx];
            let (ref all_comps, _) = _mm_components[mm_idx];
            let total = mm.max_capital;

            // Weight: positive-weight order count (floor at 1 for seller-only comps)
            let weights: Vec<(usize, f64)> = all_comps
                .iter()
                .map(|&comp| {
                    let pos_count: usize = mm
                        .order_ids
                        .iter()
                        .filter(|&&oid| {
                            order_id_to_idx.get(&oid).is_some_and(|&idx| {
                                order_components[idx] == Some(comp)
                                    && !problem.orders[idx].is_seller()
                            })
                        })
                        .count();
                    // Seller-only components get weight 1 (minimal but non-zero)
                    (comp, pos_count.max(1) as f64)
                })
                .collect();

            let total_weight: f64 = weights.iter().map(|&(_, w)| w).sum();
            let map = mm_budgets.entry(mm_idx).or_default();
            let mut allocated = 0u64;
            let last_idx = weights.len() - 1;
            for (i, &(comp, w)) in weights.iter().enumerate() {
                if i == last_idx {
                    map.insert(comp, total.0 - allocated);
                } else {
                    let budget = (total.0 as f64 * w / total_weight).round() as u64;
                    map.insert(comp, budget);
                    allocated += budget;
                }
            }
        }

        let mut best_results: Vec<PipelineResult> = Vec::new();
        let mut best_welfare = i64::MIN;
        let mut iterations_run = 0usize;
        let mut did_converge = false;
        let mut last_max_gap = None;

        for iter in 0..self.max_budget_iters {
            iterations_run = iter + 1;
            let iter_start = Instant::now();

            // Solve each component with current budget allocation
            let results = self.solves_parallel(
                problem,
                market_to_component,
                num_components,
                order_components,
                &mm_budgets,
            );

            let solve_secs = iter_start.elapsed().as_secs_f64();

            // If no spanning MMs need updating (first iter already done), break
            if spanning_mms.is_empty() {
                best_results = results;
                break;
            }

            // Measure each spanning MM's deployed value V_k^m = U_k^m + s_k^m
            // (weighted fill value + retained cash) per component.
            let deployed = compute_mm_deployed_values(
                problem,
                &results,
                order_components,
                &mm_budgets,
                spanning_mms,
            );

            // Check convergence: scarcity factors B_k^m / V_k^m agree per MM.
            let converged =
                check_convergence(&deployed, &mm_budgets, spanning_mms, self.convergence_eps);

            // Track best welfare seen across all iterations
            let iter_welfare: i64 = results.iter().map(|r| r.result.total_welfare()).sum();
            if iter_welfare > best_welfare {
                best_welfare = iter_welfare;
                best_results = results;
            }

            // Log convergence progress
            let max_gap = compute_max_log_gap(&deployed, &mm_budgets, spanning_mms);
            last_max_gap = Some(max_gap);
            tracing::debug!(
                iter,
                solve_secs = format!("{:.3}", solve_secs),
                welfare = iter_welfare,
                best_welfare,
                max_ln_scarcity_gap = format!("{:.4}", max_gap),
                converged,
                "proportional response iteration"
            );

            if converged {
                did_converge = true;
                break;
            }

            // Proportional response (Wu–Zhang 2007 analogue): reallocate each
            // spanning MM's budget in proportion to the deployed value each
            // component earned. Fixed points are equal-scarcity allocations,
            // i.e. the exact monolithic decomposition (companion note, Thm 1).
            // There is no step size — it is a direct reallocation each round.
            for &(mm_idx, ref pos_comps) in spanning_mms {
                let full_budget = problem.mm_constraints[mm_idx].max_capital;
                let budgets = mm_budgets.entry(mm_idx).or_default();

                // Budget reserved for seller-only components (not rebalanced)
                let (ref all_comps, _) = _mm_components[mm_idx];
                let seller_reserved: u64 = all_comps
                    .iter()
                    .filter(|c| !pos_comps.contains(c))
                    .map(|c| budgets.get(c).copied().unwrap_or(0))
                    .sum();
                let pos_budget = full_budget.0.saturating_sub(seller_reserved);

                let values: Vec<(usize, f64)> = pos_comps
                    .iter()
                    .map(|&comp| (comp, deployed.get(&(mm_idx, comp)).copied().unwrap_or(0.0)))
                    .collect();

                for (comp, budget) in reallocate_proportional(pos_budget, &values) {
                    budgets.insert(comp, budget);
                }
            }
        }

        ComponentSolveOutcome {
            component_failures: count_component_failures(&best_results),
            component_caps: count_component_caps(&best_results),
            results: best_results,
            iterations: iterations_run,
            converged: did_converge,
            convergence_metric: last_max_gap,
        }
    }
}

fn count_component_failures(results: &[PipelineResult]) -> usize {
    results
        .iter()
        .filter(|result| {
            matches!(
                result.diagnostics.status,
                TerminationStatus::UnsupportedInput
                    | TerminationStatus::NumericalFailure
                    | TerminationStatus::PostProcessingFailure
                    | TerminationStatus::Infeasible
            )
        })
        .count()
}

fn count_component_caps(results: &[PipelineResult]) -> usize {
    results
        .iter()
        .filter(|result| {
            matches!(
                result.diagnostics.status,
                TerminationStatus::IterationLimit | TerminationStatus::TimeLimit
            )
        })
        .count()
}

// ============================================================================
/// `'static` bound required for `Arc<dyn Solver>` usage and rayon parallelism.
impl<S: crate::Solver + 'static> crate::Solver for DecomposedSolver<S> {
    /// Forwards to the inherent `DecomposedSolver::solve` method.
    fn solve(&self, problem: &Problem) -> PipelineResult {
        DecomposedSolver::solve(self, problem)
    }
    fn name(&self) -> &str {
        "Decomposed"
    }
}

// Component partitioning
// ============================================================================

/// Partition markets into components: one per MarketGroup, one per standalone market.
fn partition_markets(problem: &Problem) -> (HashMap<MarketId, usize>, usize) {
    let mut market_to_component: HashMap<MarketId, usize> = HashMap::new();
    let mut next_component = 0usize;

    // Each MarketGroup → one component
    for group in &problem.market_groups {
        for &market_id in &group.markets {
            market_to_component.insert(market_id, next_component);
        }
        next_component += 1;
    }

    // Standalone markets (not in any group) → each gets its own component
    let grouped_markets: HashSet<MarketId> = problem
        .market_groups
        .iter()
        .flat_map(|g| g.markets.iter().copied())
        .collect();

    for market in problem.markets.iter() {
        if !grouped_markets.contains(&market.id) {
            market_to_component.insert(market.id, next_component);
            next_component += 1;
        }
    }

    (market_to_component, next_component)
}

/// Assign each order to its component. Returns None for cross-component orders.
fn assign_orders(
    orders: &[Order],
    market_to_component: &HashMap<MarketId, usize>,
) -> Vec<Option<usize>> {
    orders
        .iter()
        .map(|order| {
            let mut component = None;
            for market_id in order.active_markets() {
                let Some(&comp) = market_to_component.get(&market_id) else {
                    return None; // market not in any component
                };
                match component {
                    None => component = Some(comp),
                    Some(c) if c != comp => return None, // cross-component
                    _ => {}
                }
            }
            component
        })
        .collect()
}

/// For each MM, find which components its orders belong to.
/// Returns (all_components, positive_weight_components) per MM.
/// Mirror descent only operates on components with positive-weight orders
/// (which participate in the EG exp cone). Seller-only components get
/// fixed budget allocations.
fn assign_mms(
    problem: &Problem,
    order_components: &[Option<usize>],
) -> Vec<(HashSet<usize>, HashSet<usize>)> {
    let order_id_to_idx: HashMap<u64, usize> = problem
        .orders
        .iter()
        .enumerate()
        .map(|(i, o)| (o.id, i))
        .collect();

    problem
        .mm_constraints
        .iter()
        .map(|mm| {
            let mut all_comps = HashSet::new();
            let mut pos_comps = HashSet::new();
            for &oid in &mm.order_ids {
                if let Some(&idx) = order_id_to_idx.get(&oid)
                    && let Some(comp) = order_components[idx]
                {
                    all_comps.insert(comp);
                    if !problem.orders[idx].is_seller() {
                        pos_comps.insert(comp);
                    }
                }
            }
            (all_comps, pos_comps)
        })
        .collect()
}

/// (mm_idx, component) for MMs whose orders all lie in one component.
type LocalMm = (usize, usize);
/// (mm_idx, sorted components) for MMs spanning multiple components.
type SpanningMm = (usize, Vec<usize>);

/// Classify MMs into local (single component) vs spanning (multiple components).
/// Uses positive-weight component set for classification — components with only
/// seller orders don't need budget coordination (they don't participate in the
/// EG exp cone).
fn classify_mms(
    mm_components: &[(HashSet<usize>, HashSet<usize>)],
) -> (Vec<LocalMm>, Vec<SpanningMm>) {
    let mut local = Vec::new();
    let mut spanning = Vec::new();

    for (mm_idx, (all_comps, pos_comps)) in mm_components.iter().enumerate() {
        if all_comps.is_empty() {
            continue; // MM has no orders in any component
        }
        // Classify based on positive-weight components (which need budget coordination)
        match pos_comps.len() {
            0 => {
                // Only seller orders — treat as local to avoid budget coordination.
                // Give full budget to the first component
                if let Some(&comp) = all_comps.iter().next() {
                    local.push((mm_idx, comp));
                }
            }
            1 => {
                let comp = *pos_comps.iter().next().unwrap();
                local.push((mm_idx, comp));
            }
            _ => {
                let mut sorted: Vec<usize> = pos_comps.iter().copied().collect();
                sorted.sort();
                spanning.push((mm_idx, sorted));
            }
        }
    }

    (local, spanning)
}

// ============================================================================
// Sub-problem construction
// ============================================================================

/// Build a sub-problem for a single component.
fn build_sub_problem(
    problem: &Problem,
    market_to_component: &HashMap<MarketId, usize>,
    component: usize,
    order_components: &[Option<usize>],
    mm_budgets: &HashMap<usize, HashMap<usize, u64>>,
) -> Problem {
    // Collect markets in this component
    let component_markets: Vec<MarketId> = market_to_component
        .iter()
        .filter(|&(_, &comp)| comp == component)
        .map(|(&mid, _)| mid)
        .collect();

    let market_set: HashSet<MarketId> = component_markets.iter().copied().collect();

    // Build MarketSet
    let mut markets = MarketSet::new();
    for &mid in &component_markets {
        if let Some(market) = problem.markets.get(mid) {
            markets.add_market(market.clone());
        }
    }

    // Collect orders assigned to this component
    let mut orders = Vec::new();
    let mut order_ids_in_component: HashSet<u64> = HashSet::new();
    for (i, order) in problem.orders.iter().enumerate() {
        if order_components[i] == Some(component) {
            orders.push(order.clone());
            order_ids_in_component.insert(order.id);
        }
    }

    // Filter MM constraints: only include POSITIVE-WEIGHT (buyer) orders.
    // Seller MM orders stay in the problem as regular orders, outside the
    // budget constraint. This keeps the component's deployed value V_k^m a clean
    // function of the buyer fills (no seller capital drain), so proportional
    // response converges per the decomposition theorem. The global
    // trim_mm_budget_overflows post-processing enforces the real budget across
    // all orders including sellers.
    let mut mm_constraints = Vec::new();
    for (mm_idx, mm) in problem.mm_constraints.iter().enumerate() {
        let budget = mm_budgets
            .get(&mm_idx)
            .and_then(|m| m.get(&component))
            .copied()
            .unwrap_or(0);

        if budget == 0 {
            // Treat a zero component allocation as no flash liquidity. Leaving
            // seller-side MM orders unconstrained lets them set an endpoint
            // price, after which global trimming removes their fills but
            // cannot recover otherwise feasible retail crossing volume.
            for order in &mut orders {
                if mm.order_ids.contains(&order.id) {
                    order.max_fill = matching_engine::Qty::ZERO;
                }
            }
            continue;
        }

        // Only positive-weight (buyer) orders go into the budget constraint
        let filtered_order_ids: Vec<u64> = mm
            .order_ids
            .iter()
            .filter(|&&oid| {
                order_ids_in_component.contains(&oid) && {
                    // Check if this order is a buyer (positive welfare weight)
                    problem.orders.iter().any(|o| o.id == oid && !o.is_seller())
                }
            })
            .copied()
            .collect();

        if filtered_order_ids.is_empty() {
            continue;
        }

        let mut new_mm = MmConstraint::new(mm.mm_id, Nanos(budget));
        for &oid in &filtered_order_ids {
            if let Some(&side) = mm.order_sides.get(&oid) {
                new_mm.add_order(oid, side);
            }
        }
        mm_constraints.push(new_mm);
    }

    // Filter market groups: only groups whose markets are in this component
    let market_groups: Vec<MarketGroup> = problem
        .market_groups
        .iter()
        .filter(|g| g.markets.iter().all(|mid| market_set.contains(mid)))
        .cloned()
        .collect();

    let mut sub = Problem::new(format!("{}_comp{}", problem.name, component));
    sub.markets = markets;
    sub.orders = orders;
    sub.mm_constraints = mm_constraints;
    sub.market_groups = market_groups;
    sub
}

// ============================================================================
// MM deployed-value computation
// ============================================================================

/// Compute the deployed value `V_k^m = U_k^m + s_k^m` of each spanning MM in
/// each component, used to drive proportional-response reallocation.
///
/// `V_k^m` is the sum of two cash-denominated (nanos) quantities:
/// - `U_k^m = Σ_{i ∈ MM_k ∩ comp_m, buyers} L_i q_i` — the weighted fill value,
///   `L_i` the order's limit price and `q_i` its fill quantity, measured as a
///   notional (`notional_nanos`, dividing by `SHARE_SCALE`) so it shares units
///   with cash.
/// - `s_k^m = B_k^m − spend_k^m` — retained cash: the component budget minus
///   capital actually spent (`MmSide::capital_needed` at the fill price),
///   floored at zero.
///
/// Coordinating on deployed value (the companion note's *equal scarcity*
/// invariant), not on raw utility `U_k^m`, is what makes the fixed points equal
/// the monolithic optimum. Using `U` alone is the superseded "surrogate trap".
fn compute_mm_deployed_values(
    problem: &Problem,
    results: &[PipelineResult],
    order_components: &[Option<usize>],
    mm_budgets: &HashMap<usize, HashMap<usize, u64>>,
    spanning_mms: &[(usize, Vec<usize>)],
) -> HashMap<(usize, usize), f64> {
    use matching_engine::notional_nanos;

    // Build order_id -> (order_index, component) mapping
    let order_id_info: HashMap<u64, (usize, usize)> = problem
        .orders
        .iter()
        .enumerate()
        .filter_map(|(i, o)| order_components[i].map(|comp| (o.id, (i, comp))))
        .collect();

    // Build fill lookup: component -> order_id -> fill
    let mut fill_lookup: HashMap<usize, HashMap<u64, &matching_engine::Fill>> = HashMap::new();
    for (comp, result) in results.iter().enumerate() {
        let map = fill_lookup.entry(comp).or_default();
        for fill in &result.result.fills {
            map.insert(fill.order_id, fill);
        }
    }

    let mut deployed = HashMap::new();

    for &(mm_idx, ref comps) in spanning_mms {
        let mm = &problem.mm_constraints[mm_idx];
        for &comp in comps {
            // U_k^m: weighted fill value (notional, nanos). spend_k^m: capital used (nanos).
            let mut utility = 0.0f64;
            let mut spend = 0u128;
            for &oid in &mm.order_ids {
                if let Some(&(order_idx, order_comp)) = order_id_info.get(&oid) {
                    if order_comp != comp {
                        continue;
                    }
                    let order = &problem.orders[order_idx];
                    // L_i = sign_i × limit_price_i (welfare weight)
                    let w_i = crate::lp_solver::welfare_weight(order);
                    // Only positive-weight (buyer) orders participate in the
                    // component's budget constraint / EG utility.
                    if w_i <= 0.0 {
                        continue;
                    }
                    let Some(fills) = fill_lookup.get(&comp) else {
                        continue;
                    };
                    let Some(fill) = fills.get(&oid) else {
                        continue;
                    };
                    // Cash-denominated fill value L_i · q_i (÷ SHARE_SCALE).
                    utility += notional_nanos(order.limit_price, fill.fill_qty).0 as f64;
                    // Capital spent at the fill price for this MM side.
                    if let Some(&side) = mm.order_sides.get(&oid) {
                        spend += side.capital_needed(fill.fill_price, fill.fill_qty).0 as u128;
                    }
                }
            }

            // Retained cash s_k^m = B_k^m − spend_k^m (floored at 0). B_k^m is the
            // component budget this round produced the fills under. V = U + s, both
            // in nanos, so the proportional-response ratios are unitful-consistent.
            let component_budget = mm_budgets
                .get(&mm_idx)
                .and_then(|m| m.get(&comp))
                .copied()
                .unwrap_or(0) as u128;
            let retained_cash = component_budget.saturating_sub(spend) as f64;
            let value = (utility + retained_cash).max(0.0);
            deployed.insert((mm_idx, comp), value);
        }
    }

    deployed
}

/// Compute the maximum ln-scarcity gap across all spanning MMs (for logging).
/// Scarcity of MM `k` in component `m` is `B_k^m / V_k^m`.
fn compute_max_log_gap(
    deployed: &HashMap<(usize, usize), f64>,
    mm_budgets: &HashMap<usize, HashMap<usize, u64>>,
    spanning_mms: &[(usize, Vec<usize>)],
) -> f64 {
    let mut max_gap = 0.0f64;
    for &(mm_idx, ref comps) in spanning_mms {
        let logs: Vec<f64> = comps
            .iter()
            .filter_map(|&comp| scarcity(deployed, mm_budgets, mm_idx, comp).map(f64::ln))
            .collect();
        if logs.len() >= 2 {
            let max_l = logs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let min_l = logs.iter().cloned().fold(f64::INFINITY, f64::min);
            max_gap = max_gap.max((max_l - min_l).abs());
        }
    }
    max_gap
}

/// Proportional-response reallocation: split `pos_budget` across components in
/// proportion to their deployed value, `B^m ← pos_budget · V^m / Σ V^{m'}`.
///
/// Deterministic: components are sorted by index, integer budgets are rounded,
/// and the last component absorbs the rounding remainder so the shares sum to
/// exactly `pos_budget`. Deployed values are floored at 1.0 so integer rounding
/// cannot permanently starve a component (proportional response never zeros a
/// weight, but rounding could). Returns `(component, budget)` pairs; an empty
/// input or all-zero values yields an empty reallocation (caller keeps prior).
fn reallocate_proportional(pos_budget: u64, values: &[(usize, f64)]) -> Vec<(usize, u64)> {
    let mut values: Vec<(usize, f64)> = values.iter().map(|&(c, v)| (c, v.max(1.0))).collect();
    values.sort_by_key(|&(comp, _)| comp);

    let total_value: f64 = values.iter().map(|&(_, v)| v).sum();
    if values.is_empty() || total_value <= 0.0 {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(values.len());
    let mut allocated = 0u64;
    let last_idx = values.len() - 1;
    for (i, &(comp, value)) in values.iter().enumerate() {
        if i == last_idx {
            out.push((comp, pos_budget.saturating_sub(allocated)));
        } else {
            let budget = (pos_budget as f64 * value / total_value).round().max(1.0) as u64;
            out.push((comp, budget));
            allocated += budget;
        }
    }
    out
}

/// Scarcity factor `B_k^m / V_k^m`, or `None` when `V_k^m` is non-positive.
fn scarcity(
    deployed: &HashMap<(usize, usize), f64>,
    mm_budgets: &HashMap<usize, HashMap<usize, u64>>,
    mm_idx: usize,
    comp: usize,
) -> Option<f64> {
    let v = deployed.get(&(mm_idx, comp)).copied().unwrap_or(0.0);
    if v <= 0.0 {
        return None;
    }
    let b = mm_budgets
        .get(&mm_idx)
        .and_then(|m| m.get(&comp))
        .copied()
        .unwrap_or(0) as f64;
    Some(b / v)
}

/// Check if scarcity factors `B_k^m / V_k^m` have equalized across components
/// for all spanning MMs (the equal-scarcity fixed-point condition).
fn check_convergence(
    deployed: &HashMap<(usize, usize), f64>,
    mm_budgets: &HashMap<usize, HashMap<usize, u64>>,
    spanning_mms: &[(usize, Vec<usize>)],
    eps: f64,
) -> bool {
    for &(mm_idx, ref comps) in spanning_mms {
        // ln-scarcity of each component with positive deployed value.
        let log_scarcities: Vec<f64> = comps
            .iter()
            .filter_map(|&comp| scarcity(deployed, mm_budgets, mm_idx, comp).map(f64::ln))
            .collect();

        if log_scarcities.len() < 2 {
            continue;
        }

        let max_log = log_scarcities
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        let min_log = log_scarcities.iter().cloned().fold(f64::INFINITY, f64::min);

        if (max_log - min_log).abs() > eps {
            return false;
        }
    }

    true
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{
        MarketGroup, MmConstraint, MmId, MmSide, NANOS_PER_DOLLAR, simple_no_buy, simple_yes_buy,
    };

    /// Trivial component solver that delegates to LpSolver.
    #[cfg(feature = "lp")]
    struct TestLpSolver;

    #[cfg(feature = "lp")]
    impl crate::Solver for TestLpSolver {
        fn solve(&self, problem: &Problem) -> PipelineResult {
            crate::LpSolver::new().solve(problem)
        }
        fn name(&self) -> &str {
            "TestLP"
        }
    }

    #[cfg(feature = "lp")]
    #[test]
    fn test_independent_groups() {
        // Two independent groups, no MMs → same welfare as monolithic
        let mut problem = Problem::new("independent");
        let m0 = problem.markets.add_binary("A0");
        let m1 = problem.markets.add_binary("A1");
        let m2 = problem.markets.add_binary("B0");
        let m3 = problem.markets.add_binary("B1");

        let mut group_a = MarketGroup::new("GroupA");
        group_a.add_market(m0);
        group_a.add_market(m1);
        problem.add_market_group(group_a);

        let mut group_b = MarketGroup::new("GroupB");
        group_b.add_market(m2);
        group_b.add_market(m3);
        problem.add_market_group(group_b);

        // Group A orders
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, m0, 400_000_000, 100));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 2, m1, 350_000_000, 100));

        // Group B orders
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 3, m2, 400_000_000, 100));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 4, m3, 350_000_000, 100));

        // Monolithic solve
        let mono = crate::LpSolver::new().solve(&problem);

        // Decomposed solve
        let decomposed = DecomposedSolver::new(TestLpSolver);
        let decomp = decomposed.solve(&problem);

        // Welfare should match (within rounding)
        let diff = (mono.result.total_welfare() - decomp.result.total_welfare()).abs();
        assert!(
            diff <= NANOS_PER_DOLLAR as i64,
            "welfare should match: mono={}, decomp={}, diff={}",
            mono.result.total_welfare(),
            decomp.result.total_welfare(),
            diff
        );
    }

    #[cfg(feature = "lp")]
    #[test]
    fn test_single_component_fallback() {
        // All orders in one group → delegates directly
        let mut problem = Problem::new("single_comp");
        let m0 = problem.markets.add_binary("A");
        let m1 = problem.markets.add_binary("B");

        let mut group = MarketGroup::new("Group");
        group.add_market(m0);
        group.add_market(m1);
        problem.add_market_group(group);

        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, m0, 400_000_000, 100));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 2, m1, 350_000_000, 100));

        let decomposed = DecomposedSolver::new(TestLpSolver);
        let result = decomposed.solve(&problem);

        // Should produce fills (single component = direct delegation)
        assert!(
            result.result.total_welfare() >= 0,
            "single component should work, welfare={}",
            result.result.total_welfare()
        );
    }

    #[cfg(feature = "lp")]
    #[test]
    fn test_sub_problem_construction() {
        let mut problem = Problem::new("subproblem");
        let m0 = problem.markets.add_binary("A");
        let m1 = problem.markets.add_binary("B");

        let mut group_a = MarketGroup::new("GroupA");
        group_a.add_market(m0);
        problem.add_market_group(group_a);

        let mut group_b = MarketGroup::new("GroupB");
        group_b.add_market(m1);
        problem.add_market_group(group_b);

        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, m0, 400_000_000, 100));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 2, m1, 350_000_000, 100));

        let (market_to_comp, num_comp) = partition_markets(&problem);
        assert_eq!(num_comp, 2);
        assert_ne!(market_to_comp[&m0], market_to_comp[&m1]);

        let order_comps = assign_orders(&problem.orders, &market_to_comp);
        assert_eq!(order_comps[0], Some(market_to_comp[&m0]));
        assert_eq!(order_comps[1], Some(market_to_comp[&m1]));

        // Build sub-problem for component 0
        let mm_budgets = HashMap::new();
        let sub = build_sub_problem(&problem, &market_to_comp, 0, &order_comps, &mm_budgets);
        assert_eq!(sub.orders.len(), 1);
        assert_eq!(sub.orders[0].id, 1);
    }

    #[cfg(feature = "lp")]
    #[test]
    fn test_mm_budget_split() {
        // MM spanning two groups gets budget split
        let mut problem = Problem::new("mm_split");
        let m0 = problem.markets.add_binary("A");
        let m1 = problem.markets.add_binary("B");

        let mut group_a = MarketGroup::new("GroupA");
        group_a.add_market(m0);
        problem.add_market_group(group_a);

        let mut group_b = MarketGroup::new("GroupB");
        group_b.add_market(m1);
        problem.add_market_group(group_b);

        // YES buyers on each market
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, m0, 600_000_000, 500));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 2, m1, 600_000_000, 500));

        // MM NO buyers on each market, shared budget
        let mm_a = simple_no_buy(&problem.markets, 100, m0, 500_000_000, 1000);
        let mm_b = simple_no_buy(&problem.markets, 101, m1, 500_000_000, 1000);
        problem.orders.push(mm_a);
        problem.orders.push(mm_b);

        let mut mm = MmConstraint::new(MmId(1), Nanos(100 * NANOS_PER_DOLLAR));
        mm.add_order(100, MmSide::BuyNo);
        mm.add_order(101, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        let decomposed = DecomposedSolver::new(TestLpSolver);
        let result = decomposed.solve(&problem);

        assert!(result.result.orders_filled > 0, "should fill some orders");
        assert!(
            result.result.total_welfare() > 0,
            "should produce positive welfare"
        );
    }

    #[cfg(feature = "lp")]
    #[test]
    fn test_proportional_response_converges() {
        // Two groups with a spanning MM — proportional response should converge
        let mut problem = Problem::new("proportional_response");
        let m0 = problem.markets.add_binary("A");
        let m1 = problem.markets.add_binary("B");

        let mut group_a = MarketGroup::new("GroupA");
        group_a.add_market(m0);
        problem.add_market_group(group_a);

        let mut group_b = MarketGroup::new("GroupB");
        group_b.add_market(m1);
        problem.add_market_group(group_b);

        // More demand on market A than B
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, m0, 700_000_000, 500));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 2, m1, 550_000_000, 200));

        // MM provides NO liquidity on both markets
        let mm_a = simple_no_buy(&problem.markets, 100, m0, 500_000_000, 1000);
        let mm_b = simple_no_buy(&problem.markets, 101, m1, 500_000_000, 1000);
        problem.orders.push(mm_a);
        problem.orders.push(mm_b);

        let mut mm = MmConstraint::new(MmId(1), Nanos(200 * NANOS_PER_DOLLAR));
        mm.add_order(100, MmSide::BuyNo);
        mm.add_order(101, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        let decomposed = DecomposedSolver::new(TestLpSolver);
        let result = decomposed.solve(&problem);

        assert!(result.result.orders_filled > 0, "should produce fills");
        assert!(
            result.result.total_welfare() > 0,
            "should produce positive welfare"
        );
    }

    /// Asymmetric two-component instance where equal-*utility* budget
    /// coordination (the superseded surrogate) badly underperforms
    /// equal-*scarcity* proportional response.
    ///
    /// Asymmetric two-component coordination: the corrected rule targets *equal
    /// scarcity* (`B_k^m ∝ V_k^m`), which on an asymmetric book differs from the
    /// superseded *equal-utility* target — and proportional response reaches it.
    ///
    /// Deep component A: the MM buys NO at 0.80/share against its own
    /// high limit (0.90), saturating its 50-dollar share (no cash retained).
    /// Shallow component B: the MM can only place a small position, spending 10
    /// of its 50 and retaining 40 as cash. Deployed value `V = U + s` (weighted
    /// fill value + retained cash) is therefore very different across the two
    /// components even though both start with equal budget — so the equal-budget
    /// (≈ equal-utility) start is *not* equal scarcity. One proportional-response
    /// step reallocates budget to equalize `B_k^m / V_k^m`, the monolithic
    /// decomposition invariant (companion note, Theorem 1).
    ///
    /// We assert the coordination invariant directly on the changed functions.
    /// An end-to-end LP welfare delta is deliberately *not* asserted: the global
    /// budget-trim safety net (`trim_mm_budget_overflows`) re-caps total MM spend
    /// after aggregation, so on single-market components final welfare is nearly
    /// insensitive to the split — which is exactly why the superseded surrogate
    /// still scored ~93% on symmetric benchmarks.
    #[cfg(feature = "lp")]
    #[test]
    fn test_asymmetric_equal_scarcity_coordination() {
        use matching_engine::{Fill, MmId, Qty};

        const DOLLAR: u64 = NANOS_PER_DOLLAR;

        // Two single-market groups; one MM buying NO on both at limit 0.90.
        let mut problem = Problem::new("asymmetric_coord");
        let m0 = problem.markets.add_binary("A");
        let m1 = problem.markets.add_binary("B");
        let mut group_a = MarketGroup::new("GroupA");
        group_a.add_market(m0);
        problem.add_market_group(group_a);
        let mut group_b = MarketGroup::new("GroupB");
        group_b.add_market(m1);
        problem.add_market_group(group_b);
        problem.orders.push(simple_no_buy(
            &problem.markets,
            100,
            m0,
            900_000_000,
            1_000_000,
        ));
        problem.orders.push(simple_no_buy(
            &problem.markets,
            101,
            m1,
            900_000_000,
            1_000_000,
        ));
        let mut mm = MmConstraint::new(MmId(1), Nanos(100 * DOLLAR));
        mm.add_order(100, MmSide::BuyNo);
        mm.add_order(101, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        let (m2c, _nc) = partition_markets(&problem);
        let order_components = assign_orders(&problem.orders, &m2c);
        let comp_a = m2c[&m0];
        let comp_b = m2c[&m1];
        let spanning: Vec<(usize, Vec<usize>)> = {
            let mut comps = vec![comp_a, comp_b];
            comps.sort();
            vec![(0, comps)]
        };

        // Synthetic component fills. A buyer pays the traded outcome's price.
        //   A: 250 shares @ fill 0.80  → spend exceeds 50 (retained cash floors at 0)
        //   B:  50 shares @ fill 0.80  → spend 40 (10 retained)
        let mut res_a = PipelineResult::empty();
        res_a
            .result
            .fills
            .push(Fill::new(100, Qty(250_000), Nanos(800_000_000)));
        let mut res_b = PipelineResult::empty();
        res_b
            .result
            .fills
            .push(Fill::new(101, Qty(50_000), Nanos(800_000_000)));
        let mut results = vec![PipelineResult::empty(), PipelineResult::empty()];
        results[comp_a] = res_a;
        results[comp_b] = res_b;

        // Equal 50/50 budget split.
        let mut budgets: HashMap<usize, HashMap<usize, u64>> = HashMap::new();
        budgets.entry(0).or_default().insert(comp_a, 50 * DOLLAR);
        budgets.entry(0).or_default().insert(comp_b, 50 * DOLLAR);

        let deployed =
            compute_mm_deployed_values(&problem, &results, &order_components, &budgets, &spanning);

        // V = U + s, in nanos.  U = 0.90·qty, s = budget − spend.
        //   V_A = 0.90·250 + 0 = 225
        //   V_B = 0.90·50  + (50 − 40) = 45 + 10 = 55
        let v_a = deployed[&(0, comp_a)];
        let v_b = deployed[&(0, comp_b)];
        assert_eq!(v_a as u64, 225 * DOLLAR, "V_A = U_A + s_A");
        assert_eq!(v_b as u64, 55 * DOLLAR, "V_B = U_B + s_B");

        // The equal-budget start is NOT equal scarcity: B/V differs (0.22 vs 0.91).
        assert!(
            !check_convergence(&deployed, &budgets, &spanning, 1e-4),
            "equal budget is not equal scarcity on an asymmetric book"
        );

        // One proportional-response step: B_k^m ← 100·V_k^m / (V_A + V_B).
        let values = vec![(comp_a, v_a), (comp_b, v_b)];
        for (comp, budget) in reallocate_proportional(100 * DOLLAR, &values) {
            budgets.entry(0).or_default().insert(comp, budget);
        }

        // Now scarcity is equalized (both ≈ 100 / 280 = 0.3571) → converged.
        let new_a = budgets[&0][&comp_a] as f64 / v_a;
        let new_b = budgets[&0][&comp_b] as f64 / v_b;
        assert!(
            (new_a.ln() - new_b.ln()).abs() < 1e-3,
            "proportional response equalizes scarcity: {new_a} vs {new_b}"
        );
        assert!(
            check_convergence(&deployed, &budgets, &spanning, 1e-3),
            "post-reallocation allocation is at the equal-scarcity fixed point"
        );
        // Budget moved toward the high-deployed-value component A (225 vs 85).
        assert!(
            budgets[&0][&comp_a] > 70 * DOLLAR,
            "budget flows to deployed value: B_A = {}",
            budgets[&0][&comp_a]
        );
    }

    #[cfg(feature = "lp")]
    #[test]
    fn test_cross_group_orders_dropped() {
        // Order spanning two groups should be excluded
        let mut problem = Problem::new("cross_group");
        let m0 = problem.markets.add_binary("A");
        let m1 = problem.markets.add_binary("B");

        let mut group_a = MarketGroup::new("GroupA");
        group_a.add_market(m0);
        problem.add_market_group(group_a);

        let mut group_b = MarketGroup::new("GroupB");
        group_b.add_market(m1);
        problem.add_market_group(group_b);

        // Single-market orders
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, m0, 600_000_000, 100));
        problem
            .orders
            .push(simple_no_buy(&problem.markets, 2, m0, 500_000_000, 100));

        // Cross-group bundle order (spans m0 and m1)
        problem.orders.push(matching_engine::bundle_yes(
            &problem.markets,
            3,
            &[m0, m1],
            400_000_000,
            100,
        ));

        let (market_to_comp, _) = partition_markets(&problem);
        let order_comps = assign_orders(&problem.orders, &market_to_comp);

        // First two orders should be assigned
        assert!(order_comps[0].is_some());
        assert!(order_comps[1].is_some());
        // Bundle spanning groups should be dropped
        assert!(order_comps[2].is_none(), "cross-group order should be None");

        // Full solve should still work
        let decomposed = DecomposedSolver::new(TestLpSolver);
        let result = decomposed.solve(&problem);

        // Should fill the single-market pair at least
        assert!(result.result.orders_filled > 0);
    }

    #[cfg(feature = "lp")]
    #[test]
    fn test_empty_problem() {
        let problem = Problem::new("empty");
        let decomposed = DecomposedSolver::new(TestLpSolver);
        let result = decomposed.solve(&problem);
        assert_eq!(result.result.orders_filled, 0);
    }

    #[cfg(feature = "lp")]
    #[test]
    fn test_standalone_markets() {
        // Markets not in any group each get their own component
        let mut problem = Problem::new("standalone");
        let m0 = problem.markets.add_binary("A");
        let m1 = problem.markets.add_binary("B");
        // No market groups

        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, m0, 600_000_000, 100));
        problem
            .orders
            .push(simple_no_buy(&problem.markets, 2, m0, 500_000_000, 100));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 3, m1, 600_000_000, 50));
        problem
            .orders
            .push(simple_no_buy(&problem.markets, 4, m1, 500_000_000, 50));

        let decomposed = DecomposedSolver::new(TestLpSolver);
        let result = decomposed.solve(&problem);

        assert!(
            result.result.orders_filled > 0,
            "should fill standalone market orders"
        );
    }
}
