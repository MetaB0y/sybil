use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use matching_sequencer::SequencerHandle;
use metrics_exporter_prometheus::PrometheusHandle;
use tokio::sync::RwLock;

use crate::config::ApiConfig;

const MAX_HTTP_RATE_LIMIT_CLIENTS: usize = 10_000;
const OVERFLOW_CLIENT_KEY: &str = "__overflow__";

/// Reference market data mirrored from external systems (e.g., Polymarket).
#[derive(Clone, Debug, Default)]
pub struct MarketRefData {
    pub external_url: Option<String>,
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
    pub prometheus: PrometheusHandle,
    /// Reference prices from external systems (e.g., Polymarket).
    /// Keyed by market_id (u32). Display-only — not part of matching logic.
    pub reference_prices: Arc<RwLock<HashMap<u32, u64>>>,
    /// Reference data per market (external URLs, etc.).
    pub market_ref_data: Arc<RwLock<HashMap<u32, MarketRefData>>>,
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
        Self {
            sequencer,
            dev_mode: config.dev_mode,
            prometheus,
            reference_prices: Arc::new(RwLock::new(HashMap::new())),
            market_ref_data: Arc::new(RwLock::new(HashMap::new())),
            arena_db_path: config.arena_db_path.clone(),
            http_order_limiter: Arc::new(Mutex::new(HttpOrderRateLimiter::new(config))),
        }
    }
}
