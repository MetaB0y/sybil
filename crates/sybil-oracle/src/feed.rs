//! DataFeed primitive: the only identity that can sign resolution attestations.
//!
//! A feed is a (pubkey, human name) pair registered in the sequencer's feed
//! registry. Every resolution policy cites feeds by id; the enclave does not
//! know or care how a feed obtains its data. External signers (Polymarket
//! mirror, future LLM resolver, future UMA bridge) all plug in as feeds.

use serde::{Deserialize, Serialize};

/// Compressed SEC1 P256 public key bytes (33 bytes). Used as the serde
/// representation of a feed's public key, since `crate::types::PublicKey`
/// (p256::VerifyingKey) lives in `matching-sequencer`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FeedPubkey(pub Vec<u8>);

/// Monotonic identifier for a [`DataFeed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FeedId(pub u64);

/// Registered off-chain identity allowed to sign resolution attestations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataFeed {
    pub id: FeedId,
    pub pubkey: FeedPubkey,
    pub name: String,
    pub created_at_ms: u64,
}
