//! Market constraints for logical relationships between outcomes.
//!
//! These constraints enforce real-world relationships like:
//! - "Trump wins" -> "Republican wins"
//! - Outcomes within a market are mutually exclusive
//! - Hierarchies: "Champion" -> "Finalist" -> "Semifinalist"

use crate::types::MarketId;
use crate::state::StateSpace;

/// Types of constraints between markets or outcomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarketConstraint {
    /// A happening implies B happens.
    /// Example: Trump wins -> Republican wins
    Implication {
        if_true: (MarketId, u8),   // (market, outcome)
        then_true: (MarketId, u8),
    },

    /// Outcomes within a market are mutually exclusive (sum of probs = 1).
    /// This is implicit for all markets but can be explicitly stated.
    SumToOne { market: MarketId },

    /// Hierarchical: parent outcome requires child outcome.
    /// Example: Champion -> Finalist -> Semifinalist
    Hierarchy {
        parent: (MarketId, u8),
        child: (MarketId, u8),
    },

    /// Mutual exclusion: at most one of these outcomes can be true.
    /// Useful for cross-market exclusions.
    MutuallyExclusive {
        outcomes: Vec<(MarketId, u8)>,
    },

    /// Exactly one of these outcomes must be true.
    /// Like MutuallyExclusive but requires one to happen.
    ExactlyOne {
        outcomes: Vec<(MarketId, u8)>,
    },
}

impl MarketConstraint {
    /// Create an implication constraint.
    pub fn implies(
        if_market: MarketId,
        if_outcome: u8,
        then_market: MarketId,
        then_outcome: u8,
    ) -> Self {
        Self::Implication {
            if_true: (if_market, if_outcome),
            then_true: (then_market, then_outcome),
        }
    }

    /// Create a hierarchy constraint (parent -> child).
    pub fn hierarchy(
        parent_market: MarketId,
        parent_outcome: u8,
        child_market: MarketId,
        child_outcome: u8,
    ) -> Self {
        Self::Hierarchy {
            parent: (parent_market, parent_outcome),
            child: (child_market, child_outcome),
        }
    }

    /// Create a mutual exclusion constraint.
    pub fn mutually_exclusive(outcomes: Vec<(MarketId, u8)>) -> Self {
        Self::MutuallyExclusive { outcomes }
    }

    /// Create an exactly-one constraint.
    pub fn exactly_one(outcomes: Vec<(MarketId, u8)>) -> Self {
        Self::ExactlyOne { outcomes }
    }
}

/// A collection of constraints that define valid state spaces.
#[derive(Clone, Debug, Default)]
pub struct ConstraintSet {
    constraints: Vec<MarketConstraint>,
}

impl ConstraintSet {
    pub fn new() -> Self {
        Self {
            constraints: Vec::new(),
        }
    }

    pub fn add(&mut self, constraint: MarketConstraint) {
        self.constraints.push(constraint);
    }

    pub fn len(&self) -> usize {
        self.constraints.len()
    }

    pub fn is_empty(&self) -> bool {
        self.constraints.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &MarketConstraint> {
        self.constraints.iter()
    }

    /// Check if a state is valid given the constraints.
    /// market_outcomes maps MarketId to the outcome in that state.
    pub fn is_valid_state(&self, get_outcome: impl Fn(MarketId) -> Option<u8>) -> bool {
        for constraint in &self.constraints {
            match constraint {
                MarketConstraint::Implication { if_true, then_true } => {
                    // If antecedent is true, consequent must be true
                    if get_outcome(if_true.0) == Some(if_true.1)
                        && get_outcome(then_true.0) != Some(then_true.1)
                    {
                        return false;
                    }
                }
                MarketConstraint::Hierarchy { parent, child } => {
                    // Parent true implies child true
                    if get_outcome(parent.0) == Some(parent.1)
                        && get_outcome(child.0) != Some(child.1)
                    {
                        return false;
                    }
                }
                MarketConstraint::MutuallyExclusive { outcomes } => {
                    let count = outcomes
                        .iter()
                        .filter(|(m, o)| get_outcome(*m) == Some(*o))
                        .count();
                    if count > 1 {
                        return false;
                    }
                }
                MarketConstraint::ExactlyOne { outcomes } => {
                    let count = outcomes
                        .iter()
                        .filter(|(m, o)| get_outcome(*m) == Some(*o))
                        .count();
                    if count != 1 {
                        return false;
                    }
                }
                MarketConstraint::SumToOne { .. } => {
                    // This is implicit in the state space construction
                    // Each market has exactly one outcome per state
                }
            }
        }
        true
    }

    /// Get valid state indices from a state space given constraints.
    /// Returns indices of states that satisfy all constraints.
    pub fn valid_states(
        &self,
        space: &StateSpace,
        market_ids: &[MarketId],
    ) -> Vec<usize> {
        (0..space.total_states())
            .filter(|&idx| {
                let outcomes = space.state_to_outcomes(idx);
                self.is_valid_state(|m| {
                    market_ids
                        .iter()
                        .position(|&mid| mid == m)
                        .and_then(|pos| outcomes.get(pos).copied())
                })
            })
            .collect()
    }
}

/// Builder for creating common constraint patterns.
pub struct ConstraintBuilder {
    constraints: ConstraintSet,
}

impl ConstraintBuilder {
    pub fn new() -> Self {
        Self {
            constraints: ConstraintSet::new(),
        }
    }

    /// Add an implication: if A happens, B must happen.
    pub fn implies(
        mut self,
        if_market: MarketId,
        if_outcome: u8,
        then_market: MarketId,
        then_outcome: u8,
    ) -> Self {
        self.constraints.add(MarketConstraint::implies(
            if_market, if_outcome, then_market, then_outcome,
        ));
        self
    }

    /// Add a tournament hierarchy: winner -> finalist -> semifinalist.
    pub fn tournament_hierarchy(
        mut self,
        champion_market: MarketId,
        final_market: MarketId,
        semi_market: MarketId,
        team_outcome: u8,
    ) -> Self {
        // Champion -> Final
        self.constraints.add(MarketConstraint::hierarchy(
            champion_market, team_outcome, final_market, team_outcome,
        ));
        // Final -> Semi
        self.constraints.add(MarketConstraint::hierarchy(
            final_market, team_outcome, semi_market, team_outcome,
        ));
        self
    }

    /// Add mutual exclusion: at most one of these can be true.
    pub fn mutually_exclusive(mut self, outcomes: Vec<(MarketId, u8)>) -> Self {
        self.constraints.add(MarketConstraint::mutually_exclusive(outcomes));
        self
    }

    /// Add exactly-one: exactly one of these must be true.
    pub fn exactly_one(mut self, outcomes: Vec<(MarketId, u8)>) -> Self {
        self.constraints.add(MarketConstraint::exactly_one(outcomes));
        self
    }

    pub fn build(self) -> ConstraintSet {
        self.constraints
    }
}

impl Default for ConstraintBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_implication() {
        let m0 = MarketId::new(0); // Trump wins
        let m1 = MarketId::new(1); // Republican wins

        let mut constraints = ConstraintSet::new();
        // Trump wins (m0=0) -> Republican wins (m1=0)
        constraints.add(MarketConstraint::implies(m0, 0, m1, 0));

        // Valid: Trump wins AND Republican wins
        assert!(constraints.is_valid_state(|m| {
            if m == m0 { Some(0) }
            else if m == m1 { Some(0) }
            else { None }
        }));

        // Valid: Trump loses (doesn't trigger implication)
        assert!(constraints.is_valid_state(|m| {
            if m == m0 { Some(1) }
            else if m == m1 { Some(1) }
            else { None }
        }));

        // Invalid: Trump wins but Republican loses
        assert!(!constraints.is_valid_state(|m| {
            if m == m0 { Some(0) }
            else if m == m1 { Some(1) }
            else { None }
        }));
    }

    #[test]
    fn test_mutual_exclusion() {
        let m0 = MarketId::new(0);
        let m1 = MarketId::new(1);
        let m2 = MarketId::new(2);

        let mut constraints = ConstraintSet::new();
        // Only one candidate can win
        constraints.add(MarketConstraint::mutually_exclusive(vec![
            (m0, 0), // Trump wins
            (m1, 0), // Harris wins
            (m2, 0), // Other wins
        ]));

        // Valid: exactly one wins
        assert!(constraints.is_valid_state(|m| {
            if m == m0 { Some(0) }  // Trump wins
            else if m == m1 { Some(1) }  // Harris loses
            else if m == m2 { Some(1) }  // Other loses
            else { None }
        }));

        // Valid: none win (mutual exclusion allows zero)
        assert!(constraints.is_valid_state(|m| {
            if m == m0 { Some(1) }
            else if m == m1 { Some(1) }
            else if m == m2 { Some(1) }
            else { None }
        }));

        // Invalid: two win
        assert!(!constraints.is_valid_state(|m| {
            if m == m0 { Some(0) }  // Trump wins
            else if m == m1 { Some(0) }  // Harris also wins??
            else if m == m2 { Some(1) }
            else { None }
        }));
    }

    #[test]
    fn test_exactly_one() {
        let m0 = MarketId::new(0);
        let m1 = MarketId::new(1);

        let mut constraints = ConstraintSet::new();
        constraints.add(MarketConstraint::exactly_one(vec![(m0, 0), (m1, 0)]));

        // Valid: exactly one
        assert!(constraints.is_valid_state(|m| {
            if m == m0 { Some(0) }
            else if m == m1 { Some(1) }
            else { None }
        }));

        // Invalid: none
        assert!(!constraints.is_valid_state(|m| {
            if m == m0 { Some(1) }
            else if m == m1 { Some(1) }
            else { None }
        }));

        // Invalid: both
        assert!(!constraints.is_valid_state(|m| {
            if m == m0 { Some(0) }
            else if m == m1 { Some(0) }
            else { None }
        }));
    }

    #[test]
    fn test_constraint_builder() {
        let trump = MarketId::new(0);
        let harris = MarketId::new(1);
        let rep = MarketId::new(2);
        let dem = MarketId::new(3);

        let constraints = ConstraintBuilder::new()
            .implies(trump, 0, rep, 0)   // Trump wins -> Republican wins
            .implies(harris, 0, dem, 0)  // Harris wins -> Democrat wins
            .mutually_exclusive(vec![(trump, 0), (harris, 0)])  // Only one can win
            .build();

        assert_eq!(constraints.len(), 3);
    }
}
