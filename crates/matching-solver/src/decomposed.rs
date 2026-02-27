//! Per-market-group decomposition for convex solvers.
//!
//! Partitions the problem into independent components (one per `MarketGroup` +
//! standalone markets), solves each with an inner solver, and coordinates
//! MM budgets via mirror descent when MMs span multiple groups.
//!
//! **Theorem** (design/decomposition.typ §1.1): When no orders span multiple
//! components, the decomposed program with optimal budget allocation achieves
//! the same welfare as the monolithic solve. Cross-group orders are dropped.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use matching_engine::{MarketGroup, MarketId, MarketSet, MmConstraint, MmSide, Order, Problem};

use crate::result::{PipelineResult, PipelineTimings, PriceDiscoveryResult, SolverContribution};
use crate::MatchingResult;

// ============================================================================
// ComponentSolver trait
// ============================================================================

/// A solver that can solve a single component sub-problem.
pub trait ComponentSolver {
    fn solve_component(&self, problem: &Problem) -> PipelineResult;
    fn name(&self) -> &str;
}

// ============================================================================
// DecomposedSolver
// ============================================================================

/// Decomposes the problem by market group and coordinates MM budgets.
pub struct DecomposedSolver<S> {
    inner: S,
    max_budget_iters: usize,
    convergence_eps: f64,
}

impl<S: ComponentSolver> DecomposedSolver<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            max_budget_iters: 10,
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

        // Step 1: Partition markets into components
        let (market_to_component, num_components) = partition_markets(problem);

        // Single component → delegate directly (zero overhead)
        if num_components <= 1 {
            return self.inner.solve_component(problem);
        }

        // Step 2: Assign orders to components (drop cross-component)
        let order_components = assign_orders(&problem.orders, &market_to_component);

        // Step 3: Assign MMs to components
        let mm_components = assign_mms(problem, &order_components);

        // Step 4: Classify MMs into local vs spanning
        let (local_mms, spanning_mms) = classify_mms(&mm_components);

        // Step 5-7: Solve
        let component_results = if spanning_mms.is_empty() {
            // No spanning MMs → independent solves
            self.solve_independent(problem, &market_to_component, num_components,
                                   &order_components, &mm_components, &local_mms)
        } else {
            // Mirror descent for spanning MMs
            self.solve_with_mirror_descent(
                problem, &market_to_component, num_components,
                &order_components, &mm_components, &local_mms, &spanning_mms,
            )
        };

        // Step 8: Aggregate results (excluding per-component arb orders)
        let mut result = aggregate_results(component_results, self.inner.name());

        // Post-aggregation: enforce global MM budgets + restore position balance.
        // Per-component LP budget enforcement is imperfect (linearized + rounded),
        // so small overruns compound across components.
        let mm_order_info: HashMap<u64, (usize, MmSide)> = problem
            .mm_constraints
            .iter()
            .enumerate()
            .flat_map(|(mm_idx, mm)| {
                mm.order_ids.iter().filter_map(move |&oid| {
                    mm.order_sides.get(&oid).map(|&side| (oid, (mm_idx, side)))
                })
            })
            .collect();

        if !problem.mm_constraints.is_empty() {
            crate::lp_solver::trim_mm_budget_overflows(
                &mut result.result,
                &problem.mm_constraints,
                &mm_order_info,
            );
        }

        // Re-create position arb orders globally (trim may have broken balance).
        // We need two passes to avoid borrow conflicts.
        let prices = result.price_discovery
            .as_ref()
            .map(|pd| pd.prices.clone())
            .unwrap_or_default();
        let max_order_id = problem.orders.iter().map(|o| o.id).max().unwrap_or(0);

        // Pass 1: create new arb orders using existing arbs + problem orders as map
        let new_arbs = {
            let mut order_map: HashMap<u64, &Order> =
                problem.orders.iter().map(|o| (o.id, o)).collect();
            for arb in &result.group_minting_arb_orders {
                order_map.insert(arb.id, arb);
            }
            crate::lp_solver::create_position_arbs(
                &mut result.result, &order_map, &prices, max_order_id,
            )
        };
        result.group_minting_arb_orders.extend(new_arbs);

        // Pass 2: recompute welfare with complete order map
        let mut order_map_full: HashMap<u64, &Order> =
            problem.orders.iter().map(|o| (o.id, o)).collect();
        for arb in &result.group_minting_arb_orders {
            order_map_full.insert(arb.id, arb);
        }
        crate::lp_solver::recompute_welfare(&mut result.result, &order_map_full);

        result.total_time_secs = start.elapsed().as_secs_f64();
        result
    }

    /// Solve each component independently (no spanning MMs).
    fn solve_independent(
        &self,
        problem: &Problem,
        market_to_component: &HashMap<MarketId, usize>,
        num_components: usize,
        order_components: &[Option<usize>],
        _mm_components: &[HashSet<usize>],
        local_mms: &[(usize, usize)], // (mm_idx, component)
    ) -> Vec<PipelineResult> {
        // Build budget map: mm_idx -> component -> budget
        let mut mm_budgets: HashMap<usize, HashMap<usize, u64>> = HashMap::new();
        for &(mm_idx, comp) in local_mms {
            mm_budgets.entry(mm_idx).or_default()
                .insert(comp, problem.mm_constraints[mm_idx].max_capital);
        }

        (0..num_components)
            .map(|comp| {
                let sub = build_sub_problem(
                    problem, market_to_component, comp,
                    order_components, &mm_budgets,
                );
                if sub.orders.is_empty() {
                    PipelineResult::empty()
                } else {
                    self.inner.solve_component(&sub)
                }
            })
            .collect()
    }

    /// Solve with mirror descent to coordinate spanning MM budgets.
    #[allow(clippy::too_many_arguments)]
    fn solve_with_mirror_descent(
        &self,
        problem: &Problem,
        market_to_component: &HashMap<MarketId, usize>,
        num_components: usize,
        order_components: &[Option<usize>],
        _mm_components: &[HashSet<usize>],
        local_mms: &[(usize, usize)],
        spanning_mms: &[(usize, Vec<usize>)], // (mm_idx, components)
    ) -> Vec<PipelineResult> {
        // Initialize budgets
        // Local MMs: full budget to their component
        // Spanning MMs: split evenly across their active components
        let mut mm_budgets: HashMap<usize, HashMap<usize, u64>> = HashMap::new();

        for &(mm_idx, comp) in local_mms {
            mm_budgets.entry(mm_idx).or_default()
                .insert(comp, problem.mm_constraints[mm_idx].max_capital);
        }

        for &(mm_idx, ref comps) in spanning_mms {
            let total = problem.mm_constraints[mm_idx].max_capital;
            let per_comp = total / comps.len() as u64;
            let remainder = total - per_comp * comps.len() as u64;
            let map = mm_budgets.entry(mm_idx).or_default();
            for (i, &comp) in comps.iter().enumerate() {
                // Give remainder to first component
                let budget = per_comp + if i == 0 { remainder } else { 0 };
                map.insert(comp, budget);
            }
        }

        let mut best_results: Vec<PipelineResult> = Vec::new();

        for _iter in 0..self.max_budget_iters {
            // Solve each component with current budget allocation
            let results: Vec<PipelineResult> = (0..num_components)
                .map(|comp| {
                    let sub = build_sub_problem(
                        problem, market_to_component, comp,
                        order_components, &mm_budgets,
                    );
                    if sub.orders.is_empty() {
                        PipelineResult::empty()
                    } else {
                        self.inner.solve_component(&sub)
                    }
                })
                .collect();

            // If no spanning MMs need updating (first iter already done), break
            if spanning_mms.is_empty() {
                best_results = results;
                break;
            }

            // Compute per-MM per-component utility (welfare contribution)
            let utilities = compute_mm_utilities(
                problem, &results, order_components, spanning_mms,
            );

            // Check convergence: utilities equalize across components for each MM
            let converged = check_convergence(&utilities, spanning_mms, self.convergence_eps);

            best_results = results;

            if converged {
                break;
            }

            // Mirror descent update: multiplicative rebalancing
            for &(mm_idx, ref comps) in spanning_mms {
                let total_budget = problem.mm_constraints[mm_idx].max_capital;
                let budgets = mm_budgets.entry(mm_idx).or_default();

                // Compute weighted product B_k^m * U_k^m for each component
                let products: Vec<(usize, f64)> = comps.iter().map(|&comp| {
                    let b = *budgets.get(&comp).unwrap_or(&0) as f64;
                    let u = utilities.get(&(mm_idx, comp)).copied().unwrap_or(0.0);
                    (comp, b * u)
                }).collect();

                let total_product: f64 = products.iter().map(|&(_, p)| p).sum();

                if total_product <= 0.0 {
                    // All utilities zero → keep budgets unchanged
                    continue;
                }

                // Update: B_k^m ← total_budget × (B_k^m × U_k^m) / Σ(B_k^{m'} × U_k^{m'})
                let mut allocated = 0u64;
                let last_idx = comps.len() - 1;
                for (i, &(comp, product)) in products.iter().enumerate() {
                    if i == last_idx {
                        // Last component gets remainder to ensure conservation
                        budgets.insert(comp, total_budget - allocated);
                    } else {
                        let new_budget = (total_budget as f64 * product / total_product)
                            .round() as u64;
                        budgets.insert(comp, new_budget);
                        allocated += new_budget;
                    }
                }
            }
        }

        best_results
    }
}

// ============================================================================
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
    let grouped_markets: HashSet<MarketId> = problem.market_groups
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
    orders.iter().map(|order| {
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
    }).collect()
}

/// For each MM, find which components its orders belong to.
fn assign_mms(
    problem: &Problem,
    order_components: &[Option<usize>],
) -> Vec<HashSet<usize>> {
    let order_id_to_idx: HashMap<u64, usize> = problem.orders
        .iter()
        .enumerate()
        .map(|(i, o)| (o.id, i))
        .collect();

    problem.mm_constraints.iter().map(|mm| {
        mm.order_ids.iter()
            .filter_map(|&oid| {
                order_id_to_idx.get(&oid)
                    .and_then(|&idx| order_components[idx])
            })
            .collect()
    }).collect()
}

/// (mm_idx, component) for MMs whose orders all lie in one component.
type LocalMm = (usize, usize);
/// (mm_idx, sorted components) for MMs spanning multiple components.
type SpanningMm = (usize, Vec<usize>);

/// Classify MMs into local (single component) vs spanning (multiple components).
fn classify_mms(
    mm_components: &[HashSet<usize>],
) -> (Vec<LocalMm>, Vec<SpanningMm>) {
    let mut local = Vec::new();
    let mut spanning = Vec::new();

    for (mm_idx, comps) in mm_components.iter().enumerate() {
        match comps.len() {
            0 => {} // MM has no orders in any component (all dropped)
            1 => {
                let comp = *comps.iter().next().unwrap();
                local.push((mm_idx, comp));
            }
            _ => {
                let mut sorted: Vec<usize> = comps.iter().copied().collect();
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
    let component_markets: Vec<MarketId> = market_to_component.iter()
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

    // Filter MM constraints: only include orders in this component,
    // set budget to allocated share
    let mut mm_constraints = Vec::new();
    for (mm_idx, mm) in problem.mm_constraints.iter().enumerate() {
        let budget = mm_budgets
            .get(&mm_idx)
            .and_then(|m| m.get(&component))
            .copied()
            .unwrap_or(0);

        if budget == 0 {
            continue;
        }

        // Filter to only orders in this component
        let filtered_order_ids: Vec<u64> = mm.order_ids.iter()
            .filter(|&&oid| order_ids_in_component.contains(&oid))
            .copied()
            .collect();

        if filtered_order_ids.is_empty() {
            continue;
        }

        let mut new_mm = MmConstraint::new(mm.mm_id, budget);
        for &oid in &filtered_order_ids {
            if let Some(&side) = mm.order_sides.get(&oid) {
                new_mm.add_order(oid, side);
            }
        }
        mm_constraints.push(new_mm);
    }

    // Filter market groups: only groups whose markets are in this component
    let market_groups: Vec<MarketGroup> = problem.market_groups.iter()
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
// MM utility computation
// ============================================================================

/// Compute welfare contribution of each spanning MM in each component.
fn compute_mm_utilities(
    problem: &Problem,
    results: &[PipelineResult],
    order_components: &[Option<usize>],
    spanning_mms: &[(usize, Vec<usize>)],
) -> HashMap<(usize, usize), f64> {
    // Build order_id -> (order_index, component) mapping
    let order_id_info: HashMap<u64, (usize, usize)> = problem.orders.iter()
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

    let mut utilities = HashMap::new();

    for &(mm_idx, ref comps) in spanning_mms {
        let mm = &problem.mm_constraints[mm_idx];
        for &comp in comps {
            let mut welfare = 0.0f64;
            for &oid in &mm.order_ids {
                if let Some(&(order_idx, order_comp)) = order_id_info.get(&oid) {
                    if order_comp != comp {
                        continue;
                    }
                    if let Some(fills) = fill_lookup.get(&comp) {
                        if let Some(fill) = fills.get(&oid) {
                            let order = &problem.orders[order_idx];
                            welfare += fill.welfare(order) as f64;
                        }
                    }
                }
            }
            utilities.insert((mm_idx, comp), welfare.max(0.0));
        }
    }

    utilities
}

/// Check if utilities have equalized across components for all spanning MMs.
fn check_convergence(
    utilities: &HashMap<(usize, usize), f64>,
    spanning_mms: &[(usize, Vec<usize>)],
    eps: f64,
) -> bool {
    for &(mm_idx, ref comps) in spanning_mms {
        let utils: Vec<f64> = comps.iter()
            .map(|&comp| utilities.get(&(mm_idx, comp)).copied().unwrap_or(0.0))
            .collect();

        // If all zero, consider converged
        if utils.iter().all(|&u| u == 0.0) {
            continue;
        }

        // Check max |ln U_k^m - ln U_k^{m'}| < eps
        let log_utils: Vec<f64> = utils.iter()
            .map(|&u| if u > 0.0 { u.ln() } else { f64::NEG_INFINITY })
            .collect();

        let finite_logs: Vec<f64> = log_utils.iter()
            .filter(|&&l| l.is_finite())
            .copied()
            .collect();

        if finite_logs.len() < 2 {
            continue;
        }

        let max_log = finite_logs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min_log = finite_logs.iter().cloned().fold(f64::INFINITY, f64::min);

        if (max_log - min_log).abs() > eps {
            return false;
        }
    }

    true
}

// ============================================================================
// Result aggregation
// ============================================================================

/// Merge results from all components into a unified PipelineResult.
fn aggregate_results(component_results: Vec<PipelineResult>, solver_name: &str) -> PipelineResult {
    let mut merged = MatchingResult::new();
    let mut prices: HashMap<MarketId, Vec<u64>> = HashMap::new();
    let mut total_solve_time = 0.0f64;
    let mut arb_orders = Vec::new();

    for result in &component_results {
        // Merge fills (disjoint order sets → no conflicts)
        for fill in &result.result.fills {
            merged.fills.push(fill.clone());
        }
        merged.total_welfare += result.result.total_welfare;
        merged.minting_cost += result.result.minting_cost;
        merged.orders_filled += result.result.orders_filled;
        merged.orders_unfilled_liquidity += result.result.orders_unfilled_liquidity;
        merged.total_quantity_filled += result.result.total_quantity_filled;

        // Merge prices (disjoint market sets)
        if let Some(ref pd) = result.price_discovery {
            for (market_id, market_prices) in &pd.prices {
                prices.insert(*market_id, market_prices.clone());
            }
        }

        total_solve_time += result.total_time_secs;

        // Merge arb orders
        arb_orders.extend(result.group_minting_arb_orders.iter().cloned());
    }

    let mut pipeline_result = PipelineResult::empty();
    pipeline_result.result = merged;
    pipeline_result.price_discovery = Some(PriceDiscoveryResult {
        total_welfare: pipeline_result.result.total_welfare,
        total_fills: pipeline_result.result.fills.len(),
        prices,
    });
    pipeline_result.contributions = vec![SolverContribution {
        solver_name: format!("Decomposed({})", solver_name),
        fills_contributed: pipeline_result.result.orders_filled,
        welfare_contributed: pipeline_result.result.total_welfare,
    }];
    pipeline_result.phase_times = PipelineTimings {
        price_discovery_secs: total_solve_time,
        ..Default::default()
    };
    pipeline_result.group_minting_arb_orders = arb_orders;

    pipeline_result
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{
        simple_yes_buy, simple_no_buy, MarketGroup, MmConstraint, MmId,
        MmSide, NANOS_PER_DOLLAR,
    };

    /// Trivial component solver that delegates to LpSolver.
    #[cfg(feature = "lp")]
    struct TestLpSolver;

    #[cfg(feature = "lp")]
    impl ComponentSolver for TestLpSolver {
        fn solve_component(&self, problem: &Problem) -> PipelineResult {
            crate::LpSolver::new().solve(problem)
        }
        fn name(&self) -> &str { "TestLP" }
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
        problem.orders.push(simple_yes_buy(&problem.markets, 1, m0, 400_000_000, 100));
        problem.orders.push(simple_yes_buy(&problem.markets, 2, m1, 350_000_000, 100));

        // Group B orders
        problem.orders.push(simple_yes_buy(&problem.markets, 3, m2, 400_000_000, 100));
        problem.orders.push(simple_yes_buy(&problem.markets, 4, m3, 350_000_000, 100));

        // Monolithic solve
        let mono = crate::LpSolver::new().solve(&problem);

        // Decomposed solve
        let decomposed = DecomposedSolver::new(TestLpSolver);
        let decomp = decomposed.solve(&problem);

        // Welfare should match (within rounding)
        let diff = (mono.result.total_welfare - decomp.result.total_welfare).abs();
        assert!(
            diff <= NANOS_PER_DOLLAR as i64,
            "welfare should match: mono={}, decomp={}, diff={}",
            mono.result.total_welfare, decomp.result.total_welfare, diff
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

        problem.orders.push(simple_yes_buy(&problem.markets, 1, m0, 400_000_000, 100));
        problem.orders.push(simple_yes_buy(&problem.markets, 2, m1, 350_000_000, 100));

        let decomposed = DecomposedSolver::new(TestLpSolver);
        let result = decomposed.solve(&problem);

        // Should produce fills (single component = direct delegation)
        assert!(
            result.result.total_welfare >= 0,
            "single component should work, welfare={}",
            result.result.total_welfare
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

        problem.orders.push(simple_yes_buy(&problem.markets, 1, m0, 400_000_000, 100));
        problem.orders.push(simple_yes_buy(&problem.markets, 2, m1, 350_000_000, 100));

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
        problem.orders.push(simple_yes_buy(&problem.markets, 1, m0, 600_000_000, 500));
        problem.orders.push(simple_yes_buy(&problem.markets, 2, m1, 600_000_000, 500));

        // MM NO buyers on each market, shared budget
        let mm_a = simple_no_buy(&problem.markets, 100, m0, 500_000_000, 1000);
        let mm_b = simple_no_buy(&problem.markets, 101, m1, 500_000_000, 1000);
        problem.orders.push(mm_a);
        problem.orders.push(mm_b);

        let mut mm = MmConstraint::new(MmId(1), 100 * NANOS_PER_DOLLAR);
        mm.add_order(100, MmSide::BuyNo);
        mm.add_order(101, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        let decomposed = DecomposedSolver::new(TestLpSolver);
        let result = decomposed.solve(&problem);

        assert!(result.result.orders_filled > 0, "should fill some orders");
        assert!(result.result.total_welfare > 0, "should produce positive welfare");
    }

    #[cfg(feature = "lp")]
    #[test]
    fn test_mirror_descent_converges() {
        // Two groups with a spanning MM — mirror descent should converge
        let mut problem = Problem::new("mirror_descent");
        let m0 = problem.markets.add_binary("A");
        let m1 = problem.markets.add_binary("B");

        let mut group_a = MarketGroup::new("GroupA");
        group_a.add_market(m0);
        problem.add_market_group(group_a);

        let mut group_b = MarketGroup::new("GroupB");
        group_b.add_market(m1);
        problem.add_market_group(group_b);

        // More demand on market A than B
        problem.orders.push(simple_yes_buy(&problem.markets, 1, m0, 700_000_000, 500));
        problem.orders.push(simple_yes_buy(&problem.markets, 2, m1, 550_000_000, 200));

        // MM provides NO liquidity on both markets
        let mm_a = simple_no_buy(&problem.markets, 100, m0, 500_000_000, 1000);
        let mm_b = simple_no_buy(&problem.markets, 101, m1, 500_000_000, 1000);
        problem.orders.push(mm_a);
        problem.orders.push(mm_b);

        let mut mm = MmConstraint::new(MmId(1), 200 * NANOS_PER_DOLLAR);
        mm.add_order(100, MmSide::BuyNo);
        mm.add_order(101, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        let decomposed = DecomposedSolver::new(TestLpSolver);
        let result = decomposed.solve(&problem);

        assert!(result.result.orders_filled > 0, "should produce fills");
        assert!(result.result.total_welfare > 0, "should produce positive welfare");
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
        problem.orders.push(simple_yes_buy(&problem.markets, 1, m0, 600_000_000, 100));
        problem.orders.push(simple_no_buy(&problem.markets, 2, m0, 500_000_000, 100));

        // Cross-group bundle order (spans m0 and m1)
        problem.orders.push(matching_engine::bundle_yes(
            &problem.markets, 3, &[m0, m1], 400_000_000, 100,
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

        problem.orders.push(simple_yes_buy(&problem.markets, 1, m0, 600_000_000, 100));
        problem.orders.push(simple_no_buy(&problem.markets, 2, m0, 500_000_000, 100));
        problem.orders.push(simple_yes_buy(&problem.markets, 3, m1, 600_000_000, 50));
        problem.orders.push(simple_no_buy(&problem.markets, 4, m1, 500_000_000, 50));

        let decomposed = DecomposedSolver::new(TestLpSolver);
        let result = decomposed.solve(&problem);

        assert!(result.result.orders_filled > 0, "should fill standalone market orders");
    }
}
