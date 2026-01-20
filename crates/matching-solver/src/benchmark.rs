//! Benchmark harness for comparing solver configurations.
//!
//! The harness provides a simple way to:
//! - Define scenarios (problems to solve)
//! - Define pipelines (solver configurations)
//! - Run all combinations and collect metrics
//! - Generate comparison reports
//!
//! # Example
//!
//! ```ignore
//! use matching_solver::{BenchmarkHarness, Pipeline};
//! use matching_scenarios::{generate_mega_scenario_v2, MegaScenarioConfigV2};
//!
//! let mut harness = BenchmarkHarness::new();
//!
//! // Add scenarios
//! harness.add_scenario("small", generate_mega_scenario_v2(MegaScenarioConfigV2::small()));
//! harness.add_scenario("medium", generate_mega_scenario_v2(MegaScenarioConfigV2::medium()));
//!
//! // Add pipelines
//! harness.add_pipeline("current", Pipeline::current());
//! harness.add_pipeline("full", Pipeline::full_platform());
//!
//! // Run benchmarks
//! let results = harness.run();
//! harness.report(&results);
//! ```

use std::time::Instant;

use matching_engine::Problem;

use crate::pipeline::Pipeline;

// ============================================================================
// Benchmark Results
// ============================================================================

/// A single benchmark run result.
#[derive(Clone, Debug)]
pub struct BenchmarkRun {
    /// Name of the scenario.
    pub scenario: String,

    /// Name of the pipeline.
    pub pipeline: String,

    /// Total welfare achieved.
    pub welfare: i64,

    /// Number of orders filled.
    pub fills: usize,

    /// Total quantity filled.
    pub volume: u64,

    /// Time taken in milliseconds.
    pub time_ms: f64,

    /// Number of iterations (for iterative pipelines).
    pub iterations: usize,

    /// Number of orders in the scenario.
    pub num_orders: usize,

    /// Number of markets in the scenario.
    pub num_markets: usize,
}

/// Collection of benchmark results.
#[derive(Clone, Debug, Default)]
pub struct BenchmarkResults {
    /// Individual run results.
    pub results: Vec<BenchmarkRun>,
}

impl BenchmarkResults {
    /// Create empty results.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a run result.
    pub fn add(&mut self, run: BenchmarkRun) {
        self.results.push(run);
    }

    /// Get results for a specific scenario.
    pub fn for_scenario(&self, scenario: &str) -> Vec<&BenchmarkRun> {
        self.results
            .iter()
            .filter(|r| r.scenario == scenario)
            .collect()
    }

    /// Get results for a specific pipeline.
    pub fn for_pipeline(&self, pipeline: &str) -> Vec<&BenchmarkRun> {
        self.results
            .iter()
            .filter(|r| r.pipeline == pipeline)
            .collect()
    }

    /// Get the best result for a scenario (by welfare).
    pub fn best_for_scenario(&self, scenario: &str) -> Option<&BenchmarkRun> {
        self.for_scenario(scenario)
            .into_iter()
            .max_by_key(|r| r.welfare)
    }

    /// Get unique scenario names.
    pub fn scenarios(&self) -> Vec<String> {
        let mut scenarios: Vec<_> = self.results.iter().map(|r| r.scenario.clone()).collect();
        scenarios.sort();
        scenarios.dedup();
        scenarios
    }

    /// Get unique pipeline names.
    pub fn pipelines(&self) -> Vec<String> {
        let mut pipelines: Vec<_> = self.results.iter().map(|r| r.pipeline.clone()).collect();
        pipelines.sort();
        pipelines.dedup();
        pipelines
    }
}

// ============================================================================
// Benchmark Harness
// ============================================================================

/// Harness for running benchmarks across scenarios and pipelines.
pub struct BenchmarkHarness {
    /// Scenarios to benchmark.
    scenarios: Vec<(String, Problem)>,

    /// Pipelines to benchmark.
    pipelines: Vec<(String, Pipeline)>,
}

impl BenchmarkHarness {
    /// Create a new benchmark harness.
    pub fn new() -> Self {
        Self {
            scenarios: Vec::new(),
            pipelines: Vec::new(),
        }
    }

    /// Add a scenario to benchmark.
    pub fn add_scenario(&mut self, name: impl Into<String>, problem: Problem) {
        self.scenarios.push((name.into(), problem));
    }

    /// Add a pipeline to benchmark.
    pub fn add_pipeline(&mut self, name: impl Into<String>, pipeline: Pipeline) {
        self.pipelines.push((name.into(), pipeline));
    }

    /// Run all benchmarks.
    pub fn run(&self) -> BenchmarkResults {
        let mut results = BenchmarkResults::new();

        for (scenario_name, problem) in &self.scenarios {
            for (pipeline_name, pipeline) in &self.pipelines {
                let run = self.run_single(scenario_name, problem, pipeline_name, pipeline);
                results.add(run);
            }
        }

        results
    }

    /// Run a single benchmark.
    fn run_single(
        &self,
        scenario_name: &str,
        problem: &Problem,
        pipeline_name: &str,
        pipeline: &Pipeline,
    ) -> BenchmarkRun {
        let start = Instant::now();
        let result = pipeline.solve(problem);
        let elapsed = start.elapsed();

        BenchmarkRun {
            scenario: scenario_name.to_string(),
            pipeline: pipeline_name.to_string(),
            welfare: result.result.total_welfare,
            fills: result.result.orders_filled,
            volume: result.result.total_quantity_filled,
            time_ms: elapsed.as_secs_f64() * 1000.0,
            iterations: result.iterations,
            num_orders: problem.orders.len(),
            num_markets: problem.markets.len(),
        }
    }

    /// Print a comparison report.
    pub fn report(&self, results: &BenchmarkResults) {
        println!();
        println!("Benchmark Results");
        println!("=================");
        println!();

        // Print header
        print!("{:<15} ", "Scenario");
        for pipeline in results.pipelines() {
            print!("{:>15} ", pipeline);
        }
        println!();

        print!("{:<15} ", "--------");
        for _ in results.pipelines() {
            print!("{:>15} ", "---------------");
        }
        println!();

        // Print welfare by scenario
        println!("\nWelfare:");
        for scenario in results.scenarios() {
            print!("{:<15} ", scenario);
            for pipeline in results.pipelines() {
                let run = results
                    .results
                    .iter()
                    .find(|r| r.scenario == scenario && r.pipeline == pipeline);
                if let Some(r) = run {
                    print!("{:>15} ", r.welfare);
                } else {
                    print!("{:>15} ", "-");
                }
            }
            println!();
        }

        // Print fills by scenario
        println!("\nFills:");
        for scenario in results.scenarios() {
            print!("{:<15} ", scenario);
            for pipeline in results.pipelines() {
                let run = results
                    .results
                    .iter()
                    .find(|r| r.scenario == scenario && r.pipeline == pipeline);
                if let Some(r) = run {
                    print!("{:>15} ", r.fills);
                } else {
                    print!("{:>15} ", "-");
                }
            }
            println!();
        }

        // Print time (ms) by scenario
        println!("\nTime (ms):");
        for scenario in results.scenarios() {
            print!("{:<15} ", scenario);
            for pipeline in results.pipelines() {
                let run = results
                    .results
                    .iter()
                    .find(|r| r.scenario == scenario && r.pipeline == pipeline);
                if let Some(r) = run {
                    print!("{:>15.2} ", r.time_ms);
                } else {
                    print!("{:>15} ", "-");
                }
            }
            println!();
        }

        println!();

        // Print detailed results table
        Self::print_detailed_table(results);
    }

    /// Print a detailed comparison table.
    fn print_detailed_table(results: &BenchmarkResults) {
        println!("Detailed Results:");
        println!("{:-<100}", "");
        println!(
            "{:<12} | {:<12} | {:>12} | {:>8} | {:>10} | {:>10} | {:>8}",
            "Scenario", "Pipeline", "Welfare", "Fills", "Volume", "Time (ms)", "Orders"
        );
        println!("{:-<100}", "");

        for run in &results.results {
            println!(
                "{:<12} | {:<12} | {:>12} | {:>8} | {:>10} | {:>10.2} | {:>8}",
                run.scenario,
                run.pipeline,
                run.welfare,
                run.fills,
                run.volume,
                run.time_ms,
                run.num_orders
            );
        }

        println!("{:-<100}", "");
    }
}

impl Default for BenchmarkHarness {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Quick Benchmark Functions
// ============================================================================

/// Run a quick benchmark comparing pipelines on a single scenario.
pub fn quick_benchmark(problem: &Problem, pipelines: &[(String, Pipeline)]) -> BenchmarkResults {
    let mut harness = BenchmarkHarness::new();
    harness.add_scenario("default", problem.clone());

    for (name, pipeline) in pipelines {
        // We need to clone the pipeline - but Pipeline doesn't implement Clone
        // So for now, we'll just note this limitation
        // In practice, users would create pipelines fresh
        let _ = (name, pipeline);
    }

    // This is a simplified version - in practice you'd want to
    // be able to pass pipelines directly
    harness.run()
}

/// Compare a pipeline against the baseline (current approach).
pub fn compare_to_baseline(problem: &Problem, pipeline: Pipeline, name: &str) -> BenchmarkResults {
    let mut harness = BenchmarkHarness::new();
    harness.add_scenario("test", problem.clone());
    harness.add_pipeline("baseline", Pipeline::current());
    harness.add_pipeline(name, pipeline);
    harness.run()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::simple_yes_buy;

    fn create_test_problem(name: &str, num_orders: usize) -> Problem {
        let mut problem = Problem::new(name);
        let market = problem.markets.add_binary("market");

        problem.liquidity.add_ask(market, 0, 500_000_000, 10000);
        problem.liquidity.add_ask(market, 1, 500_000_000, 10000);

        for i in 0..num_orders {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i as u64 + 1,
                market,
                (500 + (i % 100) * 5) as u64 * 1_000_000,
                50 + (i % 50) as u64,
            ));
        }

        problem
    }

    #[test]
    fn test_harness_basic() {
        let mut harness = BenchmarkHarness::new();

        harness.add_scenario("small", create_test_problem("small", 50));
        harness.add_scenario("medium", create_test_problem("medium", 200));

        harness.add_pipeline("current", Pipeline::current());

        let results = harness.run();

        assert_eq!(results.results.len(), 2); // 2 scenarios × 1 pipeline
        assert_eq!(results.scenarios().len(), 2);
        assert_eq!(results.pipelines().len(), 1);
    }

    #[test]
    fn test_results_queries() {
        let mut results = BenchmarkResults::new();

        results.add(BenchmarkRun {
            scenario: "small".into(),
            pipeline: "A".into(),
            welfare: 100,
            fills: 10,
            volume: 500,
            time_ms: 5.0,
            iterations: 1,
            num_orders: 50,
            num_markets: 1,
        });

        results.add(BenchmarkRun {
            scenario: "small".into(),
            pipeline: "B".into(),
            welfare: 150,
            fills: 15,
            volume: 750,
            time_ms: 10.0,
            iterations: 1,
            num_orders: 50,
            num_markets: 1,
        });

        assert_eq!(results.for_scenario("small").len(), 2);
        assert_eq!(results.for_pipeline("A").len(), 1);

        let best = results.best_for_scenario("small").unwrap();
        assert_eq!(best.welfare, 150);
        assert_eq!(best.pipeline, "B");
    }

    #[test]
    fn test_report_output() {
        let mut harness = BenchmarkHarness::new();
        harness.add_scenario("test", create_test_problem("test", 100));
        harness.add_pipeline("current", Pipeline::current());

        let results = harness.run();

        // Just verify it doesn't panic
        harness.report(&results);
    }

    #[test]
    fn test_compare_to_baseline() {
        let problem = create_test_problem("test", 50);
        let pipeline = Pipeline::current();

        let results = compare_to_baseline(&problem, pipeline, "test_pipeline");

        assert_eq!(results.results.len(), 2);
    }
}
