//! Chain finder solver for implication constraint arbitrage.
//!
//! Finds arbitrage chains through implication constraints.
//!
//! Example: Champion → Finalist → Semifinalist → Participant
//! Mispricing: Champion=$0.20, Participant=$0.70
//!
//! ChainFinder realizes: buying Champion gives exposure to ALL
//! levels for $0.20 instead of $0.70!

use std::collections::{HashMap, HashSet};

use matching_engine::{
    ConstraintSet, Fill, LiquidityPool, MarketConstraint, MarketId, Nanos, Order, Problem,
};

use crate::{MatchingResult, Solver};

/// A chain of implications with potential arbitrage.
#[derive(Clone, Debug)]
pub struct ImplicationChain {
    /// Markets in the chain, from root to leaf
    pub chain: Vec<(MarketId, u8)>,
    /// Price at the start (root) of the chain
    pub root_price: Nanos,
    /// Price at the end (leaf) of the chain
    pub leaf_price: Nanos,
    /// Price advantage (leaf - root)
    pub advantage: i64,
}

/// Finds arbitrage chains through implication constraints.
///
/// Algorithm:
/// 1. Build implication graph from constraints
/// 2. DFS to find chains
/// 3. Score by price advantage (last - first)
/// 4. Prioritize orders buying early in chains
pub struct ChainFinder {
    /// Minimum price advantage to consider (in nanos)
    min_advantage: Nanos,
    /// Maximum chain length to explore
    max_chain_length: usize,
}

impl ChainFinder {
    /// Create a new chain finder with default settings.
    pub fn new() -> Self {
        Self {
            min_advantage: 10_000_000, // 0.01 dollars
            max_chain_length: 10,
        }
    }

    /// Set minimum price advantage.
    pub fn with_min_advantage(mut self, advantage: Nanos) -> Self {
        self.min_advantage = advantage;
        self
    }

    /// Set maximum chain length.
    pub fn with_max_chain_length(mut self, length: usize) -> Self {
        self.max_chain_length = length;
        self
    }

    /// Build implication graph from constraints.
    ///
    /// Returns a map: (market, outcome) -> vec of (implied_market, implied_outcome)
    fn build_implication_graph(
        &self,
        constraints: &ConstraintSet,
    ) -> HashMap<(MarketId, u8), Vec<(MarketId, u8)>> {
        let mut graph: HashMap<(MarketId, u8), Vec<(MarketId, u8)>> = HashMap::new();

        for constraint in constraints.iter() {
            match constraint {
                MarketConstraint::Implication { if_true, then_true } => {
                    graph.entry(*if_true).or_default().push(*then_true);
                }
                MarketConstraint::Hierarchy { parent, child } => {
                    // Hierarchy is like implication: parent → child
                    graph.entry(*parent).or_default().push(*child);
                }
                _ => {}
            }
        }

        graph
    }

    /// Find all implication chains with price advantage.
    pub fn find_chains(&self, problem: &Problem) -> Vec<ImplicationChain> {
        let graph = self.build_implication_graph(&problem.constraints);
        let mut chains = Vec::new();
        let mut visited = HashSet::new();

        // Find all roots (nodes with no incoming edges from our perspective)
        let all_nodes: HashSet<(MarketId, u8)> = graph.keys().cloned().collect();
        let children: HashSet<(MarketId, u8)> = graph
            .values()
            .flat_map(|v| v.iter().cloned())
            .collect();

        // Potential roots are nodes that appear as keys but not (or rarely) as children
        let roots: Vec<(MarketId, u8)> = all_nodes
            .iter()
            .filter(|n| !children.contains(n))
            .cloned()
            .collect();

        // If no clear roots, use all nodes as starting points
        let starting_points = if roots.is_empty() {
            all_nodes.iter().cloned().collect()
        } else {
            roots
        };

        // DFS from each root
        for root in starting_points {
            visited.clear();
            let mut current_chain = vec![root];
            self.dfs_chains(
                &graph,
                root,
                &mut current_chain,
                &mut visited,
                &mut chains,
                &problem.liquidity,
            );
        }

        // Sort by advantage (highest first)
        chains.sort_by(|a, b| b.advantage.cmp(&a.advantage));

        chains
    }

    /// DFS to find chains with price advantage.
    fn dfs_chains(
        &self,
        graph: &HashMap<(MarketId, u8), Vec<(MarketId, u8)>>,
        current: (MarketId, u8),
        current_chain: &mut Vec<(MarketId, u8)>,
        visited: &mut HashSet<(MarketId, u8)>,
        chains: &mut Vec<ImplicationChain>,
        liquidity: &LiquidityPool,
    ) {
        if visited.contains(&current) {
            return;
        }

        if current_chain.len() > self.max_chain_length {
            return;
        }

        visited.insert(current);

        // Check if current chain has price advantage
        if current_chain.len() >= 2 {
            let root = current_chain[0];
            let leaf = current_chain[current_chain.len() - 1];

            let root_price = self.get_best_ask(liquidity, root.0, root.1);
            let leaf_price = self.get_best_ask(liquidity, leaf.0, leaf.1);

            if let (Some(rp), Some(lp)) = (root_price, leaf_price) {
                let advantage = lp as i64 - rp as i64;

                if advantage > self.min_advantage as i64 {
                    chains.push(ImplicationChain {
                        chain: current_chain.clone(),
                        root_price: rp,
                        leaf_price: lp,
                        advantage,
                    });
                }
            }
        }

        // Continue DFS
        if let Some(children) = graph.get(&current) {
            for &child in children {
                if !visited.contains(&child) {
                    current_chain.push(child);
                    self.dfs_chains(graph, child, current_chain, visited, chains, liquidity);
                    current_chain.pop();
                }
            }
        }

        visited.remove(&current);
    }

    /// Get the best ask price for a (market, outcome).
    fn get_best_ask(&self, liquidity: &LiquidityPool, market: MarketId, outcome: u8) -> Option<Nanos> {
        liquidity.book(market, outcome).and_then(|b| b.best_ask())
    }

    /// Prioritize orders that buy at the start of advantage chains.
    fn prioritize_chain_orders(
        &self,
        chains: &[ImplicationChain],
        problem: &Problem,
    ) -> Vec<(usize, i64)> {
        // Map (market, outcome) -> best chain advantage
        let mut advantage_map: HashMap<(MarketId, u8), i64> = HashMap::new();

        for chain in chains {
            if chain.chain.is_empty() {
                continue;
            }

            let root = chain.chain[0];
            let current = advantage_map.entry(root).or_insert(0);
            if chain.advantage > *current {
                *current = chain.advantage;
            }
        }

        // Score each order by whether it buys at a chain root
        let mut order_scores: Vec<(usize, i64)> = Vec::new();

        for (idx, order) in problem.orders.iter().enumerate() {
            if order.num_markets != 1 {
                continue;
            }

            let market = order.markets[0];
            let outcome = self.determine_buying_outcome(order);

            if let Some(&advantage) = advantage_map.get(&(market, outcome)) {
                order_scores.push((idx, advantage));
            }
        }

        // Sort by advantage (highest first)
        order_scores.sort_by(|a, b| b.1.cmp(&a.1));

        order_scores
    }

    /// Fill orders prioritized by chain advantage.
    fn fill_prioritized_orders(
        &self,
        prioritized: &[(usize, i64)],
        problem: &Problem,
        result: &mut MatchingResult,
    ) {
        let mut filled_orders: HashSet<u64> = HashSet::new();

        for &(idx, _advantage) in prioritized.iter() {
            let order = &problem.orders[idx];

            if filled_orders.contains(&order.id) {
                continue;
            }

            if let Some(fill) = self.try_fill_order(order, &mut result.remaining_liquidity) {
                if fill.welfare(order) > 0 {
                    result.add_fill(fill, order);
                    filled_orders.insert(order.id);
                }
            }
        }
    }

    /// Try to fill an order.
    fn try_fill_order(&self, order: &Order, liquidity: &mut LiquidityPool) -> Option<Fill> {
        if order.num_markets != 1 {
            return None;
        }

        let market = order.markets[0];
        let outcome = self.determine_buying_outcome(order);

        if let Some(book) = liquidity.books.get_mut(&(market, outcome)) {
            let (filled, price) = book.consume_asks(order.max_fill, order.limit_price);
            if filled >= order.min_fill && filled > 0 {
                return Some(Fill::new(order.id, filled, price));
            }
        }

        None
    }

    /// Determine which outcome is being bought.
    fn determine_buying_outcome(&self, order: &Order) -> u8 {
        let mut best_outcome = 0u8;
        let mut best_payoff = i8::MIN;

        for (i, &payoff) in order.payoffs.iter().take(order.num_states as usize).enumerate() {
            if payoff > best_payoff {
                best_payoff = payoff;
                best_outcome = i as u8;
            }
        }

        best_outcome
    }
}

impl Default for ChainFinder {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for ChainFinder {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        let mut result = MatchingResult::new(problem.liquidity.snapshot());

        // Find implication chains with price advantage
        let chains = self.find_chains(problem);

        // Prioritize orders at chain roots
        let prioritized = self.prioritize_chain_orders(&chains, problem);

        // Fill prioritized orders
        self.fill_prioritized_orders(&prioritized, problem, &mut result);

        result
    }

    fn name(&self) -> &str {
        "ChainFinder"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{ConstraintBuilder, outcome_buy};

    #[test]
    fn test_build_implication_graph() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("champion");
        let m2 = problem.markets.add_binary("finalist");
        let m3 = problem.markets.add_binary("semifinalist");

        // Champion → Finalist → Semifinalist
        problem.constraints = ConstraintBuilder::new()
            .implies(m1, 0, m2, 0)
            .implies(m2, 0, m3, 0)
            .build();

        let finder = ChainFinder::new();
        let graph = finder.build_implication_graph(&problem.constraints);

        assert!(graph.contains_key(&(m1, 0)));
        assert_eq!(graph.get(&(m1, 0)).unwrap(), &vec![(m2, 0)]);
        assert!(graph.contains_key(&(m2, 0)));
        assert_eq!(graph.get(&(m2, 0)).unwrap(), &vec![(m3, 0)]);
    }

    #[test]
    fn test_find_chains_with_advantage() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("champion");
        let m2 = problem.markets.add_binary("finalist");
        let m3 = problem.markets.add_binary("semifinalist");

        problem.constraints = ConstraintBuilder::new()
            .implies(m1, 0, m2, 0)
            .implies(m2, 0, m3, 0)
            .build();

        // Mispriced liquidity: champion cheap, semifinalist expensive
        problem.liquidity.add_ask(m1, 0, 200_000_000, 1000); // $0.20
        problem.liquidity.add_ask(m2, 0, 400_000_000, 1000); // $0.40
        problem.liquidity.add_ask(m3, 0, 700_000_000, 1000); // $0.70

        let finder = ChainFinder::new();
        let chains = finder.find_chains(&problem);

        // Should find chain with advantage
        assert!(!chains.is_empty());
        let best = &chains[0];
        assert!(best.advantage > 0);
        assert!(best.chain.contains(&(m1, 0))); // Root
        assert!(best.chain.contains(&(m3, 0))); // Leaf
    }

    #[test]
    fn test_prioritize_orders() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("champion");
        let m2 = problem.markets.add_binary("finalist");

        problem.constraints = ConstraintBuilder::new()
            .implies(m1, 0, m2, 0)
            .build();

        // Mispriced liquidity
        problem.liquidity.add_ask(m1, 0, 200_000_000, 1000);
        problem.liquidity.add_ask(m2, 0, 500_000_000, 1000);

        // Order on champion (should be prioritized)
        problem.orders.push(outcome_buy(&problem.markets, 1, m1, 0, 300_000_000, 100));
        // Order on finalist (not at chain root)
        problem.orders.push(outcome_buy(&problem.markets, 2, m2, 0, 600_000_000, 100));

        let finder = ChainFinder::new();
        let chains = finder.find_chains(&problem);
        let prioritized = finder.prioritize_chain_orders(&chains, &problem);

        // Champion order should be prioritized
        assert!(!prioritized.is_empty());
        assert_eq!(prioritized[0].0, 0); // First order (champion)
    }

    #[test]
    fn test_solver_integration() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("champion");
        let m2 = problem.markets.add_binary("finalist");

        problem.constraints = ConstraintBuilder::new()
            .implies(m1, 0, m2, 0)
            .build();

        problem.liquidity.add_ask(m1, 0, 200_000_000, 1000);
        problem.liquidity.add_ask(m2, 0, 500_000_000, 1000);

        problem.orders.push(outcome_buy(&problem.markets, 1, m1, 0, 300_000_000, 100));

        let finder = ChainFinder::new();
        let result = finder.solve(&problem);

        // Should fill the champion order
        assert!(result.orders_filled > 0);
    }
}
