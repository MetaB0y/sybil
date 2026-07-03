//! Unified solver interface for prediction market matching.

#[cfg(any(feature = "lp", feature = "conic", feature = "milp"))]
use std::borrow::Cow;
#[cfg(any(feature = "lp", feature = "conic", feature = "milp"))]
use std::collections::HashSet;

use matching_engine::Problem;

use crate::PipelineResult;

#[cfg(any(feature = "lp", feature = "conic", feature = "milp"))]
pub(crate) struct SupportedProblem<'a> {
    pub problem: Cow<'a, Problem>,
    pub rejected_orders: usize,
}

#[cfg(any(feature = "lp", feature = "conic", feature = "milp"))]
pub(crate) fn filter_supported_problem<'a>(
    problem: &'a Problem,
    solver_name: &str,
) -> SupportedProblem<'a> {
    let invalid_ids: HashSet<u64> = problem
        .orders
        .iter()
        .filter_map(|order| {
            let reason = order.validate_binary_one_hot().err()?;
            tracing::error!(
                solver = solver_name,
                order_id = order.id,
                reason,
                "solver rejected unsupported order shape"
            );
            Some(order.id)
        })
        .collect();

    if invalid_ids.is_empty() {
        return SupportedProblem {
            problem: Cow::Borrowed(problem),
            rejected_orders: 0,
        };
    }

    let mut filtered = problem.clone();
    filtered
        .orders
        .retain(|order| !invalid_ids.contains(&order.id));

    let valid_ids: HashSet<u64> = filtered.orders.iter().map(|order| order.id).collect();
    for mm in &mut filtered.mm_constraints {
        mm.order_ids.retain(|order_id| valid_ids.contains(order_id));
        mm.order_sides
            .retain(|order_id, _| valid_ids.contains(order_id));
    }

    SupportedProblem {
        problem: Cow::Owned(filtered),
        rejected_orders: invalid_ids.len(),
    }
}

/// Unified solver trait. All solvers (LP, EG, Conic, IterLP, MILP, Decomposed)
/// implement this trait, making them injectable and interchangeable.
///
/// For solvers with richer return types (e.g., `MilpSolver::solve_with_status`),
/// the concrete type provides additional methods beyond this trait.
pub trait Solver: Send + Sync {
    /// Solve a matching problem, returning fills, clearing prices, and timing.
    fn solve(&self, problem: &Problem) -> PipelineResult;

    /// Human-readable solver name for logging and diagnostics.
    fn name(&self) -> &str;
}

#[cfg(all(test, any(feature = "lp", feature = "conic", feature = "milp")))]
mod tests {
    use matching_engine::{
        outcome_buy, MarketId, MmConstraint, MmId, MmSide, Order, Problem, NANOS_PER_DOLLAR,
    };

    use super::filter_supported_problem;

    #[test]
    fn filter_supported_problem_removes_invalid_orders_and_mm_refs() {
        let mut problem = Problem::new("filter_invalid");
        let market = problem.markets.add_binary("m");

        problem.orders.push(outcome_buy(
            &problem.markets,
            1,
            market,
            0,
            NANOS_PER_DOLLAR / 2,
            1_000,
        ));

        let mut invalid = Order::new(2);
        invalid.markets[0] = MarketId::new(market.0);
        invalid.num_markets = 1;
        invalid.num_states = 2;
        invalid.payoffs[0] = 2;
        invalid.limit_price = NANOS_PER_DOLLAR / 2;
        invalid.max_fill = 1_000;
        problem.orders.push(invalid);

        let mm = MmConstraint::new(MmId(7), NANOS_PER_DOLLAR)
            .with_order(1, MmSide::BuyYes)
            .with_order(2, MmSide::BuyYes);
        problem.mm_constraints.push(mm);

        let supported = filter_supported_problem(&problem, "test");
        let filtered = supported.problem.as_ref();

        assert_eq!(supported.rejected_orders, 1);
        assert_eq!(filtered.orders.len(), 1);
        assert_eq!(filtered.orders[0].id, 1);
        assert_eq!(filtered.mm_constraints[0].order_ids, vec![1]);
        assert!(!filtered.mm_constraints[0].order_sides.contains_key(&2));
    }
}
