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

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::error::Error;
use crate::mapping::MappingStore;
use crate::polymarket::gamma::GammaClient;
use crate::polymarket::types::{GammaEvent, GammaMarket};
use crate::signer::ResolutionSigner;
use sybil_api_types::SetMarketMetadataRequest;
use sybil_client::SybilClient;

const CONDITION_IDS_PER_REQUEST: usize = 50;
const CONDITION_CHUNKS_PER_TICK: usize = 4;

/// Periodic resolver. Polls closed Polymarket events, attests to clean
/// binary resolutions, and sends them to sybil-api.
pub struct ResolutionActor {
    config: Config,
    gamma: GammaClient,
    sybil: SybilClient,
    mapping: Arc<RwLock<MappingStore>>,
    signer: ResolutionSigner,
    /// Condition ids already pushed `closed: true` this process lifetime, so we
    /// write the off-block flag once per market instead of every tick. In-memory
    /// only — a restart re-flags each once via this path (and the sync actor's
    /// first-sync backfill), which is harmless and idempotent on the API side.
    flagged_closed: Mutex<HashSet<String>>,
    /// Round-robin cursor through locally mirrored condition ids. This keeps
    /// exhaustive reconciliation bounded per tick while guaranteeing markets
    /// outside Gamma's top closed-events window are still revisited.
    condition_poll_cursor: Mutex<usize>,
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
            flagged_closed: Mutex::new(HashSet::new()),
            condition_poll_cursor: Mutex::new(0),
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
            .collect::<HashMap<_, _>>();

        if mirrors.is_empty() {
            return Ok(());
        }

        let events = self
            .gamma
            .fetch_closed_events(self.config.max_events)
            .await
            .unwrap_or_else(|e| {
                warn!(error = %e, "closed-events fast path failed; continuing with mirrored condition poll");
                Vec::new()
            });

        let mut condition_ids: Vec<String> = mirrors.keys().cloned().collect();
        condition_ids.sort();
        let polled_condition_ids = self.next_condition_poll_ids(&condition_ids);
        let condition_markets = self
            .gamma
            .fetch_markets_by_condition_ids(&polled_condition_ids)
            .await?;
        let markets = mapped_resolution_markets(&events, condition_markets, &mirrors);

        // Mark-once: push `closed: true` off-block for any mapped market
        // Polymarket reports closed, so the frontend hides/greys it. Independent
        // of settlement (`maybe_resolve` only handles clean binary payouts);
        // this fires for every close, clean or not. Skips ids already flagged
        // this lifetime so we don't re-POST every tick.
        let to_flag = {
            let flagged = self.flagged_closed.lock().expect("flagged_closed poisoned");
            pending_close_flags_for_markets(&markets, &mirrors, &flagged)
        };
        if !to_flag.is_empty() {
            let req = SetMarketMetadataRequest {
                closed: Some(true),
                ..Default::default()
            };
            for (sybil_id, condition_id) in to_flag {
                match self.sybil.set_market_metadata(sybil_id, &req).await {
                    Ok(()) => {
                        self.flagged_closed
                            .lock()
                            .expect("flagged_closed poisoned")
                            .insert(condition_id);
                    }
                    Err(e) => {
                        warn!(sybil_id, error = %e, "failed to flag market closed")
                    }
                }
            }
        }

        let mut resolved = 0usize;
        for market in markets {
            let Some(&sybil_id) = mirrors.get(&market.condition_id) else {
                continue;
            };
            match self.maybe_resolve(sybil_id, &market).await {
                Ok(true) => resolved += 1,
                Ok(false) => {}
                Err(e) => warn!(sybil_id, error = %e, "failed to resolve market"),
            }
        }
        if resolved > 0 {
            info!(resolved, "settled markets via polymarket_mirror");
        }
        Ok(())
    }

    fn next_condition_poll_ids(&self, condition_ids: &[String]) -> Vec<String> {
        if condition_ids.is_empty() {
            return Vec::new();
        }

        let max_ids = CONDITION_IDS_PER_REQUEST * CONDITION_CHUNKS_PER_TICK;
        let count = condition_ids.len().min(max_ids);
        let mut cursor = self
            .condition_poll_cursor
            .lock()
            .expect("condition_poll_cursor poisoned");
        let start = *cursor % condition_ids.len();
        let ids = (0..count)
            .map(|offset| condition_ids[(start + offset) % condition_ids.len()].clone())
            .collect();
        *cursor = (start + count) % condition_ids.len();
        ids
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

/// Pure decision: which mapped markets in `events` still need their off-block
/// `closed` flag written. A market qualifies when Polymarket reports it
/// `closed`, it is in the `mirrors` map (condition_id -> sybil_market_id), and
/// it has not already been flagged this process lifetime. Returns
/// `(sybil_market_id, condition_id)` pairs. No I/O — unit-tested in isolation.
#[cfg(test)]
fn pending_close_flags(
    events: &[GammaEvent],
    mirrors: &HashMap<String, u32>,
    already_flagged: &HashSet<String>,
) -> Vec<(u32, String)> {
    let markets: Vec<_> = events
        .iter()
        .flat_map(|event| event.markets.iter().cloned())
        .collect();
    pending_close_flags_for_markets(&markets, mirrors, already_flagged)
}

fn pending_close_flags_for_markets(
    markets: &[GammaMarket],
    mirrors: &HashMap<String, u32>,
    already_flagged: &HashSet<String>,
) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for market in markets {
        if !market.closed {
            continue;
        }
        let Some(&sybil_id) = mirrors.get(&market.condition_id) else {
            continue;
        };
        if already_flagged.contains(&market.condition_id) {
            continue;
        }
        out.push((sybil_id, market.condition_id.clone()));
    }
    out
}

fn mapped_resolution_markets(
    closed_events: &[GammaEvent],
    condition_markets: Vec<GammaMarket>,
    mirrors: &HashMap<String, u32>,
) -> Vec<GammaMarket> {
    let mut by_condition = HashMap::new();
    for event in closed_events {
        for market in &event.markets {
            if mirrors.contains_key(&market.condition_id) {
                by_condition.insert(market.condition_id.clone(), market.clone());
            }
        }
    }
    for market in condition_markets {
        if mirrors.contains_key(&market.condition_id) {
            by_condition.insert(market.condition_id.clone(), market);
        }
    }
    by_condition.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::{mapped_resolution_markets, pending_close_flags, pending_close_flags_for_markets};
    use crate::polymarket::types::{GammaEvent, GammaMarket};
    use std::collections::{HashMap, HashSet};
    use sybil_api_types::NANOS_PER_DOLLAR;

    fn market(condition_id: &str, closed: bool) -> GammaMarket {
        GammaMarket {
            condition_id: condition_id.into(),
            question: "Q?".into(),
            outcomes: String::new(),
            outcome_prices: String::new(),
            clob_token_ids: String::new(),
            active: !closed,
            closed,
            neg_risk: false,
            group_item_title: None,
            best_bid: None,
            best_ask: None,
            last_trade_price: None,
            volume: None,
            liquidity: None,
            slug: None,
            description: None,
            start_date: None,
            end_date: None,
            resolution_source: None,
            image: None,
            icon: None,
            umared: None,
            resolved_by: None,
            extra: Default::default(),
        }
    }

    fn resolved_market(condition_id: &str, yes_wins: bool) -> GammaMarket {
        let mut market = market(condition_id, true);
        market.outcome_prices = if yes_wins {
            serde_json::to_string(&["1", "0"]).unwrap()
        } else {
            serde_json::to_string(&["0", "1"]).unwrap()
        };
        market
    }

    fn event(markets: Vec<GammaMarket>) -> GammaEvent {
        GammaEvent {
            id: "e1".into(),
            title: "T".into(),
            description: String::new(),
            slug: String::new(),
            active: false,
            closed: true,
            enable_neg_risk: false,
            neg_risk: false,
            markets,
            tags: Vec::new(),
            volume: None,
            liquidity: None,
            start_date: None,
            end_date: None,
            created_at: None,
            image: None,
            icon: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn flags_mapped_closed_markets_once() {
        let events = vec![event(vec![
            market("0xaaa", true),  // mapped + closed -> flag
            market("0xbbb", false), // mapped but still open -> skip
            market("0xccc", true),  // closed but NOT mapped -> skip
        ])];
        let mut mirrors = HashMap::new();
        mirrors.insert("0xaaa".to_string(), 10u32);
        mirrors.insert("0xbbb".to_string(), 11u32);
        let already = HashSet::new();

        let out = pending_close_flags(&events, &mirrors, &already);
        assert_eq!(out, vec![(10u32, "0xaaa".to_string())]);

        // Once 0xaaa is flagged, it is not returned again.
        let already: HashSet<String> = ["0xaaa".to_string()].into_iter().collect();
        let out = pending_close_flags(&events, &mirrors, &already);
        assert!(out.is_empty());
    }

    #[test]
    fn reconciles_mirrored_condition_outside_closed_events_window() {
        let closed_events = vec![event(vec![resolved_market("0xtop", true)])];
        let condition_markets = vec![resolved_market("0xoff", true)];
        let mirrors = HashMap::from([("0xoff".to_string(), 42u32)]);

        let markets = mapped_resolution_markets(&closed_events, condition_markets, &mirrors);
        assert_eq!(
            markets
                .iter()
                .map(|m| m.condition_id.as_str())
                .collect::<Vec<_>>(),
            vec!["0xoff"]
        );
        assert_eq!(markets[0].resolved_payout(), Some(NANOS_PER_DOLLAR));

        let already = HashSet::new();
        assert_eq!(
            pending_close_flags_for_markets(&markets, &mirrors, &already),
            vec![(42u32, "0xoff".to_string())]
        );
    }
}
