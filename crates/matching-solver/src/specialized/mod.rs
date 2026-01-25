//! Specialized solvers for specific problem patterns.
//!
//! Contains solvers optimized for detecting arbitrage opportunities.

pub mod arbitrage;
pub mod negrisk;

pub use arbitrage::ArbitrageDetector;
pub use negrisk::{NegriskResult, NegriskSolver};
