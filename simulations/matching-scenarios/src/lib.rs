//! Scenario generators for NP-hard matching.
//!
//! # Standard Scenarios
//!
//! - [`presidential`]: 2024 US Presidential election markets
//! - [`tournament`]: Tournament/sports bracket markets
//! - [`random`]: Configurable random hard instances
//!
//! # Complex Scenarios
//!
//! The [`complex`] module provides specialized test scenarios:
//!
//! - Nested bundles with overlapping markets
//! - Conditional order chains (A triggers B triggers C)
//! - Deep implication hierarchies
//! - Liquidity cliffs with price discontinuities
//! - Adversarial competing orders
//! - Large interconnected market networks
//!
//! # Stress Scenarios
//!
//! The [`stress`] module provides large-scale scenarios for solver testing:
//!
//! - Mega scenarios with 500-5000 orders
//! - Combined scenarios merging multiple scenario types

pub mod complex;
pub mod presidential;
pub mod random;
pub mod stress;
pub mod tournament;

// Re-export Problem from matching-engine
pub use matching_engine::{Problem, ProblemSummary};

// Re-export scenario generators
pub use presidential::{generate_presidential_scenario, PresidentialConfig};
pub use random::{generate_random_scenario, RandomConfig};
pub use tournament::{generate_tournament_scenario, TournamentConfig};

// Re-export complex scenarios
pub use complex::{
    generate_adversarial_scenario, generate_conditional_chain_scenario,
    generate_deep_implication_scenario, generate_large_interconnected_scenario,
    generate_liquidity_cliff_scenario, generate_nested_bundle_scenario, AdversarialConfig,
    ConditionalChainConfig, DeepImplicationConfig, LargeInterconnectedConfig, LiquidityCliffConfig,
    NestedBundleConfig,
};

// Re-export stress scenarios
pub use stress::{generate_mega_scenario, generate_combined_scenario, MegaScenarioConfig};
