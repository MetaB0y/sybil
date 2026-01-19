//! Scenario generators for NP-hard matching.
//!
//! # Standard Scenarios
//!
//! - [`random`]: Configurable random hard instances
//! - [`mega`]: Comprehensive scenario with multi-outcome markets and MM constraints
//!
//! # Stress Scenarios
//!
//! The [`stress`] module provides large-scale scenarios for solver testing:
//!
//! - Mega scenarios with 500-5000 orders
//! - Combined scenarios merging multiple scenario types

pub mod mega;
pub mod random;
pub mod stress;

// Re-export Problem from matching-engine
pub use matching_engine::{Problem, ProblemSummary};

// Re-export scenario generators
pub use random::{generate_random_scenario, RandomConfig};

// Re-export stress scenarios
pub use stress::{
    generate_mega_scenario, generate_combined_scenario, generate_milp_killer_scenario,
    MegaScenarioConfig, MilpKillerConfig,
};

// Re-export new mega scenario
pub use mega::{
    generate_mega_scenario_v2, MegaScenarioConfigV2, MmStrategy, PriceDistribution,
};
