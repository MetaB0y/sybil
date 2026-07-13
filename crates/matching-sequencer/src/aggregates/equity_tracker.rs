//! Off-block per-account equity series (t, portfolio_value, deposited).
//!
//! Sampled at block finalize: always for accounts that traded this block,
//! plus a periodic sweep over known accounts so price-driven equity changes
//! land between trades. The in-memory ring is a bounded recent-value cache;
//! committed block deltas are exported through the product-history outbox.

use std::collections::{HashMap, HashSet, VecDeque};

use matching_engine::{MarketId, Nanos, signed_notional_nanos};

use crate::account::{AccountId, AccountStore};

/// Minimum wall-clock gap between periodic full sweeps (ms).
pub const EQUITY_SAMPLE_INTERVAL_MS: u64 = 60_000;
/// Max points retained per account (~30 days at one point/minute).
pub const DEFAULT_MAX_RECENT_EQUITY_POINTS: usize = 43_200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EquityPoint {
    pub height: u64,
    pub timestamp_ms: u64,
    pub portfolio_value_nanos: i64,
    pub deposited_nanos: i64,
}

#[derive(Clone)]
pub struct EquityTracker {
    points: HashMap<AccountId, VecDeque<EquityPoint>>,
    known: HashSet<AccountId>,
    last_sweep_ms: u64,
    max_points: usize,
    /// Points appended since the last `clear_pending`. Cleared after commit.
    pending: Vec<(AccountId, EquityPoint)>,
}

impl Default for EquityTracker {
    fn default() -> Self {
        Self::with_retention(DEFAULT_MAX_RECENT_EQUITY_POINTS)
    }
}

/// Portfolio value = balance + Σ qty·price (price defaults to $0.50 when a
/// market has no clearing price yet — matches `compute_portfolio`).
fn portfolio_value_nanos(
    account: &crate::account::Account,
    prices: &HashMap<MarketId, Vec<Nanos>>,
) -> i64 {
    let mut total: i128 = account.balance as i128;
    for (&(market_id, outcome), &qty) in &account.positions {
        if qty == 0 {
            continue;
        }
        let price = prices
            .get(&market_id)
            .and_then(|p| p.get(outcome as usize).copied())
            .unwrap_or(matching_engine::Nanos(
                matching_engine::NANOS_PER_DOLLAR / 2,
            ));
        total += signed_notional_nanos(price, qty) as i128;
    }
    total as i64
}

impl EquityTracker {
    pub fn new() -> Self {
        Self::with_retention(DEFAULT_MAX_RECENT_EQUITY_POINTS)
    }

    pub fn with_retention(max_points: usize) -> Self {
        Self {
            points: HashMap::new(),
            known: HashSet::new(),
            last_sweep_ms: 0,
            max_points,
            pending: Vec::new(),
        }
    }

    /// Seed the swept-account set on restore so periodic sweeps resume for
    /// accounts that existed before restart (otherwise they'd be skipped until
    /// they trade again).
    pub fn seed_known(&mut self, ids: impl IntoIterator<Item = AccountId>) {
        self.known.extend(ids);
    }

    pub fn pending(&self) -> &[(AccountId, EquityPoint)] {
        &self.pending
    }

    pub fn clear_pending(&mut self) {
        self.pending.clear();
    }

    pub fn known_account_count(&self) -> usize {
        self.known.len()
    }

    pub fn retained_account_count(&self) -> usize {
        self.points.len()
    }

    pub fn retained_point_count(&self) -> usize {
        self.points.values().map(VecDeque::len).sum()
    }

    pub fn retention_per_account(&self) -> usize {
        self.max_points
    }

    /// Record equity at block finalize. `touched` = accounts that traded this
    /// block (always sampled); on a periodic sweep, every known account is
    /// sampled too.
    pub fn record(
        &mut self,
        touched: &HashSet<AccountId>,
        accounts: &AccountStore,
        prices: &HashMap<MarketId, Vec<Nanos>>,
        height: u64,
        timestamp_ms: u64,
    ) {
        for &aid in touched {
            self.known.insert(aid);
        }
        let sweep_due =
            timestamp_ms.saturating_sub(self.last_sweep_ms) >= EQUITY_SAMPLE_INTERVAL_MS;
        let candidates: Vec<AccountId> = if sweep_due {
            self.last_sweep_ms = timestamp_ms;
            self.known.iter().copied().collect()
        } else {
            touched.iter().copied().collect()
        };
        for aid in candidates {
            let Some(account) = accounts.get(aid) else {
                continue;
            };
            let point = EquityPoint {
                height,
                timestamp_ms,
                portfolio_value_nanos: portfolio_value_nanos(account, prices),
                deposited_nanos: account.total_deposited,
            };
            self.pending.push((aid, point));
            if self.max_points > 0 {
                let ring = self.points.entry(aid).or_default();
                ring.push_back(point);
                while ring.len() > self.max_points {
                    ring.pop_front();
                }
            }
        }
    }

    /// All retained points for an account, oldest-first.
    pub fn series(&self, account_id: AccountId) -> Vec<EquityPoint> {
        self.points
            .get(&account_id)
            .map(|r| r.iter().copied().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use matching_engine::NANOS_PER_DOLLAR;

    #[test]
    fn samples_touched_then_sweeps() {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(1_000 * NANOS_PER_DOLLAR as i64);
        let prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();

        let mut t = EquityTracker::new();
        let mut touched = HashSet::new();
        touched.insert(aid);

        // First block: touched account sampled.
        t.record(&touched, &accounts, &prices, 1, 1_000);
        assert_eq!(t.series(aid).len(), 1);
        assert_eq!(
            t.series(aid)[0].portfolio_value_nanos,
            1_000 * NANOS_PER_DOLLAR as i64
        );

        // Next block, not due, not touched → no new point.
        t.record(&HashSet::new(), &accounts, &prices, 2, 2_000);
        assert_eq!(t.series(aid).len(), 1);

        // Past the sweep interval → known account sampled even though untouched.
        t.record(
            &HashSet::new(),
            &accounts,
            &prices,
            3,
            1_000 + EQUITY_SAMPLE_INTERVAL_MS,
        );
        assert_eq!(t.series(aid).len(), 2);
    }
}
