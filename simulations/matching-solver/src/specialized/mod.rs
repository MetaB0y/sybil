//! Specialized solvers for specific problem types.
//!
//! This module contains solvers optimized for specific patterns:
//! - Arbitrage detection and exploitation
//! - Conditional order evaluation

pub mod arbitrage;
pub mod conditional;

pub use arbitrage::ArbitrageDetector;
pub use conditional::ConditionalEvaluator;
