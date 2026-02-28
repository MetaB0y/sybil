//! Divan benchmarks for matching solver components.
//!
//! Benchmarks the LP/EG/Conic solvers on generated scenarios of various sizes.

use matching_scenarios::{generate_scenario, ScenarioConfig};

fn main() {
    divan::main();
}

// ============================================================================
// Scenario Generation Benchmarks
// ============================================================================

#[divan::bench]
fn bench_scenario_generation_small() {
    let _ = generate_scenario(ScenarioConfig::small());
}

#[divan::bench]
fn bench_scenario_generation_medium() {
    let _ = generate_scenario(ScenarioConfig::medium());
}

#[divan::bench]
fn bench_scenario_generation_large() {
    let _ = generate_scenario(ScenarioConfig::large());
}

// ============================================================================
// LP Solver Benchmarks
// ============================================================================

#[cfg(feature = "lp")]
mod lp {
    use divan::Bencher;
    use matching_engine::Problem;
    use matching_scenarios::{generate_scenario, ScenarioConfig};
    use matching_solver::LpSolver;
    use std::sync::OnceLock;

    #[divan::bench]
    fn bench_lp_small() {
        static PROBLEM: OnceLock<Problem> = OnceLock::new();
        let problem = PROBLEM.get_or_init(|| generate_scenario(ScenarioConfig::small()));
        let solver = LpSolver::new();
        let _ = solver.solve(problem);
    }

    #[divan::bench]
    fn bench_lp_medium() {
        static PROBLEM: OnceLock<Problem> = OnceLock::new();
        let problem = PROBLEM.get_or_init(|| generate_scenario(ScenarioConfig::medium()));
        let solver = LpSolver::new();
        let _ = solver.solve(problem);
    }

    #[divan::bench]
    fn bench_lp_medium_hot_markets(bencher: Bencher) {
        static PROBLEM: OnceLock<Problem> = OnceLock::new();
        let problem = PROBLEM.get_or_init(|| {
            let mut config = ScenarioConfig::medium();
            config.hot_market_fraction = 0.3;
            generate_scenario(config)
        });

        bencher.bench_local(|| {
            let solver = LpSolver::new();
            solver.solve(problem)
        });
    }
}

// ============================================================================
// EG Solver Benchmarks
// ============================================================================

#[cfg(feature = "lp")]
mod eg {
    use matching_engine::Problem;
    use matching_scenarios::{generate_scenario, ScenarioConfig};
    use matching_solver::EgSolver;
    use std::sync::OnceLock;

    #[divan::bench]
    fn bench_eg_small() {
        static PROBLEM: OnceLock<Problem> = OnceLock::new();
        let problem = PROBLEM.get_or_init(|| generate_scenario(ScenarioConfig::small()));
        let solver = EgSolver::new();
        let _ = solver.solve(problem);
    }

    #[divan::bench]
    fn bench_eg_medium() {
        static PROBLEM: OnceLock<Problem> = OnceLock::new();
        let problem = PROBLEM.get_or_init(|| generate_scenario(ScenarioConfig::medium()));
        let solver = EgSolver::new();
        let _ = solver.solve(problem);
    }
}

// ============================================================================
// Conic Solver Benchmarks
// ============================================================================

#[cfg(feature = "conic")]
mod conic {
    use matching_engine::Problem;
    use matching_scenarios::{generate_scenario, ScenarioConfig};
    use matching_solver::ConicSolver;
    use std::sync::OnceLock;

    #[divan::bench]
    fn bench_conic_small() {
        static PROBLEM: OnceLock<Problem> = OnceLock::new();
        let problem = PROBLEM.get_or_init(|| generate_scenario(ScenarioConfig::small()));
        let solver = ConicSolver::new();
        let _ = solver.solve(problem);
    }

    #[divan::bench]
    fn bench_conic_medium() {
        static PROBLEM: OnceLock<Problem> = OnceLock::new();
        let problem = PROBLEM.get_or_init(|| generate_scenario(ScenarioConfig::medium()));
        let solver = ConicSolver::new();
        let _ = solver.solve(problem);
    }
}
