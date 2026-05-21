//! Off-block per-account history feed (the Portfolio "History" tab).
//!
//! Append-on-hook log of an account's lifecycle events. In-memory bounded
//! ring per account; resets on restart (same caveat as the other off-block
//! aggregates). Never enters state_root/events_root.

use std::collections::{HashMap, VecDeque};

use crate::account::AccountId;
use matching_engine::{MarketId, Order, NANOS_PER_DOLLAR};

pub const MAX_HISTORY_EVENTS_PER_ACCOUNT: usize = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryKind {
    Created,
    Placed,
    PartialFill,
    Filled,
    Cancelled,
    Expired,
    Deposit,
    Withdrawal,
    Resolved,
}

impl HistoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HistoryKind::Created => "created",
            HistoryKind::Placed => "placed",
            HistoryKind::PartialFill => "partial_fill",
            HistoryKind::Filled => "filled",
            HistoryKind::Cancelled => "cancelled",
            HistoryKind::Expired => "expired",
            HistoryKind::Deposit => "deposit",
            HistoryKind::Withdrawal => "withdrawal",
            HistoryKind::Resolved => "resolved",
        }
    }
    /// Filter-chip bucket.
    pub fn category(self) -> &'static str {
        match self {
            HistoryKind::Created | HistoryKind::Deposit | HistoryKind::Withdrawal => "funding",
            HistoryKind::Resolved => "settlement",
            _ => "trades",
        }
    }
}

#[derive(Clone, Debug)]
pub struct HistoryEvent {
    pub account_id: AccountId,
    pub seq: u64,
    pub block_height: u64,
    pub timestamp_ms: u64,
    pub kind: HistoryKind,
    pub market_id: Option<MarketId>,
    pub order_id: Option<u64>,
    pub side: Option<&'static str>,    // "BUY" | "SELL"
    pub outcome: Option<&'static str>, // "YES" | "NO"
    pub qty: Option<u64>,
    pub price_nanos: Option<u64>,
    pub amount_nanos: Option<i64>, // signed cash impact (+in / -out)
    pub realized_pnl_nanos: Option<i64>, // filled / resolved
    pub payout_outcome: Option<&'static str>, // resolved
}

impl HistoryEvent {
    /// Minimal constructor; callers set the optional fields they have.
    pub fn new(
        account_id: AccountId,
        kind: HistoryKind,
        block_height: u64,
        timestamp_ms: u64,
    ) -> Self {
        Self {
            account_id,
            seq: 0,
            block_height,
            timestamp_ms,
            kind,
            market_id: None,
            order_id: None,
            side: None,
            outcome: None,
            qty: None,
            price_nanos: None,
            amount_nanos: None,
            realized_pnl_nanos: None,
            payout_outcome: None,
        }
    }
    pub fn id(&self) -> String {
        format!("{}.{}", self.block_height, self.seq)
    }
}

#[derive(Clone, Default)]
pub struct AccountEventLog {
    events: HashMap<AccountId, VecDeque<HistoryEvent>>,
    next_seq: u64,
}

impl AccountEventLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one event (assigns the global seq, trims the per-account ring).
    pub fn append(&mut self, mut event: HistoryEvent) {
        event.seq = self.next_seq;
        self.next_seq += 1;
        let ring = self.events.entry(event.account_id).or_default();
        ring.push_back(event);
        while ring.len() > MAX_HISTORY_EVENTS_PER_ACCOUNT {
            ring.pop_front();
        }
    }

    /// Newest-first page. `before` = exclusive cursor `(block_height, seq)`;
    /// `category` filters by `HistoryKind::category`.
    pub fn query(
        &self,
        account_id: AccountId,
        limit: usize,
        before: Option<(u64, u64)>,
        category: Option<&str>,
    ) -> Vec<HistoryEvent> {
        let Some(ring) = self.events.get(&account_id) else {
            return Vec::new();
        };
        ring.iter()
            .rev() // newest-first
            .filter(|e| match before {
                Some((b, s)) => (e.block_height, e.seq) < (b, s),
                None => true,
            })
            .filter(|e| category.is_none_or(|c| e.kind.category() == c))
            .take(limit)
            .cloned()
            .collect()
    }
}

/// BUY/SELL + YES/NO from an order's payoff structure (binary markets).
pub fn side_outcome_from_order(order: &Order) -> (Option<&'static str>, Option<&'static str>) {
    if order.num_markets != 1 || order.num_states != 2 {
        return (Some(if order.is_seller() { "SELL" } else { "BUY" }), None);
    }
    match (order.payoffs[0], order.payoffs[1]) {
        (1, 0) => (Some("BUY"), Some("YES")),
        (0, 1) => (Some("BUY"), Some("NO")),
        (-1, 0) => (Some("SELL"), Some("YES")),
        (0, -1) => (Some("SELL"), Some("NO")),
        _ => (None, None),
    }
}

/// From a fill's `position_deltas` + YES clearing price, derive the primary
/// market, side, outcome, and signed cash impact (+in / -out).
pub fn fill_facets(
    position_deltas: &[(MarketId, u8, i64)],
    fill_price: u64,
) -> (
    Option<MarketId>,
    Option<&'static str>,
    Option<&'static str>,
    i64,
) {
    let mut cash: i128 = 0;
    let mut primary: Option<(MarketId, u8, i64)> = None;
    for &(m, outcome, delta) in position_deltas {
        if delta == 0 {
            continue;
        }
        let entry = if outcome == 0 {
            fill_price as i64
        } else {
            NANOS_PER_DOLLAR as i64 - fill_price as i64
        };
        // buying (delta>0) spends cash; selling (delta<0) receives cash
        cash -= delta as i128 * entry as i128;
        if primary.is_none_or(|(_, _, d)| delta.unsigned_abs() > d.unsigned_abs()) {
            primary = Some((m, outcome, delta));
        }
    }
    match primary {
        Some((m, outcome, delta)) => (
            Some(m),
            Some(if delta > 0 { "BUY" } else { "SELL" }),
            Some(if outcome == 0 { "YES" } else { "NO" }),
            cash as i64,
        ),
        None => (None, None, None, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(log: &mut AccountEventLog, aid: u64, kind: HistoryKind, block: u64, ts: u64) {
        log.append(HistoryEvent::new(AccountId(aid), kind, block, ts));
    }

    #[test]
    fn newest_first_with_category_filter_and_cursor() {
        let mut log = AccountEventLog::new();
        ev(&mut log, 1, HistoryKind::Created, 1, 100); // funding
        ev(&mut log, 1, HistoryKind::Placed, 2, 200); // trades
        ev(&mut log, 1, HistoryKind::Filled, 3, 300); // trades
        ev(&mut log, 2, HistoryKind::Deposit, 4, 400); // other account

        // Newest-first for account 1.
        let all = log.query(AccountId(1), 10, None, None);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].kind, HistoryKind::Filled);

        // Category filter.
        let trades = log.query(AccountId(1), 10, None, Some("trades"));
        assert_eq!(trades.len(), 2);
        assert!(trades.iter().all(|e| e.kind.category() == "trades"));

        // Cursor: before (3, seq_of_filled) excludes Filled.
        let filled_seq = all[0].seq;
        let page = log.query(AccountId(1), 10, Some((3, filled_seq)), None);
        assert!(page.iter().all(|e| e.kind != HistoryKind::Filled));
    }
}
