//! Polymarket resolution actor.
//!
//! Periodically reconciles the Sybil-side view with Polymarket's Gamma API:
//! for each locally-mirrored market, if Polymarket reports a clean binary
//! settlement, produce a signed attestation and POST it to sybil-api via
//! `/v1/markets/:id/resolve`. The signing key is pre-registered on sybil-api
//! as the `polymarket_mirror` feed; so the sequencer verifies the signature
//! against that registered identity before settling.
//!
//! SYB-23 intentionally only handles the "clean binary" case — anything
//! ambiguous (non-binary, UMA-challenged, voided) is skipped with a log line.
//! Richer ingest is tracked in the design doc.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::error::Error;
use crate::mapping::MappingStore;
use crate::polymarket::gamma::GammaClient;
use crate::polymarket::types::GammaMarket;
use crate::signer::ResolutionSigner;
use crate::sybil::client::SybilClient;

/// Periodic resolver. Polls closed Polymarket events, attests to clean
/// binary resolutions, and sends them to sybil-api.
pub struct ResolutionActor {
    config: Config,
    gamma: GammaClient,
    sybil: SybilClient,
    mapping: Arc<RwLock<MappingStore>>,
    signer: ResolutionSigner,
}

impl ResolutionActor {
    pub fn new(
        config: Config,
        gamma: GammaClient,
        sybil: SybilClient,
        mapping: Arc<RwLock<MappingStore>>,
        signer: ResolutionSigner,
    ) -> Self {
        Self {
            config,
            gamma,
            sybil,
            mapping,
            signer,
        }
    }

    pub async fn run(self, cancel: CancellationToken) {
        info!(
            poll_interval_secs = self.config.resolution_poll_interval_secs,
            pubkey = self.signer.pubkey_hex(),
            "ResolutionActor started"
        );

        let interval = Duration::from_secs(self.config.resolution_poll_interval_secs.max(1));
        loop {
            if let Err(e) = self.tick().await {
                warn!(error = %e, "resolution tick failed");
            }
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("ResolutionActor shutting down");
                    return;
                }
                _ = tokio::time::sleep(interval) => {}
            }
        }
    }

    async fn tick(&self) -> Result<(), Error> {
        // Snapshot the condition_id → sybil_market_id map under a short
        // read lock so the main sync loop isn't blocked while we do I/O.
        let mirrors = self
            .mapping
            .read()
            .await
            .all_condition_mappings()
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>();

        if mirrors.is_empty() {
            return Ok(());
        }

        let events = self
            .gamma
            .fetch_closed_events(self.config.max_events)
            .await?;

        let mut resolved = 0usize;
        for event in events {
            for market in event.markets {
                let Some(&sybil_id) = mirrors.get(&market.condition_id) else {
                    continue;
                };
                match self.maybe_resolve(sybil_id, &market).await {
                    Ok(true) => resolved += 1,
                    Ok(false) => {}
                    Err(e) => warn!(sybil_id, error = %e, "failed to resolve market"),
                }
            }
        }
        if resolved > 0 {
            info!(resolved, "settled markets via polymarket_mirror");
        }
        Ok(())
    }

    /// Returns `Ok(true)` if we successfully posted a resolution; `Ok(false)`
    /// if the market was skipped (ambiguous, already resolved, etc.).
    async fn maybe_resolve(
        &self,
        sybil_market_id: u32,
        market: &GammaMarket,
    ) -> Result<bool, Error> {
        let Some(payout_nanos) = market.resolved_payout() else {
            debug!(sybil_market_id, "skipping non-clean polymarket resolution");
            return Ok(false);
        };

        // Skip if already resolved on our side.
        match self.sybil.get_market_resolution(sybil_market_id).await {
            Ok(Some(res)) if res.status == "resolved" => {
                debug!(sybil_market_id, "market already resolved on sybil");
                return Ok(false);
            }
            Ok(_) => {}
            Err(e) => {
                warn!(sybil_market_id, error = %e, "could not query resolution; proceeding");
            }
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let att = self
            .signer
            .sign_attestation(sybil_market_id, payout_nanos, now_ms);

        self.sybil
            .resolve_market_attested(sybil_market_id, payout_nanos, att)
            .await?;

        info!(
            sybil_market_id,
            payout_nanos, "attested and submitted resolution"
        );
        Ok(true)
    }
}
