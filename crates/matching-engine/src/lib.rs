//! Core matching types and logic for prediction market matching.
//!
//! This crate provides the fundamental types and data structures for
//! the prediction market matching system.
//!
//! # Design Philosophy
//!
//! The engine is minimal and focused:
//! - All markets are binary (YES/NO)
//! - Multi-outcome concepts (e.g., 3-candidate elections) are handled
//!   at the solver layer by grouping related binary markets
//! - The engine doesn't know or care about market relationships
//!
//! # Module Structure
//!
//! - `types`: Core numeric types (Nanos, Qty, MarketId, Side)
//! - `market`: Binary market definitions
//! - `order`: Unified order representation using payoff vectors
//! - `order_builder`: Convenient constructors for common order types
//! - `state`: State indexing and payoff evaluation

pub mod market;
pub mod midprice;
pub mod mm_constraint;
pub mod order;
pub mod order_builder;
pub mod problem;
pub mod settlement;
pub mod state;
pub mod types;

// Re-exports for convenience
pub use market::{Market, MarketSet};
pub use midprice::{book_midprices, mark_yes_no};
pub use mm_constraint::{MmConstraint, MmId, MmSide};
pub use order::{
    derive_order_direction, ConditionDir, Fill, MarginalPayoff, Order, PriceCondition,
    MAX_MARKETS_PER_ORDER, MAX_STATES,
};
pub use order_builder::OrderBuilder;
pub use problem::{MarketGroup, Problem, ProblemSummary};
pub use settlement::{
    compute_fill_settlement, compute_fill_settlement_checked, derive_minting,
    derive_minting_checked, derive_minting_cost, fill_balance_delta_from_fills,
    gross_welfare_from_fills, market_totals_from_fills, minting_cost_from_adjustments,
    minting_cost_from_adjustments_checked, minting_cost_from_balance_deltas,
    minting_cost_from_fills, minting_cost_from_incremental_adjustments,
    minting_cost_from_incremental_adjustments_checked, net_welfare, MintAdjustment,
    SettlementArithmeticError, SettlementDelta,
};
pub use state::{state_index, state_to_outcomes, StateSpace};
pub use types::conversions::{dollars_to_nanos, nanos_to_dollars, nanos_to_price, price_to_nanos};
pub use types::{
    ceil_mul_ratio, checked_notional_ceil_i64, checked_notional_i64, checked_notional_nanos,
    checked_notional_nanos_ceil, checked_signed_notional_nanos,
    checked_signed_price_delta_notional, notional_nanos, notional_nanos_ceil, shares_to_qty,
    signed_notional_nanos, signed_price_delta_notional, MarketId, Nanos, OrderDirection, Qty, Side,
    MAX_ORDER_QTY, NANOS_PER_DOLLAR, SHARE_SCALE,
};

// Re-export order_builder convenience functions
pub use order_builder::{
    bundle_sell, bundle_yes, butterfly, conditional_buy, outcome_buy, outcome_sell, ratio_spread,
    simple_no_buy, simple_yes_buy, spread, spread_sell,
};
