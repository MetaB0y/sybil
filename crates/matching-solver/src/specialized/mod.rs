//! Specialized solvers for specific problem patterns.

pub mod multi_market;
pub mod negrisk;

pub use multi_market::MultiMarketSolver;
pub use negrisk::{NegriskResult, NegriskSolver};
