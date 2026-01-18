//! Specialized solvers for specific problem types.
//!
//! This module contains solvers optimized for specific patterns:
//! - Arbitrage detection and exploitation
//! - Conditional order evaluation
//! - Bundle decomposition (complementary bundle sets)
//! - Chain finding (implication constraint arbitrage)

pub mod arbitrage;
pub mod bundle_decomposer;
pub mod chain_finder;
pub mod conditional;

pub use arbitrage::ArbitrageDetector;
pub use bundle_decomposer::BundleDecomposer;
pub use chain_finder::ChainFinder;
pub use conditional::ConditionalEvaluator;
