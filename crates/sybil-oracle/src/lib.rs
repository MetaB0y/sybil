pub mod attestation;
pub mod error;
pub mod feed;
pub mod policy;
pub mod registry;
pub mod template;
pub mod types;

pub use attestation::{ResolutionAttestation, SignedAttestation};
pub use error::OracleError;
pub use feed::{DataFeed, FeedId, FeedPubkey};
pub use policy::{ResolutionPolicy, evaluate_admin_immediate, evaluate_immediate};
pub use registry::FeedRegistry;
pub use template::{ResolutionTemplate, TemplateId, TemplateRegistry};
pub use types::{MarketStatus, OracleSource, ResolutionRecord};
