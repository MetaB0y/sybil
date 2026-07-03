//! Signed resolution attestations.
//!
//! An attestation is the signed claim "at time T, market M resolves with
//! payout P". The sequencer accepts one only if it verifies against a
//! registered feed's pubkey. This is the only channel external signers have
//! into the enclave.

use matching_engine::{MarketId, Nanos};
use serde::{Deserialize, Serialize};

use crate::feed::FeedPubkey;

/// Payload signed by an off-chain resolver. Kept canonical in
/// `sybil-signing::canonical_attestation_bytes`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionAttestation {
    pub market_id: MarketId,
    pub payout_nanos: Nanos,
    /// Intended as `timestamp_ms` of the signer; replay is rejected solely by
    /// the sequencer's `AlreadyResolved` check, not by a replay set.
    pub nonce: u64,
}

/// A `ResolutionAttestation` plus the signer identity and ECDSA signature.
///
/// Represented here purely as bytes so this crate doesn't need to depend on
/// `p256`. The sequencer converts to `p256::ecdsa::Signature` + `VerifyingKey`
/// before verification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedAttestation {
    pub attestation: ResolutionAttestation,
    pub signer: FeedPubkey,
    /// DER-encoded P256 ECDSA signature.
    pub signature_der: Vec<u8>,
}
