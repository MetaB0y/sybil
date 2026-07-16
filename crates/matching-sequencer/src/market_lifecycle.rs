//! Market lifecycle management: statuses, metadata, feeds, and resolution
//! templates.

use matching_engine::{MarketId, Nanos};
use std::collections::HashMap;
use sybil_oracle::{
    DataFeed, FeedId, FeedPubkey, FeedRegistry, MarketStatus, OracleError, ResolutionPolicy,
    ResolutionRecord, ResolutionTemplate, SignedAttestation, TemplateRegistry,
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
#[derive(Clone, Default)]
pub struct MarketLifecycle {
    /// Canonical lifecycle status per market.
    market_statuses: HashMap<MarketId, MarketStatus>,
    /// Market metadata (sequencer-layer, not in matching-engine).
    market_metadata: HashMap<MarketId, MarketMetadata>,
    /// Registry of off-chain signer identities (feeds).
    feeds: FeedRegistry,
    /// System-wired resolution templates (not persisted).
    templates: TemplateRegistry,
}

impl MarketLifecycle {
    pub fn new() -> Self {
        Self::default()
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

    /// Apply the trusted admin immediate policy and commit the resulting status.
    pub fn resolve_market(
        &mut self,
        market_id: MarketId,
        payout_nanos: Nanos,
        timestamp_ms: u64,
    ) -> Result<ResolutionRecord, SequencerError> {
        let current_status = self.market_status(market_id);
        let record =
            sybil_oracle::evaluate_admin_immediate(payout_nanos, &current_status, timestamp_ms)
                .map_err(|e| SequencerError::OracleError(e.to_string()))?;
        self.commit_resolution(market_id, record.clone());
        Ok(record)
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
    ) -> Result<ResolutionRecord, SequencerError> {
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

        let record = match template.policy {
            ResolutionPolicy::Immediate { feed_id } => {
                // The signer pubkey must map to an already-registered feed.
                let signer_feed = self.feeds.resolve_pubkey(&signed.signer).ok_or_else(|| {
                    SequencerError::OracleError(OracleError::UnknownFeed.to_string())
                })?;
                sybil_oracle::evaluate_immediate(
                    feed_id,
                    signer_feed,
                    market_id,
                    signed,
                    &current_status,
                    timestamp_ms,
                )
                .map_err(|e| SequencerError::OracleError(e.to_string()))?
            }
        };

        self.commit_resolution(market_id, record.clone());
        Ok(record)
    }

    fn commit_resolution(&mut self, market_id: MarketId, record: ResolutionRecord) {
        self.market_statuses
            .insert(market_id, MarketStatus::Resolved { record });
    }
}
