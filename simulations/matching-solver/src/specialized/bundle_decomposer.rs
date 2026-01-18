//! Bundle decomposition solver.
//!
//! Finds complementary bundle sets - bundles that together cover all outcomes.
//!
//! Example: 4 bundles on 2 binary markets
//! - Bundle 1: YES/YES (state 0) at $0.28
//! - Bundle 2: YES/NO  (state 1) at $0.26
//! - Bundle 3: NO/YES  (state 2) at $0.25
//! - Bundle 4: NO/NO   (state 3) at $0.24
//!
//! Total: $1.03 for guaranteed $1.00 payout
//!
//! Together = guaranteed profit if filled!

use std::collections::HashMap;

use matching_engine::{Fill, LiquidityPool, MarketId, Nanos, Order, Problem, Qty};

use crate::{MatchingResult, Solver};

/// A set of bundles that together form a complete covering.
#[derive(Clone, Debug)]
pub struct ComplementSet {
    /// Indices of orders that form this complement set
    pub order_indices: Vec<usize>,
    /// Markets covered by this set
    pub markets: Vec<MarketId>,
    /// Combined payoff vector (should be uniform for perfect complement)
    pub combined_payoffs: Vec<i8>,
    /// Total limit price of all bundles
    pub total_limit: Nanos,
    /// Complementarity score (1.0 = perfect, uniform payoff)
    pub score: f64,
}

/// Finds complementary bundle sets that together cover all outcomes.
pub struct BundleDecomposer {
    /// Minimum complementarity score to consider a set
    min_score: f64,
    /// Maximum orders to consider per market combination
    max_orders_per_combo: usize,
}

impl BundleDecomposer {
    /// Create a new bundle decomposer with default settings.
    pub fn new() -> Self {
        Self {
            min_score: 0.7,
            max_orders_per_combo: 100,
        }
    }

    /// Set minimum complementarity score.
    pub fn with_min_score(mut self, score: f64) -> Self {
        self.min_score = score;
        self
    }

    /// Find all complement sets in the problem.
    pub fn find_complement_sets(&self, problem: &Problem) -> Vec<ComplementSet> {
        let mut sets = Vec::new();

        // Group orders by their market sets
        let grouped = self.group_by_markets(&problem.orders);

        // For each market combination, look for complementary bundles
        for (market_key, order_indices) in grouped {
            if order_indices.len() < 2 {
                continue;
            }

            // Parse market key back to MarketIds
            let markets: Vec<MarketId> = market_key
                .split(',')
                .filter_map(|s| s.parse::<u32>().ok())
                .map(MarketId::new)
                .collect();

            if markets.is_empty() {
                continue;
            }

            // Find complement sets within this group
            let group_sets = self.find_complements_in_group(
                &problem.orders,
                &order_indices,
                &markets,
            );
            sets.extend(group_sets);
        }

        // Sort by score (highest first)
        sets.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        sets
    }

    /// Group orders by their market combinations.
    fn group_by_markets(&self, orders: &[Order]) -> HashMap<String, Vec<usize>> {
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();

        for (idx, order) in orders.iter().enumerate() {
            if order.num_markets < 1 {
                continue;
            }

            // Create a key from sorted market IDs
            let mut market_ids: Vec<u32> = order
                .active_markets()
                .map(|m| m.0)
                .collect();
            market_ids.sort();

            let key = market_ids
                .iter()
                .map(|m: &u32| m.to_string())
                .collect::<Vec<_>>()
                .join(",");

            groups.entry(key).or_default().push(idx);
        }

        groups
    }

    /// Find complement sets within a group of orders on the same markets.
    fn find_complements_in_group(
        &self,
        orders: &[Order],
        indices: &[usize],
        markets: &[MarketId],
    ) -> Vec<ComplementSet> {
        let mut sets = Vec::new();

        // For small groups, check all subsets
        // For larger groups, use heuristics
        let indices_to_check: Vec<usize> = indices
            .iter()
            .take(self.max_orders_per_combo)
            .copied()
            .collect();

        // Calculate number of states for these markets
        let num_states = 1usize << markets.len().min(5);

        // Build payoff table for each order
        let mut payoff_table: Vec<(usize, Vec<i8>)> = Vec::new();
        for &idx in &indices_to_check {
            let order = &orders[idx];
            let payoffs: Vec<i8> = order.payoffs
                .iter()
                .take(num_states)
                .copied()
                .collect();
            payoff_table.push((idx, payoffs));
        }

        // Find subsets that sum to uniform payoff
        // For efficiency, focus on finding pairs and small groups
        for subset_size in 2..=num_states.min(8) {
            if payoff_table.len() < subset_size {
                break;
            }

            let subset_sets = self.find_subsets_with_uniform_sum(
                &payoff_table,
                orders,
                markets,
                subset_size,
                num_states,
            );
            sets.extend(subset_sets);

            // Stop early if we found good sets
            if sets.len() > 10 {
                break;
            }
        }

        sets
    }

    /// Find subsets of given size that sum to a uniform payoff.
    fn find_subsets_with_uniform_sum(
        &self,
        payoff_table: &[(usize, Vec<i8>)],
        orders: &[Order],
        markets: &[MarketId],
        subset_size: usize,
        num_states: usize,
    ) -> Vec<ComplementSet> {
        let mut sets = Vec::new();

        // For small tables, enumerate subsets
        if payoff_table.len() <= 20 && subset_size <= 4 {
            let subsets = self.enumerate_subsets(payoff_table.len(), subset_size);

            for subset in subsets {
                let indices: Vec<usize> = subset.iter().map(|&i| payoff_table[i].0).collect();

                // Sum payoffs
                let mut combined = vec![0i32; num_states];
                for &i in &subset {
                    for (j, &p) in payoff_table[i].1.iter().enumerate() {
                        combined[j] += p as i32;
                    }
                }

                // Check uniformity
                let score = self.compute_uniformity_score(&combined);
                if score >= self.min_score {
                    // Calculate total limit
                    let total_limit: Nanos = indices
                        .iter()
                        .map(|&i| orders[i].limit_price)
                        .sum();

                    sets.push(ComplementSet {
                        order_indices: indices,
                        markets: markets.to_vec(),
                        combined_payoffs: combined.iter().map(|&p| p as i8).collect(),
                        total_limit,
                        score,
                    });
                }
            }
        }

        sets
    }

    /// Enumerate all subsets of given size.
    fn enumerate_subsets(&self, n: usize, k: usize) -> Vec<Vec<usize>> {
        let mut subsets = Vec::new();
        let mut current = vec![0; k];

        // Initialize first subset
        for (i, slot) in current.iter_mut().enumerate().take(k) {
            *slot = i;
        }

        loop {
            subsets.push(current.clone());

            // Find rightmost element that can be incremented
            let mut i = k;
            while i > 0 {
                i -= 1;
                if current[i] < n - k + i {
                    break;
                }
            }

            if i == 0 && current[0] >= n - k {
                break;
            }

            current[i] += 1;
            for j in (i + 1)..k {
                current[j] = current[j - 1] + 1;
            }
        }

        subsets
    }

    /// Compute how uniform a combined payoff vector is.
    /// Returns 1.0 for perfectly uniform, lower for non-uniform.
    fn compute_uniformity_score(&self, payoffs: &[i32]) -> f64 {
        if payoffs.is_empty() {
            return 0.0;
        }

        let min = *payoffs.iter().min().unwrap_or(&0);
        let max = *payoffs.iter().max().unwrap_or(&0);

        if max == 0 {
            return 0.0;
        }

        if min == max {
            return 1.0; // Perfectly uniform
        }

        // Score based on how close min is to max
        let range = (max - min) as f64;
        let avg = payoffs.iter().sum::<i32>() as f64 / payoffs.len() as f64;

        if avg == 0.0 {
            return 0.0;
        }

        1.0 - (range / avg.abs()).min(1.0)
    }

    /// Fill orders from complement sets, prioritizing high-score sets.
    fn fill_complement_sets(
        &self,
        sets: &[ComplementSet],
        problem: &Problem,
        result: &mut MatchingResult,
    ) {
        let mut filled_orders: std::collections::HashSet<u64> = std::collections::HashSet::new();

        for set in sets.iter().take(20) {
            // Check if all orders in set can still be filled
            let mut can_fill_all = true;
            let mut min_qty = Qty::MAX;

            for &idx in &set.order_indices {
                let order = &problem.orders[idx];
                if filled_orders.contains(&order.id) {
                    can_fill_all = false;
                    break;
                }

                // Check liquidity for this order
                let avail = self.check_order_liquidity(order, &result.remaining_liquidity);
                if avail < order.min_fill {
                    can_fill_all = false;
                    break;
                }
                min_qty = min_qty.min(avail).min(order.max_fill);
            }

            if !can_fill_all || min_qty == 0 {
                continue;
            }

            // Fill all orders in the set together
            for &idx in &set.order_indices {
                let order = &problem.orders[idx];
                let fill_qty = min_qty.min(order.max_fill);

                if let Some(fill) = self.try_fill_order(order, fill_qty, &mut result.remaining_liquidity) {
                    result.add_fill(fill, order);
                    filled_orders.insert(order.id);
                }
            }
        }
    }

    /// Check available liquidity for an order.
    fn check_order_liquidity(&self, order: &Order, liquidity: &LiquidityPool) -> Qty {
        if order.num_markets == 1 {
            let market = order.markets[0];
            let outcome = self.determine_buying_outcome(order);

            if let Some(book) = liquidity.book(market, outcome) {
                let (avail, _) = book.available_to_buy(order.limit_price);
                return avail;
            }
        } else {
            // Bundle order - check all legs
            let mut min_avail = Qty::MAX;
            for market_idx in 0..order.num_markets as usize {
                let market = order.markets[market_idx];
                if market.is_none() {
                    continue;
                }

                let outcome = self.determine_bundle_outcome(order, market_idx);

                if let Some(book) = liquidity.book(market, outcome) {
                    let (avail, _) = book.available_to_buy(order.limit_price);
                    min_avail = min_avail.min(avail);
                } else {
                    return 0;
                }
            }
            return min_avail;
        }

        0
    }

    /// Try to fill an order and consume liquidity.
    fn try_fill_order(
        &self,
        order: &Order,
        qty: Qty,
        liquidity: &mut LiquidityPool,
    ) -> Option<Fill> {
        if order.num_markets == 1 {
            let market = order.markets[0];
            let outcome = self.determine_buying_outcome(order);

            if let Some(book) = liquidity.books.get_mut(&(market, outcome)) {
                let (filled, price) = book.consume_asks(qty, order.limit_price);
                if filled >= order.min_fill && filled > 0 {
                    return Some(Fill::new(order.id, filled, price));
                }
            }
        } else {
            // Bundle order
            let mut total_cost: u128 = 0;
            let mut filled_qty = qty;
            let mut legs = 0;

            // First pass: verify and consume
            for market_idx in 0..order.num_markets as usize {
                let market = order.markets[market_idx];
                if market.is_none() {
                    continue;
                }

                let outcome = self.determine_bundle_outcome(order, market_idx);

                if let Some(book) = liquidity.books.get_mut(&(market, outcome)) {
                    let (filled, price) = book.consume_asks(qty, order.limit_price);
                    filled_qty = filled_qty.min(filled);
                    total_cost += price as u128 * filled as u128;
                    legs += 1;
                }
            }

            if filled_qty >= order.min_fill && filled_qty > 0 && legs > 0 {
                let avg_price = (total_cost / (filled_qty as u128 * legs as u128)) as Nanos;
                return Some(Fill::new(order.id, filled_qty, avg_price));
            }
        }

        None
    }

    /// Determine which outcome is being bought for a simple order.
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

    /// Determine which outcome to buy for a specific market in a bundle.
    fn determine_bundle_outcome(&self, order: &Order, market_idx: usize) -> u8 {
        let num_markets = order.num_markets as usize;
        if market_idx >= num_markets {
            return 0;
        }

        let market_sizes: Vec<u8> = vec![2; num_markets];
        let mut outcome_votes: [i32; 4] = [0; 4];

        for state_idx in 0..order.num_states as usize {
            let payoff = order.payoffs[state_idx];
            if payoff > 0 {
                let outcome = self.extract_outcome(state_idx, market_idx, &market_sizes);
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
    fn extract_outcome(&self, state_idx: usize, market_idx: usize, market_sizes: &[u8]) -> u8 {
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
}

impl Default for BundleDecomposer {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for BundleDecomposer {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        let mut result = MatchingResult::new(problem.liquidity.snapshot());

        // Find complement sets
        let sets = self.find_complement_sets(problem);

        // Fill orders from complement sets
        self.fill_complement_sets(&sets, problem, &mut result);

        result
    }

    fn name(&self) -> &str {
        "BundleDecomposer"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{bundle_yes, outcome_buy};

    #[test]
    fn test_uniformity_score() {
        let decomposer = BundleDecomposer::new();

        // Perfect uniformity
        assert!((decomposer.compute_uniformity_score(&[1, 1, 1, 1]) - 1.0).abs() < 0.01);

        // Zero uniformity
        assert!(decomposer.compute_uniformity_score(&[0, 0, 0, 0]) == 0.0);

        // Partial uniformity
        let score = decomposer.compute_uniformity_score(&[1, 1, 2, 2]);
        assert!(score > 0.0 && score < 1.0);
    }

    #[test]
    fn test_enumerate_subsets() {
        let decomposer = BundleDecomposer::new();

        let subsets = decomposer.enumerate_subsets(4, 2);
        assert_eq!(subsets.len(), 6); // C(4,2) = 6

        let subsets = decomposer.enumerate_subsets(5, 3);
        assert_eq!(subsets.len(), 10); // C(5,3) = 10
    }

    #[test]
    fn test_group_by_markets() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("market_1");
        let m2 = problem.markets.add_binary("market_2");

        // Two orders on same markets
        problem.orders.push(outcome_buy(&problem.markets, 1, m1, 0, 500_000_000, 100));
        problem.orders.push(outcome_buy(&problem.markets, 2, m1, 0, 500_000_000, 100));
        // One on different market
        problem.orders.push(outcome_buy(&problem.markets, 3, m2, 0, 500_000_000, 100));

        let decomposer = BundleDecomposer::new();
        let groups = decomposer.group_by_markets(&problem.orders);

        assert_eq!(groups.len(), 2);
    }
}
