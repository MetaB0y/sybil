//! Market lifecycle management: statuses, oracle integration, metadata.

use std::collections::HashMap;
use std::sync::Arc;

use matching_engine::{MarketId, Nanos};
use sybil_oracle::{MarketStatus, Oracle, ResolutionAction};

use crate::error::SequencerError;
use crate::market_info::MarketMetadata;

/// Manages market lifecycle: status tracking, oracle resolution, metadata.
///
/// Does NOT own accounts or market_groups — those remain on BlockSequencer.
/// Resolution works in two steps: lifecycle decides (via oracle), caller executes
/// (settles positions, updates groups). This avoids borrow checker awkwardness.
#[derive(Clone)]
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

    pub fn set_market_status(&mut self, market_id: MarketId, status: MarketStatus) {
        self.market_statuses.insert(market_id, status);
    }

    pub fn set_market_metadata(&mut self, market_id: MarketId, metadata: MarketMetadata) {
        self.market_metadata.insert(market_id, metadata);
    }

    pub fn market_metadata(&self, market_id: MarketId) -> Option<&MarketMetadata> {
        self.market_metadata.get(&market_id)
    }

    pub fn market_metadata_all(&self) -> &HashMap<MarketId, MarketMetadata> {
        &self.market_metadata
    }

    pub fn oracle(&self) -> Arc<dyn Oracle> {
        self.oracle.clone()
    }

    /// Consult the oracle and update status. Returns the action for the caller to execute.
    ///
    /// The caller (BlockSequencer) is responsible for acting on the result:
    /// - `SettleNow` → settle positions, remove from market groups
    /// - `Propose` → no action needed (status already updated here)
    /// - `Reject` → returned as error
    pub fn resolve_market(
        &mut self,
        market_id: MarketId,
        payout_nanos: Nanos,
        timestamp_ms: u64,
    ) -> Result<ResolutionAction, SequencerError> {
        let current_status = self.market_status(market_id);
        let action = self
            .oracle
            .resolve(market_id, payout_nanos, &current_status, timestamp_ms)
            .map_err(|e| SequencerError::OracleError(e.to_string()))?;

        // Update status based on oracle decision
        match &action {
            ResolutionAction::SettleNow {
                market_id, record, ..
            } => {
                self.market_statuses.insert(
                    *market_id,
                    MarketStatus::Resolved {
                        record: record.clone(),
                    },
                );
            }
            ResolutionAction::Propose {
                proposal,
                challenge_window_ms,
            } => {
                let deadline = timestamp_ms + challenge_window_ms;
                self.market_statuses.insert(
                    market_id,
                    MarketStatus::Proposed {
                        proposal: proposal.clone(),
                        challenge_deadline_ms: deadline,
                    },
                );
            }
            ResolutionAction::Reject { .. } => {}
        }

        Ok(action)
    }
}
