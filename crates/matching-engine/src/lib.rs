//! Core matching types and logic for prediction market matching.
//!
//! This crate provides the fundamental types and data structures for
//! the NP-hard prediction market matching system.
//!
//! # Module Structure
//!
//! - `types`: Core numeric types (Nanos, Qty, MarketId, Side)
//! - `market`: Multi-outcome market definitions
//! - `order`: Unified order representation using payoff vectors
//! - `order_builder`: Convenient constructors for common order types
//! - `book`: Finite liquidity order books
//! - `constraints`: Market constraints (implications, exclusions, hierarchies)
//! - `state`: State indexing and payoff evaluation

pub mod book;
pub mod constraints;
pub mod market;
pub mod mm_constraint;
pub mod order;
pub mod order_builder;
pub mod problem;
pub mod state;
pub mod types;

// Re-exports for convenience
pub use book::{BookLevel, LiquidityBook, LiquidityPool};
pub use constraints::{ConstraintBuilder, ConstraintSet, MarketConstraint};
pub use market::{Market, MarketSet};
pub use mm_constraint::{
    MmConstraint, MmConstraintStatus, MmId, MmOrder, MmSide, MmValidationResult,
};
pub use order::{ConditionDir, Fill, Order, PriceCondition, MAX_MARKETS_PER_ORDER, MAX_STATES};
pub use order_builder::OrderBuilder;
pub use problem::{Problem, ProblemSummary};
pub use state::{state_index, state_to_outcomes, StateProbabilities, StateSpace};
pub use types::conversions::{dollars_to_nanos, nanos_to_dollars, nanos_to_price, price_to_nanos};
pub use types::{MarketId, Nanos, Qty, Side, NANOS_PER_DOLLAR};

// Re-export order_builder convenience functions
pub use order_builder::{
    bundle_yes, butterfly, conditional_buy, outcome_buy, outcome_sell, ratio_spread, simple_no_buy,
    simple_yes_buy, spread,
};
