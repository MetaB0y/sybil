//! Maximum Weight Independent Set (MWIS) solver for fill selection.
//!
//! Given a conflict graph and weights for each node (fill welfare),
//! finds the maximum weight subset of nodes with no conflicts.
//!
//! Provides multiple algorithms:
//! - Exact ILP for small instances
//! - Greedy approximation for large instances
//! - LP relaxation + rounding for medium instances

use super::conflict::ConflictGraph;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;

/// Algorithm to use for MWIS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MwisAlgorithm {
    /// Automatically select based on problem size
    Auto,
    /// Greedy: weight / (1 + degree) priority
    Greedy,
    /// Randomized greedy with multiple iterations
    RandomizedGreedy { iterations: usize, seed: u64 },
    /// Exact ILP (requires milp feature, falls back to greedy if not available)
    #[cfg(feature = "milp")]
    ExactIlp,
}

impl Default for MwisAlgorithm {
    fn default() -> Self {
        MwisAlgorithm::Auto
    }
}

/// MWIS solver.
pub struct MwisSolver {
    algorithm: MwisAlgorithm,
}

impl MwisSolver {
    /// Create a new solver with the specified algorithm.
    pub fn new(algorithm: MwisAlgorithm) -> Self {
        Self { algorithm }
    }

    /// Solve MWIS on the given graph with weights.
    pub fn solve(&self, graph: &ConflictGraph, weights: &[i64]) -> Vec<usize> {
        if graph.num_nodes() == 0 {
            return Vec::new();
        }

        // If no conflicts, select all positive-weight nodes
        if graph.is_empty() {
            return (0..weights.len())
                .filter(|&i| weights[i] > 0)
                .collect();
        }

        let algorithm = self.select_algorithm(graph);

        match algorithm {
            MwisAlgorithm::Auto => unreachable!("select_algorithm should resolve Auto"),
            MwisAlgorithm::Greedy => self.solve_greedy(graph, weights),
            MwisAlgorithm::RandomizedGreedy { iterations, seed } => {
                self.solve_randomized_greedy(graph, weights, iterations, seed)
            }
            #[cfg(feature = "milp")]
            MwisAlgorithm::ExactIlp => self.solve_ilp(graph, weights),
        }
    }

    /// Select the best algorithm based on problem characteristics.
    fn select_algorithm(&self, graph: &ConflictGraph) -> MwisAlgorithm {
        match self.algorithm {
            MwisAlgorithm::Auto => {
                let n = graph.num_nodes();
                let density = graph.density();

                if n <= 50 {
                    // Small: use exact ILP if available
                    #[cfg(feature = "milp")]
                    {
                        MwisAlgorithm::ExactIlp
                    }
                    #[cfg(not(feature = "milp"))]
                    {
                        MwisAlgorithm::RandomizedGreedy {
                            iterations: 100,
                            seed: 42,
                        }
                    }
                } else if n <= 500 {
                    // Medium: randomized greedy
                    MwisAlgorithm::RandomizedGreedy {
                        iterations: 50,
                        seed: 42,
                    }
                } else {
                    // Large: fast greedy
                    MwisAlgorithm::Greedy
                }
            }
            other => other,
        }
    }

    /// Greedy MWIS: repeatedly select node with highest weight/(1+degree).
    fn solve_greedy(&self, graph: &ConflictGraph, weights: &[i64]) -> Vec<usize> {
        let n = graph.num_nodes();
        let mut selected = Vec::new();
        let mut removed = vec![false; n];

        // Priority: weight / (1 + degree)
        let mut priorities: Vec<(usize, f64)> = (0..n)
            .map(|i| {
                let priority = if weights[i] > 0 {
                    weights[i] as f64 / (1.0 + graph.degree(i) as f64)
                } else {
                    f64::NEG_INFINITY
                };
                (i, priority)
            })
            .collect();

        while !priorities.is_empty() {
            // Find node with highest priority
            let best_idx = priorities
                .iter()
                .enumerate()
                .max_by(|(_, (_, p1)), (_, (_, p2))| p1.partial_cmp(p2).unwrap())
                .map(|(idx, _)| idx);

            let Some(best_idx) = best_idx else { break };
            let (node, priority) = priorities[best_idx];

            if priority <= 0.0 {
                break;
            }

            // Select this node
            selected.push(node);
            removed[node] = true;

            // Remove this node and its neighbors
            for &neighbor in graph.neighbors(node) {
                removed[neighbor] = true;
            }

            // Update priorities - remove selected and conflicting nodes
            priorities.retain(|(i, _)| !removed[*i]);
        }

        selected
    }

    /// Randomized greedy: run multiple iterations with random tie-breaking.
    fn solve_randomized_greedy(
        &self,
        graph: &ConflictGraph,
        weights: &[i64],
        iterations: usize,
        seed: u64,
    ) -> Vec<usize> {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut best_selected = self.solve_greedy(graph, weights);
        let mut best_weight: i64 = best_selected.iter().map(|&i| weights[i]).sum();

        for _ in 0..iterations {
            let selected = self.solve_greedy_randomized(graph, weights, &mut rng);
            let weight: i64 = selected.iter().map(|&i| weights[i]).sum();

            if weight > best_weight {
                best_weight = weight;
                best_selected = selected;
            }
        }

        best_selected
    }

    /// Single iteration of randomized greedy.
    fn solve_greedy_randomized(
        &self,
        graph: &ConflictGraph,
        weights: &[i64],
        rng: &mut ChaCha8Rng,
    ) -> Vec<usize> {
        let n = graph.num_nodes();
        let mut selected = Vec::new();
        let mut removed = vec![false; n];

        // Add randomized weights
        let randomized_weights: Vec<f64> = weights
            .iter()
            .map(|&w| {
                if w > 0 {
                    w as f64 * (0.8 + 0.4 * rng.gen::<f64>()) // Random factor 0.8-1.2
                } else {
                    f64::NEG_INFINITY
                }
            })
            .collect();

        let mut priorities: Vec<(usize, f64)> = (0..n)
            .map(|i| {
                let priority = randomized_weights[i] / (1.0 + graph.degree(i) as f64);
                (i, priority)
            })
            .collect();

        while !priorities.is_empty() {
            let best_idx = priorities
                .iter()
                .enumerate()
                .max_by(|(_, (_, p1)), (_, (_, p2))| p1.partial_cmp(p2).unwrap())
                .map(|(idx, _)| idx);

            let Some(best_idx) = best_idx else { break };
            let (node, priority) = priorities[best_idx];

            if priority <= 0.0 {
                break;
            }

            selected.push(node);
            removed[node] = true;

            for &neighbor in graph.neighbors(node) {
                removed[neighbor] = true;
            }

            priorities.retain(|(i, _)| !removed[*i]);
        }

        selected
    }

    /// Exact ILP solution using HiGHS.
    #[cfg(feature = "milp")]
    fn solve_ilp(&self, graph: &ConflictGraph, weights: &[i64]) -> Vec<usize> {
        use highs::{HighsModelStatus, RowProblem, Sense};

        let n = graph.num_nodes();
        let mut pb = RowProblem::default();

        // Binary variable x_i for each node (use add_integer_column for MIP)
        let x_cols: Vec<_> = (0..n)
            .map(|i| {
                // Objective: maximize sum of weights * x_i
                pb.add_integer_column(weights[i] as f64, 0.0..=1.0)
            })
            .collect();

        // Constraint: x_i + x_j <= 1 for each edge (i, j)
        for i in 0..n {
            for &j in graph.neighbors(i) {
                if i < j {
                    // Only add once per edge
                    pb.add_row(..=1.0, [(x_cols[i], 1.0), (x_cols[j], 1.0)]);
                }
            }
        }

        let mut model = pb.optimise(Sense::Maximise);

        // Set time limit to avoid hanging on hard instances
        model.set_option("time_limit", 5.0);

        let solved = model.solve();

        match solved.status() {
            HighsModelStatus::Optimal
            | HighsModelStatus::ObjectiveBound
            | HighsModelStatus::ReachedTimeLimit => {
                // Extract solution using Index trait
                let sol = solved.get_solution();
                (0..n)
                    .filter(|&i| sol[x_cols[i]] > 0.5)
                    .collect()
            }
            _ => {
                // Fall back to greedy
                self.solve_greedy(graph, weights)
            }
        }
    }
}

impl Default for MwisSolver {
    fn default() -> Self {
        Self::new(MwisAlgorithm::Auto)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mwis_empty() {
        let graph = ConflictGraph::new(0);
        let solver = MwisSolver::default();
        let result = solver.solve(&graph, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_mwis_no_conflicts() {
        let graph = ConflictGraph::new(3);
        let weights = vec![10, 20, 30];
        let solver = MwisSolver::default();
        let result = solver.solve(&graph, &weights);
        assert_eq!(result.len(), 3);
        let total_weight: i64 = result.iter().map(|&i| weights[i]).sum();
        assert_eq!(total_weight, 60);
    }

    #[test]
    fn test_mwis_single_edge() {
        let mut graph = ConflictGraph::new(2);
        graph.add_edge(0, 1);

        let weights = vec![10, 20];
        let solver = MwisSolver::new(MwisAlgorithm::Greedy);
        let result = solver.solve(&graph, &weights);

        // Should select node 1 (higher weight)
        assert_eq!(result.len(), 1);
        assert!(result.contains(&1));
    }

    #[test]
    fn test_mwis_triangle() {
        // Complete graph on 3 nodes - can only select one
        let mut graph = ConflictGraph::new(3);
        graph.add_edge(0, 1);
        graph.add_edge(0, 2);
        graph.add_edge(1, 2);

        let weights = vec![10, 20, 15];
        let solver = MwisSolver::new(MwisAlgorithm::Greedy);
        let result = solver.solve(&graph, &weights);

        assert_eq!(result.len(), 1);
        assert!(result.contains(&1)); // Highest weight
    }

    #[test]
    fn test_mwis_path() {
        // Path graph: 0 - 1 - 2 - 3
        let mut graph = ConflictGraph::new(4);
        graph.add_edge(0, 1);
        graph.add_edge(1, 2);
        graph.add_edge(2, 3);

        // Optimal: select 0 and 2, or 1 and 3
        let weights = vec![10, 5, 10, 5];
        let solver = MwisSolver::new(MwisAlgorithm::Greedy);
        let result = solver.solve(&graph, &weights);

        // Should select 0 and 2 (total 20)
        let total_weight: i64 = result.iter().map(|&i| weights[i]).sum();
        assert_eq!(total_weight, 20);
    }

    #[test]
    fn test_mwis_randomized() {
        let mut graph = ConflictGraph::new(5);
        graph.add_edge(0, 1);
        graph.add_edge(1, 2);
        graph.add_edge(2, 3);
        graph.add_edge(3, 4);

        let weights = vec![10, 20, 10, 20, 10];
        let solver = MwisSolver::new(MwisAlgorithm::RandomizedGreedy {
            iterations: 10,
            seed: 42,
        });
        let result = solver.solve(&graph, &weights);

        // Verify it's a valid independent set
        for &i in &result {
            for &j in &result {
                if i != j {
                    assert!(!graph.are_adjacent(i, j));
                }
            }
        }
    }

    #[cfg(feature = "milp")]
    #[test]
    fn test_mwis_ilp() {
        let mut graph = ConflictGraph::new(4);
        graph.add_edge(0, 1);
        graph.add_edge(1, 2);
        graph.add_edge(2, 3);

        let weights = vec![10, 5, 10, 5];
        let solver = MwisSolver::new(MwisAlgorithm::ExactIlp);
        let result = solver.solve(&graph, &weights);

        let total_weight: i64 = result.iter().map(|&i| weights[i]).sum();
        assert_eq!(total_weight, 20);
    }
}
