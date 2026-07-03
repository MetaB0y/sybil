//! Market lifecycle management: statuses, oracle integration, metadata,
//! feed + template registries.

use std::collections::HashMap;
use std::sync::Arc;

use matching_engine::{MarketId, Nanos};
use sybil_oracle::{
    DataFeed, FeedId, FeedPubkey, FeedRegistry, MarketStatus, Oracle, OracleError, PolicyOutcome,
    ResolutionAction, ResolutionPolicy, ResolutionTemplate, SignedAttestation, TemplateRegistry,
};

use crate::error::SequencerError;
use crate::market_info::MarketMetadata;

/// Name that the default `Immediate { feed_id: admin }` template is wired
/// under. Markets without an explicit `resolution_config` resolve via this.
pub const ADMIN_IMMEDIATE_TEMPLATE: &str = "admin_immediate";

/// Manages market lifecycle: status tracking, oracle resolution, metadata,
/// and the feed + template registries that underpin attestation-based
/// resolution.
///
/// Does NOT own accounts or market_groups — those remain on BlockSequencer.
/// Resolution works in two steps: lifecycle decides (via oracle or policy),
/// caller executes (settles positions, updates groups).
#[derive(Clone)]
pub struct MarketLifecycle {
    /// Oracle-managed lifecycle status per market.
    market_statuses: HashMap<MarketId, MarketStatus>,
    /// Legacy pluggable oracle. Still used by the unsigned admin path.
    oracle: Arc<dyn Oracle>,
    /// Market metadata (sequencer-layer, not in matching-engine).
    market_metadata: HashMap<MarketId, MarketMetadata>,
    /// Registry of off-chain signer identities (feeds).
    feeds: FeedRegistry,
    /// System-wired resolution templates (not persisted).
    templates: TemplateRegistry,
}

impl MarketLifecycle {
    pub fn new(oracle: Arc<dyn Oracle>) -> Self {
        Self {
            market_statuses: HashMap::new(),
            oracle,
            market_metadata: HashMap::new(),
            feeds: FeedRegistry::new(),
            templates: TemplateRegistry::new(),
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

    // --- Feed registry ---

    pub fn feeds(&self) -> &FeedRegistry {
        &self.feeds
    }

    pub fn register_feed(&mut self, pubkey: FeedPubkey, name: String, now_ms: u64) -> FeedId {
        self.feeds.register(pubkey, name, now_ms)
    }

    pub fn feed_by_id(&self, id: FeedId) -> Option<&DataFeed> {
        self.feeds.get(id)
    }

    pub fn feed_by_pubkey(&self, pubkey: &FeedPubkey) -> Option<&DataFeed> {
        self.feeds.resolve_pubkey(pubkey)
    }

    /// Used by the store loader to rehydrate persisted feeds verbatim.
    pub fn restore_feed(&mut self, feed: DataFeed) {
        self.feeds.restore(feed);
    }

    // --- Template registry ---

    pub fn templates(&self) -> &TemplateRegistry {
        &self.templates
    }

    pub fn install_template(&mut self, template: ResolutionTemplate) {
        self.templates.install(template);
    }

    pub fn template_for_market(&self, market_id: MarketId) -> &str {
        self.market_metadata
            .get(&market_id)
            .and_then(|m| m.resolution_config.as_ref())
            .map(|cfg| cfg.template.as_str())
            .unwrap_or(ADMIN_IMMEDIATE_TEMPLATE)
    }

    /// Consult the oracle and update status. Returns the action for the caller to execute.
    ///
    /// The caller (BlockSequencer) is responsible for acting on the result:
    /// - `SettleNow` → settle positions, shrink affected market groups
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

        self.apply_action(market_id, &action, timestamp_ms);
        Ok(action)
    }

    /// Resolve a market from a signed attestation, dispatched through the
    /// template's [`ResolutionPolicy`]. Signature verification happens in the
    /// caller (sequencer actor) — by the time we're here, identity is proven
    /// and this function is pure state-machine logic.
    pub fn resolve_from_attestation(
        &mut self,
        market_id: MarketId,
        signed: &SignedAttestation,
        timestamp_ms: u64,
    ) -> Result<ResolutionAction, SequencerError> {
        let template_name = self.template_for_market(market_id).to_string();
        let template = self
            .templates
            .get_str(&template_name)
            .ok_or_else(|| {
                SequencerError::OracleError(
                    OracleError::UnknownTemplate(template_name.clone()).to_string(),
                )
            })?
            .clone();

        let current_status = self.market_status(market_id);

        let outcome = match template.policy {
            ResolutionPolicy::Immediate { feed_id } => {
                // The signer pubkey must map to an already-registered feed.
                let signer_feed = self.feeds.resolve_pubkey(&signed.signer).ok_or_else(|| {
                    SequencerError::OracleError(OracleError::UnknownFeed.to_string())
                })?;
                sybil_oracle::evaluate_immediate(
                    feed_id,
                    signer_feed,
                    signed,
                    &current_status,
                    timestamp_ms,
                )
                .map_err(|e| SequencerError::OracleError(e.to_string()))?
            }
        };

        let action = match outcome {
            PolicyOutcome::Settle { record } => ResolutionAction::SettleNow {
                market_id,
                payout_nanos: record.payout_nanos,
                record,
            },
            PolicyOutcome::Reject { reason } => ResolutionAction::Reject { reason },
        };

        self.apply_action(market_id, &action, timestamp_ms);
        Ok(action)
    }

    fn apply_action(
        &mut self,
        fallback_market_id: MarketId,
        action: &ResolutionAction,
        timestamp_ms: u64,
    ) {
        match action {
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
                    fallback_market_id,
                    MarketStatus::Proposed {
                        proposal: proposal.clone(),
                        challenge_deadline_ms: deadline,
                    },
                );
            }
            ResolutionAction::Reject { .. } => {}
        }
    }
}
