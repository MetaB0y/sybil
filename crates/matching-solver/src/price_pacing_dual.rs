//! Price-side support for the feature-gated [`crate::DirectDualConicSolver`].
//!
//! Exact fixed-pacing minimization and primal recovery form an independent,
//! structure-aware cross-check against HiGHS. Rejected coordinate and
//! smoothing searches are recorded, rather than compiled, in
//! `design/solver-experiments/price-pacing-dual.md`.

use std::collections::HashMap;

use matching_engine::{MarketId, NANOS_PER_DOLLAR, Order, SHARE_SCALE};

use crate::retained_cash_solver::ObjectiveModel;

#[derive(Clone, Copy, Debug)]
struct LinearSegment {
    market: usize,
    start: f64,
    end: f64,
    slope: f64,
}

#[derive(Debug)]
pub(crate) struct PriceDualSolution {
    /// YES prices as probabilities in the same order as `PriceDualOracle::markets`.
    pub(crate) yes_prices: Vec<f64>,
    /// A primal optimum in protocol quantity units.
    pub(crate) q_values: Vec<f64>,
    /// Exact hinge dual objective in retained-cash objective units (nanos).
    pub(crate) dual_objective_nanos: f64,
    /// Recovered primal objective in the units used by the HiGHS wrapper.
    pub(crate) primal_objective_dollars: f64,
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
    orders_by_market: Vec<Vec<usize>>,
    components: Vec<Vec<usize>>,
    previous_q: Vec<f64>,
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
        let mut orders_by_market = vec![Vec::new(); markets.len()];
        for order in orders {
            if order.validate_binary_one_hot().is_err() {
                return None;
            }
            if !market_index.contains_key(&order.markets[0]) {
                return None;
            }
        }
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
            orders_by_market,
            components,
            previous_q: vec![0.0; orders.len()],
        })
    }

    pub(crate) fn markets(&self) -> &[MarketId] {
        &self.markets
    }

    pub(crate) fn solve(&mut self, coefficients_nanos: &[f64]) -> Option<PriceDualSolution> {
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

        let dual_objective_nanos = self.objective_at(coefficients_nanos, &yes_prices)?;
        let q_values = self.recover_primal(coefficients_nanos, &yes_prices, &self.previous_q)?;
        let primal_objective_dollars =
            self.primal_objective_dollars(coefficients_nanos, &q_values)?;
        let dual_objective_dollars =
            dual_objective_nanos * SHARE_SCALE as f64 / NANOS_PER_DOLLAR as f64;
        let objective_tolerance = 1e-8 * dual_objective_dollars.abs().max(1.0);
        if (primal_objective_dollars - dual_objective_dollars).abs() > objective_tolerance {
            return None;
        }
        self.previous_q.clone_from(&q_values);
        Some(PriceDualSolution {
            yes_prices,
            q_values,
            dual_objective_nanos,
            primal_objective_dollars,
        })
    }

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

    /// Recover a primal optimum from the price subgradient.
    ///
    /// At fixed prices, positive-surplus orders fill completely and
    /// negative-surplus orders do not fill. Only zero-surplus orders remain
    /// free. Since every supported order touches one outcome of one market,
    /// those marginal quantities control only
    /// `d_m = demand_yes_m - demand_no_m`.
    ///
    /// For one independent market, `p_yes` is a subgradient of
    /// `max(d_yes, d_no)`. For a categorical group, the YES-price vector is a
    /// subgradient of `max(0, max_m d_m)`. Primal recovery is therefore an
    /// interval problem: positive-price markets must attain one common active
    /// difference `g`, zero-price markets must not exceed it, and `g = 0`
    /// whenever the price simplex has slack.
    fn recover_primal(
        &self,
        coefficients_nanos: &[f64],
        yes_prices: &[f64],
        reference_q: &[f64],
    ) -> Option<Vec<f64>> {
        const SURPLUS_TOLERANCE_NANOS: f64 = 1e-5;
        const PRICE_TOLERANCE: f64 = 1e-10;
        const RECOVERY_TOLERANCE: f64 = 1e-5;

        if coefficients_nanos.len() != self.orders.len()
            || yes_prices.len() != self.markets.len()
            || reference_q.len() != self.orders.len()
        {
            return None;
        }

        let nanos = NANOS_PER_DOLLAR as f64;
        let mut q_values = vec![0.0; self.orders.len()];
        let mut fixed_difference = vec![0.0; self.markets.len()];
        let mut reference_difference = vec![0.0; self.markets.len()];
        let mut marginal_by_market: Vec<Vec<(usize, f64)>> = vec![Vec::new(); self.markets.len()];

        for (index, order) in self.orders.iter().enumerate() {
            let market = self.market_index[&order.markets[0]];
            let yes = yes_prices[market];
            let no = 1.0 - yes;
            let payoff_price =
                nanos * (order.payoffs[0] as f64 * yes + order.payoffs[1] as f64 * no);
            let surplus = coefficients_nanos[index] - payoff_price;
            let difference_coefficient = (order.payoffs[0] - order.payoffs[1]) as f64;
            if surplus > SURPLUS_TOLERANCE_NANOS {
                q_values[index] = order.max_fill.0 as f64;
                fixed_difference[market] += difference_coefficient * q_values[index];
            } else if surplus >= -SURPLUS_TOLERANCE_NANOS {
                q_values[index] = reference_q[index].clamp(0.0, order.max_fill.0 as f64);
                marginal_by_market[market].push((index, difference_coefficient));
            }
        }
        reference_difference.clone_from(&fixed_difference);
        for market in 0..self.markets.len() {
            for &(index, coefficient) in &marginal_by_market[market] {
                reference_difference[market] += coefficient * q_values[index];
            }
        }

        let reachable_interval = |market: usize| {
            let mut low = fixed_difference[market];
            let mut high = fixed_difference[market];
            for &(index, coefficient) in &marginal_by_market[market] {
                let contribution = coefficient * self.orders[index].max_fill.0 as f64;
                low += contribution.min(0.0);
                high += contribution.max(0.0);
            }
            (low, high)
        };

        let mut targets = fixed_difference.clone();
        for component in &self.components {
            let price_sum = component
                .iter()
                .map(|&market| yes_prices[market])
                .sum::<f64>();
            if price_sum < 1.0 - PRICE_TOLERANCE {
                for &market in component {
                    let price_is_positive = yes_prices[market] > PRICE_TOLERANCE;
                    let target = if price_is_positive {
                        0.0
                    } else {
                        reference_difference[market].min(0.0)
                    };
                    let (low, high) = reachable_interval(market);
                    if target < low - RECOVERY_TOLERANCE || target > high + RECOVERY_TOLERANCE {
                        return None;
                    }
                    targets[market] = target.clamp(low, high);
                }
                continue;
            }

            // With a tight categorical price simplex, choose the common active
            // difference nearest to the previous optimum. Positive-price
            // markets pay |g - reference_m|; zero-price markets pay
            // max(0, reference_m - g). This is the structural equivalent of a
            // warm simplex basis and avoids unrelated jumps on degenerate
            // faces as pacing coefficients move.
            let mut lower = 0.0_f64;
            let mut upper = f64::INFINITY;
            let mut events = Vec::with_capacity(component.len());
            let mut slope = 0_i64;
            for &market in component {
                let (low, high) = reachable_interval(market);
                lower = lower.max(low);
                if yes_prices[market] > PRICE_TOLERANCE {
                    upper = upper.min(high);
                    slope -= 1;
                    events.push((reference_difference[market], 2_i64));
                } else {
                    slope -= 1;
                    events.push((reference_difference[market], 1_i64));
                }
            }
            if lower > upper + RECOVERY_TOLERANCE {
                return None;
            }
            events.sort_by(|left, right| left.0.total_cmp(&right.0));
            let mut unconstrained = lower;
            for (position, change) in events {
                slope += change;
                unconstrained = position;
                if slope >= 0 {
                    break;
                }
            }
            let active_difference = unconstrained.clamp(lower, upper);

            for &market in component {
                let target = if yes_prices[market] > PRICE_TOLERANCE {
                    active_difference
                } else {
                    reference_difference[market].min(active_difference)
                };
                let (low, high) = reachable_interval(market);
                if target < low - RECOVERY_TOLERANCE || target > high + RECOVERY_TOLERANCE {
                    return None;
                }
                targets[market] = target.clamp(low, high);
            }
        }

        for market in 0..self.markets.len() {
            let mut remaining = targets[market] - reference_difference[market];
            if remaining > RECOVERY_TOLERANCE {
                let total_capacity = marginal_by_market[market]
                    .iter()
                    .map(|&(index, coefficient)| {
                        if coefficient > 0.0 {
                            self.orders[index].max_fill.0 as f64 - q_values[index]
                        } else {
                            q_values[index]
                        }
                    })
                    .sum::<f64>();
                if total_capacity + RECOVERY_TOLERANCE < remaining {
                    return None;
                }
                let fraction = (remaining / total_capacity).clamp(0.0, 1.0);
                for &(index, coefficient) in &marginal_by_market[market] {
                    let capacity = if coefficient > 0.0 {
                        self.orders[index].max_fill.0 as f64 - q_values[index]
                    } else {
                        q_values[index]
                    };
                    let quantity = (fraction * capacity).min(remaining).max(0.0);
                    q_values[index] += coefficient * quantity;
                    remaining -= quantity;
                }
            } else if remaining < -RECOVERY_TOLERANCE {
                let needed = -remaining;
                let total_capacity = marginal_by_market[market]
                    .iter()
                    .map(|&(index, coefficient)| {
                        if coefficient > 0.0 {
                            q_values[index]
                        } else {
                            self.orders[index].max_fill.0 as f64 - q_values[index]
                        }
                    })
                    .sum::<f64>();
                if total_capacity + RECOVERY_TOLERANCE < needed {
                    return None;
                }
                let fraction = (needed / total_capacity).clamp(0.0, 1.0);
                for &(index, coefficient) in &marginal_by_market[market] {
                    let capacity = if coefficient > 0.0 {
                        q_values[index]
                    } else {
                        self.orders[index].max_fill.0 as f64 - q_values[index]
                    };
                    let quantity = (fraction * capacity).min(-remaining).max(0.0);
                    q_values[index] -= coefficient * quantity;
                    remaining += quantity;
                }
            }
            if remaining.abs() > RECOVERY_TOLERANCE {
                return None;
            }
        }

        Some(q_values)
    }

    fn primal_objective_dollars(
        &self,
        coefficients_nanos: &[f64],
        q_values: &[f64],
    ) -> Option<f64> {
        if coefficients_nanos.len() != self.orders.len() || q_values.len() != self.orders.len() {
            return None;
        }

        let nanos = NANOS_PER_DOLLAR as f64;
        let mut yes = vec![0.0; self.markets.len()];
        let mut no = vec![0.0; self.markets.len()];
        let mut linear = 0.0;
        for (index, order) in self.orders.iter().enumerate() {
            let quantity = q_values[index];
            if quantity < 0.0 || quantity > order.max_fill.0 as f64 {
                return None;
            }
            linear += coefficients_nanos[index] / nanos * quantity;
            let market = self.market_index[&order.markets[0]];
            yes[market] += order.payoffs[0] as f64 * quantity;
            no[market] += order.payoffs[1] as f64 * quantity;
        }

        let mut mint_quantity = 0.0;
        for component in &self.components {
            if component.len() == 1 {
                let market = component[0];
                mint_quantity += yes[market].max(no[market]);
            } else {
                let mut max_difference = 0.0_f64;
                for &market in component {
                    mint_quantity += no[market];
                    max_difference = max_difference.max(yes[market] - no[market]);
                }
                mint_quantity += max_difference;
            }
        }
        Some(linear - mint_quantity)
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
    use matching_scenarios::{
        FlashLiquidityConfig, ScenarioConfig, generate_flash_liquidity_scenario, generate_scenario,
    };

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
            (direct.dual_objective_nanos - highs_nanos).abs() <= tolerance,
            "direct={} HiGHS={} tolerance={} prices={:?}",
            direct.dual_objective_nanos,
            highs_nanos,
            tolerance,
            direct.yes_prices,
        );
        assert!(
            (direct.primal_objective_dollars - highs.objective_value_dollars).abs()
                <= 1e-8 * highs.objective_value_dollars.abs().max(1.0),
            "recovered primal={} HiGHS={} q={:?}",
            direct.primal_objective_dollars,
            highs.objective_value_dollars,
            direct.q_values,
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

    #[test]
    fn recovered_primal_matches_highs_across_structural_profiles() {
        let mut problems = Vec::new();
        for seed in 7_500..7_524 {
            problems.push(generate_scenario(ScenarioConfig::small().with_seed(seed)));
        }
        for seed in 7_600..7_603 {
            problems.push(generate_scenario(ScenarioConfig::medium().with_seed(seed)));
            problems.push(generate_scenario(
                ScenarioConfig::market_like().with_seed(seed),
            ));
        }
        for seed in 7_700..7_708 {
            problems.push(generate_flash_liquidity_scenario(FlashLiquidityConfig {
                seed,
                num_markets: 20,
                opportunities_per_market: 10,
                num_mms: 4,
                quantity_min_shares: 1,
                quantity_max_shares: 1_000,
                initial_budget_dollars: 1_000,
            }));
        }

        for (problem_index, problem) in problems.iter().enumerate() {
            let ctx = build_solver_context(problem);
            let model = ObjectiveModel::new(problem, &ctx);
            for variant in 0..3 {
                let alpha: Vec<_> = problem
                    .mm_constraints
                    .iter()
                    .enumerate()
                    .map(|(mm_index, _)| {
                        0.05 + 0.9 * ((problem_index + variant + mm_index) % 11) as f64 / 10.0
                    })
                    .collect();
                assert_matches_highs(problem, &model.oracle_coefficients_from_alpha(&alpha));
            }
        }
    }
}
