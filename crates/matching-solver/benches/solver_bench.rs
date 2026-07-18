//! Solver microbenchmarks over compact structural workloads.
//!
//! `market-like` mirrors the book shape presented to Sybil's production
//! solver: a long-tailed resting retail book plus one-shot, dollar-sized,
//! two-sided MM quotes. `shared-capital` remains the deliberately adversarial
//! control for pacing behavior. Publishable quality claims still belong in the
//! preregistered `benchmarks/solver` protocol.

use std::fmt;
use std::hint::black_box;
#[cfg(feature = "retained-cash")]
use std::sync::OnceLock;

use matching_engine::Problem;
use matching_scenarios::{
    FlashLiquidityConfig, ScenarioConfig, generate_flash_liquidity_scenario, generate_scenario,
};

fn main() {
    divan::main();
}

#[derive(Clone, Copy, Debug)]
enum Workload {
    SmallControl,
    MarketLike,
    SharedCapital,
}

impl fmt::Display for Workload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SmallControl => "small-control",
            Self::MarketLike => "market-like",
            Self::SharedCapital => "shared-capital",
        })
    }
}

const WORKLOADS: &[Workload] = &[
    Workload::SmallControl,
    Workload::MarketLike,
    Workload::SharedCapital,
];

fn generate(workload: Workload) -> Problem {
    match workload {
        Workload::SmallControl => generate_scenario(ScenarioConfig::small()),
        Workload::MarketLike => generate_scenario(ScenarioConfig::market_like()),
        Workload::SharedCapital => generate_flash_liquidity_scenario(FlashLiquidityConfig {
            seed: 7_302,
            num_markets: 20,
            opportunities_per_market: 10,
            num_mms: 2,
            quantity_min_shares: 10,
            quantity_max_shares: 100,
            initial_budget_dollars: 2_500,
        }),
    }
}

#[cfg(feature = "retained-cash")]
fn problem(workload: Workload) -> &'static Problem {
    static SMALL: OnceLock<Problem> = OnceLock::new();
    static MARKET_LIKE: OnceLock<Problem> = OnceLock::new();
    static SHARED_CAPITAL: OnceLock<Problem> = OnceLock::new();
    match workload {
        Workload::SmallControl => SMALL.get_or_init(|| generate(workload)),
        Workload::MarketLike => MARKET_LIKE.get_or_init(|| generate(workload)),
        Workload::SharedCapital => SHARED_CAPITAL.get_or_init(|| generate(workload)),
    }
}

#[divan::bench(args = WORKLOADS)]
fn scenario_generation(workload: Workload) {
    black_box(generate(workload));
}

#[cfg(feature = "lp")]
mod lp {
    use super::*;
    use matching_solver::LpSolver;

    #[divan::bench(args = WORKLOADS)]
    fn solve(workload: Workload) {
        let solver = LpSolver::new();
        black_box(solver.solve(black_box(problem(workload))));
    }
}

#[cfg(feature = "retained-cash")]
mod retained_cash {
    use super::*;
    use matching_solver::RetainedCashSolver;

    #[divan::bench(args = WORKLOADS)]
    fn solve(workload: Workload) {
        let solver = RetainedCashSolver::new();
        black_box(solver.solve(black_box(problem(workload))));
    }
}

#[cfg(feature = "lp")]
mod pacing_bundle {
    use super::*;
    use matching_solver::PacingBundleSolver;

    #[divan::bench(args = WORKLOADS)]
    fn solve(workload: Workload) {
        let solver = PacingBundleSolver::new();
        black_box(solver.solve(black_box(problem(workload))));
    }
}

#[cfg(feature = "conic")]
mod conic {
    use super::*;
    use matching_solver::ConicSolver;

    #[divan::bench(args = WORKLOADS)]
    fn solve(workload: Workload) {
        let solver = ConicSolver::new();
        black_box(solver.solve(black_box(problem(workload))));
    }
}
