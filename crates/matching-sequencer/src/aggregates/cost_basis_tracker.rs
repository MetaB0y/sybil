//! Off-block weighted-average cost basis (WAC) + realized PnL tracker.
//!
//! Sidecar to `FillRecorder` — does not enter `state_root` / `events_root` /
//! `BlockWitness`. Hooked into `FillRecorder.record_fills` (via `apply_fill`)
//! and into `Sequencer::resolve_market` + `Sequencer::resolve_market_attested`
//! (via `apply_resolution`).
//!
//! ### Side / price convention
//!
//! `position_deltas` is the `Vec<(MarketId, u8, i64)>` returned by
//! `compute_fill_settlement`. `outcome == 0` → YES, `outcome == 1` → NO.
//! `fill_price` is the YES clearing price in nanos. The cost-basis
//! side-price is:
//!
//! - YES (outcome=0): `entry_price = fill_price`
//! - NO  (outcome=1): `entry_price = NANOS_PER_DOLLAR - fill_price`
//!
//! ### WAC update rule
//!
//! `prior_qty` is the position BEFORE the fill; `delta` is the change.
//! Both signed (the matching engine permits short positions in principle).
//!
//! - **Same sign (or opening from 0):** weighted-average basis update.
//! - **Opposite sign (reducing):** realize PnL on the overlap, leave basis
//!   alone for the residual. If the position fully closes, drop the entry;
//!   if it flips sign, reset basis to `entry_price` for the residual.
//!
//! ### Resolution rule
//!
//! On `apply_resolution(market, payout_nanos, positions)`, for each
//! `(account, outcome, qty)`:
//! - YES side close-price = `payout_nanos`
//! - NO  side close-price = `NANOS_PER_DOLLAR - payout_nanos`
//! - Long  (qty > 0): `realized += (close_price - basis) * qty`
//! - Short (qty < 0): `realized += (basis - close_price) * |qty|`
//!
//! All basis entries for that market are dropped after the realize sweep.
//!
//! ### MINT exclusion
//!
//! `AccountId::MINT` is a system account; its fills aren't user trades and
//! are excluded inside `apply_fill` (early return). Resolutions ignore MINT
//! for the same reason.

use std::collections::HashMap;

use matching_engine::{MarketId, NANOS_PER_DOLLAR};
use serde::{Deserialize, Serialize};

use crate::account::{Account, AccountId};

#[derive(Clone, Debug, Default)]
pub struct CostBasisTracker {
    /// Average entry price per `(account, market, outcome)`. Nanos.
    basis: HashMap<(AccountId, MarketId, u8), i64>,
    /// Accumulated realized PnL per account. Nanos.
    realized: HashMap<AccountId, i64>,
}

impl CostBasisTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn restore(snapshot: CostBasisTrackerSnapshot) -> Self {
        Self {
            basis: snapshot.basis.into_iter().collect(),
            realized: snapshot.realized.into_iter().collect(),
        }
    }

    pub fn snapshot(&self) -> CostBasisTrackerSnapshot {
        let mut basis: Vec<((AccountId, MarketId, u8), i64)> = self
            .basis
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();
        basis.sort_by_key(|((a, m, o), _)| (a.0, m.0, *o));

        let mut realized: Vec<(AccountId, i64)> = self
            .realized
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();
        realized.sort_by_key(|(a, _)| a.0);

        CostBasisTrackerSnapshot { basis, realized }
    }

    /// Apply one fill's per-tuple deltas. `account` reflects post-fill
    /// positions; the prior position is `current - delta`. MINT short-
    /// circuits at the top (system account, not a trader).
    pub fn apply_fill(
        &mut self,
        account_id: AccountId,
        position_deltas: &[(MarketId, u8, i64)],
        fill_price: i64,
        account: &Account,
    ) {
        if account_id == AccountId::MINT {
            return;
        }
        for &(market_id, outcome, delta) in position_deltas {
            if delta == 0 {
                continue;
            }
            let entry_price = entry_price_for_outcome(fill_price, outcome);
            let post_qty = account.position(market_id, outcome);
            let prior_qty = post_qty - delta;
            self.apply_one_delta(account_id, market_id, outcome, prior_qty, delta, entry_price);
        }
    }

    fn apply_one_delta(
        &mut self,
        account_id: AccountId,
        market_id: MarketId,
        outcome: u8,
        prior_qty: i64,
        delta: i64,
        entry_price: i64,
    ) {
        let key = (account_id, market_id, outcome);
        let prior_basis = self.basis.get(&key).copied().unwrap_or(0);
        let opening = prior_qty == 0 || prior_qty.signum() == delta.signum();

        if opening {
            let prior_abs = prior_qty.unsigned_abs() as i128;
            let delta_abs = delta.unsigned_abs() as i128;
            let total_abs = prior_abs + delta_abs;
            if total_abs == 0 {
                return;
            }
            let new_basis =
                (prior_basis as i128 * prior_abs + entry_price as i128 * delta_abs) / total_abs;
            self.basis.insert(key, new_basis as i64);
            return;
        }

        // Opposite sign — reducing or flipping.
        let close_qty = delta.unsigned_abs().min(prior_qty.unsigned_abs()) as i128;
        let pnl = if prior_qty > 0 {
            (entry_price as i128 - prior_basis as i128) * close_qty
        } else {
            (prior_basis as i128 - entry_price as i128) * close_qty
        };
        *self.realized.entry(account_id).or_insert(0) += pnl as i64;

        let post_qty = prior_qty + delta;
        if post_qty == 0 {
            self.basis.remove(&key);
        } else if post_qty.signum() != prior_qty.signum() {
            self.basis.insert(key, entry_price);
        }
    }

    /// Realize PnL for every open position in `market_id` at the resolution
    /// payout, then drop the basis entries. `positions` must be captured
    /// BEFORE `settlement::resolve_market` zeroes the account positions.
    /// MINT entries (if any) are skipped.
    pub fn apply_resolution<I>(&mut self, market_id: MarketId, payout_nanos: i64, positions: I)
    where
        I: IntoIterator<Item = (AccountId, u8, i64)>,
    {
        for (account_id, outcome, qty) in positions {
            if account_id == AccountId::MINT || qty == 0 {
                continue;
            }
            let close_price = entry_price_for_outcome(payout_nanos, outcome);
            let key = (account_id, market_id, outcome);
            let basis = self.basis.get(&key).copied().unwrap_or(0);
            let pnl = if qty > 0 {
                (close_price as i128 - basis as i128) * qty as i128
            } else {
                (basis as i128 - close_price as i128) * qty.unsigned_abs() as i128
            };
            *self.realized.entry(account_id).or_insert(0) += pnl as i64;
            self.basis.remove(&key);
        }
    }

    /// Average entry price for one (account, market, outcome). `0` if the
    /// account holds no position there.
    pub fn cost_basis(&self, account_id: AccountId, market_id: MarketId, outcome: u8) -> i64 {
        self.basis
            .get(&(account_id, market_id, outcome))
            .copied()
            .unwrap_or(0)
    }

    /// Accumulated realized PnL for one account.
    pub fn realized_pnl(&self, account_id: AccountId) -> i64 {
        self.realized.get(&account_id).copied().unwrap_or(0)
    }
}

fn entry_price_for_outcome(yes_price_nanos: i64, outcome: u8) -> i64 {
    if outcome == 0 {
        yes_price_nanos
    } else {
        (NANOS_PER_DOLLAR as i64) - yes_price_nanos
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CostBasisTrackerSnapshot {
    pub basis: Vec<((AccountId, MarketId, u8), i64)>,
    pub realized: Vec<(AccountId, i64)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::Account;
    use matching_engine::NANOS_PER_DOLLAR;

    fn aid(n: u64) -> AccountId {
        AccountId(n)
    }

    fn mid(n: u32) -> MarketId {
        MarketId::new(n)
    }

    fn account_with(market: MarketId, outcome: u8, qty: i64) -> Account {
        let id = aid(1);
        let mut account = Account::new(id, 0);
        account.positions.insert((market, outcome), qty);
        account
    }

    #[test]
    fn apply_fill_basic() {
        // Open YES at 0.40, then add at 0.60: WAC should be 0.50 for 2 lots.
        let mut t = CostBasisTracker::new();
        let m = mid(1);
        let a = aid(7);

        let mut account = Account::new(a, 0);
        // First fill: buy 5 YES at 0.40
        account.positions.insert((m, 0), 5);
        let p1 = (NANOS_PER_DOLLAR as i64) * 4 / 10;
        t.apply_fill(a, &[(m, 0, 5)], p1, &account);
        assert_eq!(t.cost_basis(a, m, 0), p1);

        // Second fill: buy 5 more YES at 0.60 → WAC = 0.50
        account.positions.insert((m, 0), 10);
        let p2 = (NANOS_PER_DOLLAR as i64) * 6 / 10;
        t.apply_fill(a, &[(m, 0, 5)], p2, &account);
        assert_eq!(t.cost_basis(a, m, 0), (p1 + p2) / 2);

        // Realized still zero (no closes).
        assert_eq!(t.realized_pnl(a), 0);
    }

    #[test]
    fn apply_fill_excludes_mint() {
        let mut t = CostBasisTracker::new();
        let m = mid(1);
        let account = account_with(m, 0, 5);
        let p = NANOS_PER_DOLLAR as i64 / 2;
        t.apply_fill(AccountId::MINT, &[(m, 0, 5)], p, &account);

        // No basis, no realized — MINT short-circuits.
        assert_eq!(t.cost_basis(AccountId::MINT, m, 0), 0);
        assert_eq!(t.realized_pnl(AccountId::MINT), 0);
    }

    #[test]
    fn apply_resolution_realizes() {
        // Long YES at 0.40, market resolves YES (payout = NANOS_PER_DOLLAR).
        // Expected realized = (1.00 - 0.40) * 10 = 0.60 * 10 = 6e9 nanos.
        let mut t = CostBasisTracker::new();
        let m = mid(1);
        let a = aid(7);

        let mut account = Account::new(a, 0);
        account.positions.insert((m, 0), 10);
        let p = (NANOS_PER_DOLLAR as i64) * 4 / 10;
        t.apply_fill(a, &[(m, 0, 10)], p, &account);
        assert_eq!(t.cost_basis(a, m, 0), p);

        let payout = NANOS_PER_DOLLAR as i64;
        t.apply_resolution(m, payout, [(a, 0u8, 10i64)]);

        let expected = ((NANOS_PER_DOLLAR as i64) - p) * 10;
        assert_eq!(t.realized_pnl(a), expected);
        // Basis cleared after resolution.
        assert_eq!(t.cost_basis(a, m, 0), 0);
    }

    #[test]
    fn cost_basis_snapshot_roundtrip() {
        let mut t = CostBasisTracker::new();
        let m = mid(1);
        let a = aid(7);

        let mut account = Account::new(a, 0);
        account.positions.insert((m, 0), 4);
        t.apply_fill(a, &[(m, 0, 4)], NANOS_PER_DOLLAR as i64 / 2, &account);

        // Close 2 at higher price: should produce some realized.
        account.positions.insert((m, 0), 2);
        t.apply_fill(
            a,
            &[(m, 0, -2)],
            (NANOS_PER_DOLLAR as i64) * 6 / 10,
            &account,
        );

        let snap = t.snapshot();
        let restored = CostBasisTracker::restore(snap);
        assert_eq!(restored.cost_basis(a, m, 0), t.cost_basis(a, m, 0));
        assert_eq!(restored.realized_pnl(a), t.realized_pnl(a));
    }

    #[test]
    fn realized_pnl_after_resolution() {
        // Two opens at 0.30 and 0.50 (WAC = 0.40), market resolves NO (payout=0).
        // Holder of YES loses entirely: realized = (0 - 0.40) * 10 = -4e9 nanos.
        let mut t = CostBasisTracker::new();
        let m = mid(1);
        let a = aid(7);

        let mut account = Account::new(a, 0);
        account.positions.insert((m, 0), 5);
        t.apply_fill(a, &[(m, 0, 5)], (NANOS_PER_DOLLAR as i64) * 3 / 10, &account);
        account.positions.insert((m, 0), 10);
        t.apply_fill(a, &[(m, 0, 5)], (NANOS_PER_DOLLAR as i64) * 5 / 10, &account);
        let basis = t.cost_basis(a, m, 0);
        // 0.30 and 0.50 average to 0.40 exactly with equal-quantity legs.
        assert_eq!(basis, (NANOS_PER_DOLLAR as i64) * 4 / 10);

        t.apply_resolution(m, 0, [(a, 0u8, 10i64)]);
        assert_eq!(t.realized_pnl(a), -basis * 10);
    }
}
