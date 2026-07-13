//! Off-block aggregate trackers.
//!
//! Each tracker mirrors `PriceTracker.market_volumes`: sidecar state that does
//! not enter `state_root` / `events_root` / `BlockWitness`. Snapshots round-trip
//! through the existing `SequencerSnapshot` / `RestoredState` pipeline; missing
//! tables on a stale store yield `Default::default()` (cold start).
//!
//! Trackers land in their own files under this module:
//! - `account_event_log` — per-account history feed (volatile)
//! - `trader_tracker` (B1) — unique placers, per-market + platform + 24h
//! - `liquidity_tracker` (B4) — last-10-batch ±band depth average
//! - `order_stats_tracker` (B6) — placed / matched / unmatched
//! - `cost_basis_tracker` (C1) — WAC + realized PnL
//! - `welfare_tracker` — cumulative + 24h platform welfare (signed)
//! - `equity_tracker` — per-account equity series (volatile; resets on restart,
//!   no snapshot round-trip)
//!
//! These read models are documented in `docs/architecture/06-operations/Persistence.md`.

pub mod account_event_log;
pub mod cost_basis_tracker;
pub mod equity_tracker;
pub mod liquidity_tracker;
pub mod order_stats_tracker;
pub mod trader_tracker;
pub mod welfare_tracker;

pub use account_event_log::{
    AccountEventLog, DEFAULT_MAX_RECENT_ACCOUNT_EVENTS_PER_ACCOUNT, HistoryEvent, HistoryKind,
    StoredHistoryEvent, fill_facets, side_outcome_from_order,
};
pub use cost_basis_tracker::{CostBasisTracker, CostBasisTrackerSnapshot};
pub use equity_tracker::{DEFAULT_MAX_RECENT_EQUITY_POINTS, EquityPoint, EquityTracker};
pub use liquidity_tracker::{LIQUIDITY_RING_CAP, LiquidityTracker, LiquidityTrackerSnapshot};
pub use order_stats_tracker::{OrderStats, OrderStatsTracker, OrderStatsTrackerSnapshot};
pub use trader_tracker::{TraderTracker, TraderTrackerSnapshot};
pub use welfare_tracker::{WelfareTracker, WelfareTrackerSnapshot};
