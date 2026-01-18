//! Randomized greedy solver for the matching problem.
//!
//! Runs the greedy algorithm multiple times with different random order permutations
//! and keeps the best result. This can find better solutions than deterministic greedy
//! when there are ties in welfare potential or when the order of processing matters.

use matching_engine::Problem;
use rand::prelude::*;
use rand::SeedableRng;

use crate::{GreedySolver, MatchingResult, Solver};

/// Randomized greedy solver that runs multiple iterations with shuffled orders.
pub struct RandomizedGreedySolver {
    /// Number of iterations to run
    pub iterations: usize,
    /// Random seed for reproducibility
    pub seed: u64,
}

impl RandomizedGreedySolver {
    pub fn new(iterations: usize, seed: u64) -> Self {
        Self { iterations, seed }
    }

    /// Create with default parameters
    pub fn default_params() -> Self {
        Self {
            iterations: 100,
            seed: 42,
        }
    }
}

impl Default for RandomizedGreedySolver {
    fn default() -> Self {
        Self::default_params()
    }
}

impl Solver for RandomizedGreedySolver {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.seed);
        let mut best_result: Option<MatchingResult> = None;
        let mut best_welfare = i64::MIN;

        // First run deterministic greedy (with sorting) as baseline
        let sorting_greedy = GreedySolver::new();
        let baseline = sorting_greedy.solve(problem);
        if baseline.total_welfare > best_welfare {
            best_welfare = baseline.total_welfare;
            best_result = Some(baseline);
        }

        // For randomized iterations, use preserve_order so shuffling matters
        let preserve_order_greedy = GreedySolver::preserve_order();

        // Run randomized iterations
        for _ in 0..self.iterations {
            // Create a problem with shuffled orders
            let shuffled_problem = shuffle_orders(problem, &mut rng);

            // Solve with greedy that preserves input order (so shuffling matters!)
            let result = preserve_order_greedy.solve(&shuffled_problem);

            // Keep best result
            if result.total_welfare > best_welfare {
                best_welfare = result.total_welfare;
                best_result = Some(result);
            }
        }

        best_result.unwrap_or_else(|| MatchingResult::new(problem.liquidity.snapshot()))
    }

    fn name(&self) -> &str {
        "Randomized"
    }
}

/// Create a new problem with shuffled order sequence.
fn shuffle_orders(problem: &Problem, rng: &mut impl Rng) -> Problem {
    let mut shuffled_orders = problem.orders.clone();
    shuffled_orders.shuffle(rng);

    Problem {
        name: problem.name.clone(),
        markets: problem.markets.clone(),
        liquidity: problem.liquidity.clone(),
        orders: shuffled_orders,
        constraints: problem.constraints.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_randomized_solver_creation() {
        let solver = RandomizedGreedySolver::new(50, 123);
        assert_eq!(solver.iterations, 50);
        assert_eq!(solver.seed, 123);
    }

    #[test]
    fn test_default_params() {
        let solver = RandomizedGreedySolver::default_params();
        assert_eq!(solver.iterations, 100);
        assert_eq!(solver.seed, 42);
    }
}
