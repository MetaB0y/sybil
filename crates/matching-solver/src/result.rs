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

/// Statistics from combining solutions.
#[derive(Clone, Debug, Default, Serialize)]
pub struct CombineStats {
    /// Number of solutions combined
    pub num_solutions: usize,
    /// Total fills across all solutions
    pub total_fills_input: usize,
    /// Fills selected in final result
    pub fills_selected: usize,
    /// Conflicts detected in graph
    pub conflicts_detected: usize,
    /// Welfare before combining
    pub input_max_welfare: i64,
    /// Final welfare after combining
    pub output_welfare: i64,
    /// Time spent building conflict graph (seconds)
    pub conflict_graph_time_secs: f64,
    /// Time spent solving MWIS (seconds)
    pub mwis_time_secs: f64,
}

/// Contribution tracking for a solver.
#[derive(Clone, Debug, Serialize)]
pub struct SolverContribution {
    /// Name of the solver
    pub solver_name: String,
    /// Number of fills contributed to final solution
    pub fills_contributed: usize,
    /// Welfare contributed by this solver's fills
    pub welfare_contributed: i64,
}

/// Result from running a solver.
#[derive(Clone, Debug, Serialize)]
pub struct PipelineResult {
    /// The final combined matching result.
    pub result: MatchingResult,

    /// Price discovery result (clearing prices per market).
    pub price_discovery: Option<PriceDiscoveryResult>,

    /// Per-solver contributions to the final result.
    pub contributions: Vec<SolverContribution>,

    /// Statistics from combining.
    pub combine_stats: Option<CombineStats>,

    /// Number of iterations (if applicable).
    pub iterations: usize,

    /// Per-iteration stats for convergence analysis.
    pub iteration_stats: Vec<IterationStats>,

    /// Total time spent (seconds).
    pub total_time_secs: f64,

    /// Time breakdown by phase.
    pub phase_times: PipelineTimings,

    /// Diagnostics from UCP enforcement (if it ran).
    pub ucp_stats: Option<UcpStats>,

    /// Per-phase snapshots for detailed visualization (viz feature only).
    #[cfg(feature = "viz")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub phase_snapshots: Vec<crate::viz::PhaseSnapshot>,
}

/// Timing breakdown for solver phases.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PipelineTimings {
    pub price_discovery_secs: f64,
    pub allocation_secs: f64,
    pub partial_solving_secs: f64,
    pub combining_secs: f64,
}

/// Diagnostics from enforce_ucp (Uniform Clearing Price enforcement).
#[derive(Clone, Debug, Default, Serialize)]
pub struct UcpStats {
    pub input_fills: usize,
    pub input_welfare: i64,
    pub after_reprice_fills: usize,
    pub dropped_by_reprice: usize,
    pub after_trim_fills: usize,
    pub dropped_by_trim: usize,
    pub final_fills: usize,
    pub final_welfare: i64,
    pub welfare_retention_pct: f64,
    /// Per-market position imbalance before trimming: (market_id, yes_qty, no_qty, excess)
    pub market_imbalances: Vec<(MarketId, u64, u64, u64)>,
}

/// Stats for a single iteration.
#[derive(Clone, Debug, Default, Serialize)]
pub struct IterationStats {
    /// Iteration number (1-indexed).
    pub iteration: usize,
    /// Total welfare after this iteration.
    pub welfare: i64,
    /// Total volume (shares) after this iteration.
    pub volume: u64,
    /// Total fills after this iteration.
    pub fills: usize,
    /// Welfare delta from previous iteration.
    pub welfare_delta: i64,
    /// Volume delta from previous iteration.
    pub volume_delta: u64,
    /// Fills delta from previous iteration.
    pub fills_delta: usize,
    /// Breakdown: fills from price discovery.
    pub price_discovery_fills: usize,
    /// Breakdown: fills from bundle matching.
    pub bundle_fills: usize,
    /// Index of first fill in this iteration (into PipelineResult.result.fills).
    pub fill_start_idx: usize,
    /// Index after last fill in this iteration.
    pub fill_end_idx: usize,
    /// Per-market clearing prices for this iteration.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub market_prices: HashMap<MarketId, Vec<Nanos>>,
}

impl PipelineResult {
    /// Create an empty result.
    pub fn empty() -> Self {
        Self {
            result: MatchingResult::new(),
            price_discovery: None,
            contributions: Vec::new(),
            combine_stats: None,
            iterations: 0,
            iteration_stats: Vec::new(),
            total_time_secs: 0.0,
            phase_times: PipelineTimings::default(),
            ucp_stats: None,
            #[cfg(feature = "viz")]
            phase_snapshots: Vec::new(),
        }
    }
}
