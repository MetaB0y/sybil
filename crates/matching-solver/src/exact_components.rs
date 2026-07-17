//! Exact decomposition by economic connectivity.
//!
//! Two markets belong to the same optimization component when they are joined
//! by a categorical market group, one order touches or conditions on both, or
//! one MM budget covers orders on both. The retained-cash objective and
//! matching constraints are additive across the resulting connected
//! components, so each component can be solved independently without splitting
//! budgets or dropping orders.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use matching_engine::{MarketGroup, MarketId, MarketSet, MmConstraint, Problem};

use crate::decomposed::assemble_final;
use crate::result::{PipelineResult, SolverDiagnostics, TerminationStatus};

/// Compact topology summary used by solver experiments and routing decisions.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExactComponentStats {
    pub components: usize,
    pub largest_markets: usize,
    pub largest_orders: usize,
    pub largest_mms: usize,
}

/// Compute the exact MM/order/market connectivity of a problem.
pub fn exact_component_stats(problem: &Problem) -> ExactComponentStats {
    ComponentPartition::new(problem)
        .map(|partition| partition.stats)
        .unwrap_or_default()
}

/// Route balanced, exactly independent liquidity components through the same
/// inner solver. Connected and strongly unbalanced books delegate directly.
pub struct ExactComponentSolver<S: crate::Solver> {
    inner: S,
}

struct ComponentRun {
    result: PipelineResult,
    max_fill: f64,
}

impl<S: crate::Solver> ExactComponentSolver<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }

    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();
        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        let Some(partition) = ComponentPartition::new(problem) else {
            return self.inner.solve(problem);
        };
        // Multiple solver setup/landing phases are worthwhile only when the
        // split removes a material amount of work. A tiny detached tail made
        // lifecycle replay slower despite exact separability; requiring the
        // largest component to contain at most 80% of orders keeps the wrapper
        // a zero-overhead delegation on such unbalanced books.
        let split_is_balanced = partition.stats.largest_orders.saturating_mul(5)
            <= problem.orders.len().saturating_mul(4);
        if partition.num_components <= 1 || !split_is_balanced {
            return self.inner.solve(problem);
        }

        let solve_one = |component| {
            let subproblem = partition.subproblem(problem, component);
            (!subproblem.orders.is_empty()).then(|| ComponentRun {
                max_fill: subproblem
                    .orders
                    .iter()
                    .map(|order| order.max_fill.0 as f64)
                    .sum(),
                result: self.inner.solve(&subproblem),
            })
        };

        #[cfg(feature = "parallel")]
        let component_results: Vec<_> = (0..partition.num_components)
            .into_par_iter()
            .filter_map(solve_one)
            .collect();

        #[cfg(not(feature = "parallel"))]
        let component_results: Vec<_> = (0..partition.num_components)
            .filter_map(solve_one)
            .collect();

        let failures = component_results
            .iter()
            .filter(|run| is_failure(&run.result.diagnostics.status))
            .count();
        let caps = component_results
            .iter()
            .filter(|run| is_cap(&run.result.diagnostics.status))
            .count();
        let delegated = component_results
            .iter()
            .all(|run| run.result.diagnostics.status == TerminationStatus::Delegated);
        let objective_value = sum_diagnostics(&component_results, |result| {
            result.diagnostics.objective_value
        });
        let iterations =
            sum_usize_diagnostics(&component_results, |result| result.diagnostics.iterations);
        let optimality_gap = sum_diagnostics(&component_results, |result| {
            result.diagnostics.optimality_gap
        });
        let oracle_calls =
            sum_usize_diagnostics(&component_results, |result| result.diagnostics.oracle_calls);
        let master_iterations = sum_usize_diagnostics(&component_results, |result| {
            result.diagnostics.master_iterations
        });
        let oracle_time_secs = sum_diagnostics(&component_results, |result| {
            result.diagnostics.oracle_time_secs
        });
        let master_time_secs = sum_diagnostics(&component_results, |result| {
            result.diagnostics.master_time_secs
        });
        let integer_landing_loss = sum_diagnostics(&component_results, |result| {
            result.diagnostics.integer_landing_loss
        });
        let integer_landing_l1_ratio = weighted_landing_l1(&component_results);
        let integer_landing_budget_trimmed = any_diagnostics(&component_results, |result| {
            result.diagnostics.integer_landing_budget_trimmed
        });
        let active_atoms =
            sum_usize_diagnostics(&component_results, |result| result.diagnostics.active_atoms);
        let convergence_metric = max_diagnostics(&component_results, |result| {
            result.diagnostics.convergence_metric
        });

        let solved_components = component_results.len();
        let mut result = assemble_final(
            problem,
            component_results
                .into_iter()
                .map(|run| run.result)
                .collect(),
        );
        result.total_time_secs = start.elapsed().as_secs_f64();
        result.diagnostics = SolverDiagnostics {
            algorithm: format!("exact-components-{}", self.inner.name().to_lowercase()),
            status: if failures > 0 {
                TerminationStatus::NumericalFailure
            } else if caps > 0 {
                TerminationStatus::IterationLimit
            } else if delegated {
                TerminationStatus::Delegated
            } else {
                TerminationStatus::Converged
            },
            iterations,
            convergence_metric,
            objective_value,
            optimality_gap,
            oracle_calls,
            master_iterations,
            active_atoms,
            oracle_time_secs,
            master_time_secs,
            integer_landing_loss,
            integer_landing_l1_ratio,
            integer_landing_budget_trimmed,
            message: Some(format!(
                "{solved_components}/{} nonempty exact components; largest has {} markets, {} orders, {} MMs; {failures} failures, {caps} caps",
                partition.num_components,
                partition.stats.largest_markets,
                partition.stats.largest_orders,
                partition.stats.largest_mms,
            )),
            ..Default::default()
        };
        result
    }
}

impl<S: crate::Solver> crate::Solver for ExactComponentSolver<S> {
    fn solve(&self, problem: &Problem) -> PipelineResult {
        ExactComponentSolver::solve(self, problem)
    }

    fn name(&self) -> &str {
        "ExactComponents"
    }
}

fn is_failure(status: &TerminationStatus) -> bool {
    matches!(
        status,
        TerminationStatus::UnsupportedInput
            | TerminationStatus::NumericalFailure
            | TerminationStatus::PostProcessingFailure
            | TerminationStatus::Infeasible
    )
}

fn is_cap(status: &TerminationStatus) -> bool {
    matches!(
        status,
        TerminationStatus::IterationLimit | TerminationStatus::TimeLimit
    )
}

fn sum_diagnostics(
    results: &[ComponentRun],
    get: impl Fn(&PipelineResult) -> Option<f64>,
) -> Option<f64> {
    results
        .iter()
        .map(|run| get(&run.result))
        .try_fold(0.0, |total, value| value.map(|value| total + value))
}

fn sum_usize_diagnostics(
    results: &[ComponentRun],
    get: impl Fn(&PipelineResult) -> Option<usize>,
) -> Option<usize> {
    results
        .iter()
        .map(|run| get(&run.result))
        .try_fold(0usize, |total, value| value.map(|value| total + value))
}

fn max_diagnostics(
    results: &[ComponentRun],
    get: impl Fn(&PipelineResult) -> Option<f64>,
) -> Option<f64> {
    results
        .iter()
        .map(|run| get(&run.result))
        .try_fold(0.0_f64, |maximum, value| {
            value.map(|value| maximum.max(value))
        })
}

fn any_diagnostics(
    results: &[ComponentRun],
    get: impl Fn(&PipelineResult) -> Option<bool>,
) -> Option<bool> {
    results
        .iter()
        .map(|run| get(&run.result))
        .try_fold(false, |any, value| value.map(|value| any || value))
}

fn weighted_landing_l1(results: &[ComponentRun]) -> Option<f64> {
    let total_weight = results.iter().map(|run| run.max_fill).sum::<f64>();
    if total_weight <= 0.0 {
        return None;
    }
    results
        .iter()
        .map(|run| {
            run.result
                .diagnostics
                .integer_landing_l1_ratio
                .map(|ratio| ratio * run.max_fill)
        })
        .try_fold(0.0, |total, value| value.map(|value| total + value))
        .map(|weighted| weighted / total_weight)
}

struct ComponentPartition {
    market_component: HashMap<MarketId, usize>,
    order_component: Vec<usize>,
    mm_component: Vec<Option<usize>>,
    num_components: usize,
    stats: ExactComponentStats,
}

impl ComponentPartition {
    fn new(problem: &Problem) -> Option<Self> {
        let markets: Vec<_> = problem.markets.iter().map(|market| market.id).collect();
        if markets.is_empty() {
            return None;
        }
        let market_index: HashMap<_, _> = markets
            .iter()
            .enumerate()
            .map(|(index, &market)| (market, index))
            .collect();
        let mut union = UnionFind::new(markets.len());

        for group in &problem.market_groups {
            union_markets(&mut union, &market_index, group.markets.iter().copied())?;
        }

        let mut order_markets = Vec::with_capacity(problem.orders.len());
        for order in &problem.orders {
            let mut active: Vec<_> = order.active_markets().collect();
            if let Some(condition) = &order.condition {
                active.push(condition.market);
            }
            if active.is_empty() {
                return None;
            }
            union_markets(&mut union, &market_index, active.iter().copied())?;
            order_markets.push(active);
        }

        let order_by_id: HashMap<_, _> = problem
            .orders
            .iter()
            .enumerate()
            .map(|(index, order)| (order.id, index))
            .collect();
        for mm in &problem.mm_constraints {
            let markets = mm
                .order_ids
                .iter()
                .filter_map(|order_id| order_by_id.get(order_id))
                .flat_map(|&order_index| order_markets[order_index].iter().copied());
            union_markets(&mut union, &market_index, markets)?;
        }

        let mut root_component = HashMap::new();
        let mut market_component = HashMap::new();
        for (index, &market) in markets.iter().enumerate() {
            let root = union.find(index);
            let next = root_component.len();
            let component = *root_component.entry(root).or_insert(next);
            market_component.insert(market, component);
        }

        let order_component: Vec<_> = order_markets
            .iter()
            .map(|active| market_component[&active[0]])
            .collect();
        let mm_component: Vec<Option<usize>> = problem
            .mm_constraints
            .iter()
            .map(|mm| {
                mm.order_ids
                    .iter()
                    .find_map(|order_id| order_by_id.get(order_id))
                    .map(|&order_index| order_component[order_index])
            })
            .collect();
        let num_components = root_component.len();
        let mut stats = ExactComponentStats {
            components: num_components,
            ..Default::default()
        };
        let mut market_counts = vec![0usize; num_components];
        let mut order_counts = vec![0usize; num_components];
        let mut mm_counts = vec![0usize; num_components];
        for &component in market_component.values() {
            market_counts[component] += 1;
        }
        for &component in &order_component {
            order_counts[component] += 1;
        }
        for component in mm_component.iter().flatten() {
            mm_counts[*component] += 1;
        }
        stats.largest_markets = market_counts.into_iter().max().unwrap_or(0);
        stats.largest_orders = order_counts.into_iter().max().unwrap_or(0);
        stats.largest_mms = mm_counts.into_iter().max().unwrap_or(0);

        Some(Self {
            market_component,
            order_component,
            mm_component,
            num_components,
            stats,
        })
    }

    fn subproblem(&self, problem: &Problem, component: usize) -> Problem {
        let component_markets: HashSet<_> = self
            .market_component
            .iter()
            .filter_map(|(&market, &candidate)| (candidate == component).then_some(market))
            .collect();
        let mut markets = MarketSet::new();
        for market in problem.markets.iter() {
            if component_markets.contains(&market.id) {
                markets.add_market(market.clone());
            }
        }

        let orders: Vec<_> = problem
            .orders
            .iter()
            .zip(&self.order_component)
            .filter_map(|(order, &candidate)| (candidate == component).then_some(order.clone()))
            .collect();
        let order_ids: HashSet<_> = orders.iter().map(|order| order.id).collect();
        let mm_constraints = problem
            .mm_constraints
            .iter()
            .zip(&self.mm_component)
            .filter(|(_, candidate)| **candidate == Some(component))
            .map(|(mm, _)| filter_mm(mm, &order_ids))
            .filter(|mm| !mm.order_ids.is_empty())
            .collect();
        let market_groups: Vec<MarketGroup> = problem
            .market_groups
            .iter()
            .filter(|group| {
                group
                    .markets
                    .iter()
                    .all(|market| component_markets.contains(market))
            })
            .cloned()
            .collect();

        let mut subproblem = Problem::new(format!("{}_exact_comp{component}", problem.name));
        subproblem.markets = markets;
        subproblem.orders = orders;
        subproblem.mm_constraints = mm_constraints;
        subproblem.market_groups = market_groups;
        subproblem
    }
}

fn filter_mm(mm: &MmConstraint, order_ids: &HashSet<u64>) -> MmConstraint {
    let mut filtered = MmConstraint::new(mm.mm_id, mm.max_capital);
    for &order_id in &mm.order_ids {
        if let Some(&side) = mm.order_sides.get(&order_id)
            && order_ids.contains(&order_id)
        {
            filtered.add_order(order_id, side);
        }
    }
    filtered
}

fn union_markets(
    union: &mut UnionFind,
    market_index: &HashMap<MarketId, usize>,
    markets: impl IntoIterator<Item = MarketId>,
) -> Option<()> {
    let mut markets = markets.into_iter();
    let Some(first_market) = markets.next() else {
        return Some(());
    };
    let first = *market_index.get(&first_market)?;
    for market in markets {
        union.union(first, *market_index.get(&market)?);
    }
    Some(())
}

struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    fn find(&mut self, item: usize) -> usize {
        if self.parent[item] != item {
            self.parent[item] = self.find(self.parent[item]);
        }
        self.parent[item]
    }

    fn union(&mut self, left: usize, right: usize) {
        let mut left = self.find(left);
        let mut right = self.find(right);
        if left == right {
            return;
        }
        if self.rank[left] < self.rank[right] {
            std::mem::swap(&mut left, &mut right);
        }
        self.parent[right] = left;
        if self.rank[left] == self.rank[right] {
            self.rank[left] += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use matching_engine::{
        ConditionDir, MarketGroup, MmConstraint, MmId, MmSide, NANOS_PER_DOLLAR, Nanos,
        OrderBuilder, Problem, Qty, conditional_buy, simple_no_buy, simple_yes_buy,
    };

    use super::*;

    fn disconnected_problem() -> Problem {
        let mut problem = Problem::new("exact_components");
        let a = problem.markets.add_binary("a");
        let b = problem.markets.add_binary("b");
        let c = problem.markets.add_binary("c");
        for (id, market) in [(1, a), (2, b), (3, c)] {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                id,
                market,
                600_000_000,
                100_000,
            ));
            problem.orders.push(simple_no_buy(
                &problem.markets,
                id + 100,
                market,
                500_000_000,
                100_000,
            ));
        }
        let mut mm = MmConstraint::new(MmId::new(1), Nanos(100 * NANOS_PER_DOLLAR));
        mm.add_order(101, MmSide::BuyNo);
        mm.add_order(102, MmSide::BuyNo);
        problem.mm_constraints.push(mm);
        problem
    }

    #[test]
    fn shared_mm_budget_unions_its_markets_only() {
        let problem = disconnected_problem();
        assert_eq!(
            exact_component_stats(&problem),
            ExactComponentStats {
                components: 2,
                largest_markets: 2,
                largest_orders: 4,
                largest_mms: 1,
            }
        );
    }

    #[test]
    fn groups_spanning_orders_and_conditions_are_connectivity_edges() {
        let mut problem = Problem::new("topology_edges");
        let a = problem.markets.add_binary("a");
        let b = problem.markets.add_binary("b");
        let c = problem.markets.add_binary("c");
        let d = problem.markets.add_binary("d");
        let e = problem.markets.add_binary("e");
        problem.add_market_group(MarketGroup::new("group").with_market(a).with_market(b));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, a, 500_000_000, 1));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 2, b, 500_000_000, 1));
        problem.orders.push(
            OrderBuilder::new(&problem.markets, 3)
                .spanning(&[c, d])
                .limit(Nanos(500_000_000))
                .quantity(Qty(1))
                .payoff_at(0, 1)
                .build(),
        );
        problem.orders.push(conditional_buy(
            &problem.markets,
            4,
            e,
            500_000_000,
            1,
            c,
            400_000_000,
            ConditionDir::Above,
        ));

        assert_eq!(
            exact_component_stats(&problem),
            ExactComponentStats {
                components: 2,
                largest_markets: 3,
                largest_orders: 2,
                largest_mms: 0,
            }
        );
    }

    #[cfg(feature = "lp")]
    #[test]
    fn exact_components_match_monolithic_lp() {
        let problem = disconnected_problem();
        let monolithic = crate::LpSolver::new().solve(&problem);
        let decomposed = ExactComponentSolver::new(crate::LpSolver::new()).solve(&problem);

        assert_eq!(
            monolithic.result.total_welfare(),
            decomposed.result.total_welfare()
        );
        assert_eq!(decomposed.diagnostics.status, TerminationStatus::Converged);
        assert_eq!(
            decomposed.diagnostics.iterations,
            monolithic
                .diagnostics
                .iterations
                .map(|iterations| iterations * 2)
        );
    }

    #[cfg(feature = "lp")]
    #[test]
    fn strongly_unbalanced_components_delegate() {
        let mut problem = Problem::new("unbalanced");
        let large = problem.markets.add_binary("large");
        let tail = problem.markets.add_binary("tail");
        for id in 1..=9 {
            problem
                .orders
                .push(simple_yes_buy(&problem.markets, id, large, 500_000_000, 1));
        }
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 10, tail, 500_000_000, 1));

        let result = ExactComponentSolver::new(crate::LpSolver::new()).solve(&problem);
        assert_eq!(result.diagnostics.algorithm, "lp");
    }
}
