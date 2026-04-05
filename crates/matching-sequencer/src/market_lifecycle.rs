//! Market lifecycle management: statuses, oracle integration, metadata.

use std::collections::HashMap;
use std::sync::Arc;

use matching_engine::{MarketGroup, MarketId, MarketSet, Nanos};
use sybil_oracle::{MarketStatus, Oracle, ResolutionAction, ResolutionRecord};

use crate::account::AccountStore;
use crate::error::SequencerError;
use crate::market_info::MarketMetadata;
use crate::settlement;

/// Manages market lifecycle: status tracking, oracle resolution, metadata.
pub struct MarketLifecycle {
    /// Oracle-managed lifecycle status per market.
    market_statuses: HashMap<MarketId, MarketStatus>,
    /// Pluggable oracle for resolution decisions.
    oracle: Arc<dyn Oracle>,
    /// Market metadata (sequencer-layer, not in matching-engine).
    market_metadata: HashMap<MarketId, MarketMetadata>,
}

impl MarketLifecycle {
    pub fn new(oracle: Arc<dyn Oracle>) -> Self {
        Self {
            market_statuses: HashMap::new(),
            oracle,
            market_metadata: HashMap::new(),
        }
    }

    pub fn market_status(&self, id: MarketId) -> MarketStatus {
        self.market_statuses
            .get(&id)
            .cloned()
            .unwrap_or(MarketStatus::Active)
    }

    pub fn market_statuses(&self) -> &HashMap<MarketId, MarketStatus> {
        &self.market_statuses
    }

    pub fn set_market_metadata(&mut self, market_id: MarketId, metadata: MarketMetadata) {
        self.market_metadata.insert(market_id, metadata);
    }

    pub fn market_metadata(&self, market_id: MarketId) -> Option<&MarketMetadata> {
        self.market_metadata.get(&market_id)
    }

    /// Resolve a market through the oracle.
    ///
    /// Takes mutable borrows of accounts and market_groups as parameters
    /// to avoid holding them inside this struct (which would conflict with
    /// borrow checker when produce_block() needs both).
    pub fn resolve_market(
        &mut self,
        market_id: MarketId,
        payout_nanos: Nanos,
        accounts: &mut AccountStore,
        markets: &MarketSet,
        market_groups: &mut Vec<MarketGroup>,
        timestamp_ms: u64,
    ) -> Result<ResolutionRecord, SequencerError> {
        // Verify market exists
        if markets.get(market_id).is_none() {
            return Err(SequencerError::MarketNotFound);
        }

        let current_status = self.market_status(market_id);
        let action = self
            .oracle
            .resolve(market_id, payout_nanos, &current_status, timestamp_ms)
            .map_err(|e| SequencerError::OracleError(e.to_string()))?;

        match action {
            ResolutionAction::SettleNow {
                market_id,
                payout_nanos,
                record,
            } => {
                settlement::resolve_market(accounts, market_id, payout_nanos);
                market_groups.retain(|g| !g.markets.contains(&market_id));
                self.market_statuses.insert(
                    market_id,
                    MarketStatus::Resolved {
                        record: record.clone(),
                    },
                );
                Ok(record)
            }
            ResolutionAction::Propose {
                proposal,
                challenge_window_ms,
            } => {
                let deadline = timestamp_ms + challenge_window_ms;
                self.market_statuses.insert(
                    market_id,
                    MarketStatus::Proposed {
                        proposal,
                        challenge_deadline_ms: deadline,
                    },
                );
                Err(SequencerError::OracleError(
                    "resolution proposed but not yet settled".to_string(),
                ))
            }
            ResolutionAction::Reject { reason } => Err(SequencerError::OracleError(reason)),
        }
    }
}
