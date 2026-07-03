//! Result types for FBA solvers.

use std::collections::HashMap;

use serde::Serialize;

use matching_engine::{MarketId, Nanos};

use crate::MatchingResult;

// ============================================================================
// Price Discovery Result
// ============================================================================

/// Result of price discovery across markets.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PriceDiscoveryResult {
    /// Clearing prices per outcome for each market.
    /// Maps MarketId -> Vec<Nanos> where index is outcome.
    pub prices: HashMap<MarketId, Vec<Nanos>>,

    /// Total welfare from price discovery.
    pub total_welfare: i64,

    /// Total number of fills.
    pub total_fills: usize,
}

impl PriceDiscoveryResult {
    /// Create an empty price discovery result.
    pub fn empty() -> Self {
        Self::default()
    }
}

// ============================================================================
// Pipeline / Solver Result
// ============================================================================

/// Result from running a solver.
#[derive(Clone, Debug, Serialize)]
pub struct PipelineResult {
    /// The final combined matching result.
    pub result: MatchingResult,

    /// Price discovery result (clearing prices per market).
    pub price_discovery: Option<PriceDiscoveryResult>,

    /// Total time spent (seconds).
    pub total_time_secs: f64,

    /// Time breakdown by phase.
    pub phase_times: PipelineTimings,
}

/// Timing breakdown for solver phases.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PipelineTimings {
    pub price_discovery_secs: f64,
    pub allocation_secs: f64,
    pub partial_solving_secs: f64,
    pub combining_secs: f64,
}

impl PipelineResult {
    /// Create an empty result.
    pub fn empty() -> Self {
        Self {
            result: MatchingResult::new(),
            price_discovery: None,
            total_time_secs: 0.0,
            phase_times: PipelineTimings::default(),
        }
    }
}
