//! Small shared utilities for the API crate.

use std::time::{SystemTime, UNIX_EPOCH};

/// Wall-clock milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Wall-clock seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    now_ms() / 1000
}
