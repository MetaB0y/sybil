use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::http::HeaderValue;
use matching_sequencer::SequencerHandle;
use metrics_exporter_prometheus::PrometheusHandle;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::ApiConfig;

const MAX_HTTP_RATE_LIMIT_CLIENTS: usize = 10_000;
const OVERFLOW_CLIENT_KEY: &str = "__overflow__";

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
struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_per_second: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(refill_per_second: u32, capacity: u32) -> Self {
        Self {
            tokens: capacity as f64,
            capacity: capacity as f64,
            refill_per_second: refill_per_second as f64,
            last_refill: Instant::now(),
        }
    }

    fn allow(&mut self) -> Result<(), u64> {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_refill);
        self.last_refill = now;
        self.tokens =
            (self.tokens + elapsed.as_secs_f64() * self.refill_per_second).min(self.capacity);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            Err(self.retry_after_secs())
        }
    }

    fn retry_after_secs(&self) -> u64 {
        if self.refill_per_second <= 0.0 {
            return 1;
        }
        ((1.0 - self.tokens).max(0.0) / self.refill_per_second)
            .ceil()
            .max(1.0) as u64
    }
}

#[derive(Debug)]
pub struct HttpOrderRateLimiter {
    global: TokenBucket,
    clients: HashMap<String, TokenBucket>,
    client_refill_per_second: u32,
    client_burst: u32,
}

impl HttpOrderRateLimiter {
    fn new(config: &ApiConfig) -> Self {
        Self {
            global: TokenBucket::new(config.http_order_global_rps, config.http_order_global_burst),
            clients: HashMap::new(),
            client_refill_per_second: config.http_order_client_rps,
            client_burst: config.http_order_client_burst,
        }
    }

    pub fn allow(&mut self, client_key: &str) -> Result<(), u64> {
        self.global.allow()?;
        let client_key = if self.clients.contains_key(client_key)
            || self.clients.len() < MAX_HTTP_RATE_LIMIT_CLIENTS
        {
            client_key
        } else {
            OVERFLOW_CLIENT_KEY
        };
        self.clients
            .entry(client_key.to_string())
            .or_insert_with(|| TokenBucket::new(self.client_refill_per_second, self.client_burst))
            .allow()
    }
}

/// Shared application state, available to all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub sequencer: SequencerHandle,
    pub dev_mode: bool,
    /// Bearer token for service/operator routes. `None` fails closed when
    /// `dev_mode` is false.
    pub service_token: Option<String>,
    /// CORS origins allowed in production. Empty means no cross-origin CORS.
    pub cors_origins: Vec<HeaderValue>,
    pub prometheus: PrometheusHandle,
    /// Reference prices from external systems (e.g., Polymarket).
    /// Keyed by market_id (u32). Display-only — not part of matching logic.
    pub reference_prices: Arc<RwLock<HashMap<u32, u64>>>,
    /// Unix milliseconds when reference prices were last updated.
    pub reference_prices_updated_at_ms: Arc<RwLock<u64>>,
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
    /// Path to arena's live decision database, when configured.
    pub arena_db_path: String,
    /// Cheap pre-handler limiter for order endpoints. Sequencer admission has
    /// authoritative account/global limits; this bounds parsing/signature work.
    pub http_order_limiter: Arc<Mutex<HttpOrderRateLimiter>>,
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
            dev_mode: config.dev_mode,
            service_token,
            cors_origins,
            prometheus,
            reference_prices: Arc::new(RwLock::new(HashMap::new())),
            reference_prices_updated_at_ms: Arc::new(RwLock::new(0)),
            market_ref_data: Arc::new(RwLock::new(initial_ref_data)),
            market_ref_data_path,
            event_snapshot_dir,
            arena_db_path: config.arena_db_path.clone(),
            http_order_limiter: Arc::new(Mutex::new(HttpOrderRateLimiter::new(config))),
        }
    }
}
