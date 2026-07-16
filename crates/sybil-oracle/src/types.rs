use matching_engine::Nanos;

use crate::feed::FeedId;

/// How the resolution was sourced.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OracleSource {
    /// Admin-initiated resolution (dev mode, governance multisig, etc.)
    Admin,
    /// Signed attestation from a registered data feed.
    DataFeed(FeedId),
}

/// Market lifecycle status tracked by the sequencer.
///
/// This is NOT stored inside `matching-engine`'s `Market` struct — it's
/// managed by the sequencer alongside the market.
///
/// The implemented immediate policy has exactly one irreversible transition:
/// `Active -> Resolved`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum MarketStatus {
    /// Market is open for trading.
    Active,
    /// Market is resolved and settled.
    Resolved { record: ResolutionRecord },
}

impl MarketStatus {
    /// Returns true if orders can be placed on this market.
    pub fn is_tradeable(&self) -> bool {
        matches!(self, MarketStatus::Active)
    }

    /// Returns a short string label for API responses.
    pub fn as_str(&self) -> &'static str {
        match self {
            MarketStatus::Active => "active",
            MarketStatus::Resolved { .. } => "resolved",
        }
    }

    /// Returns the YES payout in nanos if the market is resolved.
    pub fn payout_nanos(&self) -> Option<Nanos> {
        match self {
            MarketStatus::Resolved { record } => Some(record.payout_nanos),
            _ => None,
        }
    }
}

/// Immutable record of a completed resolution.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ResolutionRecord {
    /// Payout per YES share in nanos (0 to NANOS_PER_DOLLAR).
    /// NO shares receive `NANOS_PER_DOLLAR - payout_nanos`.
    pub payout_nanos: Nanos,
    pub resolved_by: OracleSource,
    pub resolved_at_ms: u64,
}
