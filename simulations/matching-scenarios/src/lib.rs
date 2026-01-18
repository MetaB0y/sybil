//! Scenario generators for NP-hard matching.

pub mod presidential;
pub mod tournament;
pub mod random;

// Re-export Problem from matching-engine
pub use matching_engine::{Problem, ProblemSummary};

// Re-export scenario generators
pub use presidential::{generate_presidential_scenario, PresidentialConfig};
pub use tournament::{generate_tournament_scenario, TournamentConfig};
pub use random::{generate_random_scenario, RandomConfig};
