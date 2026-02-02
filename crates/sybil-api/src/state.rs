use matching_sequencer::SequencerHandle;

use crate::config::ApiConfig;

/// Shared application state, available to all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub sequencer: SequencerHandle,
    pub dev_mode: bool,
}

impl AppState {
    pub fn new(sequencer: SequencerHandle, config: &ApiConfig) -> Self {
        Self {
            sequencer,
            dev_mode: config.dev_mode,
        }
    }
}
