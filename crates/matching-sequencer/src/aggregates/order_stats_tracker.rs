//! Off-block tracker of order placed / matched / unmatched counts.
//! Sidecar — does not enter `state_root` / `events_root` / `BlockWitness`.
//!
//! Inclusion rules (decision Q-table in BACKEND_DATA_PLAN.md):
//! - MM submissions count as placed when they enter a batch problem. They do
//!   not rest, so matched/unmatched lifecycle counts only arise if they are
//!   later represented by an exit hook.
//! - Cancellations excluded — counted separately via `OrderCancelled` (D1).
//! - Multi-market orders credit each active market; the platform counter
//!   advances once per order (sum-of-per-market over-counts vs platform).
//!
//! Exits are classified using B5's `RestingOrder.has_been_matched` flag:
//! true → matched; false → unmatched. The flag is set by `OrderBook.settle`
//! when a fill > 0 is observed; it's propagated to partial-fill remainders,
//! so a later eviction still classifies correctly.
//!
//! Snapshots round-trip through `SequencerSnapshot` / `RestoredState`.
//! Missing redb table on load yields `Default::default()`.

use std::collections::{HashMap, VecDeque};

use matching_engine::MarketId;
use serde::{Deserialize, Serialize};

use crate::order_book::RestingOrder;

const HOURLY_STATS_CAP: usize = 25;
const MILLIS_PER_HOUR: u64 = 3_600_000;
const MILLIS_PER_DAY: u64 = 24 * MILLIS_PER_HOUR;

/// Rolling counters for one (market, all-time) entry, the platform total,
/// or a single hourly bucket.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrderStats {
    #[serde(default)]
    pub placed: u64,
    #[serde(default)]
    pub matched: u64,
    #[serde(default)]
    pub unmatched: u64,
    /// Distinct orders admitted — counted once per order at intake, NOT per
    /// batch (unlike `placed`). Must stay the LAST field: snapshots use
    /// positional rmp_serde arrays, so appending keeps old 3-element blobs
    /// decodable via `#[serde(default)]` (same pattern as `created_at_ms`).
    #[serde(default)]
    pub placed_distinct: u64,
}

#[derive(Clone, Debug, Default)]
pub struct OrderStatsTracker {
    per_market: HashMap<MarketId, OrderStats>,
    platform: OrderStats,
    hourly_platform: VecDeque<(u64, OrderStats)>,
}

impl OrderStatsTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn restore(snapshot: OrderStatsTrackerSnapshot) -> Self {
        Self {
            per_market: snapshot.per_market.into_iter().collect(),
            platform: snapshot.platform,
            hourly_platform: snapshot.hourly_platform.into_iter().collect(),
        }
    }

    pub fn snapshot(&self) -> OrderStatsTrackerSnapshot {
        let mut per_market: Vec<(MarketId, OrderStats)> =
            self.per_market.iter().map(|(m, s)| (*m, *s)).collect();
        per_market.sort_by_key(|(m, _)| m.0);

        let hourly_platform: Vec<(u64, OrderStats)> =
            self.hourly_platform.iter().copied().collect();

        OrderStatsTrackerSnapshot {
            per_market,
            platform: self.platform,
            hourly_platform,
        }
    }

    /// Record a non-MM order admit. Per-market +1 for each active market;
    /// platform +1; hourly bucket +1.
    pub fn record_placed(&mut self, markets: impl IntoIterator<Item = MarketId>, ts_ms: u64) {
        for m in markets {
            self.per_market.entry(m).or_default().placed += 1;
        }
        self.platform.placed += 1;
        self.hourly_entry_mut(ts_ms).placed += 1;
    }

    /// Record a distinct order admission — once per order at intake, NOT per
    /// batch (unlike `record_placed`). Platform total + hourly bucket only;
    /// per-market distinct is never surfaced on the wire, so we don't grow
    /// per-market state to track it. MM flash orders are admitted once per
    /// block and never rest, so counting them here matches their `placed`.
    pub fn record_admitted(&mut self, ts_ms: u64) {
        self.platform.placed_distinct += 1;
        self.hourly_entry_mut(ts_ms).placed_distinct += 1;
    }

    /// Record an order exit (removed from the book by `expire`,
    /// `revalidate`, or `settle`). Routes to matched if
    /// `has_been_matched`, else unmatched. Per-market over-counts.
    pub fn record_exit(&mut self, order: &RestingOrder, ts_ms: u64) {
        self.record_outcome(order.order.active_markets(), order.has_been_matched, ts_ms);
    }

    /// Record a resolved order outcome — `matched` (received ≥1 fill) or
    /// unmatched (left without a fill). Shared by `record_exit` (resting
    /// orders leaving the book) and by MM flash orders, which live a single
    /// block and resolve in-place against that block's fills. Per-market +1
    /// each active market; platform +1; hourly bucket +1.
    pub fn record_outcome(
        &mut self,
        markets: impl IntoIterator<Item = MarketId>,
        matched: bool,
        ts_ms: u64,
    ) {
        if matched {
            for m in markets {
                self.per_market.entry(m).or_default().matched += 1;
            }
            self.platform.matched += 1;
            self.hourly_entry_mut(ts_ms).matched += 1;
        } else {
            for m in markets {
                self.per_market.entry(m).or_default().unmatched += 1;
            }
            self.platform.unmatched += 1;
            self.hourly_entry_mut(ts_ms).unmatched += 1;
        }
    }

    fn hourly_entry_mut(&mut self, ts_ms: u64) -> &mut OrderStats {
        let bucket_start = ts_ms - (ts_ms % MILLIS_PER_HOUR);
        let needs_new = self
            .hourly_platform
            .back()
            .is_none_or(|(start, _)| *start != bucket_start);
        if needs_new {
            self.hourly_platform
                .push_back((bucket_start, OrderStats::default()));
            while self.hourly_platform.len() > HOURLY_STATS_CAP {
                self.hourly_platform.pop_front();
            }
        }
        &mut self
            .hourly_platform
            .back_mut()
            .expect("just pushed a bucket")
            .1
    }

    /// All-time stats for one market.
    pub fn per_market(&self, m: MarketId) -> OrderStats {
        self.per_market.get(&m).copied().unwrap_or_default()
    }

    /// Map of every market with at least one event recorded.
    pub fn all_per_market(&self) -> HashMap<MarketId, OrderStats> {
        self.per_market.clone()
    }

    /// Platform all-time stats.
    pub fn platform(&self) -> OrderStats {
        self.platform
    }

    /// Platform stats summed across hourly buckets within the last 24h.
    pub fn platform_24h(&self, now_ms: u64) -> OrderStats {
        let cutoff = now_ms.saturating_sub(MILLIS_PER_DAY);
        let mut out = OrderStats::default();
        for (start, stats) in &self.hourly_platform {
            if *start + MILLIS_PER_HOUR > cutoff && *start <= now_ms {
                out.placed += stats.placed;
                out.matched += stats.matched;
                out.unmatched += stats.unmatched;
                out.placed_distinct += stats.placed_distinct;
            }
        }
        out
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OrderStatsTrackerSnapshot {
    pub per_market: Vec<(MarketId, OrderStats)>,
    pub platform: OrderStats,
    pub hourly_platform: Vec<(u64, OrderStats)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountId;
    use crate::validation::PositionKey;
    use matching_engine::{MarketId, Order};

    fn mid(n: u32) -> MarketId {
        MarketId::new(n)
    }

    fn unmatched_resting(order_id: u64, market: MarketId) -> RestingOrder {
        let mut order = Order::new(order_id);
        order.max_fill = matching_engine::Qty(5);
        // Single-market order: outcome=0, side=buy. Only active_markets matters
        // for the tracker; concrete order shape is irrelevant.
        order.num_markets = 1;
        order.markets[0] = market;
        RestingOrder {
            order,
            account_id: AccountId(1),
            created_at: 0,
            expires_at_block: 100,
            reserved_balance: 0,
            reserved_positions: vec![] as Vec<(PositionKey, i64)>,
            has_been_matched: false,
            original_max_fill: 5,
            created_at_ms: 0,
        }
    }

    fn matched_resting(order_id: u64, market: MarketId) -> RestingOrder {
        let mut r = unmatched_resting(order_id, market);
        r.has_been_matched = true;
        r
    }

    #[test]
    fn placed_matched_unmatched_basic() {
        let mut t = OrderStatsTracker::new();
        let m = mid(1);

        t.record_placed([m], 0);
        t.record_placed([m], 0);
        t.record_placed([m], 0);
        // 1 fully filled, 1 still resting, 1 expired unmatched
        t.record_exit(&matched_resting(1, m), 0);
        t.record_exit(&unmatched_resting(2, m), 0);

        let market_stats = t.per_market(m);
        assert_eq!(market_stats.placed, 3);
        assert_eq!(market_stats.matched, 1);
        assert_eq!(market_stats.unmatched, 1);

        let platform = t.platform();
        assert_eq!(platform.placed, 3);
        assert_eq!(platform.matched, 1);
        assert_eq!(platform.unmatched, 1);
    }

    #[test]
    fn multi_market_attribution() {
        let mut t = OrderStatsTracker::new();
        let m1 = mid(1);
        let m2 = mid(2);

        // Multi-market order: both markets +1, platform +1.
        t.record_placed([m1, m2], 0);

        assert_eq!(t.per_market(m1).placed, 1);
        assert_eq!(t.per_market(m2).placed, 1);
        assert_eq!(t.platform().placed, 1);
        // Sum-of-per-market (2) exceeds platform (1) by design.
    }

    #[test]
    fn hourly_24h_window() {
        let mut t = OrderStatsTracker::new();
        let m = mid(1);

        let h = MILLIS_PER_HOUR;
        // Two events 30h apart.
        t.record_placed([m], 0);
        t.record_placed([m], 30 * h);
        // Query at 30h: only the second event is inside the 24h window.
        let p = t.platform_24h(30 * h);
        assert_eq!(p.placed, 1);
        assert_eq!(p.matched, 0);
        assert_eq!(p.unmatched, 0);

        // Query at 0h: only the first event.
        let p0 = t.platform_24h(0);
        assert_eq!(p0.placed, 1);
    }

    #[test]
    fn cap_drops_oldest_hourly_bucket() {
        let mut t = OrderStatsTracker::new();
        let m = mid(1);
        let h = MILLIS_PER_HOUR;
        for i in 0..30 {
            t.record_placed([m], i * h);
        }
        assert_eq!(t.hourly_platform.len(), HOURLY_STATS_CAP);
        let oldest = t.hourly_platform.front().unwrap().0;
        // We pushed buckets at 0, h, ..., 29h. Cap=25 keeps the newest 25:
        // 5h .. 29h. Oldest = 5h.
        assert_eq!(oldest, 5 * h);
    }

    #[test]
    fn snapshot_roundtrip() {
        let mut t = OrderStatsTracker::new();
        let m = mid(1);
        t.record_placed([m], 0);
        t.record_exit(&matched_resting(1, m), 0);
        t.record_admitted(0);

        let snap = t.snapshot();
        let restored = OrderStatsTracker::restore(snap);
        assert_eq!(restored.platform(), t.platform());
        assert_eq!(restored.per_market(m), t.per_market(m));
        assert_eq!(restored.platform_24h(0), t.platform_24h(0));
    }

    #[test]
    fn record_admitted_is_platform_and_hourly_only() {
        let mut t = OrderStatsTracker::new();
        let m = mid(1);
        t.record_admitted(0);
        t.record_admitted(0);
        t.record_admitted(0);
        assert_eq!(t.platform().placed_distinct, 3);
        assert_eq!(t.platform_24h(0).placed_distinct, 3);
        // per-market distinct is intentionally not tracked
        assert_eq!(t.per_market(m).placed_distinct, 0);
        // and it must not touch the participation `placed` counter
        assert_eq!(t.platform().placed, 0);
    }

    #[test]
    fn record_outcome_routes_matched_and_unmatched() {
        let mut t = OrderStatsTracker::new();
        let m = mid(1);
        t.record_outcome([m], true, 0);
        t.record_outcome([m], false, 0);
        t.record_outcome([m], false, 0);
        let p = t.platform();
        assert_eq!(p.matched, 1);
        assert_eq!(p.unmatched, 2);
        assert_eq!(t.per_market(m).matched, 1);
        assert_eq!(t.per_market(m).unmatched, 2);
        assert_eq!(t.platform_24h(0).matched, 1);
        assert_eq!(t.platform_24h(0).unmatched, 2);
    }

    #[test]
    fn order_stats_decodes_old_blob_without_placed_distinct() {
        // An old snapshot encoded a 3-field OrderStats (positional rmp array).
        // The new 4-field struct must decode it with placed_distinct => 0.
        #[derive(Serialize)]
        struct OldOrderStats {
            placed: u64,
            matched: u64,
            unmatched: u64,
        }
        let old = OldOrderStats {
            placed: 7,
            matched: 3,
            unmatched: 2,
        };
        let bytes = rmp_serde::to_vec(&old).expect("encode old blob");
        let decoded: OrderStats = rmp_serde::from_slice(&bytes).expect("decode into new struct");
        assert_eq!(decoded.placed, 7);
        assert_eq!(decoded.matched, 3);
        assert_eq!(decoded.unmatched, 2);
        assert_eq!(
            decoded.placed_distinct, 0,
            "missing trailing field defaults to 0"
        );
    }
}
