//! Problem definition for matching instances.

use std::collections::HashSet;

use crate::{MarketId, MarketSet, MmConstraint, Order};

/// A group of mutually exclusive markets (exactly one resolves YES).
///
/// Used to model multi-outcome events like elections where
/// multiple binary markets represent different outcomes.
///
/// # Example
///
/// ```ignore
/// // Election: exactly one candidate wins
/// MarketGroup::new("2024 Election")
///     .with_market(trump_wins)
///     .with_market(biden_wins)
///     .with_market(other_wins)
/// ```
///
/// The solver enforces: sum of P(market_i YES) = 1 for each group.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MarketGroup {
    /// Name of this group (e.g., "2024 Election")
    pub name: String,
    /// Markets in this group (mutually exclusive)
    pub markets: Vec<MarketId>,
}

impl MarketGroup {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            markets: Vec::new(),
        }
    }

    pub fn with_market(mut self, market: MarketId) -> Self {
        self.markets.push(market);
        self
    }

    pub fn add_market(&mut self, market: MarketId) {
        self.markets.push(market);
    }
}

/// A complete problem instance for the matching system.
#[derive(Clone, Debug)]
pub struct Problem {
    /// Name of this scenario
    pub name: String,
    /// Markets in this problem (all binary)
    pub markets: MarketSet,
    /// Orders to match
    pub orders: Vec<Order>,
    /// Market maker capital constraints
    pub mm_constraints: Vec<MmConstraint>,
    /// Multi-outcome market groups (mutually exclusive markets)
    pub market_groups: Vec<MarketGroup>,
}

impl Problem {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            markets: MarketSet::new(),
            orders: Vec::new(),
            mm_constraints: Vec::new(),
            market_groups: Vec::new(),
        }
    }

    /// Add a multi-outcome market group.
    pub fn add_market_group(&mut self, group: MarketGroup) {
        self.market_groups.push(group);
    }

    pub fn num_markets(&self) -> usize {
        self.markets.len()
    }

    pub fn num_orders(&self) -> usize {
        self.orders.len()
    }

    pub fn total_demand(&self) -> u64 {
        self.orders.iter().map(|o| o.max_fill).sum()
    }

    /// Validate problem invariants.
    ///
    /// Checks:
    /// - No duplicate order IDs
    /// - All orders reference existing markets
    /// - All mm_constraint order IDs reference existing orders
    /// - All market_group market IDs exist
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // Check for duplicate order IDs
        let mut seen_ids = HashSet::new();
        for order in &self.orders {
            if !seen_ids.insert(order.id) {
                errors.push(format!("duplicate order ID: {}", order.id));
            }
        }

        // Check all orders reference existing markets
        for order in &self.orders {
            for &market in order.markets.iter().take(order.num_markets as usize) {
                if !market.is_none() && self.markets.get(market).is_none() {
                    errors.push(format!(
                        "order {} references non-existent market {}",
                        order.id, market
                    ));
                }
            }
        }

        // Check all mm_constraint order IDs reference existing orders
        let order_ids: HashSet<u64> = self.orders.iter().map(|o| o.id).collect();
        for mm in &self.mm_constraints {
            for &order_id in &mm.order_ids {
                if !order_ids.contains(&order_id) {
                    errors.push(format!(
                        "MM constraint {:?} references non-existent order {}",
                        mm.mm_id, order_id
                    ));
                }
            }
        }

        // Check all market_group market IDs exist
        for group in &self.market_groups {
            for &market_id in &group.markets {
                if self.markets.get(market_id).is_none() {
                    errors.push(format!(
                        "market group '{}' references non-existent market {}",
                        group.name, market_id
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn summary(&self) -> ProblemSummary {
        let multi_market_orders = self.orders.iter().filter(|o| o.num_markets > 1).count();
        let conditional_orders = self.orders.iter().filter(|o| o.is_conditional()).count();

        ProblemSummary {
            name: self.name.clone(),
            num_markets: self.num_markets(),
            num_orders: self.num_orders(),
            num_mm_constraints: self.mm_constraints.len(),
            multi_market_orders,
            conditional_orders,
            total_demand: self.total_demand(),
        }
    }
}

/// Summary of a problem instance
#[derive(Clone, Debug)]
pub struct ProblemSummary {
    pub name: String,
    pub num_markets: usize,
    pub num_orders: usize,
    pub num_mm_constraints: usize,
    pub multi_market_orders: usize,
    pub conditional_orders: usize,
    pub total_demand: u64,
}

impl std::fmt::Display for ProblemSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Problem: {}", self.name)?;
        writeln!(f, "  Markets: {}", self.num_markets)?;
        if self.multi_market_orders > 0 || self.conditional_orders > 0 {
            writeln!(
                f,
                "  Orders: {} (multi-market: {}, conditional: {})",
                self.num_orders, self.multi_market_orders, self.conditional_orders
            )?;
        } else {
            writeln!(f, "  Orders: {}", self.num_orders)?;
        }
        if self.num_mm_constraints > 0 {
            writeln!(f, "  MM Constraints: {}", self.num_mm_constraints)?;
        }
        writeln!(f, "  Total demand: {} shares", self.total_demand)
    }
}
