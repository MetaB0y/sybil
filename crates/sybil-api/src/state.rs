use std::collections::HashMap;
use std::sync::Arc;

use matching_sequencer::SequencerHandle;
use metrics_exporter_prometheus::PrometheusHandle;
use tokio::sync::RwLock;

use crate::config::ApiConfig;

/// Reference market data mirrored from external systems (e.g., Polymarket).
#[derive(Clone, Debug, Default)]
pub struct MarketRefData {
    pub external_url: Option<String>,
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
        }
    }
}
