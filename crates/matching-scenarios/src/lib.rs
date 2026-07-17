//! Scenario generators for matching engine testing.
//!
//! # Unified Scenarios
//!
//! The [`scenario`] module provides a single configurable generator with presets:
//!
//! - `ScenarioConfig::quick()` - Fast tests (~50 orders)
//! - `ScenarioConfig::small()` - Unit tests (~300 orders)
//! - `ScenarioConfig::medium()` - Integration tests (~3000 orders)
//! - `ScenarioConfig::large()` - Stress tests (~10000 orders)
//! - `ScenarioConfig::extreme()` - Benchmarking (~100000 orders)
//! - `ScenarioConfig::milp_killer()` - Forces MILP timeout
//!
//! # Simple Random Scenarios
//!
//! The [`random`] module provides simpler random scenarios for basic testing.

pub mod flash_liquidity;
pub mod random;
pub mod scenario;

// Re-export Problem from matching-engine
pub use matching_engine::{Problem, ProblemSummary};

// Re-export unified scenario generator (primary API)
pub use scenario::{MmQuoteStyle, ScenarioConfig, generate_scenario};

pub use flash_liquidity::{FlashLiquidityConfig, generate_flash_liquidity_scenario};

// Re-export simple random generator
pub use random::{RandomConfig, generate_random_scenario};
