//! Off-block tracker of cumulative platform welfare.
//! Sidecar — does not enter `state_root` / `events_root` / `BlockWitness`.
//!
//! Accumulates the authoritative per-block `total_welfare` scalar (the solver's
//! objective value, which counts each fill once — see the platform-total note on
//! `BlockAnalytics.total_welfare`). Keeps a running all-time sum plus rolling
//! hourly buckets for the 24h window, mirroring `PriceTracker`'s platform-volume
//! extensions and `OrderStatsTracker`'s hourly machinery.
//!
//! The canonical field remains `i64` because gross value and signed mint/burn
//! cost use signed arithmetic, but verified total welfare is non-negative.
//! Restore clamps legacy negative aggregates produced before the signed-burn
//! fix; recording also fails closed at zero if that invariant is violated.
//! Accumulators use `saturating_add` on `i64`. Per-market welfare is
//! NOT tracked here — that ships separately via `BlockAnalytics.welfare_by_market`
//! → `BlockMarketStats.welfare_nanos`.
//!
//! Snapshots round-trip through `AnalyticsSnapshot` / `AnalyticsRestoredState`.
//! Missing redb table on load yields `Default::default()` (cold start).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Retained hourly welfare buckets (24 closed hours + 1 open hour), matching
/// `OrderStatsTracker`'s `HOURLY_STATS_CAP`.
const HOURLY_WELFARE_CAP: usize = 25;
const MILLIS_PER_HOUR: u64 = 3_600_000;
const MILLIS_PER_DAY: u64 = 24 * MILLIS_PER_HOUR;

#[derive(Clone, Debug, Default)]
pub struct WelfareTracker {
    /// All-time running sum of per-block `total_welfare`, in nanos.
    platform: i64,
    /// Rolling platform welfare bucketed by hour-start (epoch ms). Cap is
    /// `HOURLY_WELFARE_CAP`; oldest bucket drops when a 26th rolls in.
    hourly_platform: VecDeque<(u64, i64)>,
}

impl WelfareTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn restore(snapshot: WelfareTrackerSnapshot) -> Self {
        Self {
            platform: snapshot.platform.max(0),
            hourly_platform: snapshot
                .hourly_platform
                .into_iter()
                .map(|(timestamp, welfare)| (timestamp, welfare.max(0)))
                .collect(),
        }
    }

    pub fn snapshot(&self) -> WelfareTrackerSnapshot {
        WelfareTrackerSnapshot {
            platform: self.platform,
            hourly_platform: self.hourly_platform.iter().copied().collect(),
        }
    }

    /// Accumulate one finalized block's authoritative `total_welfare` scalar.
    /// Platform running total += welfare; current hourly bucket += welfare.
    pub fn record(&mut self, welfare: i64, ts_ms: u64) {
        debug_assert!(
            welfare >= 0,
            "verified platform welfare must be non-negative"
        );
        let welfare = welfare.max(0);
        self.platform = self.platform.saturating_add(welfare);
        let bucket = self.hourly_entry_mut(ts_ms);
        *bucket = bucket.saturating_add(welfare);
    }

    fn hourly_entry_mut(&mut self, ts_ms: u64) -> &mut i64 {
        let bucket_start = ts_ms - (ts_ms % MILLIS_PER_HOUR);
        let needs_new = self
            .hourly_platform
            .back()
            .is_none_or(|(start, _)| *start != bucket_start);
        if needs_new {
            self.hourly_platform.push_back((bucket_start, 0));
            while self.hourly_platform.len() > HOURLY_WELFARE_CAP {
                self.hourly_platform.pop_front();
            }
        }
        &mut self
            .hourly_platform
            .back_mut()
            .expect("just pushed a bucket")
            .1
    }

    /// All-time platform welfare (running sum, constant time).
    pub fn platform_total(&self) -> i64 {
        self.platform
    }

    /// Platform welfare summed across hourly buckets within the last 24h.
    pub fn platform_24h(&self, now_ms: u64) -> i64 {
        let cutoff = now_ms.saturating_sub(MILLIS_PER_DAY);
        let mut out: i64 = 0;
        for (start, welfare) in &self.hourly_platform {
            if *start + MILLIS_PER_HOUR > cutoff && *start <= now_ms {
                out = out.saturating_add(*welfare);
            }
        }
        out
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WelfareTrackerSnapshot {
    pub platform: i64,
    pub hourly_platform: Vec<(u64, i64)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: u64 = MILLIS_PER_HOUR;

    #[test]
    fn accumulate_all_time() {
        let mut t = WelfareTracker::new();
        t.record(100, 0);
        t.record(250, 0);
        t.record(50, 0);
        assert_eq!(t.platform_total(), 400);
    }

    #[test]
    fn legacy_negative_welfare_is_clamped_on_restore() {
        let t = WelfareTracker::restore(WelfareTrackerSnapshot {
            platform: -15,
            hourly_platform: vec![(0, -15)],
        });
        assert_eq!(t.platform_total(), 0);
        assert_eq!(t.platform_24h(2_000), 0);
    }

    #[test]
    fn hourly_24h_window() {
        let mut t = WelfareTracker::new();
        // Two blocks 30h apart.
        t.record(1_000, 0);
        t.record(2_000, 30 * H);
        // At 30h: only the second block is inside the 24h window.
        assert_eq!(t.platform_24h(30 * H), 2_000);
        // At 0h: only the first.
        assert_eq!(t.platform_24h(0), 1_000);
        // All-time always covers both.
        assert_eq!(t.platform_total(), 3_000);
    }

    #[test]
    fn same_hour_blocks_coalesce_into_one_bucket() {
        let mut t = WelfareTracker::new();
        t.record(100, 100_000); // hour 0
        t.record(200, 400_000); // still hour 0
        assert_eq!(t.hourly_platform.len(), 1);
        assert_eq!(t.platform_24h(500_000), 300);
    }

    #[test]
    fn cap_drops_oldest_hourly_bucket() {
        let mut t = WelfareTracker::new();
        for i in 0..30u64 {
            t.record(1, i * H);
        }
        assert_eq!(t.hourly_platform.len(), HOURLY_WELFARE_CAP);
        // Pushed buckets at 0,h,..,29h; cap=25 keeps the newest 25: 5h..29h.
        assert_eq!(t.hourly_platform.front().unwrap().0, 5 * H);
        // Running total covers all 30 (cap doesn't affect it).
        assert_eq!(t.platform_total(), 30);
    }

    #[test]
    fn snapshot_roundtrip() {
        let mut t = WelfareTracker::new();
        t.record(500, 0);
        t.record(25, H + 1_000);
        let restored = WelfareTracker::restore(t.snapshot());
        assert_eq!(restored.platform_total(), t.platform_total());
        let now = H + 5_000;
        assert_eq!(restored.platform_24h(now), t.platform_24h(now));
    }
}
