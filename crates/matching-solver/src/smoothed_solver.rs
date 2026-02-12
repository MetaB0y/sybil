//! Smoothed gradient solver with Augmented Lagrangian for welfare-maximizing FBA.
//!
//! Uses Walrasian tatonnement with temperature annealing and dual variables to find
//! clearing prices that maximize welfare subject to:
//! - Position balance per market (price = dual variable)
//! - Bundle order integration (direct excess demand contribution)
//! - MM budget constraints (mu dual + proportional scaling fallback)
//! - Group price constraints (sum(p) ≤ $1 via post-hoc group minting)
//!
//! Algorithm:
//! 1. Initialize prices via LocalSolver per-market clearing
//! 2. Annealing loop: for each temperature ε (high → low):
//!    a. Compute smoothed excess demand per market (non-MM orders)
//!    b. Bundle contributions to excess demand
//!    c. MM dual allocation: surplus penalized by mu * capital_per_unit,
//!       with proportional scaling fallback when dual hasn't converged
//!    d. Tatonnement step (adjust prices by total excess demand)
//!    e. Clamp prices to [0, NPD]
//!    f. Update mu duals via subgradient (budget violation → higher mu)
//! 3. Extract fills: single-market (MM budget enforced inline) + bundles
//! 4. Group minting: water-filling for unfilled buy-YES demand where Σp ≥ $1,
//!    gated by simulate_enforce_ucp (only kept if post-UCP welfare improves)
//! 5. Post-process via enforce_ucp (light — mostly repricing)

use std::collections::HashMap;
use std::time::Instant;

use tracing::debug;

use matching_engine::{Fill, MarketId, MmSide, Nanos, Order, Problem, Qty, NANOS_PER_DOLLAR};

use crate::local_solver::{LocalSolver, MarketSolution};
use crate::pipeline::{PipelineResult, PipelineTimings};
use crate::traits::{PriceDiscoverer, PriceDiscoveryResult};
use crate::Pipeline;

/// Smoothed gradient solver with Augmented Lagrangian Method (ALM).
///
/// Extends Walrasian tatonnement with dual variables for:
/// - MM budget constraints (mu): biases prices to reduce capital usage when budgets bind
/// - Group price constraints (nu): enforces sum(p) <= $1 across grouped markets
pub struct SmoothedSolver {
    /// Initial temperature (large = smoother landscape)
    epsilon_start: f64,
    /// Final temperature (small = approaches hard complementary slackness)
    epsilon_min: f64,
    /// Multiply ε by this each outer step (e.g., 0.5)
    cooling_factor: f64,
    /// Gradient step size for tatonnement
    learning_rate: f64,
    /// Max gradient steps per temperature level
    max_inner_iters: usize,
    /// Convergence threshold for inner loop (max excess demand / total qty)
    inner_convergence: f64,
    /// Learning rate for MM budget dual (mu) updates
    lr_mu: f64,
}

impl Default for SmoothedSolver {
    fn default() -> Self {
        Self {
            epsilon_start: 0.1 * NANOS_PER_DOLLAR as f64,
            epsilon_min: 1000.0,
            cooling_factor: 0.5,
            learning_rate: 1.0,
            max_inner_iters: 150,
            inner_convergence: 1e-4,
            lr_mu: 0.05,
        }
    }
}

/// Internal single-market order representation for gradient math.
struct OrderInfo {
    order_id: u64,
    /// Limit price in nanos (as f64), in unified YES terms.
    /// May be shaded by lambda between passes.
    limit_price: f64,
    /// Original unshaded limit price (for resetting between passes).
    original_limit: f64,
    /// Maximum fill quantity
    max_fill: f64,
    /// true = YES buyer or NO seller (demand side in unified clearing)
    is_buy: bool,
    /// If this is an MM order: (mm_group_index, capital_uses_complement)
    /// capital_uses_complement=false → capital = p per unit
    /// capital_uses_complement=true → capital = (NPD - p) per unit
    mm_info: Option<(usize, bool)>,
}

/// Per-market order data for gradient computation.
struct MarketOrders {
    market_id: MarketId,
    orders: Vec<OrderInfo>,
}

/// Multi-market (bundle) order info for Lagrangian extension.
struct BundleInfo {
    order_id: u64,
    is_seller: bool,
    limit_price: f64,
    max_fill: f64,
    num_states: usize,
    payoffs: Vec<i8>,
    /// Per-market normalized marginal payoff: +1.0 = long 1 YES, -0.5 = short 0.5 YES
    marginal_payoffs: Vec<(MarketId, f64)>,
    /// Markets spanned by this bundle (in order)
    markets: Vec<MarketId>,
    /// Per-market group index: None = standalone, Some(idx) = group index.
    /// Same length as `markets`.
    market_group_indices: Vec<Option<usize>>,
    /// Groups that appear in this bundle: (group_idx, list of bundle-local market indices).
    /// Only includes groups with at least one market in this bundle.
    groups_in_bundle: Vec<(usize, Vec<usize>)>,
}

/// MM constraint group info for Lagrangian extension.
struct MmGroup {
    /// Budget in nanos (as f64)
    budget: f64,
    /// Precomputed references to MM orders: (market_price_index, order data).
    /// Avoids scanning all orders during dual updates.
    mm_order_refs: Vec<MmOrderRef>,
}

/// Precomputed MM order data for greedy allocation.
struct MmOrderRef {
    order_id: u64,
    market_idx: usize, // index into prices vec (MarketId.0)
    limit_price: f64,
    max_fill: f64,
    is_buy: bool,
    capital_uses_complement: bool,
}

impl SmoothedSolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Main entry point: solve the problem and return a PipelineResult.
    ///
    /// Uses multi-pass fill extraction (like DualMaster's iterative approach):
    /// fill → remove filled orders → re-run tatonnement → fill more.
    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();
        let npd = NANOS_PER_DOLLAR as f64;

        // Step 1: Initialize prices via LocalSolver
        let local_solver = LocalSolver::new();
        let initial_pd = local_solver.discover_prices(problem);

        // Build per-market order lists (unified: NO buyers → YES supply, etc.)
        let mm_order_map = build_mm_order_map(problem);
        let mut market_orders = self.build_market_orders(problem, &mm_order_map);

        // Build group membership: market_id → group_index (used by bundle pricing)
        let (group_map, _groups) = self.build_groups(problem);

        // Market-to-group mapping for lambda shading (market_idx → group_index)
        let market_to_group: HashMap<usize, usize> = problem
            .market_groups
            .iter()
            .enumerate()
            .flat_map(|(gi, g)| g.markets.iter().map(move |&m| (m.0 as usize, gi)))
            .collect();
        let num_groups = problem.market_groups.len();

        // Build bundle and MM info (bundles need group_map for precomputation)
        let mut bundles = self.build_bundle_infos(problem, &group_map);
        let mut mm_groups = self.build_mm_groups(problem, &market_orders);

        // Determine Vec size: max market index + 1
        let num_price_slots = {
            let mut max_id = 0usize;
            for mo in &market_orders {
                max_id = max_id.max(mo.market_id.0 as usize);
            }
            for b in &bundles {
                for &m in &b.markets {
                    max_id = max_id.max(m.0 as usize);
                }
            }
            max_id + 1
        };

        // Initialize prices from LocalSolver results (Vec indexed by MarketId.0)
        let mut prices = vec![npd / 2.0; num_price_slots];
        for mo in &market_orders {
            let idx = mo.market_id.0 as usize;
            let initial = initial_pd
                .prices
                .get(&mo.market_id)
                .and_then(|p| p.first().copied())
                .unwrap_or(NANOS_PER_DOLLAR / 2);
            prices[idx] = initial as f64;
        }

        // Precompute total quantity per market (Vec indexed by MarketId.0)
        let mut total_qty = vec![1.0f64; num_price_slots];
        for mo in &market_orders {
            let total: f64 = mo.orders.iter().map(|o| o.max_fill).sum();
            total_qty[mo.market_id.0 as usize] = total.max(1.0);
        }

        // Reusable excess demand buffer (zeroed each iteration)
        let mut excess_demands = vec![0.0f64; num_price_slots];

        // Order map for fills + UCP
        let order_map: HashMap<u64, &Order> =
            problem.orders.iter().map(|o| (o.id, o)).collect();

        // ===== Multi-pass fill extraction =====
        // Like DualMaster: fill → remove → shade → re-solve → fill more.
        // Between passes, lambda shading adjusts order limits to enforce
        // group price consistency (sum(p) = $1).
        let max_passes = 8;
        let lambda_step_size = 0.3; // same as DualMaster default
        let mut all_fills: Vec<Fill> = Vec::new();
        let mut filled_ids: std::collections::HashSet<u64> =
            std::collections::HashSet::new();
        let mut total_outer_iters = 0;
        let mut last_final_prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();
        let mut lambda: Vec<f64> = vec![0.0; num_groups]; // price consistency duals

        for pass in 0..max_passes {
            // Check if any orders remain
            let has_orders = market_orders.iter().any(|mo| !mo.orders.is_empty())
                || !bundles.is_empty();
            if !has_orders {
                break;
            }

            // Recompute total_qty for remaining orders
            for mo in &market_orders {
                let total: f64 = mo.orders.iter().map(|o| o.max_fill).sum();
                total_qty[mo.market_id.0 as usize] = total.max(1.0);
            }

            // Shorter annealing for subsequent passes (warm start from previous prices)
            let pass_epsilon_start = if pass == 0 {
                self.epsilon_start
            } else {
                self.epsilon_start * self.cooling_factor.powi(3)
            };

            // Reset mu duals per pass (budget context changes between passes)
            let mut mu: Vec<f64> = vec![0.0; mm_groups.len()];

            // Annealing loop
            let mut epsilon = pass_epsilon_start;

            while epsilon >= self.epsilon_min {
                let inv_eps = 1.0 / epsilon;

                for _inner in 0..self.max_inner_iters {
                    // Zero excess demands
                    for v in excess_demands.iter_mut() {
                        *v = 0.0;
                    }

                    // Single-market orders (non-MM only; MMs handled by dual allocation)
                    for mo in &market_orders {
                        let idx = mo.market_id.0 as usize;
                        let p = prices[idx];
                        let excess = smoothed_excess_demand(&mo.orders, p, inv_eps, npd);
                        excess_demands[idx] = excess;
                    }

                    // Bundle contributions to excess demand
                    for bundle in bundles.iter() {
                        let ep = bundle_expected_payoff(bundle, &prices, npd);
                        let surplus = bundle_surplus(bundle, ep);
                        let fill_prob = sigmoid(surplus * inv_eps);
                        let smoothed_qty = bundle.max_fill * fill_prob;

                        for &(market, marginal) in &bundle.marginal_payoffs {
                            excess_demands[market.0 as usize] += smoothed_qty * marginal;
                        }
                    }

                    // MM dual allocation: effective surplus reduced by mu * capital
                    dual_mm_excess_demand(
                        &mm_groups, &mu, &prices, inv_eps, npd, &mut excess_demands,
                    );

                    // Convergence check (after all contributions)
                    let mut max_rel_excess = 0.0f64;
                    for mo in &market_orders {
                        let idx = mo.market_id.0 as usize;
                        let tq = total_qty[idx];
                        max_rel_excess =
                            max_rel_excess.max((excess_demands[idx] / tq).abs());
                    }

                    // Tatonnement step
                    for mo in &market_orders {
                        let idx = mo.market_id.0 as usize;
                        let p = prices[idx];
                        let excess = excess_demands[idx];
                        let tq = total_qty[idx];

                        let step = self.learning_rate * epsilon * excess / tq;
                        prices[idx] = (p + step).clamp(0.0, npd);
                    }

                    if max_rel_excess < self.inner_convergence {
                        break;
                    }
                }

                // Dual variable subgradient updates (projected to non-negative)
                for (k, mm) in mm_groups.iter().enumerate() {
                    let budget = mm.budget.max(1.0);
                    let mut usage = 0.0;
                    for r in &mm.mm_order_refs {
                        let p = prices[r.market_idx];
                        let surplus = if r.is_buy {
                            r.limit_price - p
                        } else {
                            p - r.limit_price
                        };
                        if surplus <= 0.0 {
                            continue;
                        }
                        let capital_per_unit =
                            if r.capital_uses_complement { npd - p } else { p };
                        if capital_per_unit <= 0.0 {
                            continue;
                        }
                        let fill_prob = sigmoid(surplus / epsilon);
                        usage += capital_per_unit * r.max_fill * fill_prob;
                    }
                    mu[k] = (mu[k] + self.lr_mu * (usage - budget) / budget).max(0.0);
                }

                epsilon *= self.cooling_factor;
                total_outer_iters += 1;
            }

            // Quantize prices
            let final_prices = self.quantize_prices(&prices, &market_orders, &bundles);

            // Extract single-market fills with MM budget tracking.
            // Fresh tracker each pass; deduct capital from previous passes' fills.
            let mut mm_tracker = crate::fill_extraction::MmBudgetTracker::new(problem);
            for fill in &all_fills {
                mm_tracker.fill(fill.order_id, fill.fill_qty, fill.fill_price);
            }

            let mut pass_fills: Vec<Fill> = Vec::new();
            let mut market_ids: Vec<MarketId> = final_prices.keys().copied().collect();
            market_ids.sort();

            for &market_id in &market_ids {
                let yes_price = final_prices[&market_id][0];
                let market_orders_for_fill: Vec<&Order> = problem
                    .orders
                    .iter()
                    .filter(|o| {
                        o.num_markets == 1
                            && o.markets[0] == market_id
                            && !filled_ids.contains(&o.id)
                    })
                    .collect();
                let market_fills = crate::fill_extraction::fill_binary_market(
                    market_id,
                    &market_orders_for_fill,
                    yes_price,
                    Some(&mut mm_tracker),
                );
                pass_fills.extend(market_fills);
            }

            // Bundle fills only on pass 0 (subsequent passes focus on single-market)
            if pass == 0 {
                let bundle_fills = extract_bundle_fills(problem, &final_prices, &bundles);
                pass_fills.extend(bundle_fills);
            }

            if pass_fills.is_empty() {
                break; // keep prices from last successful pass
            }

            // Per-pass UCP: only reprice + filter limit violations.
            // Position balance trimming deferred to final UCP which includes
            // group minting arb orders (they provide slack for balancing).
            let prices_map: HashMap<MarketId, Vec<Nanos>> = final_prices.clone();
            let candidates =
                Pipeline::reprice_and_filter_fills(&pass_fills, &prices_map, &order_map);

            let mut surviving_fills: Vec<Fill> = Vec::new();
            for (fill, _) in candidates {
                if fill.fill_qty > 0 {
                    surviving_fills.push(fill);
                }
            }

            if surviving_fills.is_empty() {
                break; // keep prices from last successful pass
            }

            let pass_welfare: i64 = surviving_fills
                .iter()
                .filter_map(|f| {
                    order_map
                        .get(&f.order_id)
                        .map(|o| o.welfare_contribution(f.fill_price, f.fill_qty))
                })
                .sum();
            debug!(
                pass,
                new_fills = surviving_fills.len(),
                pass_welfare,
                total_fills = all_fills.len() + surviving_fills.len(),
                "smoothed pass complete"
            );

            // Accumulate surviving fills
            for fill in &surviving_fills {
                filled_ids.insert(fill.order_id);
            }
            all_fills.extend(surviving_fills);

            // Remove filled orders from data structures for next pass
            for mo in &mut market_orders {
                mo.orders.retain(|o| !filled_ids.contains(&o.order_id));
            }
            bundles.retain(|b| !filled_ids.contains(&b.order_id));
            for mm in &mut mm_groups {
                mm.mm_order_refs
                    .retain(|r| !filled_ids.contains(&r.order_id));
            }
            // Recompute remaining MM budgets from all accumulated fills
            for (k, mm_constraint) in problem.mm_constraints.iter().enumerate() {
                if k < mm_groups.len() {
                    let mut used = 0.0f64;
                    for fill in &all_fills {
                        if let Some(&(mm_idx, side)) = mm_order_map.get(&fill.order_id) {
                            if mm_idx == k {
                                used += side.capital_needed(fill.fill_price, fill.fill_qty)
                                    as f64;
                            }
                        }
                    }
                    mm_groups[k].budget =
                        (mm_constraint.max_capital as f64 - used).max(0.0);
                }
            }

            last_final_prices = final_prices.clone();

            // Lambda shading: compute group price residuals and update lambda.
            // Then shade remaining orders' limits for the next pass.
            // Only shade non-MM orders (MM orders have tight spreads that lambda kills).
            if num_groups > 0 {
                let step = lambda_step_size / ((pass + 1) as f64).sqrt();

                // Compute price residuals: (sum_yes - $1) / $1 per group
                for (gi, group) in problem.market_groups.iter().enumerate() {
                    let sum_yes: f64 = group
                        .markets
                        .iter()
                        .filter_map(|m| final_prices.get(m))
                        .filter_map(|p| p.first())
                        .map(|&p| p as f64)
                        .sum();
                    let residual = (sum_yes - npd) / npd;
                    lambda[gi] += step * residual;
                }

                // Apply shading to remaining orders (non-MM only).
                // In unified YES form, ALL order types shade the same way:
                // effective = original_limit - λ*NPD
                // (derivation: DualMaster subtracts λ for YES-side, adds for NO-side,
                //  but the NO→YES conversion inverts the sign, so unified is always minus)
                for mo in &mut market_orders {
                    let midx = mo.market_id.0 as usize;
                    let Some(&gi) = market_to_group.get(&midx) else {
                        continue; // not in a group, no shading
                    };
                    let lambda_nanos = lambda[gi] * npd;

                    for order in &mut mo.orders {
                        if order.mm_info.is_some() {
                            continue; // never shade MM orders
                        }
                        let effective = order.original_limit - lambda_nanos;
                        order.limit_price = effective.clamp(0.0, npd);
                    }
                }
            }
        }

        // Use the last computed prices
        let final_prices = if last_final_prices.is_empty() {
            self.quantize_prices(&prices, &market_orders, &bundles)
        } else {
            last_final_prices
        };

        debug!(
            total_outer_iters,
            total_fills = all_fills.len(),
            elapsed_ms = start.elapsed().as_millis(),
            "smoothed solver: all passes complete"
        );

        // Group minting on accumulated fills
        let mut arb_orders: Vec<Order> = Vec::new();
        let mut gm_fills: Vec<Fill> = Vec::new();
        let mut gm_arb_orders: Vec<Order> = Vec::new();

        if !problem.market_groups.is_empty() {
            let max_real_id = problem.orders.iter().map(|o| o.id).max().unwrap_or(0);
            let mut next_arb_id = max_real_id + 1_000_000_000;

            let gm_result = crate::group_minting::find_group_mints(
                &problem.market_groups,
                &order_map,
                &filled_ids,
                &std::collections::HashSet::new(),
                &mut next_arb_id,
            );

            if !gm_result.buyer_fills.is_empty() {
                for fill in &gm_result.buyer_fills {
                    let Some(&order) = order_map.get(&fill.order_id) else {
                        continue;
                    };
                    let market = order.markets[0];
                    let yes_price = final_prices
                        .get(&market)
                        .map(|p| p[0])
                        .unwrap_or(NANOS_PER_DOLLAR / 2);
                    if order.limit_price >= yes_price {
                        gm_fills.push(Fill::new(fill.order_id, fill.fill_qty, yes_price));
                    }
                }

                for arb_order in gm_result.arb_orders {
                    let market = arb_order.markets[0];
                    let yes_price = final_prices
                        .get(&market)
                        .map(|p| p[0])
                        .unwrap_or(NANOS_PER_DOLLAR / 2);

                    let group = problem
                        .market_groups
                        .iter()
                        .find(|g| g.markets.contains(&market));

                    let price_sum: u64 = group
                        .map(|g| {
                            g.markets
                                .iter()
                                .map(|m| {
                                    final_prices
                                        .get(m)
                                        .map(|p| p[0])
                                        .unwrap_or(NANOS_PER_DOLLAR / 2)
                                })
                                .sum()
                        })
                        .unwrap_or(NANOS_PER_DOLLAR);

                    let arb_limit = if price_sum > 0 {
                        ((NANOS_PER_DOLLAR as u128 * yes_price as u128) / price_sum as u128)
                            as Nanos
                    } else {
                        yes_price
                    };

                    let mut arb = Order::new(arb_order.id);
                    arb.markets[0] = market;
                    arb.num_markets = 1;
                    arb.num_states = 2;
                    arb.payoffs[0] = -1;
                    arb.payoffs[1] = 0;
                    arb.limit_price = arb_limit;
                    arb.max_fill = arb_order.max_fill;

                    gm_fills.push(Fill::new(arb.id, arb_order.max_fill, yes_price));
                    gm_arb_orders.push(arb);
                }
            }
        }

        // Simulate UCP with and without group minting; only keep if it improves welfare
        let prices_map: HashMap<MarketId, Vec<Nanos>> = final_prices.clone();
        let baseline_welfare =
            Pipeline::simulate_enforce_ucp(&all_fills, &prices_map, &order_map, &[]);

        let mut gm_combined_fills = all_fills.clone();
        gm_combined_fills.extend(gm_fills.iter().cloned());
        let gm_welfare = Pipeline::simulate_enforce_ucp(
            &gm_combined_fills,
            &prices_map,
            &order_map,
            &gm_arb_orders,
        );

        debug!(
            gm_fills = gm_fills.len(),
            baseline_welfare,
            gm_welfare,
            "group minting evaluation"
        );
        if gm_welfare > baseline_welfare && !gm_fills.is_empty() {
            all_fills.extend(gm_fills);
            arb_orders = gm_arb_orders;
        }

        let pd = self.build_price_discovery(&final_prices);

        // Build PipelineResult and enforce final UCP
        let mut pipeline_result = PipelineResult::empty();
        pipeline_result.price_discovery = Some(pd);
        pipeline_result.total_time_secs = start.elapsed().as_secs_f64();
        pipeline_result.phase_times = PipelineTimings::default();
        pipeline_result.iterations = total_outer_iters;

        let mut order_map_with_arbs = order_map;
        for arb in &arb_orders {
            order_map_with_arbs.insert(arb.id, arb);
        }

        for fill in all_fills {
            if let Some(&order) = order_map_with_arbs.get(&fill.order_id) {
                pipeline_result.result.add_fill(fill, order);
            }
        }

        Pipeline::enforce_ucp(&mut pipeline_result, &order_map_with_arbs);
        pipeline_result.group_minting_arb_orders = arb_orders;

        if pipeline_result.result.total_welfare < 0 {
            pipeline_result.result = crate::MatchingResult::new();
        }

        pipeline_result.total_time_secs = start.elapsed().as_secs_f64();

        debug!(
            welfare = pipeline_result.result.total_welfare,
            fills = pipeline_result.result.fills.len(),
            time_ms = (pipeline_result.total_time_secs * 1000.0) as u64,
            "smoothed solver: done"
        );

        pipeline_result
    }

    /// Build per-market order lists in unified form (YES-centric).
    fn build_market_orders(
        &self,
        problem: &Problem,
        mm_order_map: &HashMap<u64, (usize, MmSide)>,
    ) -> Vec<MarketOrders> {
        let mut market_map: HashMap<MarketId, Vec<OrderInfo>> = HashMap::new();

        for order in &problem.orders {
            if order.num_markets != 1 {
                continue;
            }
            let market = order.markets[0];
            let num_states = order.num_states as usize;

            let is_yes_buyer = num_states > 0 && order.payoffs[0] > 0;
            let is_no_buyer = num_states > 1 && order.payoffs[1] > 0;
            let is_yes_seller = num_states > 0 && order.payoffs[0] < 0;
            let is_no_seller = num_states > 1 && order.payoffs[1] < 0;

            // Determine MM info for this order
            let mm_info = mm_order_map.get(&order.id).map(|&(idx, side)| {
                let capital_uses_complement = matches!(side, MmSide::SellYes | MmSide::SellNo);
                (idx, capital_uses_complement)
            });

            let entry = market_map.entry(market).or_default();

            if is_yes_buyer {
                let lp = order.limit_price as f64;
                entry.push(OrderInfo {
                    order_id: order.id,
                    limit_price: lp,
                    original_limit: lp,
                    max_fill: order.max_fill as f64,
                    is_buy: true,
                    mm_info,
                });
            } else if is_no_seller {
                // NO seller ≡ YES buyer at ($1 - limit)
                let lp = (NANOS_PER_DOLLAR as f64) - (order.limit_price as f64);
                entry.push(OrderInfo {
                    order_id: order.id,
                    limit_price: lp,
                    original_limit: lp,
                    max_fill: order.max_fill as f64,
                    is_buy: true,
                    mm_info,
                });
            } else if is_yes_seller {
                let lp = order.limit_price as f64;
                entry.push(OrderInfo {
                    order_id: order.id,
                    limit_price: lp,
                    original_limit: lp,
                    max_fill: order.max_fill as f64,
                    is_buy: false,
                    mm_info,
                });
            } else if is_no_buyer {
                // NO buyer ≡ YES seller at ($1 - limit)
                let lp = (NANOS_PER_DOLLAR as f64) - (order.limit_price as f64);
                entry.push(OrderInfo {
                    order_id: order.id,
                    limit_price: lp,
                    original_limit: lp,
                    max_fill: order.max_fill as f64,
                    is_buy: false,
                    mm_info,
                });
            }
        }

        market_map
            .into_iter()
            .map(|(market_id, orders)| MarketOrders { market_id, orders })
            .collect()
    }

    /// Build BundleInfo for each multi-market order.
    fn build_bundle_infos(
        &self,
        problem: &Problem,
        group_map: &HashMap<MarketId, usize>,
    ) -> Vec<BundleInfo> {
        let mut bundles = Vec::new();

        for order in &problem.orders {
            if order.num_markets <= 1 {
                continue;
            }
            let num_markets = order.num_markets as usize;
            let num_states = order.num_states as usize;

            let markets: Vec<MarketId> = order.markets[..num_markets]
                .iter()
                .copied()
                .filter(|m| !m.is_none())
                .collect();

            let payoffs: Vec<i8> = order.payoffs[..num_states].to_vec();

            // Compute per-market marginal payoffs using stride decomposition
            let marginal_payoffs = order.marginal_payoffs_f64();

            // Precompute group membership for this bundle
            let market_group_indices: Vec<Option<usize>> = markets
                .iter()
                .map(|m| group_map.get(m).copied())
                .collect();

            // Collect which groups appear and which bundle-local markets belong to each
            let mut groups_map: HashMap<usize, Vec<usize>> = HashMap::new();
            for (m_idx, group_idx) in market_group_indices.iter().enumerate() {
                if let Some(gidx) = group_idx {
                    groups_map.entry(*gidx).or_default().push(m_idx);
                }
            }
            let mut groups_in_bundle: Vec<(usize, Vec<usize>)> =
                groups_map.into_iter().collect();
            groups_in_bundle.sort_by_key(|(gidx, _)| *gidx);

            bundles.push(BundleInfo {
                order_id: order.id,
                is_seller: order.is_seller(),
                limit_price: order.limit_price as f64,
                max_fill: order.max_fill as f64,
                num_states,
                payoffs,
                marginal_payoffs,
                markets,
                market_group_indices,
                groups_in_bundle,
            });
        }

        bundles
    }

    /// Build MmGroup info for each MM constraint, precomputing order refs.
    fn build_mm_groups(
        &self,
        problem: &Problem,
        market_orders: &[MarketOrders],
    ) -> Vec<MmGroup> {
        problem
            .mm_constraints
            .iter()
            .enumerate()
            .map(|(_, mm)| {
                let mut mm_order_refs = Vec::new();
                for mo in market_orders {
                    let midx = mo.market_id.0 as usize;
                    for order in &mo.orders {
                        if mm.order_ids.contains(&order.order_id) {
                            if let Some((_, capital_uses_complement)) = order.mm_info {
                                mm_order_refs.push(MmOrderRef {
                                    order_id: order.order_id,
                                    market_idx: midx,
                                    limit_price: order.limit_price,
                                    max_fill: order.max_fill,
                                    is_buy: order.is_buy,
                                    capital_uses_complement,
                                });
                            }
                        }
                    }
                }
                MmGroup {
                    budget: mm.max_capital as f64,
                    mm_order_refs,
                }
            })
            .collect()
    }

    /// Build group membership map and group lists.
    /// Returns (group_map for BundleInfo precomputation, groups as Vec<Vec<usize>>).
    fn build_groups(
        &self,
        problem: &Problem,
    ) -> (HashMap<MarketId, usize>, Vec<Vec<usize>>) {
        let mut group_map: HashMap<MarketId, usize> = HashMap::new();
        let mut groups: Vec<Vec<usize>> = Vec::new();

        for (i, group) in problem.market_groups.iter().enumerate() {
            let mut indices = Vec::new();
            for &market_id in &group.markets {
                group_map.insert(market_id, i);
                indices.push(market_id.0 as usize);
            }
            if indices.len() > 1 {
                groups.push(indices);
            }
        }

        (group_map, groups)
    }

    /// Convert f64 prices to quantized Nanos by snapping to the nearest
    /// order limit price. This matches real auction semantics: the clearing
    /// price is always at a tick on the order book. Avoids off-by-one issues
    /// from sigmoid smoothing convergence.
    fn quantize_prices(
        &self,
        prices: &[f64],
        market_orders: &[MarketOrders],
        bundles: &[BundleInfo],
    ) -> HashMap<MarketId, Vec<Nanos>> {
        let mut result: HashMap<MarketId, Vec<Nanos>> = HashMap::new();

        for mo in market_orders {
            let idx = mo.market_id.0 as usize;
            let raw = prices[idx];
            let yes_price = round_price(raw, &mo.orders);
            let no_price = NANOS_PER_DOLLAR.saturating_sub(yes_price);
            result.insert(mo.market_id, vec![yes_price, no_price]);
        }
        for b in bundles {
            for &m in &b.markets {
                result.entry(m).or_insert_with(|| {
                    let idx = m.0 as usize;
                    let yes_price =
                        prices[idx].round().max(0.0).min(NANOS_PER_DOLLAR as f64) as Nanos;
                    let no_price = NANOS_PER_DOLLAR.saturating_sub(yes_price);
                    vec![yes_price, no_price]
                });
            }
        }

        result
    }

    /// Build a PriceDiscoveryResult from quantized prices.
    fn build_price_discovery(
        &self,
        final_prices: &HashMap<MarketId, Vec<Nanos>>,
    ) -> PriceDiscoveryResult {
        let mut pd = PriceDiscoveryResult::empty();

        for (&market_id, prices) in final_prices {
            let yes_price = prices[0];
            let no_price = if prices.len() > 1 {
                prices[1]
            } else {
                NANOS_PER_DOLLAR.saturating_sub(yes_price)
            };

            let solution = MarketSolution {
                market_id,
                prices: vec![yes_price, no_price],
                fills: Vec::new(),
                welfare: 0,
                unfilled: Vec::new(),
                has_activity: true,
            };
            pd.prices.insert(market_id, vec![yes_price, no_price]);
            pd.market_solutions.insert(market_id, solution);
        }

        pd
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Build a lookup: order_id → (mm_group_index, MmSide).
fn build_mm_order_map(problem: &Problem) -> HashMap<u64, (usize, MmSide)> {
    let mut map = HashMap::new();
    for (idx, mm) in problem.mm_constraints.iter().enumerate() {
        for &oid in &mm.order_ids {
            if let Some(&side) = mm.order_sides.get(&oid) {
                map.insert(oid, (idx, side));
            }
        }
    }
    map
}

/// Sigmoid function: 1 / (1 + exp(-x)). Clamped to avoid overflow.
fn sigmoid(x: f64) -> f64 {
    if x > 500.0 {
        1.0
    } else if x < -500.0 {
        0.0
    } else {
        1.0 / (1.0 + (-x).exp())
    }
}

/// Compute smoothed excess demand (D - S) for a single market from non-MM orders.
/// MM orders are handled separately by greedy_mm_excess_demand.
fn smoothed_excess_demand(
    orders: &[OrderInfo],
    price: f64,
    inv_eps: f64,
    _npd: f64,
) -> f64 {
    let mut excess = 0.0;

    for order in orders {
        // Skip MM orders — handled by greedy allocation
        if order.mm_info.is_some() {
            continue;
        }

        let surplus = if order.is_buy {
            order.limit_price - price
        } else {
            price - order.limit_price
        };

        let z = surplus * inv_eps;
        let fill_prob = order.max_fill * sigmoid(z);

        if order.is_buy {
            excess += fill_prob;
        } else {
            excess -= fill_prob;
        }
    }

    excess
}

/// Compute the expected payoff of a bundle under the group-aware distribution.
///
/// For markets in the same group (mutually exclusive outcomes), the joint
/// distribution is categorical (exactly one YES), NOT independent Bernoulli.
/// States where multiple markets in the same group are YES have probability 0.
///
/// For markets in different groups or standalone, independence holds.
///
/// Uses precomputed group membership from `BundleInfo` to avoid allocations.
fn bundle_expected_payoff(bundle: &BundleInfo, prices: &[f64], npd: f64) -> f64 {
    let mut expected = 0.0;
    let inv_npd = 1.0 / npd;

    // Precompute per-group sum of in-bundle market prices (for residual computation).
    let group_price_sums: Vec<f64> = bundle
        .groups_in_bundle
        .iter()
        .map(|(_, m_indices)| {
            m_indices
                .iter()
                .map(|&mi| prices[bundle.markets[mi].0 as usize] * inv_npd)
                .sum::<f64>()
        })
        .collect();

    for s in 0..bundle.num_states {
        let payoff = bundle.payoffs[s] as f64;
        if payoff == 0.0 {
            continue;
        }

        // Check validity and compute group contributions in one pass.
        let mut state_prob = 1.0;
        let mut valid = true;

        for (g_local, (_, m_indices)) in bundle.groups_in_bundle.iter().enumerate() {
            let mut yes_market_idx: Option<usize> = None;
            let mut yes_count = 0u32;

            for &mi in m_indices {
                let outcome = (s >> mi) % 2;
                if outcome == 0 {
                    yes_count += 1;
                    if yes_count > 1 {
                        valid = false;
                        break;
                    }
                    yes_market_idx = Some(mi);
                }
            }

            if !valid {
                break;
            }

            match yes_market_idx {
                Some(mi) => {
                    let p = prices[bundle.markets[mi].0 as usize];
                    state_prob *= p * inv_npd;
                }
                None => {
                    let residual = (1.0 - group_price_sums[g_local]).max(0.0);
                    state_prob *= residual;
                }
            }
        }

        if !valid {
            continue;
        }

        // Standalone markets: independent Bernoulli
        for (m_idx, group_idx) in bundle.market_group_indices.iter().enumerate() {
            if group_idx.is_some() {
                continue;
            }
            let p = prices[bundle.markets[m_idx].0 as usize];
            let outcome = (s >> m_idx) % 2;
            if outcome == 0 {
                state_prob *= p * inv_npd;
            } else {
                state_prob *= (npd - p) * inv_npd;
            }
        }

        expected += payoff * state_prob;
    }

    expected * npd
}

/// Compute surplus for a bundle order.
///
/// For buyers: surplus = limit_price - expected_payoff
/// For sellers: surplus = |expected_payoff| - limit_price
fn bundle_surplus(bundle: &BundleInfo, expected_payoff: f64) -> f64 {
    if bundle.is_seller {
        expected_payoff.abs() - bundle.limit_price
    } else {
        bundle.limit_price - expected_payoff
    }
}

/// Dual-informed MM excess demand: each MM order's effective surplus is
/// reduced by `mu_k * capital_per_unit`, where mu_k is the budget dual.
/// When budget binds (mu > 0), high-capital orders are naturally suppressed.
/// Falls back to proportional scaling when dual hasn't converged.
fn dual_mm_excess_demand(
    mm_groups: &[MmGroup],
    mu: &[f64],
    prices: &[f64],
    inv_eps: f64,
    npd: f64,
    excess_demands: &mut [f64],
) {
    for (k, mm) in mm_groups.iter().enumerate() {
        let mu_k = mu[k];

        let mut total_capital = 0.0;
        let mut contributions: Vec<(f64, usize, bool)> = Vec::new();

        for r in &mm.mm_order_refs {
            let p = prices[r.market_idx];
            let surplus = if r.is_buy { r.limit_price - p } else { p - r.limit_price };
            if surplus <= 0.0 {
                continue;
            }
            let capital_per_unit = if r.capital_uses_complement { npd - p } else { p };
            if capital_per_unit <= 0.0 {
                continue;
            }

            // Dual reduction: surplus penalized by mu * capital
            let effective_surplus = surplus - mu_k * capital_per_unit;

            let fill_prob = sigmoid(effective_surplus * inv_eps);
            let smoothed_qty = r.max_fill * fill_prob;

            total_capital += capital_per_unit * smoothed_qty;
            contributions.push((smoothed_qty, r.market_idx, r.is_buy));
        }

        // Proportional scaling as fallback when dual hasn't converged yet
        let scale = if total_capital > mm.budget && total_capital > 0.0 {
            mm.budget / total_capital
        } else {
            1.0
        };

        for &(smoothed_qty, market_idx, is_buy) in &contributions {
            let scaled_qty = smoothed_qty * scale;
            if is_buy {
                excess_demands[market_idx] += scaled_qty;
            } else {
                excess_demands[market_idx] -= scaled_qty;
            }
        }
    }
}




/// Round f64 price to nanos, snapping to nearby order limits within a small epsilon.
///
/// Pure rounding can land 1 nanos below an order limit, causing an `is_satisfied_at_price`
/// rejection. We snap to the nearest order limit only if it's within 1000 nanos (~$0.000001),
/// which avoids edge rejections without forcing fill_price == limit_price for all orders
/// (the old `snap_to_order_price` behaviour that gave MM orders zero welfare).
fn round_price(raw: f64, orders: &[OrderInfo]) -> Nanos {
    let rounded = raw.round().max(0.0).min(NANOS_PER_DOLLAR as f64) as Nanos;
    if orders.is_empty() {
        return rounded;
    }

    // Check if rounded price is within epsilon of any order limit
    const EPSILON: u64 = 1_000; // 1000 nanos ≈ $0.000001
    let mut best = rounded;
    let mut best_dist = u64::MAX;
    for o in orders {
        let lp = o.limit_price.round().max(0.0).min(NANOS_PER_DOLLAR as f64) as Nanos;
        let dist = lp.abs_diff(rounded);
        if dist <= EPSILON && dist < best_dist {
            best_dist = dist;
            best = lp;
        }
    }

    best
}

// ============================================================================
// Fill extraction
// ============================================================================

/// Extract bundle fills, skipping bundles that would create position imbalance.
fn extract_bundle_fills(
    problem: &Problem,
    prices: &HashMap<MarketId, Vec<Nanos>>,
    bundles: &[BundleInfo],
) -> Vec<Fill> {
    let npd = NANOS_PER_DOLLAR as f64;
    let mut fills = Vec::new();

    // Convert integer prices to f64 for expected payoff computation
    // Convert integer prices to f64 vec for expected payoff computation
    let max_market = prices.keys().map(|m| m.0 as usize).max().unwrap_or(0);
    let mut f64_prices = vec![npd / 2.0; max_market + 1];
    for (&m, p) in prices {
        f64_prices[m.0 as usize] = p[0] as f64;
    }

    // Build order map for verifier marginal computation
    let order_map: HashMap<u64, &Order> =
        problem.orders.iter().map(|o| (o.id, o)).collect();

    // Compute surplus for each bundle and sort by surplus descending
    let mut bundle_surplus_list: Vec<(usize, f64)> = bundles
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            let ep = bundle_expected_payoff(b, &f64_prices, npd);
            let surplus = bundle_surplus(b, ep);
            if surplus > 0.0 {
                Some((i, surplus))
            } else {
                None
            }
        })
        .collect();

    bundle_surplus_list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Track per-market net position delta from bundle fills (using verifier's integer marginals)
    let mut net_position: HashMap<MarketId, i64> = HashMap::new();

    for (b_idx, _surplus) in &bundle_surplus_list {
        let bundle = &bundles[*b_idx];
        let ep = bundle_expected_payoff(bundle, &f64_prices, npd);

        // Determine fill_price (risk-neutral price, as Nanos)
        let fill_price = ep.abs().round().max(0.0).min(npd) as Nanos;

        // Check limit satisfaction
        let Some(&order) = order_map.get(&bundle.order_id) else {
            continue;
        };
        if !order.is_satisfied_at_price(fill_price) {
            continue;
        }

        let fill_qty = bundle.max_fill as Qty;
        if fill_qty == 0 {
            continue;
        }

        // Compute position delta using integer marginals
        let marginals = order.marginal_payoffs_i64();

        // Check if adding this bundle would create position imbalance
        // Only fill if all affected markets stay balanced (net_position stays at 0)
        // or if a matching counterparty has already been added.
        let mut would_imbalance = false;
        for &(market, marginal) in &marginals {
            let current = *net_position.get(&market).unwrap_or(&0);
            let new_val = current + marginal * fill_qty as i64;
            // Allow if it reduces imbalance or stays zero
            if new_val.abs() > current.abs() && new_val != 0 {
                would_imbalance = true;
                break;
            }
        }

        // If this bundle has non-zero verifier marginals and would increase imbalance, skip.
        // Bundles with all-zero verifier marginals (e.g., bundle_yes on 2+ markets) are safe.
        if would_imbalance {
            continue;
        }

        // Update net position
        for &(market, marginal) in &marginals {
            *net_position.entry(market).or_insert(0) += marginal * fill_qty as i64;
        }

        fills.push(Fill::new(bundle.order_id, fill_qty, fill_price));
    }

    fills
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{outcome_buy, outcome_sell, simple_yes_buy, MarketGroup};

    #[test]
    fn test_sigmoid() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-10);
        assert!(sigmoid(100.0) > 0.999);
        assert!(sigmoid(-100.0) < 0.001);
        assert_eq!(sigmoid(1000.0), 1.0);
        assert_eq!(sigmoid(-1000.0), 0.0);
    }

    #[test]
    fn test_smoothed_solver_basic() {
        let mut problem = Problem::new("test");
        let market = problem.markets.add_binary("test_market");

        // YES sellers (supply)
        problem.orders.push(outcome_sell(
            &problem.markets,
            100,
            market,
            0,
            400_000_000,
            500,
        ));

        // YES buyers (demand)
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            200,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            2,
            market,
            550_000_000,
            200,
        ));

        let solver = SmoothedSolver::default();
        let result = solver.solve(&problem);

        assert!(
            result.result.total_welfare >= 0,
            "Welfare should be non-negative"
        );
        assert!(
            !result.result.fills.is_empty(),
            "Should produce fills for crossing orders"
        );

        if let Some(ref pd) = result.price_discovery {
            if let Some(p) = pd.prices.get(&market) {
                assert!(
                    p[0] >= 350_000_000 && p[0] <= 600_000_000,
                    "YES price should be near crossing range, got {}",
                    p[0]
                );
            }
        }
    }

    #[test]
    fn test_smoothed_solver_with_group() {
        let mut problem = Problem::new("group_test");
        let m1 = problem.markets.add_binary("market_a");
        let m2 = problem.markets.add_binary("market_b");

        let mut group = MarketGroup::new("election");
        group.add_market(m1);
        group.add_market(m2);
        problem.add_market_group(group);

        // Market A: buyers at 60c, sellers at 30c
        problem
            .orders
            .push(outcome_sell(&problem.markets, 100, m1, 0, 300_000_000, 500));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, m1, 600_000_000, 200));

        // Market B: buyers at 50c, sellers at 20c
        problem
            .orders
            .push(outcome_sell(&problem.markets, 200, m2, 0, 200_000_000, 500));
        problem.orders.push(outcome_buy(
            &problem.markets,
            2,
            m2,
            0,
            500_000_000,
            200,
        ));

        let solver = SmoothedSolver::default();
        let result = solver.solve(&problem);

        assert!(result.price_discovery.is_some());

        // Welfare should be positive (trades should happen)
        assert!(
            result.result.total_welfare > 0,
            "Expected positive welfare, got {}",
            result.result.total_welfare
        );
        assert!(
            !result.result.fills.is_empty(),
            "Expected some fills"
        );
    }

    #[test]
    fn test_smoothed_solver_no_crossing() {
        // Buyer below seller — no trade should happen
        let mut problem = Problem::new("no_cross");
        let market = problem.markets.add_binary("test");

        problem.orders.push(outcome_sell(
            &problem.markets,
            100,
            market,
            0,
            700_000_000,
            500,
        ));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, market, 300_000_000, 200));

        let solver = SmoothedSolver::default();
        let result = solver.solve(&problem);

        assert!(
            result.result.fills.is_empty(),
            "No fills when buyer below seller"
        );
    }

    #[test]
    fn test_bundle_expected_payoff_standalone() {
        let npd = NANOS_PER_DOLLAR as f64;
        let m0 = MarketId::new(0);
        let m1 = MarketId::new(1);

        // Bundle YES on both markets: payoff = +1 when both YES
        let bundle = BundleInfo {
            order_id: 1,
            is_seller: false,
            limit_price: 200_000_000.0,
            max_fill: 100.0,
            num_states: 4,
            payoffs: vec![1, 0, 0, 0],
            marginal_payoffs: vec![(m0, 0.5), (m1, 0.5)],
            markets: vec![m0, m1],
            market_group_indices: vec![None, None],
            groups_in_bundle: vec![],
        };

        let prices = vec![500_000_000.0, 500_000_000.0]; // 50%, 50%

        let ep = bundle_expected_payoff(&bundle, &prices, npd);
        // Standalone: independence holds. Expected = 1 * NPD * (0.5 * 0.5) = 250M
        assert!(
            (ep - 250_000_000.0).abs() < 1.0,
            "Expected payoff should be ~250M, got {ep}"
        );
    }

    #[test]
    fn test_bundle_expected_payoff_grouped() {
        let npd = NANOS_PER_DOLLAR as f64;
        let m0 = MarketId::new(0); // standalone
        let m1 = MarketId::new(1); // in group 0
        let m2 = MarketId::new(2); // in group 0

        let prices = vec![500_000_000.0, 300_000_000.0, 400_000_000.0];

        // Bundle: payoff = +1 when m0=YES AND m1=YES AND m2=YES
        // But m1 and m2 can't both be YES → expected payoff = 0
        let bundle_impossible = BundleInfo {
            order_id: 1,
            is_seller: false,
            limit_price: 200_000_000.0,
            max_fill: 100.0,
            num_states: 8,
            payoffs: vec![1, 0, 0, 0, 0, 0, 0, 0],
            marginal_payoffs: vec![],
            markets: vec![m0, m1, m2],
            market_group_indices: vec![None, Some(0), Some(0)],
            groups_in_bundle: vec![(0, vec![1, 2])],
        };

        let ep = bundle_expected_payoff(&bundle_impossible, &prices, npd);
        assert!(
            ep.abs() < 1.0,
            "Bundle spanning mutually exclusive markets should have ~0 expected payoff, got {ep}"
        );

        // Bundle: payoff = +1 when m0=YES AND m1=YES (m2 not in bundle)
        // m1 is in group but m0 is standalone → valid joint state
        // Expected = P(m0=YES) * P(m1=YES) = 0.5 * 0.3 = 0.15
        let bundle_valid = BundleInfo {
            order_id: 2,
            is_seller: false,
            limit_price: 100_000_000.0,
            max_fill: 100.0,
            num_states: 4,
            payoffs: vec![1, 0, 0, 0],
            marginal_payoffs: vec![],
            markets: vec![m0, m1],
            market_group_indices: vec![None, Some(0)],
            groups_in_bundle: vec![(0, vec![1])],
        };

        let ep2 = bundle_expected_payoff(&bundle_valid, &prices, npd);
        // P(m0=YES) * P(m1=YES) = 0.5 * 0.3 = 0.15, expected = 0.15 * NPD = 150M
        assert!(
            (ep2 - 150_000_000.0).abs() < 1.0,
            "Bundle on standalone + single grouped market: expected ~150M, got {ep2}"
        );
    }

    #[test]
    fn test_bundle_surplus() {
        let m0 = MarketId::new(0);
        let m1 = MarketId::new(1);

        // Bundle buyer: limit > expected → positive surplus
        let bundle_buy = BundleInfo {
            order_id: 1,
            is_seller: false,
            limit_price: 300_000_000.0,
            max_fill: 100.0,
            num_states: 4,
            payoffs: vec![1, 0, 0, 0],
            marginal_payoffs: vec![(m0, 0.5), (m1, 0.5)],
            markets: vec![m0, m1],
            market_group_indices: vec![None, None],
            groups_in_bundle: vec![],
        };
        let surplus = bundle_surplus(&bundle_buy, 250_000_000.0);
        assert!(surplus > 0.0, "Buyer with limit > expected should have positive surplus");

        // Bundle seller: |expected| > limit → positive surplus
        let bundle_sell = BundleInfo {
            order_id: 2,
            is_seller: true,
            limit_price: 200_000_000.0,
            max_fill: 100.0,
            num_states: 4,
            payoffs: vec![-1, 0, 0, 0],
            marginal_payoffs: vec![(m0, -0.5), (m1, -0.5)],
            markets: vec![m0, m1],
            market_group_indices: vec![None, None],
            groups_in_bundle: vec![],
        };
        let surplus = bundle_surplus(&bundle_sell, -250_000_000.0);
        assert!(surplus > 0.0, "Seller with |expected| > limit should have positive surplus");
    }

    #[test]
    fn test_marginal_payoffs() {
        use matching_engine::Order;

        let m0 = MarketId::new(0);
        let m1 = MarketId::new(1);

        // Bundle YES [1, 0, 0, 0]: marginal = 0.5 per market (non-separable)
        let mut order = Order::new(1);
        order.markets[0] = m0;
        order.markets[1] = m1;
        order.num_markets = 2;
        order.payoffs[0] = 1;
        order.num_states = 4;
        let mp = order.marginal_payoffs_f64();
        assert_eq!(mp.len(), 2);
        assert!(mp.iter().any(|&(m, v)| m == m0 && (v - 0.5).abs() < 1e-10));
        assert!(mp.iter().any(|&(m, v)| m == m1 && (v - 0.5).abs() < 1e-10));

        // Spread [0, -1, 1, 0]: long A (+1), short B (-1)
        let mut order = Order::new(2);
        order.markets[0] = m0;
        order.markets[1] = m1;
        order.num_markets = 2;
        order.payoffs[0] = 0;
        order.payoffs[1] = -1;
        order.payoffs[2] = 1;
        order.payoffs[3] = 0;
        order.num_states = 4;
        let mp = order.marginal_payoffs_f64();
        assert_eq!(mp.len(), 2);
        assert!(mp.iter().any(|&(m, v)| m == m0 && (v - 1.0).abs() < 1e-10));
        assert!(mp.iter().any(|&(m, v)| m == m1 && (v - (-1.0)).abs() < 1e-10));

        // Single market YES [1, 0]: marginal = 1.0
        let mut order = Order::new(3);
        order.markets[0] = m0;
        order.num_markets = 1;
        order.payoffs[0] = 1;
        order.num_states = 2;
        let mp = order.marginal_payoffs_f64();
        assert_eq!(mp.len(), 1);
        assert!(mp.iter().any(|&(m, v)| m == m0 && (v - 1.0).abs() < 1e-10));
    }

    #[test]
    fn test_smoothed_solver_with_mm() {
        use matching_engine::{MmConstraint, MmId};

        let mut problem = Problem::new("mm_test");
        let market = problem.markets.add_binary("test_market");

        // Regular YES buyers (demand) — more than supply, creating a deficit
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, market, 600_000_000, 300));

        // Regular YES seller (supply) — less than demand
        problem.orders.push(outcome_sell(
            &problem.markets,
            2,
            market,
            0,
            300_000_000,
            100,
        ));

        // MM YES buyer at 55c
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 10, market, 550_000_000, 100));

        // MM YES seller at 35c
        problem.orders.push(outcome_sell(
            &problem.markets,
            11,
            market,
            0,
            350_000_000,
            200,
        ));

        let mut mm = MmConstraint::new(MmId(1), 100_000_000_000); // $100 budget
        mm.add_order(10, MmSide::BuyYes);
        mm.add_order(11, MmSide::SellYes);
        problem.mm_constraints.push(mm);

        let solver = SmoothedSolver::default();
        let result = solver.solve(&problem);

        assert!(
            result.result.total_welfare >= 0,
            "Welfare should be non-negative"
        );
        // MM orders should participate in fills
        let mm_fills: Vec<_> = result
            .result
            .fills
            .iter()
            .filter(|f| f.order_id == 10 || f.order_id == 11)
            .collect();
        assert!(
            !mm_fills.is_empty(),
            "MM orders should participate in fills with sufficient budget"
        );
    }
}
