//! Off-block aggregate trackers.
//!
//! Each tracker mirrors `PriceTracker.market_volumes`: sidecar state that does
//! not enter `state_root` / `events_root` / `BlockWitness`. Snapshots round-trip
//! through the existing `SequencerSnapshot` / `RestoredState` pipeline; missing
//! tables on a stale store yield `Default::default()` (cold start).
//!
//! Trackers land in their own files under this module:
//! - `trader_tracker` (B1) — unique placers, per-market + platform + 24h
//! - `liquidity_tracker` (B4) — last-10-batch ±band depth average
//! - `order_stats_tracker` (B6) — placed / matched / unmatched
//! - `cost_basis_tracker` (C1) — WAC + realized PnL
//!
//! See `frontend/BACKEND_IMPLEMENTATION_PLAN.md` for the full plan.

pub mod trader_tracker;

pub use trader_tracker::{TraderTracker, TraderTrackerSnapshot};
