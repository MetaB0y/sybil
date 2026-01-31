//! Problem definition for matching instances.

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
#[derive(Clone, Debug)]
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

    pub fn summary(&self) -> ProblemSummary {
        let bundle_orders = self.orders.iter().filter(|o| o.num_markets > 1).count();
        let aon_orders = self.orders.iter().filter(|o| o.is_all_or_none()).count();
        let conditional_orders = self.orders.iter().filter(|o| o.is_conditional()).count();

        ProblemSummary {
            name: self.name.clone(),
            num_markets: self.num_markets(),
            num_orders: self.num_orders(),
            num_mm_constraints: self.mm_constraints.len(),
            bundle_orders,
            aon_orders,
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
    pub bundle_orders: usize,
    pub aon_orders: usize,
    pub conditional_orders: usize,
    pub total_demand: u64,
}

impl std::fmt::Display for ProblemSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Problem: {}", self.name)?;
        writeln!(f, "  Markets: {}", self.num_markets)?;
        writeln!(
            f,
            "  Orders: {} (bundles: {}, AON: {}, conditional: {})",
            self.num_orders, self.bundle_orders, self.aon_orders, self.conditional_orders
        )?;
        if self.num_mm_constraints > 0 {
            writeln!(f, "  MM Constraints: {}", self.num_mm_constraints)?;
        }
        writeln!(f, "  Total demand: {} shares", self.total_demand)
    }
}
