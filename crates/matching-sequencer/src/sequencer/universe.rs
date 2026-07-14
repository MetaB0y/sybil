use std::collections::BTreeSet;

use super::*;

impl BlockSequencer {
    /// Stage an exact market allow-list for atomic activation in `height + 1`.
    pub fn activate_liquidity_universe(
        &mut self,
        generation: u64,
        policy_digest: [u8; 32],
        market_ids: Vec<MarketId>,
    ) -> Result<sybil_verifier::LiquidityUniverseSnapshot, SequencerError> {
        let current = self
            .pending_liquidity_universe
            .as_ref()
            .unwrap_or(&self.liquidity_universe);
        if generation != current.generation.saturating_add(1) {
            return Err(SequencerError::LiquidityUniverseGeneration {
                current: current.generation,
                requested: generation,
            });
        }
        if market_ids.is_empty() {
            return Err(SequencerError::LiquidityUniverseEmpty);
        }
        let selected: BTreeSet<_> = market_ids.iter().copied().collect();
        if selected.len() != market_ids.len() {
            return Err(SequencerError::InvalidMarketState(
                "liquidity universe contains duplicate market ids".into(),
            ));
        }
        for market_id in &selected {
            if self.markets.get(*market_id).is_none()
                || !self.lifecycle.market_status(*market_id).is_tradeable()
            {
                return Err(SequencerError::LiquidityUniverseInvalidMarket(*market_id));
            }
        }
        for (group_id, group) in self.market_groups.iter().enumerate() {
            let tradeable_members = group
                .markets
                .iter()
                .filter(|market| self.lifecycle.market_status(**market).is_tradeable())
                .count();
            let selected_count = group
                .markets
                .iter()
                .filter(|market| {
                    self.lifecycle.market_status(**market).is_tradeable()
                        && selected.contains(market)
                })
                .count();
            if selected_count != 0 && selected_count != tradeable_members {
                return Err(SequencerError::LiquidityUniversePartialGroup(
                    group_id as u64,
                ));
            }
        }

        let candidate = crate::universe::LiquidityUniverse {
            generation,
            policy_digest,
            activated_at_height: self.height.saturating_add(1),
            market_ids: selected,
        };
        let snapshot = candidate.snapshot();
        self.pending_liquidity_universe = Some(candidate);
        self.record_system_event(SystemEvent::LiquidityUniverseActivated {
            generation,
            policy_digest,
            activated_at_height: snapshot.activated_at_height,
            market_ids: snapshot.market_ids.clone(),
        });
        Ok(snapshot)
    }

    pub fn liquidity_universe(&self) -> sybil_verifier::LiquidityUniverseSnapshot {
        let mut snapshot = self.liquidity_universe.snapshot();
        snapshot
            .market_ids
            .retain(|market| self.lifecycle.market_status(*market).is_tradeable());
        snapshot
    }

    pub fn committed_liquidity_universe(&self) -> sybil_verifier::LiquidityUniverseSnapshot {
        self.liquidity_universe.snapshot()
    }

    pub fn market_is_universe_active(&self, market_id: MarketId) -> bool {
        self.liquidity_universe.permits(market_id)
            && self.lifecycle.market_status(market_id).is_tradeable()
    }
}
