pub mod informed;
pub mod market_maker;
pub mod noise;

use std::collections::HashMap;

use matching_engine::{MarketGroup, MarketId, MmConstraint, Nanos, Order};

use crate::account::{Account, AccountId};

/// View of the current market state, provided to agents each batch.
pub struct MarketView {
    pub batch: usize,
    pub markets: Vec<(MarketId, String)>,
    pub last_prices: HashMap<MarketId, Vec<Nanos>>,
    pub market_groups: Vec<MarketGroup>,
    /// Public probability beliefs (from news). None = use last_prices as base.
    pub public_beliefs: Option<HashMap<MarketId, f64>>,
}

/// What an agent submits each batch.
pub struct AgentSubmission {
    pub orders: Vec<Order>,
    pub mm_constraint: Option<MmConstraint>,
}

impl AgentSubmission {
    pub fn empty() -> Self {
        Self {
            orders: Vec::new(),
            mm_constraint: None,
        }
    }

    pub fn with_orders(orders: Vec<Order>) -> Self {
        Self {
            orders,
            mm_constraint: None,
        }
    }

    pub fn with_mm(orders: Vec<Order>, mm_constraint: MmConstraint) -> Self {
        Self {
            orders,
            mm_constraint: Some(mm_constraint),
        }
    }
}

/// Trait for simulation agents that submit orders each batch.
pub trait Agent: Send {
    fn name(&self) -> &str;
    fn account_id(&self) -> AccountId;
    fn submit_orders(&mut self, view: &MarketView, account: &Account) -> AgentSubmission;
}
