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

    /// Machine-readable solver termination information. This is intentionally
    /// separate from verifier validity: a numerically failed solver can return
    /// an empty result that is vacuously feasible, while a completed solver can
    /// still fail integer verification.
    pub diagnostics: SolverDiagnostics,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminationStatus {
    /// No optimization was needed because the input was empty.
    EmptyInput,
    /// The solver rejected all supported input shapes.
    UnsupportedInput,
    /// The configured algorithm completed its convergence/optimality test.
    Converged,
    /// The algorithm returned its best iterate at its configured cap.
    IterationLimit,
    /// A solver-specific time limit was reached.
    TimeLimit,
    /// The backend reported infeasibility.
    Infeasible,
    /// Backend construction or numerical progress failed.
    NumericalFailure,
    /// Post-processing or projection failed after the core solve.
    PostProcessingFailure,
    /// The requested mode is mathematically identical to and delegated to a
    /// different implementation (for example, conic Linear mode to LP).
    Delegated,
    /// Older/auxiliary code did not report a termination state.
    #[default]
    NotReported,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SolverDiagnostics {
    pub algorithm: String,
    pub status: TerminationStatus,
    pub iterations: Option<usize>,
    /// Legacy solver-specific convergence number. New solvers should prefer
    /// `optimality_gap` when they have a certified objective bound.
    pub convergence_metric: Option<f64>,
    /// Continuous backend objective, in solver-documented units.
    pub objective_value: Option<f64>,
    /// Certified upper bound on objective suboptimality, in the same units as
    /// `objective_value`. `None` means the solver has no such certificate.
    pub optimality_gap: Option<f64>,
    /// Number of optimization-oracle calls made by an iterative method.
    pub oracle_calls: Option<usize>,
    /// Objective lost when the continuous allocation is landed into integer
    /// protocol quantities. Negative numerical noise is reported as zero.
    pub integer_landing_loss: Option<f64>,
    /// Backend-reported residuals when available.
    pub primal_residual: Option<f64>,
    pub dual_residual: Option<f64>,
    pub message: Option<String>,
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
            diagnostics: SolverDiagnostics {
                status: TerminationStatus::EmptyInput,
                ..Default::default()
            },
        }
    }

    pub fn failure(
        algorithm: impl Into<String>,
        status: TerminationStatus,
        message: impl Into<String>,
        total_time_secs: f64,
    ) -> Self {
        Self {
            total_time_secs,
            diagnostics: SolverDiagnostics {
                algorithm: algorithm.into(),
                status,
                message: Some(message.into()),
                ..Default::default()
            },
            ..Self::empty()
        }
    }
}
