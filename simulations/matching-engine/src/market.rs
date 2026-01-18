//! Multi-outcome market definition.

use crate::types::MarketId;

/// A prediction market with multiple possible outcomes.
///
/// For binary markets, outcomes are typically ["Yes", "No"] or equivalent.
/// For multi-outcome markets (e.g., "Who wins the election?"), outcomes
/// list all mutually exclusive possibilities.
#[derive(Clone, Debug)]
pub struct Market {
    pub id: MarketId,
    pub name: String,
    pub outcomes: Vec<String>,
}

impl Market {
    /// Create a new market with the given outcomes.
    pub fn new(id: MarketId, name: impl Into<String>, outcomes: Vec<String>) -> Self {
        assert!(!outcomes.is_empty(), "Market must have at least one outcome");
        Self {
            id,
            name: name.into(),
            outcomes,
        }
    }

    /// Create a binary market (YES/NO).
    pub fn binary(id: MarketId, name: impl Into<String>) -> Self {
        Self::new(id, name, vec!["Yes".to_string(), "No".to_string()])
    }

    /// Number of outcomes in this market.
    pub fn num_outcomes(&self) -> u8 {
        self.outcomes.len() as u8
    }

    /// Check if this is a binary market.
    pub fn is_binary(&self) -> bool {
        self.outcomes.len() == 2
    }

    /// Get outcome name by index.
    pub fn outcome_name(&self, idx: u8) -> Option<&str> {
        self.outcomes.get(idx as usize).map(|s| s.as_str())
    }
}

/// A collection of markets that can be used together in cross-market orders.
#[derive(Clone, Debug, Default)]
pub struct MarketSet {
    markets: Vec<Market>,
}

impl MarketSet {
    pub fn new() -> Self {
        Self { markets: Vec::new() }
    }

    /// Add a market to the set. Returns the assigned MarketId.
    pub fn add(&mut self, name: impl Into<String>, outcomes: Vec<String>) -> MarketId {
        let id = MarketId::new(self.markets.len() as u32);
        self.markets.push(Market::new(id, name, outcomes));
        id
    }

    /// Add a binary market. Returns the assigned MarketId.
    pub fn add_binary(&mut self, name: impl Into<String>) -> MarketId {
        let id = MarketId::new(self.markets.len() as u32);
        self.markets.push(Market::binary(id, name));
        id
    }

    /// Get a market by ID.
    pub fn get(&self, id: MarketId) -> Option<&Market> {
        if id.is_none() {
            None
        } else {
            self.markets.get(id.0 as usize)
        }
    }

    /// Get the number of outcomes for a market.
    pub fn num_outcomes(&self, id: MarketId) -> u8 {
        self.get(id).map(|m| m.num_outcomes()).unwrap_or(0)
    }

    /// Iterate over all markets.
    pub fn iter(&self) -> impl Iterator<Item = &Market> {
        self.markets.iter()
    }

    /// Number of markets in the set.
    pub fn len(&self) -> usize {
        self.markets.len()
    }

    /// Check if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.markets.is_empty()
    }

    /// Calculate total number of atomic states for a set of markets.
    /// For N binary markets, this is 2^N.
    /// For multi-outcome markets, it's the product of outcome counts.
    pub fn total_states(&self, market_ids: &[MarketId]) -> usize {
        market_ids
            .iter()
            .filter(|id| !id.is_none())
            .map(|id| self.num_outcomes(*id) as usize)
            .product()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_market() {
        let m = Market::binary(MarketId::new(0), "Trump wins");
        assert!(m.is_binary());
        assert_eq!(m.num_outcomes(), 2);
        assert_eq!(m.outcome_name(0), Some("Yes"));
        assert_eq!(m.outcome_name(1), Some("No"));
    }

    #[test]
    fn test_multi_outcome_market() {
        let m = Market::new(
            MarketId::new(0),
            "2024 President",
            vec!["Trump".to_string(), "Harris".to_string(), "Other".to_string()],
        );
        assert!(!m.is_binary());
        assert_eq!(m.num_outcomes(), 3);
        assert_eq!(m.outcome_name(0), Some("Trump"));
        assert_eq!(m.outcome_name(1), Some("Harris"));
        assert_eq!(m.outcome_name(2), Some("Other"));
    }

    #[test]
    fn test_market_set() {
        let mut set = MarketSet::new();
        let m0 = set.add_binary("Market A");
        let m1 = set.add("Market B", vec!["X".to_string(), "Y".to_string(), "Z".to_string()]);

        assert_eq!(set.len(), 2);
        assert_eq!(set.num_outcomes(m0), 2);
        assert_eq!(set.num_outcomes(m1), 3);

        // 2 outcomes * 3 outcomes = 6 total states
        assert_eq!(set.total_states(&[m0, m1]), 6);
    }
}
