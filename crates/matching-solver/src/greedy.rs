//! Greedy solver for the matching problem.
//!
//! Processes orders in decreasing order of welfare potential (limit_price * max_fill).
//! This is a reasonable heuristic but will fail to find optimal solutions on hard instances.

use matching_engine::{Fill, JointOutcome, LiquidityPool, Order, Problem};

use crate::{MatchingResult, Solver};

/// Greedy solver that processes orders by welfare potential.
pub struct GreedySolver {
    /// Whether to randomize order of equal-welfare orders
    pub randomize_ties: bool,
    /// If true, process orders in input order without sorting by welfare
    pub preserve_order: bool,
}

impl GreedySolver {
    pub fn new() -> Self {
        Self {
            randomize_ties: false,
            preserve_order: false,
        }
    }

    /// Create a greedy solver that processes orders in input order (no sorting)
    pub fn preserve_order() -> Self {
        Self {
            randomize_ties: false,
            preserve_order: true,
        }
    }

    pub fn with_randomize(mut self, randomize: bool) -> Self {
        self.randomize_ties = randomize;
        self
    }

    /// Sort orders by welfare potential (descending).
    fn sort_by_welfare(orders: &[Order]) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..orders.len()).collect();
        indices.sort_by(|&a, &b| {
            let welfare_a = orders[a].limit_price as u128 * orders[a].max_fill as u128;
            let welfare_b = orders[b].limit_price as u128 * orders[b].max_fill as u128;
            welfare_b.cmp(&welfare_a)
        });
        indices
    }

    /// Try to fill a single order against available liquidity.
    /// Public static version for use by other solvers.
    pub fn try_fill_order_static(order: &Order, liquidity: &mut LiquidityPool) -> Option<Fill> {
        Self::try_fill_order(order, liquidity)
    }

    /// Try to fill a single order against available liquidity.
    fn try_fill_order(order: &Order, liquidity: &mut LiquidityPool) -> Option<Fill> {
        if order.num_markets == 0 {
            return None;
        }

        if order.num_markets == 1 {
            return Self::try_fill_simple_order(order, liquidity);
        }

        // Multi-market orders (bundles, spreads)
        Self::try_fill_bundle_order(order, liquidity)
    }

    /// Fill a simple single-market order.
    fn try_fill_simple_order(order: &Order, liquidity: &mut LiquidityPool) -> Option<Fill> {
        let market = order.markets[0];

        // Determine which outcome we're buying based on payoffs
        let buying_outcome = Self::determine_buying_outcome(order);

        if let Some(book) = liquidity.books.get_mut(&(market, buying_outcome)) {
            // Try to consume from asks (we're buying)
            let (filled_qty, avg_price) = book.consume_asks(order.max_fill, order.limit_price);

            if filled_qty >= order.min_fill && filled_qty > 0 {
                return Some(Fill::new(order.id, filled_qty, avg_price));
            } else if order.is_all_or_none() && filled_qty < order.min_fill {
                return None;
            }
        }

        None
    }

    /// Determine which outcome the order is buying based on payoffs.
    fn determine_buying_outcome(order: &Order) -> u8 {
        let mut best_outcome = 0u8;
        let mut best_payoff = i8::MIN;

        for (i, &payoff) in order
            .payoffs
            .iter()
            .take(order.num_states as usize)
            .enumerate()
        {
            if payoff > best_payoff {
                best_payoff = payoff;
                best_outcome = i as u8;
            }
        }

        best_outcome
    }

    /// Fill a bundle order using joint liquidity only.
    ///
    /// Bundle orders can ONLY be filled from joint liquidity books.
    /// Leg liquidity has different payoff structure and cannot replicate bundles.
    fn try_fill_bundle_order(order: &Order, liquidity: &mut LiquidityPool) -> Option<Fill> {
        // Build the joint outcome for this bundle
        let joint_outcome = Self::build_joint_outcome(order)?;

        // Try to fill from joint liquidity - the ONLY valid source for bundles
        if let Some(joint_book) = liquidity.joint_book_get_mut(&joint_outcome) {
            let (avail, best_price) = joint_book.available_to_buy(order.limit_price);
            if avail >= order.min_fill && best_price <= order.limit_price {
                let fill_qty = avail.min(order.max_fill);
                joint_book.consume_asks(fill_qty, order.limit_price);
                return Some(Fill::new(order.id, fill_qty, best_price));
            }
        }

        // No joint liquidity available - cannot fill this bundle
        None
    }

    /// Build a JointOutcome from a bundle order.
    fn build_joint_outcome(order: &Order) -> Option<JointOutcome> {
        if order.num_markets <= 1 {
            return None;
        }

        let mut legs = Vec::new();
        for market_idx in 0..order.num_markets as usize {
            let market = order.markets[market_idx];
            if market.is_none() {
                continue;
            }

            let outcome = Self::determine_bundle_outcome(order, market_idx);
            legs.push((market, outcome));
        }

        if legs.len() >= 2 {
            Some(JointOutcome::new(legs))
        } else {
            None
        }
    }

    /// Determine which outcome to buy for a specific market in a bundle.
    ///
    /// For bundle orders, we analyze the payoff vector to find which outcome
    /// is being bought for a specific market. The payoff vector encodes payoffs
    /// for each atomic state (Cartesian product of outcomes). We look for states
    /// with positive payoffs and determine the common outcome for that market.
    fn determine_bundle_outcome(order: &Order, market_idx: usize) -> u8 {
        // Get the market sizes for state indexing
        // For binary markets, size is 2
        let num_markets = order.num_markets as usize;
        if market_idx >= num_markets {
            return 0;
        }

        // Assume binary markets (size 2) for all markets in the bundle
        // This is a simplification - in a more general case, we'd need market metadata
        let market_sizes: Vec<u8> = vec![2; num_markets];

        // Find states where the payoff is positive
        let mut outcome_votes: [i32; 4] = [0; 4]; // Support up to 4 outcomes per market

        for state_idx in 0..order.num_states as usize {
            let payoff = order.payoffs[state_idx];
            if payoff > 0 {
                // Decode this state to find the outcome for market_idx
                let outcome = Self::extract_outcome(state_idx, market_idx, &market_sizes);
                if (outcome as usize) < outcome_votes.len() {
                    outcome_votes[outcome as usize] += payoff as i32;
                }
            }
        }

        // Return the outcome with the highest positive votes
        outcome_votes
            .iter()
            .enumerate()
            .max_by_key(|(_, &v)| v)
            .map(|(idx, _)| idx as u8)
            .unwrap_or(0)
    }

    /// Extract the outcome for a specific market from a state index.
    fn extract_outcome(state_idx: usize, market_idx: usize, market_sizes: &[u8]) -> u8 {
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

impl Default for GreedySolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for GreedySolver {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        let mut liquidity = problem.liquidity.snapshot();
        let mut result = MatchingResult::new(liquidity.clone());

        // Either sort by welfare or preserve input order
        let order_indices: Vec<usize> = if self.preserve_order {
            (0..problem.orders.len()).collect()
        } else {
            Self::sort_by_welfare(&problem.orders)
        };

        for &idx in &order_indices {
            let order = &problem.orders[idx];

            if order.is_conditional() {
                continue;
            }

            match Self::try_fill_order(order, &mut liquidity) {
                Some(fill) => {
                    result.add_fill(fill, order);
                }
                None => {
                    if order.is_all_or_none() {
                        result.orders_unfilled_aon += 1;
                    } else {
                        result.orders_unfilled_liquidity += 1;
                    }
                }
            }
        }

        result.remaining_liquidity = liquidity;
        result
    }

    fn name(&self) -> &str {
        "Greedy"
    }
}

// ============================================================================
// PartialSolver Trait Implementation
// ============================================================================

use crate::combiner::SolutionConfidence;
use crate::traits::{PartialSolution, PartialSolver};

impl PartialSolver for GreedySolver {
    fn solve_partial(&self, problem: &Problem) -> PartialSolution {
        let result = Solver::solve(self, problem);
        PartialSolution::with_fills(
            "Greedy",
            result.fills,
            result.total_welfare,
            SolutionConfidence::Heuristic,
        )
    }

    fn name(&self) -> &str {
        "Greedy"
    }

    fn confidence(&self) -> SolutionConfidence {
        SolutionConfidence::Heuristic
    }
}
