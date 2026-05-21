# Portfolio Unified History Feed Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Companion spec:** `docs/superpowers/specs/2026-05-21-portfolio-history-feed-design.md` (this plan implements its "Backend — what/why/how" section). Supersedes the separate closed-positions endpoint (issue #17): realized P&L surfaces inline on `filled`/`resolved` rows instead.

**Goal:** A durable per-account event log + `GET /v1/accounts/{id}/events?limit&before&category` returning a chronological feed of the account's lifecycle events (created, placed, partial_fill, filled, cancelled, expired, deposit, withdrawal, resolved), newest-first, paginated.

**Architecture:** A new off-block `AccountEventLog` sidecar in `AnalyticsState` (same pattern as the other sidecars; never touches `state_root`/`events_root`). It is appended at the moments the sequencer already hits: order admitted (`placed`), fill applied (`partial_fill`/`filled`, with the fill's realized-PnL delta), order cancelled, order expired, and the funding/settlement `SystemEvent`s (`created`/`deposit`/`withdrawal`/`resolved`). Two genuinely new append points are `placed` (at admit) and `expired` (at the expiry-block drop); the rest reuse existing hooks. In-memory bounded ring per account ("since last restart" caveat, same as the other off-block aggregates).

**Tech Stack:** Rust, ractor actor RPC, axum, serde.

**Conventions:** jj VCS; `just fmt`/`just lint`; tests via `cargo test -p <crate>`.

---

## File Structure

- Create `crates/matching-sequencer/src/aggregates/account_event_log.rs` — `HistoryEvent`, `HistoryKind`, `AccountEventLog`, constructors, query/pagination, tests.
- Modify `crates/matching-sequencer/src/aggregates/mod.rs` — export.
- Modify `crates/matching-sequencer/src/analytics.rs` — hold the log; `record_history` + `account_history` methods; init in `new`/`restore` (volatile).
- Modify `crates/matching-sequencer/src/fill_recorder.rs` — append `partial_fill`/`filled` (with realized delta) inside `record_fills`.
- Modify `crates/matching-sequencer/src/sequencer.rs` — append `placed` (batch admit), `cancelled`, `expired`, and the system-event mappings; thread the log into `record_finalized_block`; add `account_history` accessor.
- Modify `crates/matching-sequencer/src/actor.rs` — append `placed` (direct admit); `GetAccountEvents` message/arm/handle.
- Modify `crates/sybil-api-types/src/response.rs` — `HistoryEventResponse`.
- Modify `crates/sybil-api/src/routes/accounts.rs` — `get_account_history` handler + params.
- Modify `crates/sybil-api/src/app.rs` — register `GET /v1/accounts/{id}/events`.
- Test `crates/sybil-api/tests/api_integration.rs`.

---

## Task 1: The `AccountEventLog` sidecar + types

**Files:**
- Create/Test: `crates/matching-sequencer/src/aggregates/account_event_log.rs`
- Modify: `crates/matching-sequencer/src/aggregates/mod.rs`

- [ ] **Step 1: Write the failing unit test**

Create `crates/matching-sequencer/src/aggregates/account_event_log.rs`:

```rust
//! Off-block per-account history feed (the Portfolio "History" tab).
//!
//! Append-on-hook log of an account's lifecycle events. In-memory bounded
//! ring per account; resets on restart (same caveat as the other off-block
//! aggregates). Never enters state_root/events_root.

use std::collections::{HashMap, VecDeque};

use crate::account::AccountId;
use matching_engine::MarketId;

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
    pub side: Option<&'static str>, // "BUY" | "SELL"
    pub outcome: Option<&'static str>, // "YES" | "NO"
    pub qty: Option<u64>,
    pub price_nanos: Option<u64>,
    pub amount_nanos: Option<i64>, // signed cash impact (+in / -out)
    pub realized_pnl_nanos: Option<i64>, // filled / resolved
    pub payout_outcome: Option<&'static str>, // resolved
}

impl HistoryEvent {
    /// Minimal constructor; callers set the optional fields they have.
    pub fn new(account_id: AccountId, kind: HistoryKind, block_height: u64, timestamp_ms: u64) -> Self {
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
        let trades = log.query(AccountId(1), 10, Some("trades"), None.map(|_: ()| "").or(None));
        // (use the explicit form below)
        let trades = log.query(AccountId(1), 10, None, Some("trades"));
        assert_eq!(trades.len(), 2);
        assert!(trades.iter().all(|e| e.kind.category() == "trades"));

        // Cursor: before (3, seq_of_filled) excludes Filled.
        let filled_seq = all[0].seq;
        let page = log.query(AccountId(1), 10, Some((3, filled_seq)), None);
        assert!(page.iter().all(|e| e.kind != HistoryKind::Filled));
    }
}
```

> Note: delete the stray first `let trades = …None.map(…)` line above — it's left only to make the intent obvious; keep the second `Some("trades")` form.

- [ ] **Step 2: Run, verify failure**

Run: `cargo test -p matching-sequencer newest_first_with_category_filter_and_cursor`
Expected: FAIL — module not declared.

- [ ] **Step 3: Export the module**

In `crates/matching-sequencer/src/aggregates/mod.rs`:

```rust
pub mod account_event_log;
pub use account_event_log::{AccountEventLog, HistoryEvent, HistoryKind};
```

- [ ] **Step 4: Run, verify pass**

Run: `cargo test -p matching-sequencer newest_first_with_category_filter_and_cursor`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
just fmt && just lint
jj describe -m "feat(analytics): AccountEventLog sidecar (per-account history feed)"
```

---

## Task 2: Wire into AnalyticsState + accessor

**Files:**
- Modify: `crates/matching-sequencer/src/analytics.rs`
- Modify: `crates/matching-sequencer/src/sequencer.rs` (accessor)

- [ ] **Step 1: Hold the log (volatile)**

In `analytics.rs`: extend the `crate::aggregates::{...}` import with `AccountEventLog, HistoryEvent`. Add the field to `AnalyticsState` (after `equity_tracker` if plan #5 landed, else after `first_deposit_ms`):

```rust
    account_event_log: AccountEventLog,
```

Init in `new` and `restore` (volatile):

```rust
            account_event_log: AccountEventLog::new(),
```

- [ ] **Step 2: Append + query forwarders**

In `analytics.rs`:

```rust
    pub fn record_history(&mut self, event: HistoryEvent) {
        self.account_event_log.append(event);
    }

    pub fn account_history(
        &self,
        account_id: AccountId,
        limit: usize,
        before: Option<(u64, u64)>,
        category: Option<&str>,
    ) -> Vec<HistoryEvent> {
        self.account_event_log.query(account_id, limit, before, category)
    }
```

- [ ] **Step 3: BlockSequencer accessor + a thin append helper**

In `sequencer.rs` (on `BlockSequencer`):

```rust
    pub fn record_history(&mut self, event: crate::aggregates::HistoryEvent) {
        self.analytics.record_history(event);
    }

    pub fn account_history(
        &self,
        account_id: AccountId,
        limit: usize,
        before: Option<(u64, u64)>,
        category: Option<&str>,
    ) -> Vec<crate::aggregates::HistoryEvent> {
        self.analytics.account_history(account_id, limit, before, category)
    }
```

- [ ] **Step 4: Build**

Run: `cargo build -p matching-sequencer`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
just fmt && just lint
jj describe -m "feat(analytics): hold AccountEventLog in AnalyticsState"
```

---

## Task 3: Append hooks

Each hook constructs a `HistoryEvent` and calls `record_history`. Helpers below derive side/outcome/cash from the order or `position_deltas`.

**3a — `placed` (two admit paths)**

- [ ] **Direct admit (`actor.rs`):** in `admit_or_defer`, in the `Admitted { order_id, resting_order }` arm after the durability + trader-tracker hook (line ~883), append:

```rust
                self.sequencer.record_history({
                    use crate::aggregates::{HistoryEvent, HistoryKind};
                    let o = &resting_order.order;
                    let mut e = HistoryEvent::new(resting_order.account_id, HistoryKind::Placed, self.sequencer.height(), now_ms);
                    e.order_id = Some(order_id);
                    e.market_id = o.active_markets().next();
                    e.qty = Some(o.max_fill);
                    e.price_nanos = Some(o.limit_price);
                    let (side, outcome) = crate::aggregates::side_outcome_from_order(o);
                    e.side = side; e.outcome = outcome;
                    e
                });
```

> Implementer note: `now_ms` is the admit timestamp computed in `admit_or_defer` (plan #1, Task B Step 8). `self.sequencer.height()` is the current height accessor (use the existing height getter; if absent, add `pub fn height(&self) -> u64 { self.height }` on `BlockSequencer`). `side_outcome_from_order` — add a small free fn in `account_event_log.rs` (see 3f).

- [ ] **Batch admit (`sequencer.rs`):** in `produce_block_in_place`, in the non-MM accept branch (right after `order_book.accept(...)` succeeds, ~line 2209, where `order`, `account_id`, `order_id`, `self.height`, `timestamp_ms` are in scope), append the same `Placed` event (MM orders skipped — this branch is `!is_mm`):

```rust
                    {
                        use crate::aggregates::{HistoryEvent, HistoryKind};
                        let mut e = HistoryEvent::new(account_id, HistoryKind::Placed, self.height, timestamp_ms);
                        e.order_id = Some(order_id);
                        e.market_id = order.active_markets().next();
                        e.qty = Some(order.max_fill);
                        e.price_nanos = Some(order.limit_price);
                        let (side, outcome) = crate::aggregates::side_outcome_from_order(&order);
                        e.side = side; e.outcome = outcome;
                        self.analytics.record_history(e);
                    }
```

**3b — `partial_fill` / `filled` (in `record_fills`)**

- [ ] **Thread the log into `record_fills`:** change `FillRecorder::record_fills` (fill_recorder.rs:121) to take `event_log: &mut AccountEventLog` as a final parameter, and update the single caller `record_finalized_block` (analytics.rs:248) to pass `&mut self.account_event_log` (disjoint field borrow alongside `&mut self.cost_basis_tracker`).

Inside the per-fill loop (after the `cost_basis_tracker.apply_fill` call, line ~155), append:

```rust
            // History row for this fill, with the realized-PnL delta this fill produced.
            if account_id != AccountId::MINT {
                use crate::aggregates::{HistoryEvent, HistoryKind};
                let realized_before = cost_basis_tracker.realized_pnl(account_id); // capture BEFORE apply_fill
                // ^ move this line ABOVE the apply_fill call; compute delta after.
                let realized_after = cost_basis_tracker.realized_pnl(account_id);
                let kind = if fill.fill_qty == order.max_fill { HistoryKind::Filled } else { HistoryKind::PartialFill };
                let mut e = HistoryEvent::new(account_id, kind, height, timestamp_ms);
                e.order_id = Some(fill.order_id);
                e.qty = Some(fill.fill_qty);
                e.price_nanos = Some(fill.fill_price);
                let (mid, side, outcome, cash) = crate::aggregates::fill_facets(&position_deltas, fill.fill_price);
                e.market_id = mid;
                e.side = side;
                e.outcome = outcome;
                e.amount_nanos = Some(cash);
                let delta = realized_after - realized_before;
                e.realized_pnl_nanos = (delta != 0).then_some(delta);
                event_log.append(e);
            }
```

> Implementer note: move the `let realized_before = …` capture to immediately **before** the existing `cost_basis_tracker.apply_fill(...)` call so the delta is correct; the snippet shows both lines for clarity. `fill_facets` (3f) returns `(Option<MarketId>, Option<&'static str> side, Option<&'static str> outcome, i64 cash)` from `position_deltas` + `fill_price`. Label rule: `fill_qty == order.max_fill` ⇒ the order fully filled this batch ⇒ `filled`, else `partial_fill`.

**3c — `cancelled` (in `cancel_pending_order`)**

- [ ] In `sequencer.rs` `cancel_pending_order` (line 1429), in the `Ok(ro)` arm right after pushing `SystemEvent::OrderCancelled`, append a history row:

```rust
                {
                    use crate::aggregates::{HistoryEvent, HistoryKind};
                    let mut e = HistoryEvent::new(account_id, HistoryKind::Cancelled, self.height, self.last_block_timestamp_ms());
                    e.order_id = Some(order_id);
                    e.market_id = market_ids.first().copied();
                    e.qty = Some(ro.order.max_fill); // remaining at cancel
                    self.analytics.record_history(e);
                }
```

> Implementer note: `cancel_pending_order` runs between blocks; use a wall-clock ms for `timestamp_ms`. If `BlockSequencer` has no timestamp accessor, compute `SystemTime::now()` ms inline (matches the actor's pattern) — replace `self.last_block_timestamp_ms()` accordingly.

**3d — `expired` (at the expiry-block drop)**

- [ ] In `sequencer.rs`, at the expire hook (line 2073-2076, `let expired = self.order_book.expire(self.height); for ro in &expired { record_order_exit(...) }`), append an `Expired` row per `ro`:

```rust
        for ro in &expired {
            self.analytics.record_order_exit(ro, timestamp_ms);
            use crate::aggregates::{HistoryEvent, HistoryKind};
            let mut e = HistoryEvent::new(ro.account_id, HistoryKind::Expired, self.height, timestamp_ms);
            e.order_id = Some(ro.order.id);
            e.market_id = ro.order.active_markets().next();
            e.qty = Some(ro.order.max_fill); // unfilled remainder
            self.analytics.record_history(e);
        }
```

**3e — `created` / `deposit` / `withdrawal` / `resolved` (system-event mappings)**

- [ ] In `sequencer.rs`, in the `produce_block_in_place` system-event loop (lines 1942-2028, where each drained `SystemEvent` is processed), add a history append per variant. Place it inside the existing `match`/loop where each event is already inspected:

```rust
                use crate::aggregates::{HistoryEvent, HistoryKind};
                match event {
                    SystemEvent::CreateAccount { account_id, initial_balance } => {
                        let mut e = HistoryEvent::new(*account_id, HistoryKind::Created, self.height, timestamp_ms);
                        e.amount_nanos = Some(*initial_balance);
                        self.analytics.record_history(e);
                    }
                    SystemEvent::Deposit { account_id, amount } => {
                        let mut e = HistoryEvent::new(*account_id, HistoryKind::Deposit, self.height, timestamp_ms);
                        e.amount_nanos = Some(*amount);
                        self.analytics.record_history(e);
                    }
                    SystemEvent::L1Deposit { account_id, amount, .. } => {
                        let mut e = HistoryEvent::new(*account_id, HistoryKind::Deposit, self.height, timestamp_ms);
                        e.amount_nanos = Some(*amount);
                        self.analytics.record_history(e);
                    }
                    SystemEvent::WithdrawalCreated { account_id, amount, .. } => {
                        let mut e = HistoryEvent::new(*account_id, HistoryKind::Withdrawal, self.height, timestamp_ms);
                        e.amount_nanos = Some(-*amount);
                        self.analytics.record_history(e);
                    }
                    SystemEvent::MarketResolved { market_id, payout_nanos, affected_accounts } => {
                        let payout_outcome = if *payout_nanos >= matching_engine::NANOS_PER_DOLLAR {
                            Some("YES")
                        } else if *payout_nanos == 0 {
                            Some("NO")
                        } else {
                            None
                        };
                        for aid in affected_accounts {
                            let mut e = HistoryEvent::new(*aid, HistoryKind::Resolved, self.height, timestamp_ms);
                            e.market_id = Some(*market_id);
                            e.payout_outcome = payout_outcome;
                            self.analytics.record_history(e);
                        }
                    }
                    SystemEvent::OrderCancelled { .. } => {} // recorded at cancel_pending_order (3c)
                }
```

> Implementer note: this mirrors the variant set already matched in that loop (and in `convert_system_event`, sequencer.rs:398-457). If the existing loop borrows `event` by value vs reference, adjust the `*` derefs. `MarketResolved`'s per-account payout *amount* is left `None` here (the FE renders the resolution; exact per-account payout can be attached later at the settlement site `resolve_market`, where each account's settled value is computed — a follow-up refinement).

**3f — shared derivation helpers**

- [ ] In `account_event_log.rs`, add:

```rust
use matching_engine::{Order, NANOS_PER_DOLLAR};

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
) -> (Option<MarketId>, Option<&'static str>, Option<&'static str>, i64) {
    let mut cash: i128 = 0;
    let mut primary: Option<(MarketId, u8, i64)> = None;
    for &(m, outcome, delta) in position_deltas {
        if delta == 0 {
            continue;
        }
        let entry = if outcome == 0 { fill_price as i64 } else { NANOS_PER_DOLLAR as i64 - fill_price as i64 };
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
```

> Implementer note: confirm `Order::is_seller`, `Order::payoffs`, `num_markets`, `num_states` (used the same way in `sequencer.rs::classify_order_side`). `MarketId`/`Order` imports already present at the top of `account_event_log.rs` after these additions.

- [ ] **Build + sequencer suite**

Run: `cargo test -p matching-sequencer`
Expected: PASS.

- [ ] **Commit**

```bash
just fmt && just lint
jj describe -m "feat(history): append created/placed/fill/cancel/expire/deposit/withdrawal/resolved events"
```

---

## Task 4: Actor RPC + API endpoint

**Files:**
- Modify: `crates/matching-sequencer/src/actor.rs` (enum near line 107; arm near 1421; handle near 2000)
- Modify: `crates/sybil-api-types/src/response.rs`
- Modify: `crates/sybil-api/src/routes/accounts.rs`
- Modify: `crates/sybil-api/src/app.rs`
- Test: `crates/sybil-api/tests/api_integration.rs`

- [ ] **Step 1: Write the failing integration test**

In `api_integration.rs`:

```rust
#[tokio::test]
async fn account_history_shows_placed_then_cancelled() {
    let (app, _) = test_app(true).await;

    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "Hist?" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();
    let (_, body) = post_json(app.clone(), "/v1/accounts", json!({ "initial_balance_nanos": 10_000_000_000u64 })).await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();

    post_json(app.clone(), "/v1/orders", json!({
        "account_id": account_id,
        "orders": [{ "type": "BuyYes", "market_id": market_id, "limit_price_nanos": 400_000_000u64, "quantity": 7 }]
    })).await;

    // Read the order id, then cancel it.
    let (_, body) = get(app.clone(), &format!("/v1/accounts/{account_id}/orders")).await;
    let order_id = parse_json(&body)[0]["order_id"].as_u64().unwrap();
    post_json(app.clone(), "/v1/orders/cancel", json!({ "account_id": account_id, "order_id": order_id })).await;

    let (status, body) = get(app, &format!("/v1/accounts/{account_id}/events?limit=20")).await;
    assert_eq!(status, StatusCode::OK);
    let events = parse_json(&body);
    let events = events.as_array().unwrap();
    let types: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert!(types.contains(&"placed"), "history: {types:?}");
    assert!(types.contains(&"cancelled"), "history: {types:?}");
    // newest-first: cancelled before placed
    let pc = types.iter().position(|t| *t == "cancelled").unwrap();
    let pp = types.iter().position(|t| *t == "placed").unwrap();
    assert!(pc < pp, "expected cancelled newest-first: {types:?}");
}
```

> Implementer note: confirm the cancel route path/payload from `app.rs` (the dev-mode cancel route) — adjust `"/v1/orders/cancel"` if the registered path differs.

- [ ] **Step 2: Run, verify failure**

Run: `cargo test -p sybil-api --test api_integration account_history_shows_placed_then_cancelled`
Expected: FAIL — route 404.

- [ ] **Step 3: Add the response type**

In `response.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct HistoryEventResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub category: String,
    pub timestamp_ms: u64,
    pub block_height: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qty: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_nanos: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount_nanos: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realized_pnl_nanos: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payout_outcome: Option<String>,
}
```

- [ ] **Step 4: Add the actor message, arm, handle**

`actor.rs` enum (after `GetAccountFills`, line 107):

```rust
    GetAccountEvents(
        AccountId,
        usize,
        Option<(u64, u64)>,
        Option<String>,
        RpcReplyPort<Vec<crate::aggregates::HistoryEvent>>,
    ),
```

Match arm (near line 1421):

```rust
            SequencerMsg::GetAccountEvents(account_id, limit, before, category, reply) => {
                let _ = reply.send(state.sequencer.account_history(
                    account_id,
                    limit,
                    before,
                    category.as_deref(),
                ));
            }
```

Handle (near `get_account_fills`, line 2000):

```rust
    pub async fn get_account_events(
        &self,
        account_id: AccountId,
        limit: usize,
        before: Option<(u64, u64)>,
        category: Option<String>,
    ) -> Result<Vec<crate::aggregates::HistoryEvent>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetAccountEvents(account_id, limit, before, category, reply)).await
    }
```

- [ ] **Step 5: Add the route handler**

In `crates/sybil-api/src/routes/accounts.rs` (import `HistoryEventResponse`):

```rust
#[derive(Debug, serde::Deserialize)]
pub struct HistoryParams {
    pub limit: Option<usize>,
    /// Cursor "<block>.<seq>" — return events strictly before it.
    pub before: Option<String>,
    /// "trades" | "funding" | "settlement".
    pub category: Option<String>,
}

fn parse_cursor(s: &str) -> Option<(u64, u64)> {
    let (b, q) = s.split_once('.')?;
    Some((b.parse().ok()?, q.parse().ok()?))
}

/// GET /v1/accounts/{id}/events?limit&before&category
pub async fn get_account_history(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<Vec<HistoryEventResponse>>, AppError> {
    let limit = params.limit.unwrap_or(50).min(500);
    let before = params.before.as_deref().and_then(parse_cursor);
    let events = state
        .sequencer
        .get_account_events(AccountId(id), limit, before, params.category)
        .await?;
    let out: Vec<HistoryEventResponse> = events
        .into_iter()
        .map(|e| HistoryEventResponse {
            id: e.id(),
            event_type: e.kind.as_str().to_string(),
            category: e.kind.category().to_string(),
            timestamp_ms: e.timestamp_ms,
            block_height: e.block_height,
            market_id: e.market_id.map(|m| m.0),
            order_id: e.order_id,
            side: e.side.map(|s| s.to_string()),
            outcome: e.outcome.map(|o| o.to_string()),
            qty: e.qty,
            price_nanos: e.price_nanos,
            amount_nanos: e.amount_nanos,
            realized_pnl_nanos: e.realized_pnl_nanos,
            payout_outcome: e.payout_outcome.map(|p| p.to_string()),
        })
        .collect();
    Ok(Json(out))
}
```

- [ ] **Step 6: Register the route**

In `crates/sybil-api/src/app.rs`, near the other `/v1/accounts/{id}/...` routes:

```rust
        .route(
            "/v1/accounts/{id}/events",
            axum::routing::get(routes::accounts::get_account_history),
        )
```

- [ ] **Step 7: Run, verify pass**

Run: `cargo test -p sybil-api --test api_integration account_history_shows_placed_then_cancelled`
Expected: PASS.

- [ ] **Step 8: Full suites**

Run: `cargo test -p matching-sequencer && cargo test -p sybil-api`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
just fmt && just lint
jj describe -m "feat(api): GET /v1/accounts/{id}/events unified history feed"
```

---

## Task 5: Manual local verification

- [ ] **Step 1: Drive a full lifecycle and read the feed**

```bash
cargo run --release -p sybil-api -- --dev-mode --port 3001 &
MID=$(curl -s -XPOST localhost:3001/v1/markets -H 'content-type: application/json' -d '{"name":"hist"}' | jq .market_id)
AID=$(curl -s -XPOST localhost:3001/v1/accounts -H 'content-type: application/json' -d '{"initial_balance_nanos":10000000000}' | jq .account_id)
curl -s -XPOST localhost:3001/v1/orders -H 'content-type: application/json' \
  -d "{\"account_id\":$AID,\"orders\":[{\"type\":\"BuyYes\",\"market_id\":$MID,\"limit_price_nanos\":400000000,\"quantity\":7}]}" >/dev/null
OID=$(curl -s "localhost:3001/v1/accounts/$AID/orders" | jq '.[0].order_id')
curl -s -XPOST localhost:3001/v1/orders/cancel -H 'content-type: application/json' -d "{\"account_id\":$AID,\"order_id\":$OID}" >/dev/null
curl -s "localhost:3001/v1/accounts/$AID/events?limit=20" | jq '[.[] | {type, category, market_id, qty, price_nanos, amount_nanos}]'
```

Expected: newest-first rows including `created` (funding), `placed` (trades), and `cancelled` (trades). Filter check: `…/events?category=funding` returns only `created`.

---

## Self-Review Notes

- **Spec coverage (design doc taxonomy):** created ✅(3e), placed ✅(3a ×2), partial_fill/filled ✅(3b, with realized delta), cancelled ✅(3c), expired ✅(3d), deposit ✅(3e), withdrawal ✅(3e), resolved ✅(3e). Endpoint + cursor pagination + category filter ✅(Task 4). Realized P&L inline on `filled` rows ✅; `resolved` per-account amount noted as a follow-up.
- **Two new append points:** `placed` (admit) and `expired` (expiry drop) — as the spec calls out; the rest reuse existing hooks.
- **Supersedes #17:** no `/closed` endpoint; realized P&L is inline.
- **Type consistency:** internal `HistoryEvent`/`HistoryKind` ↔ `account_history` RPC ↔ `HistoryEventResponse` (`type`/`category` from `kind.as_str()`/`kind.category()`).
- **Caveat:** in-memory ring (`MAX_HISTORY_EVENTS_PER_ACCOUNT`), resets on restart — same documented behavior as the other off-block aggregates; "full history since creation" needs persistence (future).
- **Implementer notes (bounded):** `record_fills` realized-before capture ordering (3b); `BlockSequencer::height()`/cancel timestamp accessors (3a/3c); system-event loop borrow form (3e); cancel route path in the test (Task 4); `Order` field accessors for the helpers (3f). All have exact formulas/locations — no open design questions.
