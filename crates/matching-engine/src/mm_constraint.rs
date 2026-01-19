//! Market Maker capital constraints for flash liquidity.
//!
//! MM constraints allow market makers to provide liquidity across many markets
//! with limited capital. The actual capital usage is determined at clearing time,
//! never exceeding the MM's budget.
//!
//! # Example
//!
//! ```ignore
//! let mm = MmConstraint::new(MmId(1), 10_000_000_000) // $10k budget
//!     .with_order(order_id_1)
//!     .with_order(order_id_2)
//!     .with_order(order_id_3);
//! ```

use crate::types::{Nanos, Qty};

/// Unique identifier for a market maker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MmId(pub u64);

impl MmId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Side of an MM order for capital calculation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmSide {
    /// Selling YES tokens (capital = (1 - price) * qty)
    SellYes,
    /// Buying YES tokens (capital = price * qty)
    BuyYes,
    /// Selling NO tokens (capital = price * qty)
    SellNo,
    /// Buying NO tokens (capital = (1 - price) * qty)
    BuyNo,
}

impl MmSide {
    /// Calculate capital needed for this side at given price and quantity.
    ///
    /// Prices are in nanos (1e9 = $1). Quantity is in shares.
    /// Returns capital needed in nanos.
    pub fn capital_needed(&self, price: Nanos, quantity: Qty) -> Nanos {
        const NANOS_PER_DOLLAR: u64 = 1_000_000_000;

        match self {
            MmSide::SellYes | MmSide::BuyNo => {
                // Net cost: (1 - price) per unit
                // price is in nanos, so (1 - price_fraction) = (NANOS - price) / NANOS
                // capital = (1 - price_fraction) * qty * NANOS = (NANOS - price) * qty
                (NANOS_PER_DOLLAR - price) * quantity
            }
            MmSide::BuyYes | MmSide::SellNo => {
                // Net cost: price per unit
                // capital = price_fraction * qty * NANOS = price * qty
                price * quantity
            }
        }
    }
}

/// An order that is part of an MM's constrained order set.
#[derive(Clone, Debug)]
pub struct MmOrder {
    /// The order ID this refers to
    pub order_id: u64,
    /// Side for capital calculation
    pub side: MmSide,
}

/// A capital constraint for a market maker.
///
/// The MM can submit orders across multiple markets, but the total capital
/// used (computed at clearing prices) must not exceed the budget.
#[derive(Clone, Debug)]
pub struct MmConstraint {
    /// Unique ID for this market maker
    pub mm_id: MmId,
    /// Maximum capital that can be used
    pub max_capital: Nanos,
    /// Order IDs that are part of this constraint
    pub order_ids: Vec<u64>,
    /// Side information for each order (for capital calculation)
    /// Maps order_id -> MmSide
    pub order_sides: std::collections::HashMap<u64, MmSide>,
}

impl MmConstraint {
    /// Create a new MM constraint with the given budget.
    pub fn new(mm_id: MmId, max_capital: Nanos) -> Self {
        Self {
            mm_id,
            max_capital,
            order_ids: Vec::new(),
            order_sides: std::collections::HashMap::new(),
        }
    }

    /// Add an order to this constraint.
    pub fn with_order(mut self, order_id: u64, side: MmSide) -> Self {
        self.order_ids.push(order_id);
        self.order_sides.insert(order_id, side);
        self
    }

    /// Add an order to this constraint (mutable version).
    pub fn add_order(&mut self, order_id: u64, side: MmSide) {
        self.order_ids.push(order_id);
        self.order_sides.insert(order_id, side);
    }

    /// Check if an order is part of this constraint.
    pub fn contains_order(&self, order_id: u64) -> bool {
        self.order_ids.contains(&order_id)
    }

    /// Calculate total capital needed at given prices and fill quantities.
    ///
    /// `fills` maps order_id -> (price, quantity)
    pub fn capital_used(&self, fills: &std::collections::HashMap<u64, (Nanos, Qty)>) -> Nanos {
        let mut total = 0;
        for &order_id in &self.order_ids {
            if let (Some(&(price, qty)), Some(&side)) =
                (fills.get(&order_id), self.order_sides.get(&order_id))
            {
                total += side.capital_needed(price, qty);
            }
        }
        total
    }

    /// Check if the constraint is satisfied at given fills.
    pub fn is_satisfied(&self, fills: &std::collections::HashMap<u64, (Nanos, Qty)>) -> bool {
        self.capital_used(fills) <= self.max_capital
    }

    /// Calculate how much more capital can be used.
    pub fn remaining_capital(&self, fills: &std::collections::HashMap<u64, (Nanos, Qty)>) -> Nanos {
        self.max_capital.saturating_sub(self.capital_used(fills))
    }

    /// Number of orders in this constraint.
    pub fn num_orders(&self) -> usize {
        self.order_ids.len()
    }
}

/// Result of validating MM constraints.
#[derive(Clone, Debug)]
pub struct MmValidationResult {
    /// Whether all constraints are satisfied
    pub all_satisfied: bool,
    /// Per-MM results
    pub per_mm: Vec<MmConstraintStatus>,
}

/// Status of a single MM constraint.
#[derive(Clone, Debug)]
pub struct MmConstraintStatus {
    pub mm_id: MmId,
    pub capital_used: Nanos,
    pub max_capital: Nanos,
    pub is_satisfied: bool,
    /// How much the constraint was exceeded (0 if satisfied)
    pub excess: Nanos,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mm_side_capital_sell_yes() {
        // Selling YES at $0.60 = capital cost of $0.40 per unit
        let capital = MmSide::SellYes.capital_needed(600_000_000, 100);
        assert_eq!(capital, 40_000_000_000); // $40
    }

    #[test]
    fn test_mm_side_capital_buy_yes() {
        // Buying YES at $0.60 = capital cost of $0.60 per unit
        let capital = MmSide::BuyYes.capital_needed(600_000_000, 100);
        assert_eq!(capital, 60_000_000_000); // $60
    }

    #[test]
    fn test_mm_constraint_creation() {
        let constraint = MmConstraint::new(MmId(1), 10_000_000_000)
            .with_order(100, MmSide::SellYes)
            .with_order(101, MmSide::SellYes)
            .with_order(102, MmSide::BuyYes);

        assert_eq!(constraint.num_orders(), 3);
        assert!(constraint.contains_order(100));
        assert!(!constraint.contains_order(999));
    }

    #[test]
    fn test_mm_constraint_capital_used() {
        let constraint = MmConstraint::new(MmId(1), 100_000_000_000) // $100
            .with_order(100, MmSide::SellYes)
            .with_order(101, MmSide::BuyYes);

        let mut fills = std::collections::HashMap::new();
        // Order 100: Sell YES at $0.60, qty 50 → capital = $20
        fills.insert(100, (600_000_000, 50));
        // Order 101: Buy YES at $0.40, qty 100 → capital = $40
        fills.insert(101, (400_000_000, 100));

        let capital = constraint.capital_used(&fills);
        assert_eq!(capital, 60_000_000_000); // $60 total

        assert!(constraint.is_satisfied(&fills)); // $60 < $100
    }

    #[test]
    fn test_mm_constraint_exceeded() {
        let constraint = MmConstraint::new(MmId(1), 50_000_000_000) // $50
            .with_order(100, MmSide::SellYes)
            .with_order(101, MmSide::BuyYes);

        let mut fills = std::collections::HashMap::new();
        fills.insert(100, (600_000_000, 50)); // $20
        fills.insert(101, (400_000_000, 100)); // $40

        let capital = constraint.capital_used(&fills);
        assert_eq!(capital, 60_000_000_000); // $60 total

        assert!(!constraint.is_satisfied(&fills)); // $60 > $50
    }
}
