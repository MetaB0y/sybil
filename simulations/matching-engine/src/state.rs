//! State indexing and payoff evaluation.
//!
//! For N markets with varying numbers of outcomes, we define atomic states
//! as the Cartesian product of all outcomes. State indices use a mixed-radix
//! encoding.
//!
//! # Indexing Convention
//!
//! For N binary markets, state index uses binary encoding:
//! ```text
//! state_index = m0_outcome + 2*m1_outcome + 4*m2_outcome + ...
//! ```
//!
//! Example for 2 binary markets (states 0-3):
//! - 0: (m0=0, m1=0) - both outcome 0
//! - 1: (m0=1, m1=0) - market 0 outcome 1, market 1 outcome 0
//! - 2: (m0=0, m1=1) - market 0 outcome 0, market 1 outcome 1
//! - 3: (m0=1, m1=1) - both outcome 1
//!
//! For multi-outcome markets, use mixed-radix where each digit position
//! corresponds to a market with its own base (number of outcomes).

/// Convert outcome indices to a state index.
///
/// # Arguments
/// * `outcomes` - Outcome index for each market (0-indexed)
/// * `market_sizes` - Number of outcomes in each market
///
/// # Example
/// ```
/// use matching_engine::state::state_index;
/// // Two binary markets: outcome 0 for first, outcome 1 for second
/// let idx = state_index(&[0, 1], &[2, 2]);
/// assert_eq!(idx, 2); // 0 + 1*2 = 2
/// ```
pub fn state_index(outcomes: &[u8], market_sizes: &[u8]) -> usize {
    let mut idx = 0;
    let mut multiplier = 1;
    for (i, &outcome) in outcomes.iter().enumerate() {
        idx += outcome as usize * multiplier;
        if i < market_sizes.len() {
            multiplier *= market_sizes[i] as usize;
        }
    }
    idx
}

/// Convert a state index back to outcome indices.
///
/// # Arguments
/// * `state_idx` - The state index to decode
/// * `market_sizes` - Number of outcomes in each market
///
/// # Returns
/// Vector of outcome indices for each market
pub fn state_to_outcomes(state_idx: usize, market_sizes: &[u8]) -> Vec<u8> {
    let mut outcomes = Vec::with_capacity(market_sizes.len());
    let mut remaining = state_idx;

    for &size in market_sizes {
        outcomes.push((remaining % size as usize) as u8);
        remaining /= size as usize;
    }

    outcomes
}

/// Calculate the total number of states for given market sizes.
pub fn total_states(market_sizes: &[u8]) -> usize {
    market_sizes.iter().map(|&s| s as usize).product()
}

/// Represents the state space for a set of markets.
#[derive(Clone, Debug)]
pub struct StateSpace {
    market_sizes: Vec<u8>,
    total: usize,
}

impl StateSpace {
    pub fn new(market_sizes: &[u8]) -> Self {
        let total = total_states(market_sizes);
        Self {
            market_sizes: market_sizes.to_vec(),
            total,
        }
    }

    /// Total number of atomic states.
    pub fn total_states(&self) -> usize {
        self.total
    }

    /// Convert outcomes to state index.
    pub fn state_index(&self, outcomes: &[u8]) -> usize {
        state_index(outcomes, &self.market_sizes)
    }

    /// Convert state index to outcomes.
    pub fn state_to_outcomes(&self, state_idx: usize) -> Vec<u8> {
        state_to_outcomes(state_idx, &self.market_sizes)
    }

    /// Iterate over all states, yielding (index, outcomes).
    pub fn iter_states(&self) -> impl Iterator<Item = (usize, Vec<u8>)> + '_ {
        (0..self.total).map(move |idx| (idx, self.state_to_outcomes(idx)))
    }

    /// Get the market sizes.
    pub fn market_sizes(&self) -> &[u8] {
        &self.market_sizes
    }

    /// Check if a specific outcome is set in a state.
    pub fn has_outcome(&self, state_idx: usize, market_idx: usize, outcome: u8) -> bool {
        if market_idx >= self.market_sizes.len() {
            return false;
        }
        let outcomes = self.state_to_outcomes(state_idx);
        outcomes.get(market_idx).map(|&o| o == outcome).unwrap_or(false)
    }
}

/// State probability distribution.
#[derive(Clone, Debug)]
pub struct StateProbabilities {
    probs: Vec<f64>,
}

impl StateProbabilities {
    /// Create uniform probabilities over all states.
    pub fn uniform(num_states: usize) -> Self {
        let p = 1.0 / num_states as f64;
        Self {
            probs: vec![p; num_states],
        }
    }

    /// Create from explicit probabilities (should sum to 1).
    pub fn from_vec(probs: Vec<f64>) -> Self {
        Self { probs }
    }

    /// Get probability of a specific state.
    pub fn prob(&self, state_idx: usize) -> f64 {
        self.probs.get(state_idx).copied().unwrap_or(0.0)
    }

    /// Get all probabilities.
    pub fn as_slice(&self) -> &[f64] {
        &self.probs
    }

    /// Calculate expected value of a payoff vector.
    pub fn expected_value(&self, payoffs: &[i8]) -> f64 {
        self.probs
            .iter()
            .zip(payoffs.iter())
            .map(|(&p, &payoff)| p * payoff as f64)
            .sum()
    }

    /// Calculate marginal probability that a specific market has a specific outcome.
    pub fn marginal(&self, space: &StateSpace, market_idx: usize, outcome: u8) -> f64 {
        let mut total = 0.0;
        for (idx, p) in self.probs.iter().enumerate() {
            if space.has_outcome(idx, market_idx, outcome) {
                total += p;
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_state_index() {
        // Two binary markets
        assert_eq!(state_index(&[0, 0], &[2, 2]), 0);
        assert_eq!(state_index(&[1, 0], &[2, 2]), 1);
        assert_eq!(state_index(&[0, 1], &[2, 2]), 2);
        assert_eq!(state_index(&[1, 1], &[2, 2]), 3);
    }

    #[test]
    fn test_multi_outcome_state_index() {
        // One 3-outcome market and one binary market
        // Sizes: [3, 2], total states = 6
        assert_eq!(state_index(&[0, 0], &[3, 2]), 0);
        assert_eq!(state_index(&[1, 0], &[3, 2]), 1);
        assert_eq!(state_index(&[2, 0], &[3, 2]), 2);
        assert_eq!(state_index(&[0, 1], &[3, 2]), 3);
        assert_eq!(state_index(&[1, 1], &[3, 2]), 4);
        assert_eq!(state_index(&[2, 1], &[3, 2]), 5);
    }

    #[test]
    fn test_state_to_outcomes() {
        let sizes = vec![2, 2];
        assert_eq!(state_to_outcomes(0, &sizes), vec![0, 0]);
        assert_eq!(state_to_outcomes(1, &sizes), vec![1, 0]);
        assert_eq!(state_to_outcomes(2, &sizes), vec![0, 1]);
        assert_eq!(state_to_outcomes(3, &sizes), vec![1, 1]);
    }

    #[test]
    fn test_roundtrip() {
        let sizes = vec![3, 2, 4]; // 24 total states
        for i in 0..24 {
            let outcomes = state_to_outcomes(i, &sizes);
            let recovered = state_index(&outcomes, &sizes);
            assert_eq!(i, recovered, "Roundtrip failed for state {}", i);
        }
    }

    #[test]
    fn test_state_space() {
        let space = StateSpace::new(&[2, 3]);
        assert_eq!(space.total_states(), 6);

        let states: Vec<_> = space.iter_states().collect();
        assert_eq!(states.len(), 6);
        assert_eq!(states[0], (0, vec![0, 0]));
        assert_eq!(states[5], (5, vec![1, 2]));
    }

    #[test]
    fn test_state_probabilities() {
        let probs = StateProbabilities::uniform(4);
        assert!((probs.prob(0) - 0.25).abs() < 1e-9);

        let payoffs = [1, 0, 0, -1i8];
        let ev = probs.expected_value(&payoffs);
        assert!((ev - 0.0).abs() < 1e-9); // (0.25*1 + 0.25*0 + 0.25*0 + 0.25*-1) = 0
    }

    #[test]
    fn test_marginal_probability() {
        let space = StateSpace::new(&[2, 2]);
        // States: 00, 10, 01, 11
        // Probs:  0.1, 0.2, 0.3, 0.4
        let probs = StateProbabilities::from_vec(vec![0.1, 0.2, 0.3, 0.4]);

        // P(market 0 = 0) = P(state 0) + P(state 2) = 0.1 + 0.3 = 0.4
        let m0_0 = probs.marginal(&space, 0, 0);
        assert!((m0_0 - 0.4).abs() < 1e-9);

        // P(market 0 = 1) = P(state 1) + P(state 3) = 0.2 + 0.4 = 0.6
        let m0_1 = probs.marginal(&space, 0, 1);
        assert!((m0_1 - 0.6).abs() < 1e-9);
    }
}
