use std::collections::BTreeSet;

use matching_engine::MarketId;

/// Exact validity allow-list for new order admission.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LiquidityUniverse {
    pub generation: u64,
    pub policy_digest: [u8; 32],
    pub activated_at_height: u64,
    pub market_ids: BTreeSet<MarketId>,
}

impl LiquidityUniverse {
    /// Bootstrap generation preserves legacy public trading while actors fail closed.
    pub fn permits(&self, market_id: MarketId) -> bool {
        self.generation == 0 || self.market_ids.contains(&market_id)
    }

    pub fn snapshot(&self) -> sybil_verifier::LiquidityUniverseSnapshot {
        sybil_verifier::LiquidityUniverseSnapshot {
            generation: self.generation,
            policy_digest: self.policy_digest,
            activated_at_height: self.activated_at_height,
            market_ids: self.market_ids.iter().copied().collect(),
        }
    }

    pub fn from_snapshot(snapshot: sybil_verifier::LiquidityUniverseSnapshot) -> Self {
        Self {
            generation: snapshot.generation,
            policy_digest: snapshot.policy_digest,
            activated_at_height: snapshot.activated_at_height,
            market_ids: snapshot.market_ids.into_iter().collect(),
        }
    }
}
