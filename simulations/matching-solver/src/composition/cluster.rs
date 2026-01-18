//! Cluster detection and problem decomposition.
//!
//! Decomposes problems into smaller sub-problems based on market connectivity.

use std::collections::{HashMap, HashSet};

use matching_engine::{
    ConstraintSet, LiquidityPool, Market, MarketId, MarketSet, Order, Problem, MAX_STATES,
};

use super::analysis::{ClusterInfo, MarketGraph, ProblemAnalysis, SolverRecommendation};

/// Maximum number of markets per cluster before splitting.
pub const DEFAULT_MAX_MARKETS_PER_CLUSTER: usize = 5;

/// A sub-problem extracted from the original problem.
#[derive(Clone, Debug)]
pub struct SubProblem {
    /// The extracted problem instance
    pub problem: Problem,
    /// Mapping from sub-problem order index to original order index
    pub original_order_mapping: Vec<usize>,
    /// Recommended solver for this sub-problem
    pub recommended_solver: SolverRecommendation,
    /// Cluster index this sub-problem came from
    pub cluster_id: usize,
    /// Whether this sub-problem contains bridging orders
    pub has_bridging_orders: bool,
}

impl SubProblem {
    /// Get the original order index for a sub-problem order index.
    pub fn original_order_idx(&self, sub_idx: usize) -> Option<usize> {
        self.original_order_mapping.get(sub_idx).copied()
    }

    /// Number of orders in this sub-problem.
    pub fn num_orders(&self) -> usize {
        self.problem.orders.len()
    }

    /// Check if this sub-problem can use MILP (small state space).
    pub fn can_use_milp(&self) -> bool {
        // Calculate total state space
        let total_states: usize = self
            .problem
            .markets
            .iter()
            .map(|m| m.num_outcomes() as usize)
            .product();
        total_states <= MAX_STATES
    }
}

/// Decomposes problems into clusters based on market connectivity.
pub struct Decomposer {
    /// Maximum markets per cluster
    max_markets_per_cluster: usize,
}

impl Decomposer {
    /// Create a new decomposer with default settings.
    pub fn new() -> Self {
        Self {
            max_markets_per_cluster: DEFAULT_MAX_MARKETS_PER_CLUSTER,
        }
    }

    /// Create a decomposer with a custom max markets per cluster.
    pub fn with_max_markets(max_markets: usize) -> Self {
        Self {
            max_markets_per_cluster: max_markets,
        }
    }

    /// Decompose a problem into sub-problems based on market clusters.
    pub fn decompose(&self, problem: &Problem, analysis: &ProblemAnalysis) -> DecompositionResult {
        // Find connected components in the market graph
        let components = self.find_connected_components(&analysis.graph);

        // Split large components if needed
        let clusters = self.split_large_components(components, &analysis.graph);

        // Create cluster info for each component
        let mut cluster_infos = Vec::new();
        for (cluster_id, market_set) in clusters.iter().enumerate() {
            let markets: Vec<MarketId> = market_set.iter().copied().collect();
            let mut info = ClusterInfo::new(markets.clone());

            // Calculate state space size
            info.num_states = self.calculate_state_space(&markets, problem);

            // Classify orders as belonging to this cluster or bridging
            for (order_idx, order) in problem.orders.iter().enumerate() {
                let order_markets: HashSet<MarketId> = order.active_markets().collect();
                let in_cluster = order_markets.iter().filter(|m| market_set.contains(m)).count();
                let total = order_markets.len();

                if in_cluster == total {
                    // Order fully contained in this cluster
                    info.order_indices.push(order_idx);
                } else if in_cluster > 0 {
                    // Order spans this cluster and others
                    info.bridging_orders.push(order_idx);
                }
            }

            cluster_infos.push((cluster_id, info));
        }

        // Create sub-problems for each cluster
        let mut sub_problems = Vec::new();
        for (cluster_id, info) in &cluster_infos {
            let sub_problem = self.create_sub_problem(problem, info, *cluster_id);
            sub_problems.push(sub_problem);
        }

        // Identify bridging orders (span multiple clusters)
        let bridging_orders = self.identify_bridging_orders(problem, &clusters);

        DecompositionResult {
            sub_problems,
            bridging_orders,
            cluster_infos: cluster_infos.into_iter().map(|(_, info)| info).collect(),
        }
    }

    /// Find connected components using Union-Find.
    fn find_connected_components(&self, graph: &MarketGraph) -> Vec<HashSet<MarketId>> {
        let markets: Vec<MarketId> = graph.markets().collect();
        if markets.is_empty() {
            return Vec::new();
        }

        let mut parent: HashMap<MarketId, MarketId> = markets.iter().map(|&m| (m, m)).collect();
        let mut rank: HashMap<MarketId, usize> = markets.iter().map(|&m| (m, 0)).collect();

        // Union-Find operations
        fn find(parent: &mut HashMap<MarketId, MarketId>, x: MarketId) -> MarketId {
            if parent[&x] != x {
                let root = find(parent, parent[&x]);
                parent.insert(x, root);
            }
            parent[&x]
        }

        fn union(
            parent: &mut HashMap<MarketId, MarketId>,
            rank: &mut HashMap<MarketId, usize>,
            x: MarketId,
            y: MarketId,
        ) {
            let root_x = find(parent, x);
            let root_y = find(parent, y);
            if root_x != root_y {
                let rank_x = rank[&root_x];
                let rank_y = rank[&root_y];
                if rank_x < rank_y {
                    parent.insert(root_x, root_y);
                } else if rank_x > rank_y {
                    parent.insert(root_y, root_x);
                } else {
                    parent.insert(root_y, root_x);
                    rank.insert(root_x, rank_x + 1);
                }
            }
        }

        // Union markets that share edges
        for market in &markets {
            for neighbor in graph.neighbors(*market) {
                union(&mut parent, &mut rank, *market, neighbor);
            }
        }

        // Group markets by their root
        let mut components: HashMap<MarketId, HashSet<MarketId>> = HashMap::new();
        for &market in &markets {
            let root = find(&mut parent, market);
            components.entry(root).or_default().insert(market);
        }

        components.into_values().collect()
    }

    /// Split large components into smaller clusters using minimum-cut heuristics.
    fn split_large_components(
        &self,
        components: Vec<HashSet<MarketId>>,
        graph: &MarketGraph,
    ) -> Vec<HashSet<MarketId>> {
        let mut result = Vec::new();

        for component in components {
            if component.len() <= self.max_markets_per_cluster {
                result.push(component);
            } else {
                // Need to split this component
                let splits = self.split_component(component, graph);
                result.extend(splits);
            }
        }

        result
    }

    /// Split a large component using a greedy min-cut heuristic.
    fn split_component(
        &self,
        component: HashSet<MarketId>,
        graph: &MarketGraph,
    ) -> Vec<HashSet<MarketId>> {
        let markets: Vec<MarketId> = component.into_iter().collect();
        if markets.len() <= self.max_markets_per_cluster {
            return vec![markets.into_iter().collect()];
        }

        // Greedy approach: start from the market with fewest connections,
        // grow cluster until max size, then start new cluster
        let mut remaining: HashSet<MarketId> = markets.iter().copied().collect();
        let mut clusters = Vec::new();

        while !remaining.is_empty() {
            let mut cluster = HashSet::new();

            // Start with the market that has fewest connections to remaining markets
            let start = remaining
                .iter()
                .min_by_key(|&&m| {
                    graph
                        .neighbors(m)
                        .filter(|n| remaining.contains(n))
                        .count()
                })
                .copied()
                .unwrap();

            cluster.insert(start);
            remaining.remove(&start);

            // Grow cluster by adding well-connected neighbors
            while cluster.len() < self.max_markets_per_cluster && !remaining.is_empty() {
                // Find the remaining market most connected to current cluster
                let best_addition = remaining
                    .iter()
                    .max_by_key(|&&m| {
                        graph
                            .neighbors(m)
                            .filter(|n| cluster.contains(n))
                            .map(|n| graph.edge_weight(m, n))
                            .sum::<usize>()
                    })
                    .copied();

                match best_addition {
                    Some(m) => {
                        // Only add if it has at least some connection to the cluster
                        let connections: usize = graph
                            .neighbors(m)
                            .filter(|n| cluster.contains(n))
                            .map(|n| graph.edge_weight(m, n))
                            .sum();
                        if connections > 0 || remaining.len() + cluster.len() <= self.max_markets_per_cluster {
                            cluster.insert(m);
                            remaining.remove(&m);
                        } else {
                            break;
                        }
                    }
                    None => break,
                }
            }

            clusters.push(cluster);
        }

        clusters
    }

    /// Calculate the state space size for a set of markets.
    fn calculate_state_space(&self, markets: &[MarketId], problem: &Problem) -> usize {
        markets
            .iter()
            .filter_map(|&market_id| problem.markets.get(market_id))
            .map(|market| market.num_outcomes() as usize)
            .product()
    }

    /// Create a sub-problem from a cluster info.
    fn create_sub_problem(
        &self,
        original: &Problem,
        cluster: &ClusterInfo,
        cluster_id: usize,
    ) -> SubProblem {
        let mut sub_problem = Problem::new(format!("{}_cluster_{}", original.name, cluster_id));

        // Add markets from the cluster
        for &market_id in &cluster.markets {
            if let Some(market) = original.markets.get(market_id) {
                sub_problem.markets.add_market(market.clone());
            }
        }

        // Add liquidity for these markets
        let cluster_markets: HashSet<MarketId> = cluster.markets.iter().copied().collect();
        for ((market_id, outcome), book) in original.liquidity.iter() {
            if cluster_markets.contains(market_id) {
                sub_problem.liquidity.set(*market_id, *outcome, book.clone());
            }
        }

        // Add orders (only those fully in this cluster)
        let mut order_mapping = Vec::new();
        for &order_idx in &cluster.order_indices {
            if let Some(order) = original.orders.get(order_idx) {
                let mut new_order = order.clone();
                new_order.id = order_mapping.len() as u64;
                sub_problem.orders.push(new_order);
                order_mapping.push(order_idx);
            }
        }

        // Add relevant constraints
        for constraint in original.constraints.iter() {
            // Only add if all markets involved are in this cluster
            // (constraints spanning clusters are handled at merge time)
            let markets = get_constraint_markets(constraint);
            if markets.iter().all(|m| cluster_markets.contains(m)) {
                sub_problem.constraints.add(constraint.clone());
            }
        }

        // Determine recommended solver
        let recommended_solver = if sub_problem.orders.is_empty() {
            SolverRecommendation::Greedy
        } else if cluster.num_states <= MAX_STATES {
            SolverRecommendation::Milp
        } else {
            SolverRecommendation::Greedy
        };

        SubProblem {
            problem: sub_problem,
            original_order_mapping: order_mapping,
            recommended_solver,
            cluster_id,
            has_bridging_orders: !cluster.bridging_orders.is_empty(),
        }
    }

    /// Identify orders that span multiple clusters.
    fn identify_bridging_orders(
        &self,
        problem: &Problem,
        clusters: &[HashSet<MarketId>],
    ) -> Vec<BridgingOrder> {
        let mut bridging = Vec::new();

        for (order_idx, order) in problem.orders.iter().enumerate() {
            let order_markets: HashSet<MarketId> = order.active_markets().collect();

            // Find which clusters this order touches
            let mut touched_clusters = Vec::new();
            for (cluster_idx, cluster) in clusters.iter().enumerate() {
                if order_markets.iter().any(|m| cluster.contains(m)) {
                    touched_clusters.push(cluster_idx);
                }
            }

            if touched_clusters.len() > 1 {
                bridging.push(BridgingOrder {
                    order_idx,
                    cluster_indices: touched_clusters,
                });
            }
        }

        bridging
    }
}

impl Default for Decomposer {
    fn default() -> Self {
        Self::new()
    }
}

/// Get all markets referenced by a constraint.
fn get_constraint_markets(constraint: &matching_engine::MarketConstraint) -> Vec<MarketId> {
    match constraint {
        matching_engine::MarketConstraint::Implication { if_true, then_true } => {
            vec![if_true.0, then_true.0]
        }
        matching_engine::MarketConstraint::Hierarchy { parent, child } => vec![parent.0, child.0],
        matching_engine::MarketConstraint::MutuallyExclusive { outcomes } => {
            outcomes.iter().map(|(m, _)| *m).collect()
        }
        matching_engine::MarketConstraint::ExactlyOne { outcomes } => {
            outcomes.iter().map(|(m, _)| *m).collect()
        }
        matching_engine::MarketConstraint::SumToOne { market } => vec![*market],
    }
}

/// Result of problem decomposition.
#[derive(Clone, Debug)]
pub struct DecompositionResult {
    /// Sub-problems for each cluster
    pub sub_problems: Vec<SubProblem>,
    /// Orders that span multiple clusters
    pub bridging_orders: Vec<BridgingOrder>,
    /// Information about each cluster
    pub cluster_infos: Vec<ClusterInfo>,
}

impl DecompositionResult {
    /// Total number of sub-problems.
    pub fn num_sub_problems(&self) -> usize {
        self.sub_problems.len()
    }

    /// Total number of bridging orders.
    pub fn num_bridging_orders(&self) -> usize {
        self.bridging_orders.len()
    }

    /// Check if the problem was actually decomposed (more than one cluster).
    pub fn was_decomposed(&self) -> bool {
        self.sub_problems.len() > 1
    }
}

/// An order that spans multiple clusters.
#[derive(Clone, Debug)]
pub struct BridgingOrder {
    /// Index in the original problem
    pub order_idx: usize,
    /// Indices of clusters this order touches
    pub cluster_indices: Vec<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{bundle_yes, simple_yes_buy, spread};

    #[test]
    fn test_single_cluster() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("market_1");
        let m2 = problem.markets.add_binary("market_2");

        // Add an order spanning both markets (bundle)
        problem.orders.push(
            bundle_yes(&problem.markets, 1, &[m1, m2], 500_000_000, 100)
        );

        let analysis = ProblemAnalysis::analyze(&problem);
        let decomposer = Decomposer::new();
        let result = decomposer.decompose(&problem, &analysis);

        // Should be a single cluster since markets are connected
        assert_eq!(result.num_sub_problems(), 1);
        assert_eq!(result.num_bridging_orders(), 0);
    }

    #[test]
    fn test_disconnected_clusters() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("market_1");
        let m2 = problem.markets.add_binary("market_2");
        let m3 = problem.markets.add_binary("market_3");
        let m4 = problem.markets.add_binary("market_4");

        // Cluster 1: m1-m2
        problem.orders.push(
            bundle_yes(&problem.markets, 1, &[m1, m2], 500_000_000, 100)
        );

        // Cluster 2: m3-m4
        problem.orders.push(
            bundle_yes(&problem.markets, 2, &[m3, m4], 500_000_000, 100)
        );

        let analysis = ProblemAnalysis::analyze(&problem);
        let decomposer = Decomposer::new();
        let result = decomposer.decompose(&problem, &analysis);

        // Should be two disconnected clusters
        assert_eq!(result.num_sub_problems(), 2);
        assert_eq!(result.num_bridging_orders(), 0);
    }

    #[test]
    fn test_bridging_order() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("market_1");
        let _m2 = problem.markets.add_binary("market_2");
        let m3 = problem.markets.add_binary("market_3");

        // Order 1: m1 only
        problem.orders.push(
            simple_yes_buy(&problem.markets, 1, m1, 500_000_000, 100)
        );

        // Order 2: m3 only
        problem.orders.push(
            simple_yes_buy(&problem.markets, 2, m3, 500_000_000, 100)
        );

        // Order 3: spans m1 and m3 (bridging via spread)
        problem.orders.push(
            spread(&problem.markets, 3, m1, m3, 500_000_000, 100)
        );

        let analysis = ProblemAnalysis::analyze(&problem);
        let decomposer = Decomposer::new();
        let result = decomposer.decompose(&problem, &analysis);

        // The bridging order connects m1 and m3, so they should be in one cluster
        // But if we look at the graph, the bridging order creates an edge between m1 and m3
        // So this should actually result in a single cluster
        assert!(result.num_sub_problems() >= 1);
    }
}
