//! Asymmetric multi-component benchmark for the decomposed solver's budget
//! coordination rule (SYB-236).
//!
//! Generates a well-conditioned book with the production scenario generator,
//! then makes the market *groups* (the decomposition components) strongly
//! asymmetric in **depth and ROI**:
//!
//! - "Deep / high-ROI" groups: retail supply is thinned and retail demand
//!   amplified, so MM liquidity is scarce and every budget dollar the MM
//!   deploys there clears a lot of otherwise-unmatched retail (high marginal
//!   welfare).
//! - "Shallow / low-ROI" groups: retail supply is abundant and demand thinned,
//!   so the MM is a redundant counterparty — extra budget produces little
//!   welfare and is largely retained as cash.
//!
//! A single MM spans one market in every group with a tight total budget, so
//! the budget *split* across components is the binding decision. The monolithic
//! optimum concentrates budget in the deep groups; the equal-scarcity
//! (proportional-response) rule should track that, whereas the superseded
//! equal-utility surrogate over-allocates to the shallow, capacity-limited
//! groups.
//!
//! Usage:
//!   cargo run --release -p matching-solver --features conic --example asym_bench -- [ITERS]
//! ITERS (default 20) sets DecomposedSolver::max_budget_iters.
//! Env: LP=1 also runs monolithic LP; DEBUG=1 dumps component welfare.

use rand::prelude::*;
use rand_chacha::ChaCha8Rng;

use matching_engine::{MarketId, Nanos, Order, Problem, NANOS_PER_DOLLAR};
use matching_scenarios::{generate_scenario, ScenarioConfig};
use matching_solver::{ConicConfig, ConicSolver, DecomposedSolver, ObjectiveMode};

const D: u64 = NANOS_PER_DOLLAR;

/// Which markets belong to which group (component).
fn group_of(problem: &Problem) -> std::collections::HashMap<MarketId, usize> {
    let mut m = std::collections::HashMap::new();
    for (gi, g) in problem.market_groups.iter().enumerate() {
        for &mid in &g.markets {
            m.insert(mid, gi);
        }
    }
    m
}

/// Post-process a generated problem into an asymmetric one.
///
/// Half the market groups become "deep/high-ROI": we drop most retail *sell*
/// supply (buyers go unmatched without an MM) and duplicate retail *buy* demand.
/// The other half become "shallow/low-ROI": we drop most retail *buy* demand and
/// keep abundant supply, so the MM adds little.
///
/// Then we install a single MM (id 1) with a tight budget spanning one market in
/// every group, buying the scarce side.
fn make_asymmetric(problem: &mut Problem, seed: u64) {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let g_of = group_of(problem);
    // Deep groups: even index. Shallow: odd index.
    let is_deep = |gi: usize| gi % 2 == 0;

    // Keep the generator's native MMs (two-sided ladders spanning many groups):
    // their budgets genuinely bind and their liquidity provision is what creates
    // MM welfare. We only skew the *retail* book to make groups asymmetric in
    // depth/ROI, and (optionally) scale MM budgets so the MM's welfare share is
    // large enough to stress the budget-coordination rule.

    // Identify MM order ids so we never drop them.
    let mm_order_ids: std::collections::HashSet<u64> = problem
        .mm_constraints
        .iter()
        .flat_map(|mm| mm.order_ids.iter().copied())
        .collect();

    let mut new_orders: Vec<Order> = Vec::new();
    for o in problem.orders.drain(..) {
        if mm_order_ids.contains(&o.id) {
            new_orders.push(o);
            continue;
        }
        // Orders not in any group (standalone markets) are left as-is.
        let Some(&gi) = o.active_markets().next().and_then(|m| g_of.get(&m)) else {
            new_orders.push(o);
            continue;
        };
        if is_deep(gi) {
            // Deep / high-ROI: thin retail SELL supply hard (MM asks become the
            // pivotal supply), amplify retail buy demand.
            if o.is_seller() {
                if rng.random_bool(0.10) {
                    new_orders.push(o);
                }
            } else {
                new_orders.push(o.clone());
                new_orders.push(o); // duplicate demand
            }
        } else {
            // Shallow / low-ROI: abundant supply, thin demand → MM redundant.
            if o.is_seller() {
                new_orders.push(o.clone());
                new_orders.push(o);
            } else if rng.random_bool(0.25) {
                new_orders.push(o);
            }
        }
    }
    problem.orders = new_orders;

    // Reassign unique ids to the cloned retail orders (keep MM order ids intact).
    let mut next_id = 30_000_000u64;
    for o in &mut problem.orders {
        if !mm_order_ids.contains(&o.id) {
            o.id = next_id;
            next_id += 1;
        }
    }

    // Optionally scale MM budgets (MMSCALE, default 1.0) to grow the MM welfare
    // share and make the split the binding decision.
    let scale: f64 = std::env::var("MMSCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    if (scale - 1.0).abs() > 1e-9 {
        for mm in &mut problem.mm_constraints {
            mm.max_capital = Nanos((mm.max_capital.0 as f64 * scale) as u64);
        }
    }
}

fn welfare_dollars(w: i64) -> f64 {
    w as f64 / D as f64
}

fn main() {
    let iters: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);

    // Base: a medium generated book (30 markets, many groups), then asymmetrize.
    let seed: u64 = std::env::var("SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(42);
    let cfg = ScenarioConfig::medium().with_seed(seed);
    let mut problem = generate_scenario(cfg);
    // RAW=1 keeps the plain (near-symmetric) generated book for convergence/
    // damping experiments on the existing presets; otherwise asymmetrize.
    let raw = std::env::var("RAW").is_ok();
    if !raw {
        make_asymmetric(&mut problem, seed);
    }

    let conic_cfg = ConicConfig {
        mode: ObjectiveMode::QuasiFisher,
        ..Default::default()
    };

    let mono = ConicSolver::with_config(conic_cfg.clone()).solve(&problem);
    let w_mono = mono.result.total_welfare();

    if std::env::var("LP").is_ok() {
        let lp = matching_solver::LpSolver::new().solve(&problem);
        println!(
            "Monolithic LP    : welfare = ${:.2}  ({} fills)",
            welfare_dollars(lp.result.total_welfare()),
            lp.result.fills.len()
        );
    }

    let decomp_solver =
        DecomposedSolver::with_config(ConicSolver::with_config(conic_cfg.clone()), iters, 1e-4);
    let decomp = decomp_solver.solve(&problem);
    let w_decomp = decomp.result.total_welfare();

    let pct = |w: i64| 100.0 * w as f64 / w_mono as f64;

    let total_budget: f64 = problem
        .mm_constraints
        .iter()
        .map(|mm| mm.max_capital.0 as f64)
        .sum::<f64>()
        / D as f64;
    println!("=== Asymmetric multi-component benchmark (iters={iters}) ===");
    println!(
        "groups={} orders={} mms={} total_mm_budget=${:.0}",
        problem.market_groups.len(),
        problem.orders.len(),
        problem.mm_constraints.len(),
        total_budget,
    );
    println!(
        "Monolithic conic : welfare = ${:.2}  ({} fills)",
        welfare_dollars(w_mono),
        mono.result.fills.len()
    );
    println!(
        "Decomposed conic : welfare = ${:.2}  ({:.2}% of monolithic, {} fills)",
        welfare_dollars(w_decomp),
        pct(w_decomp),
        decomp.result.fills.len()
    );
}
