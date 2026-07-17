//! Price-side support for the feature-gated [`crate::DirectDualConicSolver`].
//!
//! Exact fixed-pacing minimization remains as a test-only cross-check against
//! HiGHS. Rejected coordinate and smoothing searches are recorded, rather than
//! compiled, in `design/solver-experiments/price-pacing-dual.md`.

use std::collections::HashMap;

use matching_engine::{MarketId, NANOS_PER_DOLLAR, Order, SHARE_SCALE};

use crate::retained_cash_solver::ObjectiveModel;

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
struct LinearSegment {
    market: usize,
    start: f64,
    end: f64,
    slope: f64,
}

#[cfg(test)]
#[derive(Debug)]
struct PriceDualSolution {
    /// YES prices as probabilities in the same order as `PriceDualOracle::markets`.
    pub(crate) yes_prices: Vec<f64>,
    /// Exact hinge objective in retained-cash objective units (nanos).
    pub(crate) objective_nanos: f64,
}

/// Exact price dual of the ordinary zero-temperature matching LP.
///
/// For fixed fill coefficients `c_i`, eliminating fill quantities gives the
/// convex hinge objective
///
/// `sum_i Q_i [c_i - <payoff_i, p>]_+ / SHARE_SCALE`.
///
/// Every standalone binary market has `p_yes + p_no = 1`. Markets in a
/// categorical group additionally satisfy `sum_m p_yes_m <= 1`. Because
/// production orders touch one binary market, the objective is separable by
/// market and each group is a small separable piecewise-linear resource
/// allocation problem. Sorting its negative-slope segments solves it exactly.
pub(crate) struct PriceDualOracle<'a> {
    orders: &'a [Order],
    markets: Vec<MarketId>,
    market_index: HashMap<MarketId, usize>,
    #[cfg(test)]
    orders_by_market: Vec<Vec<usize>>,
    components: Vec<Vec<usize>>,
}

impl<'a> PriceDualOracle<'a> {
    pub(crate) fn new(
        orders: &'a [Order],
        markets: &[MarketId],
        market_to_group: &HashMap<MarketId, usize>,
        num_groups: usize,
    ) -> Option<Self> {
        let market_index: HashMap<_, _> = markets
            .iter()
            .enumerate()
            .map(|(index, &market)| (market, index))
            .collect();
        #[cfg(test)]
        let mut orders_by_market = vec![Vec::new(); markets.len()];
        for order in orders {
            if order.validate_binary_one_hot().is_err() {
                return None;
            }
            if !market_index.contains_key(&order.markets[0]) {
                return None;
            }
        }
        #[cfg(test)]
        for (order_index, order) in orders.iter().enumerate() {
            let index = market_index[&order.markets[0]];
            orders_by_market[index].push(order_index);
        }

        let mut grouped = vec![Vec::new(); num_groups];
        let mut components = Vec::new();
        for (index, market) in markets.iter().enumerate() {
            if let Some(&group) = market_to_group.get(market) {
                grouped.get_mut(group)?.push(index);
            } else {
                components.push(vec![index]);
            }
        }
        components.extend(grouped.into_iter().filter(|group| !group.is_empty()));

        Some(Self {
            orders,
            markets: markets.to_vec(),
            market_index,
            #[cfg(test)]
            orders_by_market,
            components,
        })
    }

    #[cfg(test)]
    fn solve(&self, coefficients_nanos: &[f64]) -> Option<PriceDualSolution> {
        if coefficients_nanos.len() != self.orders.len() {
            return None;
        }

        let mut yes_prices = vec![0.0; self.markets.len()];
        for component in &self.components {
            let mut segments = Vec::new();
            for &market in component {
                segments.extend(self.market_segments(market, coefficients_nanos));
            }
            segments.sort_by(|left, right| {
                left.slope
                    .total_cmp(&right.slope)
                    .then(left.market.cmp(&right.market))
                    .then(left.start.total_cmp(&right.start))
            });

            // The categorical price simplex is `sum p_yes <= 1`. Starting at
            // zero, consume the globally cheapest marginal segments until the
            // unit capacity is exhausted or all remaining slopes are
            // non-negative. Within each market slopes are nondecreasing, so
            // this merge automatically respects every curve's prefix order.
            let mut remaining = 1.0;
            for segment in segments {
                if remaining <= 0.0 || segment.slope >= 0.0 {
                    break;
                }
                if yes_prices[segment.market] + 1e-12 < segment.start {
                    return None;
                }
                let length = segment.end - segment.start;
                let take = length.min(remaining);
                yes_prices[segment.market] += take;
                remaining -= take;
            }
        }

        let objective_nanos = self.objective_at(coefficients_nanos, &yes_prices)?;
        Some(PriceDualSolution {
            yes_prices,
            objective_nanos,
        })
    }

    #[cfg(test)]
    fn market_segments(&self, market: usize, coefficients_nanos: &[f64]) -> Vec<LinearSegment> {
        let nanos = NANOS_PER_DOLLAR as f64;
        let quantity_scale = SHARE_SCALE as f64;
        let mut derivative = 0.0;
        let mut events = Vec::new();

        for &order_index in &self.orders_by_market[market] {
            let order = &self.orders[order_index];
            let payoff_yes = order.payoffs[0] as f64;
            let payoff_no = order.payoffs[1] as f64;
            let intercept = coefficients_nanos[order_index] - payoff_no * nanos;
            let surplus_slope = (payoff_no - payoff_yes) * nanos;
            let quantity = order.max_fill.0 as f64 / quantity_scale;
            let objective_slope = quantity * surplus_slope;

            if intercept > 0.0 || (intercept == 0.0 && surplus_slope > 0.0) {
                derivative += objective_slope;
            }
            let root = -intercept / surplus_slope;
            if root > 0.0 && root < 1.0 {
                events.push((root, objective_slope.abs()));
            }
        }

        events.sort_by(|left, right| left.0.total_cmp(&right.0));
        let mut segments = Vec::with_capacity(events.len() + 1);
        let mut start = 0.0;
        let mut index = 0;
        while index < events.len() {
            let end = events[index].0;
            if end > start {
                segments.push(LinearSegment {
                    market,
                    start,
                    end,
                    slope: derivative,
                });
            }
            while index < events.len() && events[index].0 == end {
                derivative += events[index].1;
                index += 1;
            }
            start = end;
        }
        if start < 1.0 {
            segments.push(LinearSegment {
                market,
                start,
                end: 1.0,
                slope: derivative,
            });
        }
        segments
    }

    fn objective_at(&self, coefficients_nanos: &[f64], yes_prices: &[f64]) -> Option<f64> {
        if yes_prices.len() != self.markets.len() {
            return None;
        }
        let nanos = NANOS_PER_DOLLAR as f64;
        let quantity_scale = SHARE_SCALE as f64;
        let mut objective = 0.0;
        for (index, order) in self.orders.iter().enumerate() {
            let &market = self.market_index.get(&order.markets[0])?;
            let yes = yes_prices[market];
            let no = 1.0 - yes;
            let payoff_price =
                nanos * (order.payoffs[0] as f64 * yes + order.payoffs[1] as f64 * no);
            let surplus = (coefficients_nanos[index] - payoff_price).max(0.0);
            objective += order.max_fill.0 as f64 * surplus / quantity_scale;
        }
        Some(objective)
    }

    pub(crate) fn project_prices(&self, yes_prices: &mut [f64]) -> Option<()> {
        if yes_prices.len() != self.markets.len() {
            return None;
        }
        for component in &self.components {
            project_capped_simplex(yes_prices, component);
        }
        Some(())
    }

    pub(crate) fn joint_objective_at(
        &self,
        model: &ObjectiveModel<'_>,
        alpha: &[f64],
        yes_prices: &[f64],
    ) -> Option<f64> {
        let coefficients = model.oracle_coefficients_from_alpha(alpha);
        let fixed = self.objective_at(&coefficients, yes_prices)?;
        joint_objective(model, fixed, alpha)
    }
}

fn joint_objective(
    model: &ObjectiveModel<'_>,
    fixed_price_objective: f64,
    alpha: &[f64],
) -> Option<f64> {
    let mut objective = fixed_price_objective;
    for (&budget, &pacing) in model.budgets().iter().zip(alpha) {
        if budget <= 0.0 {
            continue;
        }
        if !(pacing > 0.0 && pacing <= 1.0) {
            return None;
        }
        objective -= budget * pacing.ln();
    }
    Some(objective)
}

fn project_capped_simplex(point: &mut [f64], indices: &[usize]) {
    for &index in indices {
        point[index] = point[index].max(0.0);
    }
    let sum = indices.iter().map(|&index| point[index]).sum::<f64>();
    if sum <= 1.0 {
        return;
    }

    let mut values: Vec<_> = indices.iter().map(|&index| point[index]).collect();
    values.sort_by(|left, right| right.total_cmp(left));
    let mut prefix = 0.0;
    let mut threshold = 0.0;
    for (rank, &value) in values.iter().enumerate() {
        prefix += value;
        let candidate = (prefix - 1.0) / (rank + 1) as f64;
        if rank + 1 == values.len() || values[rank + 1] <= candidate {
            threshold = candidate;
            break;
        }
    }
    for &index in indices {
        point[index] = (point[index] - threshold).max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use matching_engine::{NANOS_PER_DOLLAR, SHARE_SCALE};
    use matching_scenarios::{ScenarioConfig, generate_scenario};

    use super::*;
    use crate::lp_solver::{ReusableLpOracle, build_solver_context, welfare_weights};
    use crate::retained_cash_solver::ObjectiveModel;
    use crate::test_fixtures::group_minting_problem;

    fn assert_matches_highs(problem: &matching_engine::Problem, coefficients: &[f64]) {
        let ctx = build_solver_context(problem);
        let direct = PriceDualOracle::new(
            &problem.orders,
            &ctx.markets,
            &ctx.market_to_group,
            ctx.num_groups,
        )
        .expect("valid direct price dual")
        .solve(coefficients)
        .expect("direct price dual solves");
        let highs = ReusableLpOracle::new(
            &problem.orders,
            &ctx.markets,
            &ctx.market_to_group,
            ctx.num_groups,
            &[],
        )
        .expect("valid HiGHS oracle")
        .solve(coefficients)
        .expect("HiGHS solves");
        let highs_nanos =
            highs.objective_value_dollars * NANOS_PER_DOLLAR as f64 / SHARE_SCALE as f64;
        let tolerance = 1e-8 * highs_nanos.abs().max(1.0);
        assert!(
            (direct.objective_nanos - highs_nanos).abs() <= tolerance,
            "direct={} HiGHS={} tolerance={} prices={:?}",
            direct.objective_nanos,
            highs_nanos,
            tolerance,
            direct.yes_prices,
        );
    }

    #[test]
    fn fixed_price_dual_matches_group_minting_lp() {
        let problem = group_minting_problem();
        assert_matches_highs(&problem, &welfare_weights(&problem.orders));
    }

    #[test]
    fn fixed_price_dual_matches_shaded_generated_books() {
        for seed in 7_400..7_408 {
            let problem = generate_scenario(ScenarioConfig::small().with_seed(seed));
            let ctx = build_solver_context(&problem);
            let model = ObjectiveModel::new(&problem, &ctx);
            let alpha: Vec<_> = problem
                .mm_constraints
                .iter()
                .enumerate()
                .map(|(index, _)| 0.2 + 0.6 * ((seed as usize + index) % 7) as f64 / 6.0)
                .collect();
            let coefficients = model.oracle_coefficients_from_alpha(&alpha);
            assert_matches_highs(&problem, &coefficients);
        }
    }
}
