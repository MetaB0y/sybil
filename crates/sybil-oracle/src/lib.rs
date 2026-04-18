pub mod admin;
pub mod attestation;
pub mod error;
pub mod feed;
pub mod policy;
pub mod registry;
pub mod template;
pub mod traits;
pub mod types;

pub use admin::AdminOracle;
pub use attestation::{ResolutionAttestation, SignedAttestation};
pub use error::OracleError;
pub use feed::{DataFeed, FeedId, FeedPubkey};
pub use policy::{evaluate_immediate, PolicyOutcome, ResolutionPolicy};
pub use registry::FeedRegistry;
pub use template::{ResolutionTemplate, TemplateId, TemplateRegistry};
pub use traits::{ChallengeAction, Oracle, ResolutionAction};
pub use types::{
    Challenge, ChallengeId, MarketStatus, OracleSource, ProposalId, ResolutionProposal,
    ResolutionRecord,
};
