pub mod admin;
pub mod error;
pub mod traits;
pub mod types;

pub use admin::AdminOracle;
pub use error::OracleError;
pub use traits::{ChallengeAction, Oracle, ResolutionAction};
pub use types::{
    Challenge, ChallengeId, MarketStatus, OracleSource, ProposalId, ResolutionProposal,
    ResolutionRecord,
};
