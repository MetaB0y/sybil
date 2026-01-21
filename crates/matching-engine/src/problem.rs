//! Problem definition for matching instances.

use crate::{LiquidityPool, MarketSet, MmConstraint, Order};

/// A complete problem instance for the matching system.
#[derive(Clone, Debug)]
pub struct Problem {
    /// Name of this scenario
    pub name: String,
    /// Markets in this problem (all binary)
    pub markets: MarketSet,
    /// Liquidity available
    pub liquidity: LiquidityPool,
    /// Orders to match
    pub orders: Vec<Order>,
    /// Market maker capital constraints
    pub mm_constraints: Vec<MmConstraint>,
}

impl Problem {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            markets: MarketSet::new(),
            liquidity: LiquidityPool::new(),
            orders: Vec::new(),
            mm_constraints: Vec::new(),
        }
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
