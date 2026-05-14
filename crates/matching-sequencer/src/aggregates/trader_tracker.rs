//! Off-block tracker of unique placers per (market, time-window) and
//! platform-wide. Sidecar — does not enter `state_root` / `events_root` /
//! `BlockWitness`.
//!
//! Inclusion rules (decision Q-table in BACKEND_DATA_PLAN.md):
//! - MM-constrained submissions excluded (liquidity provider, not trader).
//! - `AccountId::MINT` excluded (system account).
//! - Multi-market orders credit each active market; the platform set
//!   accounts for the placer once (so platform total != sum-of-per-market).
//!
//! Snapshots round-trip through the existing `SequencerSnapshot` /
//! `RestoredState` pipeline. Missing redb table on load yields
//! `Default::default()` — cold start until activity accumulates.

use std::collections::{HashMap, HashSet, VecDeque};

use matching_engine::MarketId;
use serde::{Deserialize, Serialize};

use crate::account::AccountId;

/// 24h-rolling buckets at ±1h resolution. One extra entry beyond 24
/// makes the inclusion check unambiguous when `now_ms` sits mid-bucket.
const HOURLY_BUCKET_CAP: usize = 25;
const MILLIS_PER_HOUR: u64 = 3_600_000;
const MILLIS_PER_DAY: u64 = 24 * MILLIS_PER_HOUR;

#[derive(Clone, Debug, Default)]
pub struct TraderTracker {
    /// Per-market all-time placers (excludes MM, MINT).
    per_market: HashMap<MarketId, HashSet<AccountId>>,
    /// Platform-wide all-time placers.
    platform: HashSet<AccountId>,
    /// Newest-at-back hourly buckets, cap 25.
    hourly_buckets: VecDeque<(u64, HashSet<AccountId>)>,
}

impl TraderTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn restore(snapshot: TraderTrackerSnapshot) -> Self {
        let per_market = snapshot
            .per_market
            .into_iter()
            .map(|(m, accs)| (m, accs.into_iter().collect::<HashSet<_>>()))
            .collect();
        let platform: HashSet<AccountId> = snapshot.platform.into_iter().collect();
        let hourly_buckets = snapshot
            .hourly_buckets
            .into_iter()
            .map(|(start, accs)| (start, accs.into_iter().collect::<HashSet<_>>()))
            .collect();
        Self {
            per_market,
            platform,
            hourly_buckets,
        }
    }

    pub fn snapshot(&self) -> TraderTrackerSnapshot {
        let mut per_market: Vec<(MarketId, Vec<AccountId>)> = self
            .per_market
            .iter()
            .map(|(m, set)| {
                let mut accs: Vec<_> = set.iter().copied().collect();
                accs.sort_by_key(|a| a.0);
                (*m, accs)
            })
            .collect();
        per_market.sort_by_key(|(m, _)| m.0);

        let mut platform: Vec<AccountId> = self.platform.iter().copied().collect();
        platform.sort_by_key(|a| a.0);

        let hourly_buckets: Vec<(u64, Vec<AccountId>)> = self
            .hourly_buckets
            .iter()
            .map(|(start, set)| {
                let mut accs: Vec<_> = set.iter().copied().collect();
                accs.sort_by_key(|a| a.0);
                (*start, accs)
            })
            .collect();

        TraderTrackerSnapshot {
            per_market,
            platform,
            hourly_buckets,
        }
    }

    /// Record an admitted placement. No-op for MM submissions and MINT.
    /// `markets` is every active market the order touches.
    pub fn record_placed(
        &mut self,
        account_id: AccountId,
        markets: impl IntoIterator<Item = MarketId>,
        ts_ms: u64,
        is_mm: bool,
    ) {
        if is_mm || account_id == AccountId::MINT {
            return;
        }
        for m in markets {
            self.per_market.entry(m).or_default().insert(account_id);
        }
        self.platform.insert(account_id);
        self.bump_bucket(account_id, ts_ms);
    }

    fn bump_bucket(&mut self, account_id: AccountId, ts_ms: u64) {
        let bucket_start = ts_ms - (ts_ms % MILLIS_PER_HOUR);
        let push_new = match self.hourly_buckets.back_mut() {
            Some((start, set)) if *start == bucket_start => {
                set.insert(account_id);
                false
            }
            _ => true,
        };
        if push_new {
            let mut set = HashSet::new();
            set.insert(account_id);
            self.hourly_buckets.push_back((bucket_start, set));
            while self.hourly_buckets.len() > HOURLY_BUCKET_CAP {
                self.hourly_buckets.pop_front();
            }
        }
    }

    /// Per-market all-time trader count.
    pub fn per_market_count(&self, market_id: MarketId) -> u32 {
        self.per_market
            .get(&market_id)
            .map(|s| s.len() as u32)
            .unwrap_or(0)
    }

    /// Platform-wide all-time placer count.
    pub fn platform_count(&self) -> u32 {
        self.platform.len() as u32
    }

    /// Unique placers in the last 24h (union over hourly buckets within
    /// the window). ±1h resolution; finer resolution can come from
    /// 5-minute buckets later.
    pub fn platform_24h_count(&self, now_ms: u64) -> u32 {
        let cutoff = now_ms.saturating_sub(MILLIS_PER_DAY);
        let mut seen: HashSet<AccountId> = HashSet::new();
        for (start, set) in &self.hourly_buckets {
            // Bucket covers [start, start + 1h). Include if any part of the
            // bucket falls within (cutoff, now_ms].
            if *start + MILLIS_PER_HOUR > cutoff && *start <= now_ms {
                seen.extend(set.iter().copied());
            }
        }
        seen.len() as u32
    }

    /// Union of per-market placers across the markets of an event.
    pub fn event_count(&self, market_ids: &[MarketId]) -> u32 {
        let mut seen: HashSet<AccountId> = HashSet::new();
        for m in market_ids {
            if let Some(set) = self.per_market.get(m) {
                seen.extend(set.iter().copied());
            }
        }
        seen.len() as u32
    }

    /// Map of `market_id → trader_count` for every market with at least
    /// one placer recorded.
    pub fn all_market_counts(&self) -> HashMap<MarketId, u32> {
        self.per_market
            .iter()
            .map(|(m, s)| (*m, s.len() as u32))
            .collect()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TraderTrackerSnapshot {
    pub per_market: Vec<(MarketId, Vec<AccountId>)>,
    pub platform: Vec<AccountId>,
    pub hourly_buckets: Vec<(u64, Vec<AccountId>)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mid(n: u32) -> MarketId {
        MarketId::new(n)
    }

    fn acc(n: u64) -> AccountId {
        AccountId(n)
    }

    #[test]
    fn records_single_market_placement() {
        let mut t = TraderTracker::new();
        t.record_placed(acc(1), [mid(7)], 0, false);
        assert_eq!(t.per_market_count(mid(7)), 1);
        assert_eq!(t.platform_count(), 1);
    }

    #[test]
    fn deduplicates_repeat_placements() {
        let mut t = TraderTracker::new();
        t.record_placed(acc(1), [mid(7)], 0, false);
        t.record_placed(acc(1), [mid(7)], 1_000, false);
        assert_eq!(t.per_market_count(mid(7)), 1);
        assert_eq!(t.platform_count(), 1);
    }

    #[test]
    fn skips_mm_orders() {
        let mut t = TraderTracker::new();
        t.record_placed(acc(1), [mid(7)], 0, true);
        assert_eq!(t.per_market_count(mid(7)), 0);
        assert_eq!(t.platform_count(), 0);
    }

    #[test]
    fn skips_mint_account() {
        let mut t = TraderTracker::new();
        t.record_placed(AccountId::MINT, [mid(7)], 0, false);
        assert_eq!(t.per_market_count(mid(7)), 0);
        assert_eq!(t.platform_count(), 0);
    }

    #[test]
    fn multi_market_credits_each_but_platform_once() {
        let mut t = TraderTracker::new();
        t.record_placed(acc(1), [mid(7), mid(8)], 0, false);
        assert_eq!(t.per_market_count(mid(7)), 1);
        assert_eq!(t.per_market_count(mid(8)), 1);
        // Platform counts the placer once.
        assert_eq!(t.platform_count(), 1);
    }

    #[test]
    fn event_count_unions_over_markets() {
        let mut t = TraderTracker::new();
        t.record_placed(acc(1), [mid(7)], 0, false);
        t.record_placed(acc(2), [mid(8)], 0, false);
        t.record_placed(acc(3), [mid(7), mid(8)], 0, false);
        // Union over {7, 8} = {1, 2, 3}.
        assert_eq!(t.event_count(&[mid(7), mid(8)]), 3);
    }

    #[test]
    fn bucket_rolls_on_hour_boundary() {
        let mut t = TraderTracker::new();
        t.record_placed(acc(1), [mid(7)], 0, false);
        t.record_placed(acc(2), [mid(7)], MILLIS_PER_HOUR, false);
        // Two distinct buckets, each holding one placer.
        let snap = t.snapshot();
        assert_eq!(snap.hourly_buckets.len(), 2);
        assert_eq!(snap.hourly_buckets[0].0, 0);
        assert_eq!(snap.hourly_buckets[1].0, MILLIS_PER_HOUR);
    }

    #[test]
    fn bucket_cap_drops_oldest() {
        let mut t = TraderTracker::new();
        // 26 distinct hours → cap 25 retains the latest 25 (the head is dropped).
        for h in 0..26u64 {
            t.record_placed(acc(h), [mid(7)], h * MILLIS_PER_HOUR, false);
        }
        let snap = t.snapshot();
        assert_eq!(snap.hourly_buckets.len(), HOURLY_BUCKET_CAP);
        assert_eq!(snap.hourly_buckets[0].0, MILLIS_PER_HOUR);
        assert_eq!(
            snap.hourly_buckets.last().unwrap().0,
            25 * MILLIS_PER_HOUR
        );
    }

    #[test]
    fn platform_24h_count_is_window_union() {
        let mut t = TraderTracker::new();
        // Three placers in three distinct hourly buckets.
        t.record_placed(acc(1), [mid(7)], 0, false);
        t.record_placed(acc(2), [mid(7)], 12 * MILLIS_PER_HOUR, false);
        t.record_placed(acc(3), [mid(7)], 23 * MILLIS_PER_HOUR, false);
        // At now=24h all three buckets are inside the 24h window
        // (bucket-at-0 right edge = 1h > cutoff = 0).
        assert_eq!(t.platform_24h_count(24 * MILLIS_PER_HOUR), 3);
        // At now=26h, cutoff=2h slides past the bucket-at-0 → drops to two.
        assert_eq!(t.platform_24h_count(26 * MILLIS_PER_HOUR), 2);
    }

    #[test]
    fn snapshot_round_trip_preserves_state() {
        let mut t = TraderTracker::new();
        t.record_placed(acc(1), [mid(7), mid(8)], 0, false);
        t.record_placed(acc(2), [mid(8)], MILLIS_PER_HOUR, false);
        let snap = t.snapshot();
        let restored = TraderTracker::restore(snap);
        assert_eq!(restored.per_market_count(mid(7)), 1);
        assert_eq!(restored.per_market_count(mid(8)), 2);
        assert_eq!(restored.platform_count(), 2);
        assert_eq!(restored.all_market_counts().len(), 2);
    }

    #[test]
    fn snapshot_is_deterministic_for_same_state() {
        let mut a = TraderTracker::new();
        let mut b = TraderTracker::new();
        for n in 0..5u64 {
            a.record_placed(acc(n), [mid(7)], 0, false);
            b.record_placed(acc(n), [mid(7)], 0, false);
        }
        let sa = a.snapshot();
        let sb = b.snapshot();
        assert_eq!(sa.per_market, sb.per_market);
        assert_eq!(sa.platform, sb.platform);
        assert_eq!(sa.hourly_buckets, sb.hourly_buckets);
    }
}
