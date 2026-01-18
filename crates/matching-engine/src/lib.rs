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

pub mod types;
pub mod market;
pub mod order;
pub mod order_builder;
pub mod book;
pub mod constraints;
pub mod state;
pub mod problem;

// Re-exports for convenience
pub use types::{MarketId, Nanos, Qty, Side, NANOS_PER_DOLLAR};
pub use types::conversions::{price_to_nanos, nanos_to_price, dollars_to_nanos, nanos_to_dollars};
pub use market::{Market, MarketSet};
pub use order::{Order, Fill, PriceCondition, ConditionDir, MAX_MARKETS_PER_ORDER, MAX_STATES};
pub use order_builder::OrderBuilder;
pub use book::{BookLevel, LiquidityBook, LiquidityPool};
pub use constraints::{MarketConstraint, ConstraintSet, ConstraintBuilder};
pub use state::{state_index, state_to_outcomes, StateSpace, StateProbabilities};
pub use problem::{Problem, ProblemSummary};

// Re-export order_builder convenience functions
pub use order_builder::{simple_yes_buy, simple_no_buy, spread, butterfly, bundle_yes, outcome_buy, ratio_spread, conditional_buy};
