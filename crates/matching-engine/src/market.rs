//! Binary market definition.
//!
//! All markets in the engine are binary (YES/NO). Multi-outcome concepts
//! like "Who wins: Trump/Harris/Other" are represented as multiple binary
//! markets grouped at the solver/UI layer.

use std::collections::HashMap;

use crate::types::MarketId;

/// A binary prediction market (YES/NO).
///
/// The engine only deals with binary markets. For multi-outcome scenarios
/// (e.g., 3-candidate elections), create multiple binary markets and group
/// them at the solver layer using `OutcomeGroup`.
#[derive(Clone, Debug)]
pub struct Market {
    pub id: MarketId,
    pub name: String,
}

impl Market {
    /// Create a new binary market.
    pub fn new(id: MarketId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
        }
    }

    /// Number of outcomes (always 2 for binary markets).
    #[inline]
    pub fn num_outcomes(&self) -> u8 {
        2
    }

    /// Binary markets are always binary.
    #[inline]
    pub fn is_binary(&self) -> bool {
        true
    }
}

/// A collection of markets.
#[derive(Clone, Debug, Default)]
pub struct MarketSet {
    markets: HashMap<MarketId, Market>,
    next_id: u32,
}

impl MarketSet {
    pub fn new() -> Self {
        Self {
            markets: HashMap::new(),
            next_id: 0,
        }
    }

    /// Add a binary market. Returns the assigned MarketId.
    pub fn add_binary(&mut self, name: impl Into<String>) -> MarketId {
        let id = MarketId::new(self.next_id);
        self.next_id += 1;
        self.markets.insert(id, Market::new(id, name));
        id
    }

    /// Add a market directly (preserving its existing ID).
    pub fn add_market(&mut self, market: Market) {
        let id = market.id;
        if id.0 >= self.next_id {
            self.next_id = id.0 + 1;
        }
        self.markets.insert(id, market);
    }

    /// Get a market by ID.
    pub fn get(&self, id: MarketId) -> Option<&Market> {
        if id.is_none() {
            None
        } else {
            self.markets.get(&id)
        }
    }

    /// Number of outcomes for a market (always 2).
    pub fn num_outcomes(&self, id: MarketId) -> u8 {
        if self.get(id).is_some() {
            2
        } else {
            0
        }
    }

    /// Iterate over all markets.
    pub fn iter(&self) -> impl Iterator<Item = &Market> {
        self.markets.values()
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
    pub fn total_states(&self, market_ids: &[MarketId]) -> usize {
        let active_markets = market_ids.iter().filter(|id| !id.is_none()).count();
        1usize << active_markets // 2^N
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_market() {
        let m = Market::new(MarketId::new(0), "Trump wins");
        assert!(m.is_binary());
        assert_eq!(m.num_outcomes(), 2);
    }

    #[test]
    fn test_market_set() {
        let mut set = MarketSet::new();
        let m0 = set.add_binary("Market A");
        let m1 = set.add_binary("Market B");

        assert_eq!(set.len(), 2);
        assert_eq!(set.num_outcomes(m0), 2);
        assert_eq!(set.num_outcomes(m1), 2);

        // 2 binary markets = 4 total states (2^2)
        assert_eq!(set.total_states(&[m0, m1]), 4);
    }
}
