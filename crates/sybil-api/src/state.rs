use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::HeaderValue;
use matching_sequencer::{AccountId, LeaderboardBase, SequencerHandle};
use metrics_exporter_prometheus::PrometheusHandle;
use ratelimit::Ratelimiter;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex as AsyncMutex, RwLock, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::arena::ArenaReadClient;
use crate::config::ApiConfig;
use crate::history::HistoryClient;
use crate::webauthn::WebAuthnVerifierConfig;

const MAX_HTTP_RATE_LIMIT_CLIENTS: usize = 10_000;
const OVERFLOW_CLIENT_KEY: &str = "__overflow__";
type ReadApiKeyOwners = HashMap<[u8; 32], AccountId>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReferencePriceEntry {
    price_nanos: u64,
    updated_at_ms: u64,
}

/// API-owned freshness boundary for off-block external prices. The publisher
/// can update any subset of markets; every retained value keeps its own clock.
#[derive(Debug, Default)]
struct ReferencePriceBook {
    entries: HashMap<u32, ReferencePriceEntry>,
    last_publisher_update_at_ms: u64,
}

#[derive(Debug, Default)]
pub(crate) struct ReferencePriceSnapshot {
    pub fresh_prices: HashMap<u32, FreshReferencePrice>,
    pub age_ms_by_market: HashMap<u32, u64>,
    pub stored_count: u64,
    pub expired_count: u64,
    pub last_publisher_update_at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FreshReferencePrice {
    pub price_nanos: u64,
    pub expires_at_ms: u64,
}

impl ReferencePriceBook {
    fn update(&mut self, updates: HashMap<u32, u64>, now_ms: u64) {
        for (market_id, price_nanos) in updates {
            // Zero is the existing publisher eviction sentinel. Make it a
            // real deletion at this boundary so consumers see `None`, not an
            // ambiguous zero-probability reference.
            if price_nanos == 0 {
                self.entries.remove(&market_id);
            } else {
                self.entries.insert(
                    market_id,
                    ReferencePriceEntry {
                        price_nanos,
                        updated_at_ms: now_ms,
                    },
                );
            }
        }
        self.last_publisher_update_at_ms = now_ms;
    }

    fn fresh_price(&self, market_id: u32, now_ms: u64, ttl_ms: u64) -> Option<FreshReferencePrice> {
        self.entries.get(&market_id).and_then(|entry| {
            (now_ms.saturating_sub(entry.updated_at_ms) <= ttl_ms).then_some(FreshReferencePrice {
                price_nanos: entry.price_nanos,
                expires_at_ms: entry.updated_at_ms.saturating_add(ttl_ms),
            })
        })
    }

    fn snapshot(&self, now_ms: u64, ttl_ms: u64) -> ReferencePriceSnapshot {
        let mut snapshot = ReferencePriceSnapshot {
            stored_count: u64::try_from(self.entries.len()).unwrap_or(u64::MAX),
            last_publisher_update_at_ms: self.last_publisher_update_at_ms,
            ..ReferencePriceSnapshot::default()
        };
        for (&market_id, entry) in &self.entries {
            let age_ms = now_ms.saturating_sub(entry.updated_at_ms);
            snapshot.age_ms_by_market.insert(market_id, age_ms);
            if age_ms <= ttl_ms {
                snapshot.fresh_prices.insert(
                    market_id,
                    FreshReferencePrice {
                        price_nanos: entry.price_nanos,
                        expires_at_ms: entry.updated_at_ms.saturating_add(ttl_ms),
                    },
                );
            } else {
                snapshot.expired_count = snapshot.expired_count.saturating_add(1);
            }
        }
        snapshot
    }
}

/// Reference market data mirrored from external systems (e.g., Polymarket).
///
/// Off-block: this never enters `MarketMetadata` or any block-hashed state.
/// A third-party verifier can't prove "this market was Sports at block N" —
/// these fields are display chrome only.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MarketRefData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
    /// Polymarket parent event id — frontend grouping key. Distinct from the
    /// matching engine's NegRisk `MarketGroup` (which it does not affect).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_image_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_icon_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_end_date_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_image_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_icon_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_end_date_ms: Option<u64>,
    /// Single display category. **Legacy** — populated only for sybil-native
    /// markets at create time. Mirrored markets leave this `None` and use
    /// `categories` instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// All category buckets the parent event matched in the mirror's
    /// tag-to-bucket lookup. Frontend picks one for display via its own
    /// priority list. Off-block; stored once at sync time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
    /// Polymarket on-chain condition id — FE join key into the event JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polymarket_condition_id: Option<String>,
    /// Parent event start date (epoch ms) from Polymarket. Display/sort only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_start_date_ms: Option<u64>,
    /// Per-market start date (epoch ms) from Polymarket. Display/sort only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_start_date_ms: Option<u64>,
    /// Polymarket short outcome label (`groupItemTitle`). Off-block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_item_title: Option<String>,
    /// Whether Polymarket has closed this market. Off-block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
}

/// Load `market_ref_data` snapshot from disk, or return an empty map if the
/// file is missing or corrupt (logged at `warn!`). Matches the
/// `MappingStore::load` pattern in `sybil-polymarket`.
fn load_market_ref_data(path: &Path) -> HashMap<u32, MarketRefData> {
    if !path.exists() {
        return HashMap::new();
    }
    match std::fs::read_to_string(path) {
        Ok(data) => match serde_json::from_str::<HashMap<u32, MarketRefData>>(&data) {
            Ok(map) => {
                tracing::info!(path = %path.display(), entries = map.len(), "loaded market_ref_data");
                map
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "market_ref_data file is corrupt; starting empty");
                HashMap::new()
            }
        },
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "failed to read market_ref_data; starting empty");
            HashMap::new()
        }
    }
}

/// Persist the current `market_ref_data` map to disk, when a path is
/// configured. Errors are logged but non-fatal — the next mutation retries.
pub fn save_market_ref_data(data: &HashMap<u32, MarketRefData>, path: Option<&Path>) {
    let Some(path) = path else { return };
    match serde_json::to_string_pretty(data) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                tracing::warn!(path = %path.display(), error = %e, "failed to write market_ref_data");
            }
        }
        Err(e) => tracing::warn!(error = %e, "failed to serialize market_ref_data"),
    }
}

#[derive(Debug)]
pub struct HttpRateLimiter {
    global: Ratelimiter,
    clients: Mutex<HashMap<String, Ratelimiter>>,
    client_rate: u32,
    client_burst: u32,
}

impl HttpRateLimiter {
    fn new(global_rate: u32, global_burst: u32, client_rate: u32, client_burst: u32) -> Self {
        Self {
            global: rate_limiter(global_rate, global_burst),
            clients: Mutex::new(HashMap::new()),
            client_rate,
            client_burst,
        }
    }

    pub fn allow(&self, client_key: &str) -> Result<(), u64> {
        self.global.try_wait().map_err(|_| 1_u64)?;
        let mut clients = self.clients.lock().map_err(|_| 1_u64)?;
        let client_key =
            if clients.contains_key(client_key) || clients.len() < MAX_HTTP_RATE_LIMIT_CLIENTS {
                client_key
            } else {
                OVERFLOW_CLIENT_KEY
            };
        clients
            .entry(client_key.to_string())
            .or_insert_with(|| rate_limiter(self.client_rate, self.client_burst))
            .try_wait()
            .map_err(|_| 1_u64)
    }
}

fn rate_limiter(rate: u32, burst: u32) -> Ratelimiter {
    assert!(rate > 0, "rate limits must have a positive refill rate");
    assert!(burst > 0, "rate limits must have positive burst capacity");
    Ratelimiter::builder(u64::from(rate))
        .max_tokens(u64::from(burst))
        .initial_available(u64::from(burst))
        .build()
        .expect("validated rate limit")
}

/// Shared application state, available to all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub sequencer: SequencerHandle,
    /// Private remote history projection. Historical endpoints fail explicitly
    /// when absent; they never fall back to scanning the sequencer actor.
    pub history: Option<HistoryClient>,
    /// Typed private boundary to Python-owned Arena analytics.
    pub arena: Option<ArenaReadClient>,
    /// API-owned immutable authorization view. `None` means it has not yet
    /// been initialized; the first read performs one snapshot RPC. Normal
    /// history reads never enter the sequencer mailbox.
    read_api_key_owners: Arc<RwLock<Option<ReadApiKeyOwners>>>,
    /// Published current-state inputs for leaderboard ranking. Refreshed once
    /// per committed block, never recomputed by each HTTP request.
    leaderboard_bases: Arc<RwLock<Option<Vec<LeaderboardBase>>>>,
    read_model_init_lock: Arc<AsyncMutex<()>>,
    pub dev_mode: bool,
    /// Bearer token for service/operator routes. `None` fails closed when
    /// `dev_mode` is false.
    pub service_token: Option<String>,
    /// Single operational L1 domain accepted by monetary bridge routes.
    /// This is an admission guard, not a validity-guest input.
    pub bridge_domain: Option<crate::config::BridgeDomain>,
    /// CORS origins allowed in production. Empty means no cross-origin CORS.
    pub cors_origins: Vec<HeaderValue>,
    /// Networks whose direct connections may supply forwarding headers for
    /// per-client rate limits. Empty is the safe direct-peer-only default.
    pub http_trusted_proxy_cidrs: Arc<Vec<ipnet::IpNet>>,
    pub prometheus: PrometheusHandle,
    /// Per-market external prices and publisher timestamps. Display-only —
    /// never part of matching logic or committed state.
    reference_prices: Arc<RwLock<ReferencePriceBook>>,
    reference_price_ttl_ms: u64,
    /// Reference data per market (external URLs, images, categories, etc.).
    /// Off-block; populated by the Polymarket mirror via
    /// `POST /v1/markets/{id}/metadata`. Persists across restarts when
    /// `market_ref_data_path` is configured.
    pub market_ref_data: Arc<RwLock<HashMap<u32, MarketRefData>>>,
    /// Optional JSON-on-disk persistence path for `market_ref_data`. `None`
    /// means volatile-in-memory only (state lost on restart; mirror re-fills
    /// on the next sync cycle).
    pub market_ref_data_path: Option<PathBuf>,
    /// Directory for full Polymarket event JSON snapshots (`{event_id}.json`).
    /// `None` disables the raw-event endpoints. Ensured to exist (never wiped)
    /// on startup in `main` (SYB-153) so snapshots persist across restart.
    pub event_snapshot_dir: Option<PathBuf>,
    /// Cheap pre-handler limiter for order endpoints. Sequencer admission has
    /// authoritative account/global limits; this bounds parsing/signature work.
    pub http_order_limiter: Arc<HttpRateLimiter>,
    /// Lifetime public account-id stock. The current stock comes from the
    /// sequencer's durable next account id; zero disables public onboarding.
    pub public_account_capacity: u64,
    /// Server-selected balance for public onboarding. Caller-selected funding
    /// remains confined to service/dev account creation.
    pub public_account_grant_nanos: u64,
    /// Cheap pre-handler budget for anonymous onboarding key material.
    pub http_onboarding_limiter: Arc<HttpRateLimiter>,
    /// Public DA reads have their own low-rate bucket and a hard in-flight cap
    /// so retained-history serving cannot monopolize the sequencer/store.
    pub http_da_limiter: Arc<HttpRateLimiter>,
    pub http_da_concurrency: Arc<Semaphore>,
    /// Hard lifetime cap for anonymous public WebSocket block streams. A
    /// permit is held until the upgrade task is dropped.
    pub http_public_stream_concurrency: Arc<Semaphore>,
    /// WebSocket client-liveness window. Public and service streams share the
    /// protocol behavior, while only public streams consume the anonymous cap.
    pub ws_client_idle_timeout: Duration,
    /// WebAuthn verifier policy for passkey account actions.
    pub webauthn: WebAuthnVerifierConfig,
    /// Serializes account creation and the deprecated first-key bootstrap.
    /// The public atomic create path holds this guard until its initial key is
    /// registered, closing the cross-request first-key race at the API edge.
    pub account_bootstrap_lock: Arc<AsyncMutex<()>>,
    /// Review board for automated LLM resolutions (SYB-48). Metadata only — it
    /// records proposals and operator decisions but never settles a market.
    pub auto_resolutions: crate::auto_resolution::AutoResolutionStore,
}

impl AppState {
    pub fn new(
        sequencer: SequencerHandle,
        config: &ApiConfig,
        prometheus: PrometheusHandle,
    ) -> Self {
        let market_ref_data_path = if config.market_ref_data_path.is_empty() {
            None
        } else {
            Some(PathBuf::from(&config.market_ref_data_path))
        };
        let event_snapshot_dir = if config.event_snapshot_dir.is_empty() {
            None
        } else {
            // Enabled only if the dir exists. `main` creates it on startup
            // (without wiping — SYB-153); if that creation failed, self-disable
            // (the endpoints return a clean "snapshots disabled" 404) instead of
            // serving misleading per-event errors.
            let p = PathBuf::from(&config.event_snapshot_dir);
            p.is_dir().then_some(p)
        };
        let initial_ref_data = match &market_ref_data_path {
            Some(p) => load_market_ref_data(p),
            None => HashMap::new(),
        };
        let service_token =
            (!config.service_token.trim().is_empty()).then(|| config.service_token.clone());
        let bridge_domain = config
            .bridge_domain()
            .expect("bridge domain must pass startup preflight");
        let cors_origins = config
            .cors_origins
            .iter()
            .filter_map(|origin| {
                let trimmed = origin.trim();
                if trimmed.is_empty() {
                    return None;
                }
                match HeaderValue::from_str(trimmed) {
                    Ok(value) => Some(value),
                    Err(e) => {
                        tracing::warn!(origin = %trimmed, error = %e, "ignoring invalid CORS origin");
                        None
                    }
                }
            })
            .collect();
        Self {
            sequencer,
            history: match HistoryClient::from_config(config) {
                Ok(client) => client,
                Err(error) => {
                    tracing::warn!(%error, "history client disabled");
                    None
                }
            },
            arena: match ArenaReadClient::from_config(config) {
                Ok(client) => client,
                Err(error) => {
                    tracing::warn!(%error, "Arena read client disabled");
                    None
                }
            },
            read_api_key_owners: Arc::new(RwLock::new(None)),
            leaderboard_bases: Arc::new(RwLock::new(None)),
            read_model_init_lock: Arc::new(AsyncMutex::new(())),
            dev_mode: config.dev_mode,
            service_token,
            bridge_domain,
            cors_origins,
            http_trusted_proxy_cidrs: Arc::new(config.http_trusted_proxy_cidrs.clone()),
            prometheus,
            reference_prices: Arc::new(RwLock::new(ReferencePriceBook::default())),
            reference_price_ttl_ms: config.reference_price_ttl_ms,
            market_ref_data: Arc::new(RwLock::new(initial_ref_data)),
            market_ref_data_path,
            event_snapshot_dir,
            http_order_limiter: Arc::new(HttpRateLimiter::new(
                config.http_order_global_rps,
                config.http_order_global_burst,
                config.http_order_client_rps,
                config.http_order_client_burst,
            )),
            public_account_capacity: config.public_account_capacity,
            public_account_grant_nanos: config.public_account_grant_nanos,
            http_onboarding_limiter: Arc::new(HttpRateLimiter::new(
                config.http_onboarding_global_rps,
                config.http_onboarding_global_burst,
                config.http_onboarding_client_rps,
                config.http_onboarding_client_burst,
            )),
            http_da_limiter: Arc::new(HttpRateLimiter::new(
                config.http_da_global_rps,
                config.http_da_global_burst,
                config.http_da_client_rps,
                config.http_da_client_burst,
            )),
            http_da_concurrency: Arc::new(Semaphore::new(config.http_da_max_concurrency.max(1))),
            http_public_stream_concurrency: Arc::new(Semaphore::new(
                config.http_public_stream_max_connections.max(1),
            )),
            ws_client_idle_timeout: Duration::from_millis(config.ws_client_idle_timeout_ms),
            webauthn: WebAuthnVerifierConfig::from_api_config(config),
            account_bootstrap_lock: Arc::new(AsyncMutex::new(())),
            auto_resolutions: crate::auto_resolution::AutoResolutionStore::new(),
        }
    }

    pub async fn rehydrate_auto_resolutions(
        &self,
    ) -> Result<(), matching_sequencer::SequencerError> {
        let records = self.sequencer.list_auto_resolution_records().await?;
        self.auto_resolutions.rehydrate(records);
        Ok(())
    }

    pub(crate) async fn update_reference_prices(&self, updates: HashMap<u32, u64>) {
        self.reference_prices
            .write()
            .await
            .update(updates, crate::util::now_ms());
    }

    pub(crate) async fn fresh_reference_prices(&self) -> HashMap<u32, FreshReferencePrice> {
        self.reference_price_snapshot().await.fresh_prices
    }

    pub(crate) async fn fresh_reference_price(
        &self,
        market_id: u32,
    ) -> Option<FreshReferencePrice> {
        self.reference_prices.read().await.fresh_price(
            market_id,
            crate::util::now_ms(),
            self.reference_price_ttl_ms,
        )
    }

    pub(crate) async fn reference_price_snapshot(&self) -> ReferencePriceSnapshot {
        self.reference_prices
            .read()
            .await
            .snapshot(crate::util::now_ms(), self.reference_price_ttl_ms)
    }

    /// Populate API-owned read models before accepting traffic. Tests that
    /// construct a router directly retain a one-time lazy fallback.
    pub async fn initialize_read_models(&self) -> Result<(), matching_sequencer::SequencerError> {
        let _guard = self.read_model_init_lock.lock().await;
        let owners = self.sequencer.active_api_key_owners().await?;
        *self.read_api_key_owners.write().await = Some(owners.into_iter().collect());
        *self.leaderboard_bases.write().await = Some(self.sequencer.leaderboard_bases().await?);
        self.record_public_account_stock(self.sequencer.account_stock().await?);
        Ok(())
    }

    /// Publish the configured lifetime ceiling beside the durable account-id
    /// stock. Re-running this after each allocation makes restart and live
    /// capacity visible without maintaining a second mutable counter.
    pub fn record_public_account_stock(&self, accounts_allocated: u64) {
        metrics::gauge!("sybil_public_account_capacity").set(self.public_account_capacity as f64);
        metrics::gauge!("sybil_public_account_stock").set(accounts_allocated as f64);
        metrics::gauge!("sybil_public_account_remaining").set(
            self.public_account_capacity
                .saturating_sub(accounts_allocated) as f64,
        );
    }

    pub async fn read_api_key_owner(
        &self,
        token_hash: [u8; 32],
    ) -> Result<Option<AccountId>, matching_sequencer::SequencerError> {
        if let Some(owners) = self.read_api_key_owners.read().await.as_ref() {
            return Ok(owners.get(&token_hash).copied());
        }
        let _guard = self.read_model_init_lock.lock().await;
        if self.read_api_key_owners.read().await.is_none() {
            let owners = self.sequencer.active_api_key_owners().await?;
            *self.read_api_key_owners.write().await = Some(owners.into_iter().collect());
        }
        Ok(self
            .read_api_key_owners
            .read()
            .await
            .as_ref()
            .and_then(|owners| owners.get(&token_hash).copied()))
    }

    pub async fn insert_read_api_key_owner(&self, hash: [u8; 32], account_id: AccountId) {
        let mut owners = self.read_api_key_owners.write().await;
        owners
            .get_or_insert_with(HashMap::new)
            .insert(hash, account_id);
    }

    pub async fn remove_read_api_key(&self, hash: &[u8; 32]) {
        if let Some(owners) = self.read_api_key_owners.write().await.as_mut() {
            owners.remove(hash);
        }
    }

    pub async fn cached_leaderboard_bases(
        &self,
    ) -> Result<Vec<LeaderboardBase>, matching_sequencer::SequencerError> {
        if let Some(bases) = self.leaderboard_bases.read().await.as_ref() {
            return Ok(bases.clone());
        }
        let _guard = self.read_model_init_lock.lock().await;
        if self.leaderboard_bases.read().await.is_none() {
            *self.leaderboard_bases.write().await = Some(self.sequencer.leaderboard_bases().await?);
        }
        Ok(self
            .leaderboard_bases
            .read()
            .await
            .as_ref()
            .cloned()
            .unwrap_or_default())
    }

    /// Refresh the published leaderboard input after each committed block.
    /// The refresh is one actor read per block, independent of HTTP volume.
    pub async fn refresh_leaderboard_read_model(&self, cancel: CancellationToken) {
        let state = self.clone();
        let subscription = tokio::select! {
            _ = cancel.cancelled() => return,
            result = state.sequencer.subscribe_blocks() => result,
        };
        let Ok(mut blocks) = subscription else {
            tracing::warn!("failed to subscribe leaderboard read model to blocks");
            return;
        };
        loop {
            let received = tokio::select! {
                _ = cancel.cancelled() => return,
                result = blocks.recv() => result,
            };
            match received {
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let refresh = tokio::select! {
                        _ = cancel.cancelled() => return,
                        result = state.sequencer.leaderboard_bases() => result,
                    };
                    match refresh {
                        Ok(bases) => *state.leaderboard_bases.write().await = Some(bases),
                        Err(error) => {
                            tracing::warn!(%error, "failed to refresh leaderboard read model")
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    }
}

#[cfg(test)]
mod reference_price_tests {
    use super::*;

    fn updates(entries: &[(u32, u64)]) -> HashMap<u32, u64> {
        entries.iter().copied().collect()
    }

    #[test]
    fn publisher_death_expires_values_at_the_server_boundary() {
        let mut book = ReferencePriceBook::default();
        book.update(updates(&[(7, 400_000_000)]), 1_000);

        assert_eq!(
            book.fresh_price(7, 1_100, 100),
            Some(FreshReferencePrice {
                price_nanos: 400_000_000,
                expires_at_ms: 1_100,
            })
        );
        assert_eq!(book.fresh_price(7, 1_101, 100), None);
        let expired = book.snapshot(1_101, 100);
        assert_eq!(expired.expired_count, 1);
        assert!(expired.fresh_prices.is_empty());
    }

    #[test]
    fn partial_updates_refresh_only_the_tokens_the_publisher_sent() {
        let mut book = ReferencePriceBook::default();
        book.update(updates(&[(1, 400_000_000), (2, 600_000_000)]), 1_000);
        book.update(updates(&[(1, 450_000_000)]), 1_080);

        let snapshot = book.snapshot(1_110, 100);
        assert_eq!(
            snapshot.fresh_prices.get(&1),
            Some(&FreshReferencePrice {
                price_nanos: 450_000_000,
                expires_at_ms: 1_180,
            })
        );
        assert!(!snapshot.fresh_prices.contains_key(&2));
        assert_eq!(snapshot.expired_count, 1);
    }

    #[test]
    fn explicit_eviction_and_process_restart_cannot_resurrect_a_price() {
        let mut book = ReferencePriceBook::default();
        book.update(updates(&[(7, 400_000_000)]), 1_000);
        book.update(updates(&[(7, 0)]), 1_010);
        assert_eq!(book.snapshot(1_010, 100).stored_count, 0);

        let restarted = ReferencePriceBook::default();
        assert_eq!(restarted.fresh_price(7, 1_010, 100), None);
    }
}
