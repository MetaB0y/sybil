//! Problem analysis for solver composition.
//!
//! Analyzes problem structure to identify market connectivity,
//! order classifications, and optimal solver routing.

use std::collections::{HashMap, HashSet};

use matching_engine::{MarketConstraint, MarketId, Order, Problem};

/// Graph representation of market connectivity.
///
/// Markets are connected if any order spans both of them.
#[derive(Clone, Debug)]
pub struct MarketGraph {
    /// Adjacency list: market -> connected markets
    adjacency: HashMap<MarketId, HashSet<MarketId>>,
    /// Edge weights: (market_a, market_b) -> number of orders spanning both
    edge_weights: HashMap<(MarketId, MarketId), usize>,
    /// All markets in the graph
    markets: HashSet<MarketId>,
}

impl MarketGraph {
    /// Create an empty market graph.
    pub fn new() -> Self {
        Self {
            adjacency: HashMap::new(),
            edge_weights: HashMap::new(),
            markets: HashSet::new(),
        }
    }

    /// Build a market graph from a problem's orders.
    pub fn from_problem(problem: &Problem) -> Self {
        let mut graph = Self::new();

        for order in &problem.orders {
            let markets: Vec<MarketId> = order.active_markets().collect();

            // Add all markets to the graph
            for &market in &markets {
                graph.add_market(market);
            }

            // Add edges between all pairs of markets in this order
            for i in 0..markets.len() {
                for j in (i + 1)..markets.len() {
                    graph.add_edge(markets[i], markets[j]);
                }
            }
        }

        // Also add edges from constraints (implications must stay together)
        for constraint in problem.constraints.iter() {
            // Constraints often link markets that should be in the same cluster
            if let Some((m1, m2)) = get_linked_markets(constraint) {
                graph.add_market(m1);
                graph.add_market(m2);
                graph.add_edge(m1, m2);
            }
        }

        graph
    }

    /// Add a market to the graph.
    pub fn add_market(&mut self, market: MarketId) {
        self.markets.insert(market);
        self.adjacency.entry(market).or_default();
    }

    /// Add an edge between two markets.
    pub fn add_edge(&mut self, a: MarketId, b: MarketId) {
        if a == b {
            return;
        }

        self.adjacency.entry(a).or_default().insert(b);
        self.adjacency.entry(b).or_default().insert(a);

        // Normalize edge key (smaller first based on inner value)
        let key = if a.0 < b.0 { (a, b) } else { (b, a) };
        *self.edge_weights.entry(key).or_insert(0) += 1;
    }

    /// Get all markets in the graph.
    pub fn markets(&self) -> impl Iterator<Item = MarketId> + '_ {
        self.markets.iter().copied()
    }

    /// Get neighbors of a market.
    pub fn neighbors(&self, market: MarketId) -> impl Iterator<Item = MarketId> + '_ {
        self.adjacency
            .get(&market)
            .map(|s| s.iter().copied())
            .into_iter()
            .flatten()
    }

    /// Get the edge weight between two markets.
    pub fn edge_weight(&self, a: MarketId, b: MarketId) -> usize {
        let key = if a.0 < b.0 { (a, b) } else { (b, a) };
        self.edge_weights.get(&key).copied().unwrap_or(0)
    }

    /// Get the number of markets in the graph.
    pub fn num_markets(&self) -> usize {
        self.markets.len()
    }

    /// Get the number of edges in the graph.
    pub fn num_edges(&self) -> usize {
        self.edge_weights.len()
    }

    /// Check if the graph is empty.
    pub fn is_empty(&self) -> bool {
        self.markets.is_empty()
    }
}

impl Default for MarketGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a cluster of connected markets.
#[derive(Clone, Debug)]
pub struct ClusterInfo {
    /// Markets in this cluster
    pub markets: Vec<MarketId>,
    /// Indices of orders that only involve markets in this cluster
    pub order_indices: Vec<usize>,
    /// Indices of orders that span this cluster and others (bridging orders)
    pub bridging_orders: Vec<usize>,
    /// Total number of atomic states (product of outcomes per market)
    pub num_states: usize,
}

impl ClusterInfo {
    /// Create a new cluster info.
    pub fn new(markets: Vec<MarketId>) -> Self {
        Self {
            markets,
            order_indices: Vec::new(),
            bridging_orders: Vec::new(),
            num_states: 0,
        }
    }

    /// Check if a market is in this cluster.
    pub fn contains_market(&self, market: MarketId) -> bool {
        self.markets.contains(&market)
    }

    /// Get the number of markets in this cluster.
    pub fn num_markets(&self) -> usize {
        self.markets.len()
    }

    /// Check if this cluster can use MILP (small enough state space).
    pub fn can_use_milp(&self) -> bool {
        self.num_states <= 32 // MAX_STATES
    }
}

/// Classification of an order for solver routing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderClassification {
    /// Simple single-market order
    Simple,
    /// Multi-market bundle order
    Bundle,
    /// Order with price condition
    Conditional,
    /// Potential arbitrage opportunity
    Arbitrage,
    /// All-or-none order
    AllOrNone,
}

/// Complete analysis of a problem's structure.
#[derive(Clone, Debug)]
pub struct ProblemAnalysis {
    /// Market connectivity graph
    pub graph: MarketGraph,
    /// Cluster information (populated after decomposition)
    pub clusters: Vec<ClusterInfo>,
    /// Order classifications
    pub order_classifications: Vec<(usize, OrderClassification)>,
    /// Indices of conditional orders
    pub conditional_orders: Vec<usize>,
    /// Indices of potential arbitrage orders
    pub arbitrage_candidates: Vec<usize>,
    /// Total number of orders
    pub num_orders: usize,
    /// Number of bundle orders
    pub num_bundles: usize,
    /// Whether the problem has constraints (implications)
    pub has_constraints: bool,
}

impl ProblemAnalysis {
    /// Analyze a problem's structure.
    pub fn analyze(problem: &Problem) -> Self {
        let graph = MarketGraph::from_problem(problem);
        let mut order_classifications = Vec::new();
        let mut conditional_orders = Vec::new();
        let mut arbitrage_candidates = Vec::new();
        let mut num_bundles = 0;

        for (idx, order) in problem.orders.iter().enumerate() {
            let classification = classify_order(order);
            order_classifications.push((idx, classification));

            match classification {
                OrderClassification::Bundle => num_bundles += 1,
                OrderClassification::Conditional => conditional_orders.push(idx),
                OrderClassification::Arbitrage => arbitrage_candidates.push(idx),
                _ => {}
            }
        }

        // Detect arbitrage opportunities from constraints
        let constraint_arbitrage = detect_constraint_arbitrage(problem);
        for idx in constraint_arbitrage {
            if !arbitrage_candidates.contains(&idx) {
                arbitrage_candidates.push(idx);
            }
        }

        Self {
            graph,
            clusters: Vec::new(), // Populated by decomposer
            order_classifications,
            conditional_orders,
            arbitrage_candidates,
            num_orders: problem.num_orders(),
            num_bundles,
            has_constraints: !problem.constraints.is_empty(),
        }
    }

    /// Check if the problem is small enough to solve directly with MILP.
    pub fn can_solve_directly(&self) -> bool {
        // Small state space (≤32 states total)
        self.graph.num_markets() <= 5 && !self.has_constraints
    }

    /// Get recommended solver for an order index.
    pub fn recommended_solver(&self, order_idx: usize) -> SolverRecommendation {
        for (idx, class) in &self.order_classifications {
            if *idx == order_idx {
                return match class {
                    OrderClassification::Arbitrage => SolverRecommendation::Arbitrage,
                    OrderClassification::Conditional => SolverRecommendation::ConditionalFirst,
                    OrderClassification::AllOrNone => SolverRecommendation::Milp,
                    OrderClassification::Bundle => SolverRecommendation::Milp,
                    OrderClassification::Simple => SolverRecommendation::Greedy,
                };
            }
        }
        SolverRecommendation::Greedy
    }
}

/// Recommended solver type for an order or cluster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolverRecommendation {
    /// Use MILP for optimal solution
    Milp,
    /// Use greedy for fast approximate solution
    Greedy,
    /// Check for arbitrage first
    Arbitrage,
    /// Evaluate conditional first
    ConditionalFirst,
}

/// Classify an order based on its structure.
fn classify_order(order: &Order) -> OrderClassification {
    if order.is_conditional() {
        return OrderClassification::Conditional;
    }

    if order.is_all_or_none() {
        return OrderClassification::AllOrNone;
    }

    if order.num_markets > 1 {
        // Check if this is a potential arbitrage (e.g., buying same outcome cheaper elsewhere)
        if is_potential_arbitrage(order) {
            return OrderClassification::Arbitrage;
        }
        return OrderClassification::Bundle;
    }

    OrderClassification::Simple
}

/// Check if an order might be an arbitrage opportunity.
fn is_potential_arbitrage(order: &Order) -> bool {
    // An order buying and selling the same effective position across markets
    // could be arbitrage. For now, detect spread orders with opposing payoffs.
    let positive_count = order
        .payoffs
        .iter()
        .take(order.num_states as usize)
        .filter(|&&p| p > 0)
        .count();
    let negative_count = order
        .payoffs
        .iter()
        .take(order.num_states as usize)
        .filter(|&&p| p < 0)
        .count();

    // If order has both positive and negative payoffs, might be arbitrage
    positive_count > 0 && negative_count > 0
}

/// Get linked markets from a constraint.
fn get_linked_markets(constraint: &MarketConstraint) -> Option<(MarketId, MarketId)> {
    match constraint {
        MarketConstraint::Implication { if_true, then_true } => Some((if_true.0, then_true.0)),
        MarketConstraint::Hierarchy { parent, child } => Some((parent.0, child.0)),
        MarketConstraint::MutuallyExclusive { outcomes } => {
            if outcomes.len() >= 2 {
                Some((outcomes[0].0, outcomes[1].0))
            } else {
                None
            }
        }
        MarketConstraint::ExactlyOne { outcomes } => {
            if outcomes.len() >= 2 {
                Some((outcomes[0].0, outcomes[1].0))
            } else {
                None
            }
        }
        MarketConstraint::SumToOne { .. } => None,
    }
}

/// Check if a constraint involves a specific market.
fn constraint_involves_market(constraint: &MarketConstraint, market: MarketId) -> bool {
    match constraint {
        MarketConstraint::Implication { if_true, then_true } => {
            if_true.0 == market || then_true.0 == market
        }
        MarketConstraint::Hierarchy { parent, child } => parent.0 == market || child.0 == market,
        MarketConstraint::MutuallyExclusive { outcomes } => {
            outcomes.iter().any(|(m, _)| *m == market)
        }
        MarketConstraint::ExactlyOne { outcomes } => outcomes.iter().any(|(m, _)| *m == market),
        MarketConstraint::SumToOne { market: m } => *m == market,
    }
}

/// Detect arbitrage opportunities from market constraints.
///
/// If A→B (A implies B) and price(A) > price(B), buying A and selling B is riskless profit.
fn detect_constraint_arbitrage(problem: &Problem) -> Vec<usize> {
    let mut candidates = Vec::new();

    // This would require price information which we don't have at analysis time.
    // Mark orders that could participate in constraint arbitrage for later evaluation.
    for (idx, order) in problem.orders.iter().enumerate() {
        // Check if any of the order's markets are involved in constraints
        for market in order.active_markets() {
            let involved = problem
                .constraints
                .iter()
                .any(|c| constraint_involves_market(c, market));
            if involved {
                // This order might be part of an arbitrage with constraint-linked markets
                candidates.push(idx);
                break;
            }
        }
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_graph_construction() {
        let mut graph = MarketGraph::new();
        let m1 = MarketId::new(1);
        let m2 = MarketId::new(2);
        let m3 = MarketId::new(3);

        graph.add_market(m1);
        graph.add_market(m2);
        graph.add_market(m3);
        graph.add_edge(m1, m2);
        graph.add_edge(m2, m3);

        assert_eq!(graph.num_markets(), 3);
        assert_eq!(graph.num_edges(), 2);
        assert_eq!(graph.edge_weight(m1, m2), 1);
        assert_eq!(graph.edge_weight(m2, m3), 1);
        assert_eq!(graph.edge_weight(m1, m3), 0);
    }

    #[test]
    fn test_cluster_info() {
        let m1 = MarketId::new(1);
        let m2 = MarketId::new(2);

        let cluster = ClusterInfo::new(vec![m1, m2]);
        assert!(cluster.contains_market(m1));
        assert!(cluster.contains_market(m2));
        assert!(!cluster.contains_market(MarketId::new(3)));
        assert_eq!(cluster.num_markets(), 2);
    }

    #[test]
    fn test_order_classification() {
        let mut order = Order::new(1);
        order.num_markets = 1;
        order.num_states = 2;
        assert_eq!(classify_order(&order), OrderClassification::Simple);

        order.num_markets = 2;
        assert_eq!(classify_order(&order), OrderClassification::Bundle);

        order.min_fill = 100;
        order.max_fill = 100;
        assert_eq!(classify_order(&order), OrderClassification::AllOrNone);
    }
}
