use matching_sequencer::SequencerHandle;
use metrics_exporter_prometheus::PrometheusHandle;

use crate::config::ApiConfig;

/// Shared application state, available to all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub sequencer: SequencerHandle,
    pub dev_mode: bool,
    pub prometheus: PrometheusHandle,
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
        }
    }
}
