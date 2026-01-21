//! Conflict graph builder for solution combining.
//!
//! Builds a graph where nodes are fills and edges represent conflicts
//! (fills that cannot coexist in a valid solution).

use std::collections::HashMap;

use matching_engine::{Fill, MarketId, MarketSet, Order, Qty};

/// Conflict graph represented as adjacency lists.
#[derive(Clone, Debug)]
pub struct ConflictGraph {
    /// Number of nodes (fills)
    num_nodes: usize,
    /// Adjacency list: for each node, the set of conflicting nodes
    adjacency: Vec<Vec<usize>>,
    /// Number of edges (conflicts)
    num_edges: usize,
}

impl ConflictGraph {
    /// Create a new empty conflict graph with n nodes.
    pub fn new(num_nodes: usize) -> Self {
        Self {
            num_nodes,
            adjacency: vec![Vec::new(); num_nodes],
            num_edges: 0,
        }
    }

    /// Add an undirected edge (conflict) between two nodes.
    pub fn add_edge(&mut self, a: usize, b: usize) {
        if a < self.num_nodes && b < self.num_nodes && a != b {
            // Check if edge already exists
            if !self.adjacency[a].contains(&b) {
                self.adjacency[a].push(b);
                self.adjacency[b].push(a);
                self.num_edges += 1;
            }
        }
    }

    /// Get the number of nodes.
    pub fn num_nodes(&self) -> usize {
        self.num_nodes
    }

    /// Get the number of edges (conflicts).
    pub fn num_edges(&self) -> usize {
        self.num_edges
    }

    /// Get neighbors (conflicting nodes) for a given node.
    pub fn neighbors(&self, node: usize) -> &[usize] {
        if node < self.num_nodes {
            &self.adjacency[node]
        } else {
            &[]
        }
    }

    /// Get the degree (number of conflicts) for a node.
    pub fn degree(&self, node: usize) -> usize {
        if node < self.num_nodes {
            self.adjacency[node].len()
        } else {
            0
        }
    }

    /// Check if two nodes are connected (conflict).
    #[cfg(test)]
    pub fn are_adjacent(&self, a: usize, b: usize) -> bool {
        if a < self.num_nodes {
            self.adjacency[a].contains(&b)
        } else {
            false
        }
    }

    /// Check if the graph is empty (no conflicts).
    pub fn is_empty(&self) -> bool {
        self.num_edges == 0
    }

    /// Get all nodes with no conflicts (can be freely selected).
    #[cfg(test)]
    pub fn isolated_nodes(&self) -> Vec<usize> {
        (0..self.num_nodes)
            .filter(|&i| self.adjacency[i].is_empty())
            .collect()
    }
}

/// Tracks what resources a fill consumes.
#[derive(Clone, Debug)]
pub struct FillFootprint {
    /// Liquidity consumed: (market, outcome) -> quantity
    pub liquidity_consumed: HashMap<(MarketId, u8), Qty>,
}

impl FillFootprint {
    /// Create a footprint from a fill and order.
    ///
    /// `markets` provides the actual market definitions to get correct outcome counts.
    pub fn from_fill(order: &Order, fill: &Fill, markets: &MarketSet) -> Self {
        let mut liquidity_consumed = HashMap::new();

        // Get actual market sizes from MarketSet
        let market_sizes: Vec<u8> = (0..order.num_markets as usize)
            .map(|i| {
                let market_id = order.markets[i];
                if market_id.is_none() {
                    2 // Default to binary if market not found
                } else {
                    markets.num_outcomes(market_id).max(2)
                }
            })
            .collect();

        // For each market in the order, determine which outcome is being bought
        for market_idx in 0..order.num_markets as usize {
            let market = order.markets[market_idx];
            if market.is_none() {
                continue;
            }

            // Determine outcome being bought
            let outcome = Self::determine_outcome_for_market(order, market_idx, &market_sizes);
            let key = (market, outcome);

            // Add the fill quantity as liquidity consumed
            liquidity_consumed.insert(key, fill.fill_qty);
        }

        Self { liquidity_consumed }
    }

    /// Determine which outcome is being bought for a specific market.
    fn determine_outcome_for_market(order: &Order, market_idx: usize, market_sizes: &[u8]) -> u8 {
        let num_markets = order.num_markets as usize;
        if market_idx >= num_markets {
            return 0;
        }

        // Simple case: single market order
        if num_markets == 1 {
            // Find the outcome with highest payoff
            let mut best_outcome = 0u8;
            let mut best_payoff = i8::MIN;

            for (i, &payoff) in order.payoffs.iter().take(order.num_states as usize).enumerate() {
                if payoff > best_payoff {
                    best_payoff = payoff;
                    best_outcome = i as u8;
                }
            }
            return best_outcome;
        }

        // Multi-market case: analyze payoff vector using actual market sizes
        let max_outcomes = market_sizes.iter().max().copied().unwrap_or(2) as usize;
        let mut outcome_votes: Vec<i32> = vec![0; max_outcomes.max(4)];

        for state_idx in 0..order.num_states as usize {
            let payoff = order.payoffs[state_idx];
            if payoff > 0 {
                let outcome = Self::extract_outcome_from_state(state_idx, market_idx, market_sizes);
                if (outcome as usize) < outcome_votes.len() {
                    outcome_votes[outcome as usize] += payoff as i32;
                }
            }
        }

        outcome_votes
            .iter()
            .enumerate()
            .max_by_key(|(_, &v)| v)
            .map(|(idx, _)| idx as u8)
            .unwrap_or(0)
    }

    /// Extract the outcome for a specific market from a state index.
    fn extract_outcome_from_state(state_idx: usize, market_idx: usize, market_sizes: &[u8]) -> u8 {
        let mut remaining = state_idx;
        for (i, &size) in market_sizes.iter().enumerate() {
            let outcome = (remaining % size as usize) as u8;
            if i == market_idx {
                return outcome;
            }
            remaining /= size as usize;
        }
        0
    }

    /// Check if this footprint overlaps with another on any market/outcome.
    #[cfg(test)]
    pub fn overlaps_with(&self, other: &FillFootprint) -> bool {
        for key in self.liquidity_consumed.keys() {
            if other.liquidity_consumed.contains_key(key) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conflict_graph_basic() {
        let mut graph = ConflictGraph::new(5);

        graph.add_edge(0, 1);
        graph.add_edge(0, 2);
        graph.add_edge(1, 2);

        assert_eq!(graph.num_nodes(), 5);
        assert_eq!(graph.num_edges(), 3);
        assert!(graph.are_adjacent(0, 1));
        assert!(graph.are_adjacent(1, 0));
        assert!(!graph.are_adjacent(0, 3));
        assert_eq!(graph.degree(0), 2);
        assert_eq!(graph.degree(3), 0);
    }

    #[test]
    fn test_conflict_graph_isolated() {
        let mut graph = ConflictGraph::new(5);

        graph.add_edge(0, 1);

        let isolated = graph.isolated_nodes();
        assert_eq!(isolated.len(), 3);
        assert!(isolated.contains(&2));
        assert!(isolated.contains(&3));
        assert!(isolated.contains(&4));
    }

    #[test]
    fn test_fill_footprint_overlap() {
        let market = MarketId::new(1);

        let mut footprint_a = FillFootprint {
            liquidity_consumed: HashMap::new(),
        };
        footprint_a.liquidity_consumed.insert((market, 0), 100);

        let mut footprint_b = FillFootprint {
            liquidity_consumed: HashMap::new(),
        };
        footprint_b.liquidity_consumed.insert((market, 0), 50);

        let mut footprint_c = FillFootprint {
            liquidity_consumed: HashMap::new(),
        };
        footprint_c.liquidity_consumed.insert((market, 1), 100);

        assert!(footprint_a.overlaps_with(&footprint_b));
        assert!(!footprint_a.overlaps_with(&footprint_c));
    }
}
