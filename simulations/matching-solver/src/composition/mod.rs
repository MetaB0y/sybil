//! Solver composition and problem decomposition system.
//!
//! This module provides infrastructure for composing multiple solvers,
//! decomposing problems into smaller clusters, and merging partial solutions.
//!
//! # Architecture
//!
//! ```text
//! Problem
//!    │
//!    ▼
//! ┌─────────────────┐
//! │ ProblemAnalyzer │ → MarketGraph, ClusterInfo, OrderClassification
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │  Decomposer     │ → SubProblems[], BridgingOrders[]
//! └────────┬────────┘
//!          │
//!     ┌────┴────┬────────────┐
//!     ▼         ▼            ▼
//! ┌────────┐ ┌────────┐ ┌──────────┐
//! │Cluster │ │Cluster │ │ Bridging │
//! │Solver 1│ │Solver 2│ │  Solver  │
//! └───┬────┘ └───┬────┘ └────┬─────┘
//!     │          │           │
//!     └────┬─────┴───────────┘
//!          ▼
//! ┌─────────────────┐
//! │ SolutionMerger  │ → Resolve conflicts, validate liquidity
//! └────────┬────────┘
//!          │
//!          ▼
//!     MatchingResult
//! ```

pub mod analysis;
pub mod builder;
pub mod cluster;
pub mod composite;
pub mod merge;
pub mod partial;

pub use analysis::{ClusterInfo, MarketGraph, OrderClassification, ProblemAnalysis};
pub use builder::SolverBuilder;
pub use cluster::{Decomposer, SubProblem};
pub use composite::CompositeSolver;
pub use merge::SolutionMerger;
pub use partial::{PartialSolution, SolutionConfidence};
