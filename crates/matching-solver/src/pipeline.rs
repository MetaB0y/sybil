//! Pipeline configuration for FBA solving.
//!
//! The pipeline provides a flexible way to combine solver components:
//! - Price discovery (LocalSolver)
//! - Order allocation (MmAllocator)
//! - Partial solvers (MILP, etc.) for alternative solutions
//!
//! # Example
//!
//! ```ignore
//! let pipeline = Pipeline::with_dual_decomposition();
//! let result = pipeline.solve(&problem);
//! ```

use std::collections::HashMap;
use std::time::Instant;

use serde::Serialize;
use tracing::debug;

use matching_engine::{MarketId, MmConstraint, Nanos, Order, Problem, Qty};

use crate::dual_master::{DualConfig, DualMaster};

/// Statistics from combining solutions.
#[derive(Clone, Debug, Default, Serialize)]
pub struct CombineStats {
    /// Number of solutions combined
    pub num_solutions: usize,
    /// Total fills across all solutions
    pub total_fills_input: usize,
    /// Fills selected in final result
    pub fills_selected: usize,
    /// Conflicts detected in graph
    pub conflicts_detected: usize,
    /// Welfare before combining
    pub input_max_welfare: i64,
    /// Final welfare after combining
    pub output_welfare: i64,
    /// Time spent building conflict graph (seconds)
    pub conflict_graph_time_secs: f64,
    /// Time spent solving MWIS (seconds)
    pub mwis_time_secs: f64,
}

/// Contribution tracking for a solver.
#[derive(Clone, Debug, Serialize)]
pub struct SolverContribution {
    /// Name of the solver
    pub solver_name: String,
    /// Number of fills contributed to final solution
    pub fills_contributed: usize,
    /// Welfare contributed by this solver's fills
    pub welfare_contributed: i64,
}
use crate::local_solver::LocalSolver;
use crate::mm_allocator::MmAllocator;
use crate::specialized::MultiMarketSolver;
use crate::traits::{
    AllocationResult, OrderAllocator, PartialSolver, PriceDiscoverer, PriceDiscoveryResult,
};
use crate::{MatchingResult, Solver};

#[cfg(feature = "milp")]
use crate::milp::MilpSolver;

// ============================================================================
// Helpers
// ============================================================================

/// Estimate fills for MM orders that weren't matched in price discovery.
///
/// For each MM order not already in `fills`, checks if it would be willing
/// to trade at the clearing price and inserts an estimated fill if so.
fn estimate_mm_fills(
    mm_constraints: &[MmConstraint],
    order_map: &HashMap<u64, &Order>,
    prices: &PriceDiscoveryResult,
    fills: &mut HashMap<u64, (Nanos, Qty)>,
) {
    #[allow(clippy::map_entry)]
    for mm in mm_constraints {
        for &order_id in &mm.order_ids {
            if !fills.contains_key(&order_id) {
                if let Some(order) = order_map.get(&order_id) {
                    if order.num_markets == 1 {
                        let market = order.markets[0];
                        if let Some(market_prices) = prices.prices.get(&market) {
                            let num_states = order.num_states as usize;
                            let is_buyer = order.payoffs[..num_states].iter().any(|&p| p > 0);
                            let is_seller = order.payoffs[..num_states].iter().any(|&p| p < 0);

                            let price = if is_buyer {
                                let o = order.payoffs[..num_states]
                                    .iter()
                                    .position(|&p| p > 0)
                                    .unwrap_or(0);
                                market_prices.get(o).copied().unwrap_or(500_000_000)
                            } else if is_seller {
                                let o = order.payoffs[..num_states]
                                    .iter()
                                    .position(|&p| p < 0)
                                    .unwrap_or(0);
                                market_prices.get(o).copied().unwrap_or(500_000_000)
                            } else {
                                continue;
                            };

                            if order.is_satisfied_at_price(price) {
                                // Conservative estimate: 80% of max fill
                                let estimated_qty = order.max_fill * 4 / 5;
                                fills.insert(order_id, (price, estimated_qty));
                            }
                        }
                    }
                }
            }
        }
    }
}

// ============================================================================
// Pipeline Configuration
// ============================================================================

/// Configuration options for the pipeline.
#[derive(Clone, Debug, Serialize)]
pub struct PipelineConfig {
    /// Whether to use fixed-point iteration between pricing and allocation.
    pub use_fixed_point: bool,

    /// Maximum iterations for fixed-point convergence.
    pub max_iterations: usize,

    /// Convergence threshold for fixed-point (welfare change).
    pub convergence_threshold: f64,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            use_fixed_point: false,
            max_iterations: 5,
            convergence_threshold: 0.01,
        }
    }
}

// ============================================================================
// Pipeline Result
// ============================================================================

/// Result from running a pipeline.
#[derive(Clone, Debug, Serialize)]
pub struct PipelineResult {
    /// The final combined matching result.
    pub result: MatchingResult,

    /// Price discovery result (if applicable).
    pub price_discovery: Option<PriceDiscoveryResult>,


    /// Allocation result (if applicable).
    pub allocation: Option<AllocationResult>,

    /// Per-solver contributions to the final result.
    pub contributions: Vec<SolverContribution>,

    /// Statistics from combining.
    pub combine_stats: Option<CombineStats>,

    /// Number of fixed-point iterations (if applicable).
    pub iterations: usize,

    /// Per-iteration stats for convergence analysis.
    pub iteration_stats: Vec<IterationStats>,

    /// Total time spent (seconds).
    pub total_time_secs: f64,

    /// Time breakdown by phase.
    pub phase_times: PipelineTimings,

    /// Diagnostics from enforce_ucp (if it ran).
    pub ucp_stats: Option<UcpStats>,

    /// Synthetic arb orders from group minting (needed for witness/verification).
    #[serde(skip)]
    pub group_minting_arb_orders: Vec<matching_engine::Order>,

    /// Per-phase snapshots for detailed visualization (viz feature only).
    #[cfg(feature = "viz")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub phase_snapshots: Vec<crate::viz::PhaseSnapshot>,
}

/// Timing breakdown for pipeline phases.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PipelineTimings {
    pub price_discovery_secs: f64,
    pub allocation_secs: f64,
    pub partial_solving_secs: f64,
    pub combining_secs: f64,
}

/// Diagnostics from enforce_ucp (Uniform Clearing Price enforcement).
#[derive(Clone, Debug, Default, Serialize)]
pub struct UcpStats {
    pub input_fills: usize,
    pub input_welfare: i64,
    pub after_reprice_fills: usize,
    pub dropped_by_reprice: usize,
    pub after_trim_fills: usize,
    pub dropped_by_trim: usize,
    pub final_fills: usize,
    pub final_welfare: i64,
    pub welfare_retention_pct: f64,
    /// Per-market position imbalance before trimming: (market_id, yes_qty, no_qty, excess)
    pub market_imbalances: Vec<(MarketId, u64, u64, u64)>,
}

/// Stats for a single fixed-point iteration.
#[derive(Clone, Debug, Default, Serialize)]
pub struct IterationStats {
    /// Iteration number (1-indexed).
    pub iteration: usize,
    /// Total welfare after this iteration.
    pub welfare: i64,
    /// Total volume (shares) after this iteration.
    pub volume: u64,
    /// Total fills after this iteration.
    pub fills: usize,
    /// Welfare delta from previous iteration.
    pub welfare_delta: i64,
    /// Volume delta from previous iteration.
    pub volume_delta: u64,
    /// Fills delta from previous iteration.
    pub fills_delta: usize,
    /// Breakdown: fills from price discovery.
    pub price_discovery_fills: usize,
    /// Breakdown: fills from bundle matching.
    pub bundle_fills: usize,
    /// Index of first fill in this iteration (into PipelineResult.result.fills).
    pub fill_start_idx: usize,
    /// Index after last fill in this iteration.
    pub fill_end_idx: usize,
    /// Per-market clearing prices for this iteration.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub market_prices: HashMap<MarketId, Vec<Nanos>>,
}

impl PipelineResult {
    /// Create an empty result.
    pub fn empty() -> Self {
        Self {
            result: MatchingResult::new(),
            price_discovery: None,
            allocation: None,
            contributions: Vec::new(),
            combine_stats: None,
            iterations: 0,
            iteration_stats: Vec::new(),
            total_time_secs: 0.0,
            phase_times: PipelineTimings::default(),
            ucp_stats: None,
            group_minting_arb_orders: Vec::new(),
            #[cfg(feature = "viz")]
            phase_snapshots: Vec::new(),
        }
    }
}

// ============================================================================
// Pipeline
// ============================================================================

/// A configured pipeline for solving matching problems.
pub struct Pipeline {
    /// Name for identification in benchmarks.
    name: String,

    /// Price discovery component (optional).
    price_discoverer: Option<Box<dyn PriceDiscoverer>>,

    /// Multi-market repricing solver (optional).
    /// Runs after price discovery to handle bundles/spreads via direct price-shifting.
    multi_market_solver: Option<MultiMarketSolver>,


    /// Order allocator (optional).
    allocator: Option<Box<dyn OrderAllocator>>,

    /// Dual decomposition master (optional).
    /// When present, `solve` uses `solve_dual_decomposition` instead of
    /// `solve_sequential` / `solve_single_pass`.
    dual_master: Option<DualMaster>,

    /// Partial solvers for MWIS combination.
    partial_solvers: Vec<Box<dyn PartialSolver>>,

    /// Pipeline configuration.
    config: PipelineConfig,
}

impl Pipeline {
    /// Create a pipeline with the current default approach.
    ///
    /// Uses dual decomposition for principled price consistency and MM budget handling.
    pub fn current() -> Self {
        Self::with_dual_decomposition()
    }

    /// Create a pipeline with only local price discovery and MM allocation.
    ///
    /// No cross-market price consistency. Useful for benchmarks or simple problems.
    pub fn local_only() -> Self {
        Self::builder()
            .name("Local Only")
            .price_discoverer(LocalSolver::new())
            .allocator(MmAllocator::new())
            .build()
    }

    /// Create a full platform pipeline with all solvers.
    #[cfg(feature = "milp")]
    pub fn full_platform() -> Self {
        Self::builder()
            .name("Full Platform")
            .partial_solver(MilpSolver::with_timeout(1.0))
            .build()
    }

    /// Create a full platform pipeline without MILP feature.
    /// Falls back to current() since there are no partial solvers.
    #[cfg(not(feature = "milp"))]
    pub fn full_platform() -> Self {
        Self::current()
    }

    /// Create an iterative pipeline with fixed-point iteration.
    ///
    /// Iterates between price discovery and allocation until convergence.
    pub fn iterative() -> Self {
        Self::builder()
            .name("Iterative")
            .price_discoverer(LocalSolver::new())
            .allocator(MmAllocator::new())
            .multi_market_solver(MultiMarketSolver::new())
            .use_fixed_point(true)
            .max_iterations(5)
            .build()
    }


    /// Create a pipeline using dual decomposition.
    ///
    /// Uses Lagrangian relaxation with subgradient updates to handle
    /// coupling constraints (price consistency across MarketGroups,
    /// MM budget limits) in a principled way.
    ///
    /// MultiMarketSolver runs inside the dual loop for bundle repricing,
    /// Price consistency is handled via Lagrangian multipliers (lambda).
    pub fn with_dual_decomposition() -> Self {
        Self::builder()
            .name("Dual Decomposition")
            .price_discoverer(LocalSolver::new())
            .dual_master(DualMaster::new().with_multi_market_solver(MultiMarketSolver::new()))
            .build()
    }

    /// Create a pipeline using dual decomposition with custom config.
    pub fn with_dual_decomposition_config(config: DualConfig) -> Self {
        Self::builder()
            .name("Dual Decomposition")
            .price_discoverer(LocalSolver::new())
            .dual_master(
                DualMaster::with_config(config).with_multi_market_solver(MultiMarketSolver::new()),
            )
            .build()
    }

    /// Create a new pipeline builder.
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder::new()
    }

    /// Solve a matching problem using this pipeline.
    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        debug_assert!(
            problem.validate().is_ok(),
            "Problem validation failed: {:?}",
            problem.validate().unwrap_err()
        );

        if self.dual_master.is_some() {
            self.solve_dual_decomposition(problem)
        } else {
            self.solve_sequential(problem)
        }
    }

    /// Dual decomposition solving.
    ///
    /// Uses Lagrangian relaxation to handle coupling constraints:
    /// 1. DualMaster iterates to find equilibrium prices
    /// 2. Partial solvers handle multi-market orders
    #[tracing::instrument(skip_all, name = "pipeline.dual")]
    fn solve_dual_decomposition(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();
        let mut result = PipelineResult::empty();
        let mut timings = PipelineTimings::default();

        let dual_master = self.dual_master.as_ref().expect("dual_master must be set");

        // Phase 1: Dual decomposition — handles single-market clearing with
        // price consistency and budget constraints
        let pd_start = Instant::now();
        let dual_result = dual_master.solve(problem);
        timings.price_discovery_secs = pd_start.elapsed().as_secs_f64();
        debug!(
            dual_iters = dual_result.iterations,
            fills = dual_result.matching_result.fills.len(),
            welfare = dual_result.matching_result.total_welfare,
            pd_secs = timings.price_discovery_secs,
            "dual decomposition complete"
        );

        // Transfer fills from dual decomposition
        result.result = dual_result.matching_result;
        result.price_discovery = Some(dual_result.prices.clone());

        // Track filled orders for partial solvers (mutable — updated as solvers run)
        let mut filled_order_ids: std::collections::HashSet<u64> =
            result.result.fills.iter().map(|f| f.order_id).collect();

        let order_map: std::collections::HashMap<u64, &matching_engine::Order> =
            problem.orders.iter().map(|o| (o.id, o)).collect();

        // Phase 2: Bundle/Spread matching (partial solvers)
        // Note: MultiMarketSolver now runs inside DualMaster's loop (2.2)
        let mut bundle_fills = 0usize;
        let partial_start = Instant::now();
        let pd_fills = result.result.fills.len();
        for solver in &self.partial_solvers {
            let partial_orders: Vec<_> = problem
                .orders
                .iter()
                .filter(|o| !filled_order_ids.contains(&o.id))
                .cloned()
                .collect();

            let partial_problem = Problem {
                name: problem.name.clone(),
                markets: problem.markets.clone(),
                orders: partial_orders,
                mm_constraints: problem.mm_constraints.clone(),
                market_groups: problem.market_groups.clone(),
            };

            let partial_result = solver.solve_partial(&partial_problem);

            for fill in partial_result.fills {
                if let Some(&order) = order_map.get(&fill.order_id) {
                    if !order.is_satisfied_at_price(fill.fill_price) {
                        continue;
                    }
                    filled_order_ids.insert(fill.order_id);
                    result.result.add_fill(fill.clone(), order);
                    bundle_fills += 1;
                    result.contributions.push(SolverContribution {
                        solver_name: solver.name().to_string(),
                        fills_contributed: 1,
                        welfare_contributed: order
                            .welfare_contribution(fill.fill_price, fill.fill_qty),
                    });
                }
            }
        }
        timings.partial_solving_secs = partial_start.elapsed().as_secs_f64();

        // Record iteration stats from dual decomposition
        result.iteration_stats.push(IterationStats {
            iteration: 1,
            welfare: result.result.total_welfare,
            volume: result.result.fills.iter().map(|f| f.fill_qty).sum(),
            fills: result.result.fills.len(),
            welfare_delta: result.result.total_welfare,
            volume_delta: result.result.fills.iter().map(|f| f.fill_qty).sum(),
            fills_delta: result.result.fills.len(),
            price_discovery_fills: pd_fills,
            bundle_fills,
            fill_start_idx: 0,
            fill_end_idx: result.result.fills.len(),
            market_prices: dual_result.prices.prices.clone(),
        });

        result.phase_times = timings;
        result.total_time_secs = start.elapsed().as_secs_f64();
        result.iterations = dual_result.iterations;

        // Phase 3: Group minting — fill residual buy-YES demand via group-level minting.
        //
        // Two stages:
        // 3a. Water-filling at DualMaster's clearing prices — fills unfilled buyers
        //     using virtual supply from group minting. Doesn't change prices.
        // 3b. Simplex price search — finds better prices on the Σp = $1 surface.
        //     Changes clearing prices. Only kept if post-UCP welfare improves.
        let mut arb_orders: Vec<matching_engine::Order> = Vec::new();
        if !problem.market_groups.is_empty() {
            if let Some(ref pd) = result.price_discovery {
                let max_real_id = problem.orders.iter().map(|o| o.id).max().unwrap_or(0);
                let mut next_arb_id = max_real_id + 2_000_000_000;
                let npd = matching_engine::NANOS_PER_DOLLAR;

                // Stage 3a: Water-filling group minting at DualMaster prices
                for group in &problem.market_groups {
                    if group.markets.len() < 2 {
                        continue;
                    }

                    let market_prices: Vec<(MarketId, Nanos)> = group
                        .markets
                        .iter()
                        .filter_map(|&m| {
                            pd.prices.get(&m).and_then(|p| p.first()).map(|&p| (m, p))
                        })
                        .collect();

                    if market_prices.len() != group.markets.len() {
                        continue;
                    }

                    let price_sum: u128 =
                        market_prices.iter().map(|(_, p)| *p as u128).sum();
                    if price_sum < npd as u128 {
                        continue; // Not profitable: Σp < $1
                    }

                    // Collect unfilled buy-YES orders per market, sorted by limit desc
                    let mut demand_per_market: Vec<(MarketId, Nanos, Vec<(u64, u64)>)> =
                        Vec::new();

                    for &(market, cp) in &market_prices {
                        let mut eligible: Vec<(u64, u64)> = Vec::new();
                        for (&oid, &order) in &order_map {
                            if filled_order_ids.contains(&oid) {
                                continue;
                            }
                            if order.num_markets != 1 || order.num_states != 2 {
                                continue;
                            }
                            if order.markets[0] != market {
                                continue;
                            }
                            if order.payoffs[0] <= 0 || order.payoffs[1] != 0 {
                                continue;
                            }
                            if order.limit_price < cp {
                                continue;
                            }
                            eligible.push((oid, order.max_fill));
                        }
                        eligible.sort_by(|a, b| {
                            order_map[&b.0]
                                .limit_price
                                .cmp(&order_map[&a.0].limit_price)
                        });
                        demand_per_market.push((market, cp, eligible));
                    }

                    // Q* = min eligible demand across markets (linking constraint)
                    let q_star = demand_per_market
                        .iter()
                        .map(|(_, _, orders)| orders.iter().map(|(_, q)| q).sum::<u64>())
                        .min()
                        .unwrap_or(0);

                    if q_star == 0 {
                        continue;
                    }

                    // Fill Q* units on each market + create arb sell-YES for position balance
                    for &(market, cp, ref orders) in &demand_per_market {
                        let mut remaining = q_star;
                        for &(oid, max_fill) in orders {
                            if remaining == 0 {
                                break;
                            }
                            let fill_qty = remaining.min(max_fill);
                            let order = order_map[&oid];
                            filled_order_ids.insert(oid);
                            result.result.add_fill(
                                matching_engine::Fill {
                                    order_id: oid,
                                    fill_price: cp,
                                    fill_qty,
                                },
                                order,
                            );
                            remaining -= fill_qty;
                        }

                        // Arb limit = proportional minting cost: $1 × p_m / Σp
                        let arb_limit =
                            ((npd as u128 * cp as u128) / price_sum) as Nanos;
                        let arb_id = next_arb_id;
                        next_arb_id += 1;

                        let mut arb_order = matching_engine::Order::new(arb_id);
                        arb_order.markets[0] = market;
                        arb_order.num_markets = 1;
                        arb_order.num_states = 2;
                        arb_order.payoffs[0] = -1;
                        arb_order.payoffs[1] = 0;
                        arb_order.limit_price = arb_limit;
                        arb_order.max_fill = q_star;

                        result.result.add_fill(
                            matching_engine::Fill {
                                order_id: arb_id,
                                fill_price: cp,
                                fill_qty: q_star,
                            },
                            &arb_order,
                        );
                        arb_orders.push(arb_order);
                    }
                }

                // Stage 3b: Simplex price search — try to find better prices on Σp = $1.
                // Snapshot baseline state, apply simplex, simulate enforce_ucp on both
                // versions, and only keep simplex if post-UCP welfare strictly improves.
                let mm_order_ids: std::collections::HashSet<u64> = problem
                    .mm_constraints
                    .iter()
                    .flat_map(|mm| mm.order_ids.iter().copied())
                    .collect();

                let simplex_results = crate::group_minting::simplex_search(
                    &problem.market_groups,
                    &problem.orders,
                    &problem.markets,
                    &result.result.fills,
                    &order_map,
                    &mm_order_ids,
                    &mut next_arb_id,
                );

                if !simplex_results.is_empty() {
                    // Save baseline state before applying simplex
                    let baseline_fills = result.result.fills.clone();
                    let baseline_arb_orders = arb_orders.clone();
                    let baseline_prices = result
                        .price_discovery
                        .as_ref()
                        .map(|pd| pd.prices.clone())
                        .unwrap_or_default();

                    // Apply simplex results
                    for sr in simplex_results {
                        let replaced_set: std::collections::HashSet<u64> =
                            sr.replaced_order_ids.iter().copied().collect();
                        let replaced_markets: std::collections::HashSet<MarketId> =
                            sr.clearing_prices.keys().copied().collect();

                        // Remove replaced non-MM fills
                        result
                            .result
                            .fills
                            .retain(|f| !replaced_set.contains(&f.order_id));

                        // Remove old arb fills/orders on replaced markets
                        let old_arb_ids: std::collections::HashSet<u64> = arb_orders
                            .iter()
                            .filter(|o| replaced_markets.contains(&o.markets[0]))
                            .map(|o| o.id)
                            .collect();
                        result
                            .result
                            .fills
                            .retain(|f| !old_arb_ids.contains(&f.order_id));
                        arb_orders
                            .retain(|o| !replaced_markets.contains(&o.markets[0]));

                        // Add simplex fills
                        for fill in &sr.fills {
                            if let Some(&order) = order_map.get(&fill.order_id) {
                                result.result.add_fill(fill.clone(), order);
                            }
                        }
                        for (arb_order, arb_fill) in
                            sr.arb_orders.iter().zip(&sr.arb_fills)
                        {
                            result.result.add_fill(arb_fill.clone(), arb_order);
                            arb_orders.push(arb_order.clone());
                        }

                        // Update clearing prices for simplex markets
                        if let Some(ref mut pd) = result.price_discovery {
                            for (&market, &price) in &sr.clearing_prices {
                                let no_price = npd.saturating_sub(price);
                                pd.prices.insert(market, vec![price, no_price]);
                            }
                        }
                    }

                    let simplex_prices = result
                        .price_discovery
                        .as_ref()
                        .map(|pd| pd.prices.clone())
                        .unwrap_or_default();

                    // Simulate enforce_ucp on both versions to compare post-UCP welfare
                    let baseline_welfare = Self::simulate_enforce_ucp(
                        &baseline_fills,
                        &baseline_prices,
                        &order_map,
                        &baseline_arb_orders,
                    );

                    let simplex_welfare = Self::simulate_enforce_ucp(
                        &result.result.fills,
                        &simplex_prices,
                        &order_map,
                        &arb_orders,
                    );

                    if simplex_welfare <= baseline_welfare {
                        // Simplex didn't help after UCP — revert to baseline
                        result.result.fills = baseline_fills;
                        arb_orders = baseline_arb_orders;
                        if let Some(ref mut pd) = result.price_discovery {
                            pd.prices = baseline_prices;
                        }
                        debug!(
                            simplex_welfare,
                            baseline_welfare,
                            "simplex reverted: post-UCP welfare did not improve"
                        );
                    } else {
                        debug!(
                            simplex_welfare,
                            baseline_welfare,
                            gain = simplex_welfare - baseline_welfare,
                            "simplex accepted: post-UCP welfare improved"
                        );
                    }
                }
            }
        }

        // Build order map including arb orders for enforce_ucp lookups
        let mut order_map_with_arbs = order_map;
        for arb in &arb_orders {
            order_map_with_arbs.insert(arb.id, arb);
        }

        // Enforce UCP: re-price all single-market fills at the final clearing price.
        Self::enforce_ucp(&mut result, &order_map_with_arbs);

        // Store arb orders on result for witness/verification (after enforce_ucp)
        result.group_minting_arb_orders = arb_orders;

        // Gate: if total welfare is negative, return empty result.
        // Negative welfare means fills are collectively value-destroying.
        if result.result.total_welfare < 0 {
            result.result = MatchingResult::new();
        }

        result
    }

    /// Sequential solving with fixed-point iteration.
    ///
    /// Runs phases in order, each consuming liquidity:
    /// 1. Price Discovery (LocalSolver) - fills single-market orders
    /// 2. MM Allocation - activates MM orders within budget
    /// 4. Partial Solvers - fills bundles/spreads
    ///    Repeats until convergence or max iterations.
    #[tracing::instrument(skip_all, name = "pipeline.sequential")]
    fn solve_sequential(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();
        let mut result = PipelineResult::empty();
        let mut timings = PipelineTimings::default();

        let mut prev_welfare = 0i64;
        let mut prev_volume = 0u64;
        let mut prev_fills = 0usize;
        let mut iterations = 0;

        // Accumulate arbitrage orders across all iterations

        // All arb orders for order lookup (owned, since they're not in problem.orders)
        let arb_order_map: std::collections::HashMap<u64, matching_engine::Order> =
            std::collections::HashMap::new();

        // Track next arbitrage order ID across iterations

        // Build market_names map for phase snapshots (viz feature)
        #[cfg(feature = "viz")]
        let market_names: std::collections::HashMap<matching_engine::MarketId, String> = problem
            .markets
            .iter()
            .map(|m| (m.id, m.name.clone()))
            .collect();

        #[cfg(feature = "viz")]
        let mut phase_snapshots: Vec<crate::viz::PhaseSnapshot> = Vec::new();

        // Capture initial phase snapshot
        #[cfg(feature = "viz")]
        phase_snapshots.push(crate::viz::PhaseSnapshot::capture(
            crate::viz::PipelinePhase::Initial,
            0,
            &market_names,
            0,
            0,
            start.elapsed().as_secs_f64(),
        ));

        // Track orders that have already been filled
        let mut filled_order_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();

        // Track cumulative fills for MM budget tracking across iterations
        let mut cumulative_mm_fills: std::collections::HashMap<
            u64,
            (matching_engine::Nanos, matching_engine::Qty),
        > = std::collections::HashMap::new();

        // Build order lookup map
        let order_map: std::collections::HashMap<u64, &matching_engine::Order> =
            problem.orders.iter().map(|o| (o.id, o)).collect();

        // Build MM order IDs set ONCE
        let mm_order_ids: std::collections::HashSet<u64> = problem
            .mm_constraints
            .iter()
            .flat_map(|mm| mm.order_ids.iter().copied())
            .collect();

        let effective_max_iterations = if self.config.use_fixed_point {
            self.config.max_iterations
        } else {
            1
        };

        for iter in 0..effective_max_iterations {
            iterations = iter + 1;

            // Track fills for this iteration
            let fill_start_idx = result.result.fills.len();
            let mut iter_price_discovery_fills = 0usize;
            let mut iter_bundle_fills = 0usize;

            // Filter out already-filled orders (MM orders stay included for price discovery)
            let remaining_orders: Vec<_> = problem
                .orders
                .iter()
                .filter(|o| !filled_order_ids.contains(&o.id))
                .cloned()
                .collect();

            // Create problem view with unfilled orders
            let iter_problem = Problem {
                name: problem.name.clone(),
                markets: problem.markets.clone(),
                orders: remaining_orders,
                mm_constraints: problem.mm_constraints.clone(),
                market_groups: problem.market_groups.clone(),
            };

            // Phase 1: Price Discovery
            let price_result = if let Some(ref discoverer) = self.price_discoverer {
                let pd_start = Instant::now();
                let pd_result = discoverer.discover_prices(&iter_problem);
                timings.price_discovery_secs += pd_start.elapsed().as_secs_f64();
                Some(pd_result)
            } else {
                None
            };

            // Capture after price discovery
            // NOTE: At this point, fills aren't confirmed yet - we show POTENTIAL fills as phase_fills
            // The main fills_count/welfare show confirmed state (from previous iterations)
            #[cfg(feature = "viz")]
            {
                let pd_fills = price_result.as_ref().map(|p| p.total_fills).unwrap_or(0);
                let pd_welfare = price_result.as_ref().map(|p| p.total_welfare).unwrap_or(0);
                let markets_priced = price_result.as_ref().map(|p| p.prices.len()).unwrap_or(0);
                phase_snapshots.push(crate::viz::PhaseSnapshot::capture_with_phase_data(
                    crate::viz::PipelinePhase::PriceDiscovery,
                    iterations,
                    &market_names,
                    result.result.fills.len(),   // Confirmed fills so far
                    result.result.total_welfare, // Confirmed welfare so far
                    start.elapsed().as_secs_f64(),
                    Some(pd_fills), // POTENTIAL fills from this phase (not yet confirmed)
                    Some(pd_welfare), // POTENTIAL welfare (not yet confirmed)
                    Some(crate::viz::PhaseMetadata::PriceDiscovery { markets_priced }),
                ));
            }

            // Phase 2: Multi-Market Repricing
            // Injects bundle leg demand into per-market curves and re-clears.
            // Updates price_result in place for affected markets.
            let mut price_result = price_result;
            let mut repricing_bundle_fills: Vec<matching_engine::Fill> = Vec::new();
            if let (Some(ref mm_solver), Some(ref pd_result)) =
                (&self.multi_market_solver, &price_result)
            {
                let repricing_result = mm_solver.solve_with_repricing(&iter_problem, pd_result);

                if repricing_result.bundles_matched > 0 {
                    // Update price_result with repriced market solutions
                    if let Some(ref mut pd) = price_result {
                        for (mid, sol) in &repricing_result.repriced_solutions {
                            // Update welfare: subtract old, add new
                            if let Some(old_sol) = pd.market_solutions.get(mid) {
                                pd.total_welfare -= old_sol.welfare;
                                pd.total_fills -= old_sol.fills.len();
                            }
                            pd.total_welfare += sol.welfare;
                            pd.total_fills += sol.fills.len();
                            pd.prices.insert(*mid, sol.prices.clone());
                            pd.market_solutions.insert(*mid, sol.clone());
                        }
                    }

                    repricing_bundle_fills = repricing_result.bundle_fills;
                }
            }

            // Phase 2: MM Allocation
            let allocation_result =
                if let (Some(ref allocator), Some(ref prices)) = (&self.allocator, &price_result) {
                    let alloc_start = Instant::now();

                    // Build fills map from price discovery (current iteration only)
                    let mut current_fills: HashMap<u64, (Nanos, Qty)> = prices
                        .all_fills()
                        .into_iter()
                        .map(|f| (f.order_id, (f.fill_price, f.fill_qty)))
                        .collect();

                    // Add MM orders that weren't matched in price discovery with estimated fills.
                    estimate_mm_fills(
                        &problem.mm_constraints,
                        &order_map,
                        prices,
                        &mut current_fills,
                    );

                    // Merge with cumulative fills for budget calculation
                    // Cumulative fills represent already-committed capital from previous iterations
                    let mut all_fills = cumulative_mm_fills.clone();
                    for (id, fill) in &current_fills {
                        all_fills.entry(*id).or_insert(*fill);
                    }

                    // Create adjusted MM constraints with reduced budgets based on cumulative usage
                    let adjusted_constraints: Vec<matching_engine::MmConstraint> = problem
                        .mm_constraints
                        .iter()
                        .map(|mm| {
                            let used_capital = mm.capital_used(&cumulative_mm_fills);
                            let remaining_budget = mm.max_capital.saturating_sub(used_capital);
                            matching_engine::MmConstraint {
                                mm_id: mm.mm_id,
                                max_capital: remaining_budget,
                                order_ids: mm.order_ids.clone(),
                                order_sides: mm.order_sides.clone(),
                            }
                        })
                        .collect();

                    // Pass iter_problem orders (includes arb orders) so allocator activates them
                    let alloc_result = allocator.allocate(
                        &adjusted_constraints,
                        &prices.prices,
                        &iter_problem.orders,
                        &current_fills,
                    );
                    timings.allocation_secs += alloc_start.elapsed().as_secs_f64();
                    Some(alloc_result)
                } else {
                    None
                };

            // Build result from price discovery + allocation
            if let Some(ref pd_result) = price_result {
                let iter_result =
                    self.build_result_from_prices(&iter_problem, pd_result, &allocation_result);

                // Track filled order IDs.
                // Look up in both original orders and arb orders.
                // Arb fills are NOT added to the output — they are synthetic price-pressure
                // mechanisms with no real account behind them.
                for fill in &iter_result.fills {
                    let is_arb = arb_order_map.contains_key(&fill.order_id);
                    let order_ref = order_map
                        .get(&fill.order_id)
                        .copied()
                        .or_else(|| arb_order_map.get(&fill.order_id));
                    if let Some(_order) = order_ref {
                        filled_order_ids.insert(fill.order_id);
                        if !is_arb {
                            iter_price_discovery_fills += 1;
                        }

                        // Track MM fills for cumulative budget calculation
                        if mm_order_ids.contains(&fill.order_id) {
                            cumulative_mm_fills
                                .insert(fill.order_id, (fill.fill_price, fill.fill_qty));
                        }
                    }
                }

                // Merge ALL fills (including arb) into result.
                // Arb fills are essential for position balance: they represent
                // minting + arbitrage that ensures YES qty == NO qty per market.
                // Settlement assigns arb fills to a system/house account.
                for fill in iter_result.fills {
                    let order_ref = order_map
                        .get(&fill.order_id)
                        .copied()
                        .or_else(|| arb_order_map.get(&fill.order_id));
                    if let Some(order) = order_ref {
                        result.result.add_fill(fill, order);
                    }
                }
            }

            // Add bundle fills from repricing (Phase 2)
            for fill in repricing_bundle_fills {
                if let Some(&order) = order_map.get(&fill.order_id) {
                    if !order.is_satisfied_at_price(fill.fill_price) {
                        continue;
                    }
                    filled_order_ids.insert(fill.order_id);
                    result.result.add_fill(fill, order);
                    iter_bundle_fills += 1;
                }
            }

            // Per-iteration UCP: reprice fills from THIS iteration at current prices.
            // This ensures fills are always valid at the iteration's clearing price,
            // reducing welfare destruction by the final enforce_ucp.
            // Arb fills are treated like normal fills here — they participate in
            // repricing and trimming to maintain per-iteration position balance.
            if let Some(ref pd) = price_result {
                let mut combined_map = order_map.clone();
                for (id, order) in &arb_order_map {
                    combined_map.insert(*id, order);
                }
                Self::enforce_ucp_iteration(
                    &mut result,
                    &combined_map,
                    &pd.prices,
                    fill_start_idx,
                );
            }

            // Capture after MM allocation - shows ACTUAL confirmed fills (not estimates)
            #[cfg(feature = "viz")]
            {
                let orders_activated = allocation_result
                    .as_ref()
                    .map(|a| a.activated_orders.len())
                    .unwrap_or(0);
                let mm_count = allocation_result
                    .as_ref()
                    .map(|a| a.mm_allocations.len())
                    .unwrap_or(0);

                // Count how many fills in this iteration were from MM orders
                let mm_fills_this_iter = result
                    .result
                    .fills
                    .iter()
                    .filter(|f| mm_order_ids.contains(&f.order_id))
                    .count();

                phase_snapshots.push(crate::viz::PhaseSnapshot::capture_with_phase_data(
                    crate::viz::PipelinePhase::MmAllocation,
                    iterations,
                    &market_names,
                    result.result.fills.len(),   // ACTUAL confirmed fills
                    result.result.total_welfare, // ACTUAL confirmed welfare
                    start.elapsed().as_secs_f64(),
                    Some(mm_fills_this_iter),
                    Some(0), // MM welfare is included in total, not separate
                    Some(crate::viz::PhaseMetadata::MmAllocation {
                        orders_activated,
                        mm_count,
                    }),
                ));
            }

            // Phase 4: Run partial solvers (e.g., MILP)
            let partial_start = Instant::now();
            for solver in &self.partial_solvers {
                // Filter out already-filled orders for partial solvers too
                let partial_orders: Vec<_> = problem
                    .orders
                    .iter()
                    .filter(|o| !filled_order_ids.contains(&o.id))
                    .cloned()
                    .collect();

                // Create problem with unfilled orders
                let partial_problem = Problem {
                    name: problem.name.clone(),
                    markets: problem.markets.clone(),
                    orders: partial_orders,
                    mm_constraints: problem.mm_constraints.clone(),
                    market_groups: problem.market_groups.clone(),
                };

                let partial_result = solver.solve_partial(&partial_problem);

                // Add fills and track filled order IDs
                // Use order_map for O(1) lookups instead of O(n) .find()
                for fill in partial_result.fills {
                    if let Some(&order) = order_map.get(&fill.order_id) {
                        if !order.is_satisfied_at_price(fill.fill_price) {
                            continue;
                        }
                        filled_order_ids.insert(fill.order_id);
                        result.result.add_fill(fill.clone(), order);
                        iter_bundle_fills += 1;

                        result.contributions.push(SolverContribution {
                            solver_name: solver.name().to_string(),
                            fills_contributed: 1,
                            welfare_contributed: order.welfare_contribution(0, 1), // Approx
                        });
                    }
                }
            }
            timings.partial_solving_secs += partial_start.elapsed().as_secs_f64();

            // Capture after bundle matching - with bundle fills info
            // NOTE: At this point, result.result contains ALL confirmed fills (PD + MM + bundles)
            // This is the ACTUAL welfare, not an estimate
            #[cfg(feature = "viz")]
            {
                // Calculate welfare from bundle fills in this iteration
                let bundle_welfare: i64 = result
                    .contributions
                    .iter()
                    .rev()
                    .take(iter_bundle_fills)
                    .map(|c| c.welfare_contributed)
                    .sum();
                phase_snapshots.push(crate::viz::PhaseSnapshot::capture_with_phase_data(
                    crate::viz::PipelinePhase::BundleMatching,
                    iterations,
                    &market_names,
                    result.result.fills.len(),   // ACTUAL confirmed fills
                    result.result.total_welfare, // ACTUAL confirmed welfare
                    start.elapsed().as_secs_f64(),
                    Some(iter_bundle_fills),
                    Some(bundle_welfare),
                    Some(crate::viz::PhaseMetadata::BundleMatching {
                        solver_name: "Arbitrage".to_string(),
                    }),
                ));
            }

            // Get per-iteration market prices (before moving price_result)
            let iter_market_prices = price_result
                .as_ref()
                .map(|pd| pd.prices.clone())
                .unwrap_or_default();

            // Store last iteration's metadata
            result.price_discovery = price_result;
            result.allocation = allocation_result;

            // Calculate current totals
            let current_welfare = result.result.total_welfare;
            let current_volume: u64 = result.result.fills.iter().map(|f| f.fill_qty).sum();
            let current_fills = result.result.fills.len();

            // Track iteration stats
            result.iteration_stats.push(IterationStats {
                iteration: iterations,
                welfare: current_welfare,
                volume: current_volume,
                fills: current_fills,
                welfare_delta: current_welfare - prev_welfare,
                volume_delta: current_volume.saturating_sub(prev_volume),
                fills_delta: current_fills.saturating_sub(prev_fills),
                price_discovery_fills: iter_price_discovery_fills,
                bundle_fills: iter_bundle_fills,
                fill_start_idx,
                fill_end_idx: current_fills,
                market_prices: iter_market_prices,
            });

            // Check convergence
            let welfare_delta = current_welfare - prev_welfare;
            let converged = welfare_delta.abs() as f64 / (prev_welfare.abs() as f64 + 1.0)
                < self.config.convergence_threshold;

            prev_welfare = current_welfare;
            prev_volume = current_volume;
            prev_fills = current_fills;

            if converged && iter > 0 {
                break;
            }
        }

        // NOTE: We intentionally do NOT add synthetic fills for the last iteration's
        // arb orders. Those orders never went through price discovery, so any fills
        // would be non-market-cleared and unsound. The arb orders from earlier
        // iterations already influenced prices through proper clearing.

        // Remove arb fills before final UCP enforcement.
        // Arb orders are synthetic price-pressure mechanisms with no real account.
        // They participated in price discovery and per-iteration UCP, but shouldn't
        // be in the final result. The final enforce_ucp will trim real fills to
        // restore position balance.
        let arb_ids: std::collections::HashSet<u64> = arb_order_map.keys().copied().collect();
        if !arb_ids.is_empty() {
            result.result.fills.retain(|f| !arb_ids.contains(&f.order_id));
            // Recompute totals after removing arb fills
            let mut welfare: i64 = 0;
            let mut volume: u64 = 0;
            let mut filled: usize = 0;
            for fill in &result.result.fills {
                if let Some(&order) = order_map.get(&fill.order_id) {
                    welfare += order.welfare_contribution(fill.fill_price, fill.fill_qty);
                }
                volume += fill.fill_qty;
                filled += 1;
            }
            result.result.total_welfare = welfare;
            result.result.total_quantity_filled = volume;
            result.result.orders_filled = filled;
        }

        // Enforce UCP on real fills only
        Self::enforce_ucp(&mut result, &order_map);

        // Gate: if total welfare is negative, return empty result.
        if result.result.total_welfare < 0 {
            result.result = MatchingResult::new();
        }

        // Capture final phase snapshot
        #[cfg(feature = "viz")]
        phase_snapshots.push(crate::viz::PhaseSnapshot::capture(
            crate::viz::PipelinePhase::Final,
            iterations,
            &market_names,
            result.result.fills.len(),
            result.result.total_welfare,
            start.elapsed().as_secs_f64(),
        ));

        // Rebuild allocation result with cumulative data (original budgets + total capital used)
        // so the display shows accurate multi-iteration totals, not the last iteration's snapshot.
        if !problem.mm_constraints.is_empty() {
            let mut mm_allocations = Vec::new();
            let mut all_activated: Vec<u64> = Vec::new();
            let mut total_welfare: i64 = 0;

            for mm in &problem.mm_constraints {
                let capital_used = mm.capital_used(&cumulative_mm_fills);
                let activated: Vec<u64> = mm
                    .order_ids
                    .iter()
                    .filter(|id| cumulative_mm_fills.contains_key(id))
                    .copied()
                    .collect();
                let mm_welfare: i64 = activated
                    .iter()
                    .filter_map(|id| {
                        let (price, qty) = cumulative_mm_fills.get(id)?;
                        order_map
                            .get(id)
                            .map(|o| o.welfare_contribution(*price, *qty))
                    })
                    .sum();

                all_activated.extend(&activated);
                total_welfare += mm_welfare;

                mm_allocations.push(crate::mm_allocator::MmAllocation {
                    mm_id: mm.mm_id,
                    activated_orders: activated,
                    capital_used,
                    budget: mm.max_capital,
                    utilization: if mm.max_capital > 0 {
                        capital_used as f64 / mm.max_capital as f64
                    } else {
                        0.0
                    },
                    lambda: 0.0,
                });
            }

            // Add non-MM orders to activated list
            for order in problem.orders.iter() {
                let is_mm = problem
                    .mm_constraints
                    .iter()
                    .any(|mm| mm.order_ids.contains(&order.id));
                if !is_mm && filled_order_ids.contains(&order.id) {
                    all_activated.push(order.id);
                }
            }

            result.allocation = Some(crate::traits::AllocationResult {
                activated_orders: all_activated,
                total_welfare,
                iterations,
                mm_allocations,
                stats: crate::mm_allocator::AllocationStats::default(),
            });
        }

        result.phase_times = timings;
        result.total_time_secs = start.elapsed().as_secs_f64();
        result.iterations = iterations;

        // Set phase snapshots
        #[cfg(feature = "viz")]
        {
            result.phase_snapshots = phase_snapshots;
        }

        result
    }

    /// Build a MatchingResult from price discovery and allocation.
    fn build_result_from_prices(
        &self,
        problem: &Problem,
        prices: &PriceDiscoveryResult,
        allocation: &Option<AllocationResult>,
    ) -> MatchingResult {
        let mut result = MatchingResult::new();

        let mut all_fills = prices.all_fills();

        if let Some(ref alloc) = allocation {
            let activated_set: std::collections::HashSet<_> =
                alloc.activated_orders.iter().copied().collect();
            all_fills.retain(|f| activated_set.contains(&f.order_id));
        }

        let order_map: std::collections::HashMap<_, _> =
            problem.orders.iter().map(|o| (o.id, o)).collect();

        for fill in all_fills {
            if let Some(order) = order_map.get(&fill.order_id) {
                if fill.fill_qty > 0 {
                    // LocalSolver already enforces limit prices during clearing.
                    // No redundant check here — the old buy-side-only check
                    // (fill_price <= limit_price) incorrectly rejected sell orders
                    // where fill_price > limit_price is the DESIRED outcome
                    // (seller receives more than their minimum).
                    result.add_fill(fill, order);
                }
            }
        }

        result
    }

    /// Enforce Uniform Clearing Price: re-price all single-market fills at the
    /// final clearing price. Drops fills that would violate their order's limit
    /// at the new price (these only existed due to transient intermediate prices).
    /// Then enforces position balance: for each binary market, net YES qty must
    /// equal net NO qty (minting invariant). Excess fills are trimmed.
    /// Recomputes welfare from scratch.
    pub(crate) fn enforce_ucp(
        result: &mut PipelineResult,
        order_map: &HashMap<u64, &matching_engine::Order>,
    ) {
        let final_prices = match result.price_discovery {
            Some(ref pd) => &pd.prices,
            None => return,
        };

        let input_fills = result.result.fills.len();
        let input_welfare = result.result.total_welfare;

        let mut candidates = Self::reprice_and_filter_fills(
            &result.result.fills,
            final_prices,
            order_map,
        );
        let after_reprice_fills = candidates.len();
        debug!(input_fills, after_reprice = after_reprice_fills, "enforce_ucp: repriced");

        // Collect per-market imbalance before trimming
        let market_imbalances = Self::compute_market_imbalances(&candidates, order_map);

        Self::trim_position_imbalance(&mut candidates, order_map);
        let active_after_trim = candidates.iter().filter(|(f, _)| f.fill_qty > 0).count();
        debug!(after_trim = active_after_trim, "enforce_ucp: trimmed");

        let (fills, welfare, volume, filled) = Self::collect_final_fills(candidates, order_map);
        debug!(final_fills = filled, welfare, volume, "enforce_ucp: done");

        let retention = if input_welfare != 0 {
            welfare as f64 / input_welfare as f64 * 100.0
        } else {
            100.0
        };

        result.ucp_stats = Some(UcpStats {
            input_fills,
            input_welfare,
            after_reprice_fills,
            dropped_by_reprice: input_fills - after_reprice_fills,
            after_trim_fills: active_after_trim,
            dropped_by_trim: after_reprice_fills - active_after_trim,
            final_fills: filled,
            final_welfare: welfare,
            welfare_retention_pct: retention,
            market_imbalances,
        });

        result.result.fills = fills;
        result.result.total_welfare = welfare;
        result.result.total_quantity_filled = volume;
        result.result.orders_filled = filled;
    }

    /// Simulate enforce_ucp without modifying any state. Returns the welfare
    /// that would result from repricing + position balance trimming.
    ///
    /// Used by the simplex search to compare post-UCP welfare of different
    /// fill/price combinations before committing.
    pub fn simulate_enforce_ucp(
        fills: &[matching_engine::Fill],
        prices: &HashMap<MarketId, Vec<Nanos>>,
        order_map: &HashMap<u64, &matching_engine::Order>,
        arb_orders: &[matching_engine::Order],
    ) -> i64 {
        let mut combined_map = order_map.clone();
        for arb in arb_orders {
            combined_map.insert(arb.id, arb);
        }

        let mut candidates = Self::reprice_and_filter_fills(fills, prices, &combined_map);
        Self::trim_position_imbalance(&mut candidates, &combined_map);
        let (_, welfare, _, _) = Self::collect_final_fills(candidates, &combined_map);
        welfare
    }

    /// Lightweight per-iteration UCP enforcement.
    ///
    /// Reprices only the fills from the current iteration (starting at `fill_start_idx`)
    /// at the current iteration's clearing prices. Drops fills that violate their order's
    /// limit at the current price. Also trims position imbalances among the new fills.
    ///
    /// This ensures fills are always valid at the prices where they were matched,
    /// so the final `enforce_ucp` has much less work to do.
    fn enforce_ucp_iteration(
        result: &mut PipelineResult,
        order_map: &HashMap<u64, &matching_engine::Order>,
        iter_prices: &HashMap<MarketId, Vec<Nanos>>,
        fill_start_idx: usize,
    ) {
        if fill_start_idx >= result.result.fills.len() {
            return;
        }

        let iter_fills: Vec<matching_engine::Fill> =
            result.result.fills[fill_start_idx..].to_vec();

        let mut candidates = Self::reprice_and_filter_fills(
            &iter_fills,
            iter_prices,
            order_map,
        );

        Self::trim_position_imbalance(&mut candidates, order_map);

        // Replace the iteration's fills with the filtered ones and recompute totals
        result.result.fills.truncate(fill_start_idx);

        // Recompute welfare/volume from the retained prefix
        let mut welfare: i64 = 0;
        let mut volume: u64 = 0;
        let mut filled: usize = 0;
        for fill in &result.result.fills {
            if let Some(&order) = order_map.get(&fill.order_id) {
                welfare += order.welfare_contribution(fill.fill_price, fill.fill_qty);
            }
            volume += fill.fill_qty;
            filled += 1;
        }

        // Add back surviving iteration fills
        for (fill, _welfare) in candidates {
            if fill.fill_qty > 0 {
                if let Some(&order) = order_map.get(&fill.order_id) {
                    welfare += order.welfare_contribution(fill.fill_price, fill.fill_qty);
                }
                volume += fill.fill_qty;
                filled += 1;
                result.result.fills.push(fill);
            }
        }

        result.result.total_welfare = welfare;
        result.result.total_quantity_filled = volume;
        result.result.orders_filled = filled;
    }

    /// Compute per-market position imbalance using full marginal payoff analysis.
    /// Returns (market_id, yes_qty, no_qty, excess) for imbalanced markets.
    fn compute_market_imbalances(
        candidates: &[(matching_engine::Fill, i64)],
        order_map: &HashMap<u64, &matching_engine::Order>,
    ) -> Vec<(MarketId, u64, u64, u64)> {
        let mut net_position: HashMap<MarketId, i64> = HashMap::new();
        let mut market_yes_qty: HashMap<MarketId, u64> = HashMap::new();
        let mut market_no_qty: HashMap<MarketId, u64> = HashMap::new();

        for (fill, _) in candidates {
            if fill.fill_qty == 0 {
                continue;
            }
            let Some(&order) = order_map.get(&fill.order_id) else {
                continue;
            };

            for (market, normalized) in order.marginal_payoffs_i64() {
                let contribution = normalized * fill.fill_qty as i64;
                *net_position.entry(market).or_insert(0) += contribution;

                if contribution > 0 {
                    *market_yes_qty.entry(market).or_insert(0) += contribution as u64;
                } else if contribution < 0 {
                    *market_no_qty.entry(market).or_insert(0) += (-contribution) as u64;
                }
            }
        }

        let mut imbalances: Vec<(MarketId, u64, u64, u64)> = net_position
            .iter()
            .filter_map(|(&market, &net)| {
                if net == 0 {
                    return None;
                }
                let yes = *market_yes_qty.get(&market).unwrap_or(&0);
                let no = *market_no_qty.get(&market).unwrap_or(&0);
                Some((market, yes, no, net.unsigned_abs()))
            })
            .collect();

        imbalances.sort_by(|a, b| b.3.cmp(&a.3));
        imbalances
    }

    /// Phase 1: Re-price single-market binary fills at final clearing prices.
    /// Drops fills that violate limit prices.
    pub(crate) fn reprice_and_filter_fills(
        fills: &[matching_engine::Fill],
        final_prices: &HashMap<MarketId, Vec<Nanos>>,
        order_map: &HashMap<u64, &matching_engine::Order>,
    ) -> Vec<(matching_engine::Fill, i64)> {
        let mut candidate_fills: Vec<(matching_engine::Fill, i64)> =
            Vec::with_capacity(fills.len());

        for fill in fills {
            if fill.fill_qty == 0 {
                continue;
            }

            let Some(&order) = order_map.get(&fill.order_id) else {
                candidate_fills.push((fill.clone(), 0));
                continue;
            };

            // Only re-price single-market binary orders
            if order.num_markets == 1 && order.num_states == 2 {
                let market = order.markets[0];
                if let Some(prices) = final_prices.get(&market) {
                    let yes_payoff = order.payoffs[0];
                    let no_payoff = order.payoffs[1];

                    let final_price = if yes_payoff != 0 && no_payoff == 0 {
                        prices.first().copied()
                    } else if yes_payoff == 0 && no_payoff != 0 {
                        prices.get(1).copied()
                    } else {
                        None
                    };

                    if let Some(fp) = final_price {
                        if order.is_satisfied_at_price(fp) {
                            let mut repriced = fill.clone();
                            repriced.fill_price = fp;
                            let welfare = order.welfare_contribution(fp, fill.fill_qty);
                            candidate_fills.push((repriced, welfare));
                        }
                        continue;
                    }
                }
            }

            // Multi-market or no clearing price: keep as-is, but validate limit
            if !order.is_satisfied_at_price(fill.fill_price) {
                continue;
            }
            let welfare = order.welfare_contribution(fill.fill_price, fill.fill_qty);
            candidate_fills.push((fill.clone(), welfare));
        }

        candidate_fills
    }

    /// Phase 2: Enforce position balance per binary market.
    /// Uses full marginal payoff computation (handles multi-market/bundle fills).
    /// Trims single-market fills only (safe — no cross-market side effects).
    pub(crate) fn trim_position_imbalance(
        candidate_fills: &mut [(matching_engine::Fill, i64)],
        order_map: &HashMap<u64, &matching_engine::Order>,
    ) {
        // Step 1: Compute per-market net position from ALL fills using marginal payoffs.
        // Positive = excess YES, negative = excess NO.
        let mut net_position: HashMap<matching_engine::MarketId, i64> = HashMap::new();
        // Track single-market YES/NO separately (only these can be trimmed)
        let mut single_yes_qty: HashMap<matching_engine::MarketId, u64> = HashMap::new();
        let mut single_no_qty: HashMap<matching_engine::MarketId, u64> = HashMap::new();

        for (fill, _) in candidate_fills.iter() {
            if fill.fill_qty == 0 {
                continue;
            }
            let Some(&order) = order_map.get(&fill.order_id) else {
                continue;
            };

            for (market, normalized) in order.marginal_payoffs_i64() {
                *net_position.entry(market).or_insert(0) +=
                    normalized * fill.fill_qty as i64;
            }

            // Track single-market fills for trimming
            let num_markets = order.num_markets as usize;
            let num_states = order.num_states as usize;
            if num_markets == 1 && num_states == 2 {
                let market = order.markets[0];
                let is_yes = (order.payoffs[0] > 0 && order.payoffs[1] == 0)
                    || (order.payoffs[0] == 0 && order.payoffs[1] < 0);
                let is_no = (order.payoffs[0] == 0 && order.payoffs[1] > 0)
                    || (order.payoffs[0] < 0 && order.payoffs[1] == 0);
                if is_yes {
                    *single_yes_qty.entry(market).or_insert(0) += fill.fill_qty;
                }
                if is_no {
                    *single_no_qty.entry(market).or_insert(0) += fill.fill_qty;
                }
            }
        }

        // Step 2: For each market with nonzero net position, trim single-market fills.
        for (&market, &net) in &net_position {
            let yes_qty = *single_yes_qty.get(&market).unwrap_or(&0);
            let no_qty = *single_no_qty.get(&market).unwrap_or(&0);
            if net != 0 {
                tracing::debug!(
                    ?market,
                    net,
                    yes_qty,
                    no_qty,
                    "trim_position_imbalance: nonzero net"
                );
            }
            if net == 0 {
                continue;
            }

            // net > 0 means excess YES, trim YES side; net < 0 means excess NO
            let trim_side_is_yes = net > 0;
            let excess = net.unsigned_abs();

            // Check that there are enough single-market fills to trim
            let trimmable = if trim_side_is_yes {
                *single_yes_qty.get(&market).unwrap_or(&0)
            } else {
                *single_no_qty.get(&market).unwrap_or(&0)
            };
            if trimmable == 0 {
                continue;
            }

            // Collect indices of single-market fills on the excess side
            let mut excess_indices: Vec<(usize, i64, u64)> = Vec::new();
            for (i, (fill, welfare)) in candidate_fills.iter().enumerate() {
                if fill.fill_qty == 0 {
                    continue;
                }
                let Some(&order) = order_map.get(&fill.order_id) else {
                    continue;
                };
                if order.num_markets != 1 || order.num_states != 2 {
                    continue;
                }
                if order.markets[0] != market {
                    continue;
                }

                let is_excess_side = if trim_side_is_yes {
                    (order.payoffs[0] > 0 && order.payoffs[1] == 0)
                        || (order.payoffs[0] == 0 && order.payoffs[1] < 0)
                } else {
                    (order.payoffs[0] == 0 && order.payoffs[1] > 0)
                        || (order.payoffs[0] < 0 && order.payoffs[1] == 0)
                };

                if is_excess_side {
                    excess_indices.push((i, *welfare, fill.fill_qty));
                }
            }

            // Sort by welfare ascending (drop lowest welfare first)
            excess_indices.sort_by_key(|&(_, w, _)| w);

            let mut remaining_excess = excess;
            for (idx, _, _) in &excess_indices {
                if remaining_excess == 0 {
                    break;
                }

                let fill_qty = candidate_fills[*idx].0.fill_qty;

                if remaining_excess >= fill_qty {
                    candidate_fills[*idx].0.fill_qty = 0;
                    remaining_excess -= fill_qty;
                } else {
                    candidate_fills[*idx].0.fill_qty = fill_qty - remaining_excess;
                    remaining_excess = 0;
                }
            }
        }
    }

    /// Phase 3: Filter zero-qty fills and recompute welfare/volume/count.
    fn collect_final_fills(
        candidate_fills: Vec<(matching_engine::Fill, i64)>,
        order_map: &HashMap<u64, &matching_engine::Order>,
    ) -> (Vec<matching_engine::Fill>, i64, u64, usize) {
        let mut new_fills = Vec::new();
        let mut new_welfare: i64 = 0;
        let mut new_volume: u64 = 0;
        let mut orders_filled: usize = 0;

        for (fill, welfare) in candidate_fills {
            if fill.fill_qty == 0 {
                continue;
            }
            // Recompute welfare for partially trimmed fills
            let w = if let Some(&order) = order_map.get(&fill.order_id) {
                order.welfare_contribution(fill.fill_price, fill.fill_qty)
            } else {
                welfare
            };
            new_welfare += w;
            new_volume += fill.fill_qty;
            orders_filled += 1;
            new_fills.push(fill);
        }

        (new_fills, new_welfare, new_volume, orders_filled)
    }
}

impl Solver for Pipeline {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        Pipeline::solve(self, problem).result
    }

    fn name(&self) -> &str {
        &self.name
    }
}

// ============================================================================
// Pipeline Builder
// ============================================================================

/// Builder for constructing pipelines.
pub struct PipelineBuilder {
    name: String,
    price_discoverer: Option<Box<dyn PriceDiscoverer>>,
    multi_market_solver: Option<MultiMarketSolver>,
    allocator: Option<Box<dyn OrderAllocator>>,
    dual_master: Option<DualMaster>,
    partial_solvers: Vec<Box<dyn PartialSolver>>,
    config: PipelineConfig,
}

impl PipelineBuilder {
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self {
            name: "Pipeline".to_string(),
            price_discoverer: None,
            multi_market_solver: None,
            allocator: None,
            dual_master: None,
            partial_solvers: Vec::new(),
            config: PipelineConfig::default(),
        }
    }

    /// Set the pipeline name.
    pub fn name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    /// Set the price discoverer.
    pub fn price_discoverer<P: PriceDiscoverer + 'static>(mut self, discoverer: P) -> Self {
        self.price_discoverer = Some(Box::new(discoverer));
        self
    }

    /// Set the multi-market repricing solver.
    pub fn multi_market_solver(mut self, solver: MultiMarketSolver) -> Self {
        self.multi_market_solver = Some(solver);
        self
    }

    /// Set the order allocator.
    pub fn allocator<A: OrderAllocator + 'static>(mut self, allocator: A) -> Self {
        self.allocator = Some(Box::new(allocator));
        self
    }

    /// Set the dual decomposition master.
    pub fn dual_master(mut self, master: DualMaster) -> Self {
        self.dual_master = Some(master);
        self
    }

    /// Add a partial solver.
    pub fn partial_solver<S: PartialSolver + 'static>(mut self, solver: S) -> Self {
        self.partial_solvers.push(Box::new(solver));
        self
    }

    /// Set whether to use fixed-point iteration.
    pub fn use_fixed_point(mut self, use_it: bool) -> Self {
        self.config.use_fixed_point = use_it;
        self
    }

    /// Set maximum iterations for fixed-point.
    pub fn max_iterations(mut self, max: usize) -> Self {
        self.config.max_iterations = max;
        self
    }

    /// Set convergence threshold for fixed-point.
    pub fn convergence_threshold(mut self, threshold: f64) -> Self {
        self.config.convergence_threshold = threshold;
        self
    }


    /// Build the pipeline.
    pub fn build(self) -> Pipeline {
        Pipeline {
            name: self.name,
            price_discoverer: self.price_discoverer,
            multi_market_solver: self.multi_market_solver,
            allocator: self.allocator,
            dual_master: self.dual_master,
            partial_solvers: self.partial_solvers,
            config: self.config,
        }
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{outcome_sell, simple_yes_buy};

    fn create_test_problem() -> Problem {
        let mut problem = Problem::new("test");
        let market = problem.markets.add_binary("market");

        // Sell orders provide supply instead of liquidity pool
        problem.orders.push(outcome_sell(
            &problem.markets,
            100,
            market,
            0,
            500_000_000,
            1000,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets,
            101,
            market,
            1,
            500_000_000,
            1000,
        ));

        for i in 0..10 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i + 1,
                market,
                (500 + i * 10) as u64 * 1_000_000,
                50 + i * 5,
            ));
        }

        problem
    }

    #[test]
    fn test_pipeline_current() {
        let problem = create_test_problem();
        let pipeline = Pipeline::current();
        let result = pipeline.solve(&problem);

        assert!(result.price_discovery.is_some());
        assert!(result.total_time_secs >= 0.0);
    }

    #[test]
    fn test_pipeline_builder() {
        let problem = create_test_problem();

        let pipeline = Pipeline::builder()
            .price_discoverer(LocalSolver::new())
            .build();

        let result = pipeline.solve(&problem);
        assert!(result.result.orders_filled > 0);
    }

    #[test]
    fn test_pipeline_full_platform() {
        let problem = create_test_problem();
        let pipeline = Pipeline::full_platform();
        let result = pipeline.solve(&problem);

        assert!(result.total_time_secs >= 0.0);
    }

    #[test]
    fn test_pipeline_iterative() {
        let problem = create_test_problem();
        let pipeline = Pipeline::iterative();
        let result = pipeline.solve(&problem);

        assert!(result.price_discovery.is_some());
    }

}
