use matching_engine::{MarketId, Nanos};

use crate::feed::FeedId;

/// Unique identifier for a resolution proposal.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ProposalId(pub u64);

/// Unique identifier for a challenge.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ChallengeId(pub u64);

/// How the resolution was sourced.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OracleSource {
    /// Admin-initiated resolution (dev mode, governance multisig, etc.)
    Admin,
    /// Signed attestation from a registered data feed.
    DataFeed(FeedId),
    /// Automated L0 oracle (future: price feeds, API oracles).
    AutomatedL0,
}

/// Market lifecycle status tracked by the sequencer.
///
/// This is NOT stored inside `matching-engine`'s `Market` struct — it's
/// managed by the sequencer alongside the market.
///
/// `Proposed`, `Challenged`, and `Voided` are reserved for future policy
/// variants (`Optimistic`, `External`, etc.). The current `Immediate` policy
/// only ever transitions `Active -> Resolved`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum MarketStatus {
    /// Market is open for trading.
    Active,
    /// A resolution has been proposed; challenge window is open.
    Proposed {
        proposal: ResolutionProposal,
        challenge_deadline_ms: u64,
    },
    /// A challenge has been filed; awaiting adjudication.
    Challenged {
        proposal: ResolutionProposal,
        challenge: Challenge,
    },
    /// Market is resolved and settled.
    Resolved { record: ResolutionRecord },
    /// Market is voided (reserved for future use).
    Voided,
}

impl MarketStatus {
    /// Returns true if orders can be placed on this market.
    pub fn is_tradeable(&self) -> bool {
        matches!(self, MarketStatus::Active | MarketStatus::Proposed { .. })
    }

    /// Returns a short string label for API responses.
    pub fn as_str(&self) -> &'static str {
        match self {
            MarketStatus::Active => "active",
            MarketStatus::Proposed { .. } => "proposed",
            MarketStatus::Challenged { .. } => "challenged",
            MarketStatus::Resolved { .. } => "resolved",
            MarketStatus::Voided => "voided",
        }
    }

    /// Returns the YES payout in nanos if the market is resolved.
    pub fn payout_nanos(&self) -> Option<Nanos> {
        match self {
            MarketStatus::Resolved { record } => Some(record.payout_nanos),
            _ => None,
        }
    }

    /// Returns the challenge deadline if the market is in Proposed state.
    pub fn challenge_deadline_ms(&self) -> Option<u64> {
        match self {
            MarketStatus::Proposed {
                challenge_deadline_ms,
                ..
            } => Some(*challenge_deadline_ms),
            _ => None,
        }
    }
}

/// A proposal to resolve a market.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ResolutionProposal {
    pub id: ProposalId,
    pub market_id: MarketId,
    /// Payout per YES share in nanos (0 to NANOS_PER_DOLLAR).
    pub payout_nanos: Nanos,
    pub source: OracleSource,
    pub proposed_at_ms: u64,
    pub reason: Option<String>,
}

/// A challenge against a resolution proposal.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Challenge {
    pub id: ChallengeId,
    /// Account ID of the challenger (plain u64 to avoid sequencer dependency).
    pub challenger: u64,
    pub proposal_id: ProposalId,
    pub bond_amount: Nanos,
    /// Challenger's proposed payout per YES share in nanos.
    pub proposed_payout_nanos: Nanos,
    pub reason: String,
    pub challenged_at_ms: u64,
}

/// Immutable record of a completed resolution.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ResolutionRecord {
    pub market_id: MarketId,
    /// Payout per YES share in nanos (0 to NANOS_PER_DOLLAR).
    /// NO shares receive `NANOS_PER_DOLLAR - payout_nanos`.
    pub payout_nanos: Nanos,
    pub resolved_by: OracleSource,
    pub resolved_at_ms: u64,
    pub proposal: Option<ResolutionProposal>,
    pub challenge: Option<Challenge>,
}
