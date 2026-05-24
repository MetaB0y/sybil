# Equity & History redb Offload — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the two unbounded-by-account in-memory aggregates — the per-account **equity series** and **history event log** — out of the heap and into the existing redb store, writing them as append-only rows per block and reading them back on demand, so `sybil-api` RAM stops growing with the (ever-increasing) account population.

**Architecture:** Each block already deep-clones the whole sequencer (`prepare_block` → `self.clone()`) and persists a snapshot through `Store::save_block_with_witness`. We piggyback on that: the equity/history trackers accumulate this block's new items in a small `pending` buffer; the persist step writes those rows into two new redb tables (`EQUITY_POINTS`, `HISTORY_EVENTS`); the in-memory rings are kept only as a fallback (configurable cap, set to a tiny value in prod). Reads go through the actor (Option A): when a store is present the handler range-scans redb; otherwise it uses the in-memory ring (tests with no store). The public HTTP contract and every frontend hook are unchanged.

**Tech Stack:** Rust, redb (embedded KV, range scans, MVCC), rmp-serde (msgpack), ractor (actor), axum (API).

---

## Background facts established during investigation (read once)

- **Read path (unchanged contract):** `GET /v1/accounts/{id}/equity` → `SequencerHandle::get_equity_series` → `SequencerMsg::GetEquitySeries` handler (`actor.rs:1494`) → `sequencer.equity_series`. `GET /v1/accounts/{id}/events` → `get_account_events` → `SequencerMsg::GetAccountEvents` handler (`actor.rs:1497`) → `sequencer.account_history`. Response shapes `EquitySeriesResponse` / `HistoryEventResponse[]` are built in `routes/accounts.rs` and consumed by `frontend/web/src/lib/account/use-account-history.ts`, `use-account-events.ts`, and the equity/portfolio hooks. **We must keep the actor method signatures and return types identical.**
- **Write path:** `on_tick` (`actor.rs:511`) → `prepare_block` (clones sequencer, records this block's analytics on the clone) → `persist_block` → `Store::save_block_with_witness(prepared.next_sequencer().snapshot(), …)` → `commit_prepared_block` (`*self = next_sequencer`). `ProduceBlock` (test path, `actor.rs:1321`) calls `on_tick`, so persist runs in tests too.
- **History events are recorded on the LIVE sequencer during order admission/cancel** (`actor.rs:934`; cancels via `cancel_pending_order`, which contains the `record_history` at `sequencer.rs:1489`, reached from `actor.rs:754`/`:893`) AND on the clone during production (`sequencer.rs:2102`–`:2232`). Equity is recorded once per block during production (`sequencer.rs:2605`). Therefore `pending` must be cleared **after commit**, not at production start, so admission-time events since the last commit are included. Verified safe: `ractor` processes one mailbox message to completion across `.await`, so no admission message interleaves the `on_tick` prepare→persist→commit→clear sequence; and redb keys are idempotent (`(account,height)` / `(account,height,seq)`), so a persist-failure retry re-derives identical keys — no loss, no duplication.
- **redb store exists in prod** (`docker-compose.prod.yml`: `SYBIL_DATA_DIR=/data`, volume `sybil-data:/data`) and in `_with_store` tests; `test_app` runs with **no** store. Equity/history currently reset on restart only because they are absent from `AnalyticsRestoredState` (`store.rs:285`), not because there is no store.
- **Tables are pre-created in `Store::open`** (`store.rs:375-400`); restore reads them with `table.iter()` (`store.rs:834+`). Existing per-record table precedent: `FILL_HISTORY` keyed by a 24-byte `(account_id, …)` key (`store.rs:141`, `fill_history_key` `store.rs:218`).
- **Gotcha:** `HistoryEvent` (`aggregates/account_event_log.rs:51`) has `side`/`outcome`/`payout_outcome` typed `Option<&'static str>`. These serialize but cannot deserialize into `&'static str`, so persistence uses an owned DTO (`StoredHistoryEvent`) with explicit conversions.

---

## File structure

- `crates/matching-sequencer/src/aggregates/equity_tracker.rs` — add serde to `EquityPoint`; add `pending` + configurable cap; constructor with retention.
- `crates/matching-sequencer/src/aggregates/account_event_log.rs` — add serde to `HistoryKind`; add `pending` + configurable cap; constructor with retention.
- `crates/matching-sequencer/src/store.rs` — new tables, key helpers, `StoredHistoryEvent` DTO + conversions, write rows in `save_block_with_witness`, new `equity_series`/`account_events` read methods, pre-create tables.
- `crates/matching-sequencer/src/analytics.rs` — thread configurable caps into the two trackers; expose per-block `pending` deltas on the snapshot; `clear_pending`.
- `crates/matching-sequencer/src/sequencer.rs` — add `clear_offblock_pending` call in `commit_prepared_block`; add cap fields to `SequencerConfig` + its `Default`; seed equity sweep in `restore`.
- `crates/matching-sequencer/src/actor.rs` — read handlers use the store when present.
- `crates/matching-sequencer/src/aggregates/mod.rs` — **re-export** `MAX_EQUITY_POINTS`, `MAX_HISTORY_EVENTS_PER_ACCOUNT`, `StoredHistoryEvent` (currently NOT re-exported — mandatory for the `crate::aggregates::…` paths to resolve).
- `crates/sybil-api/src/config.rs` — add the two clap/env fields to `ApiConfig` **and** to its `impl Default`.
- `crates/sybil-api/src/main.rs` — add the two fields to the `SequencerConfig { … }` literal at `main.rs:183-200` (this is where `ApiConfig`→`SequencerConfig` mapping actually lives, NOT config.rs).
- Note: the HTTP response types (`HistoryEventResponse`/`EquitySeriesResponse`) live in `crates/sybil-api-types/src/response.rs` and are **left untouched** — contract preserved.
- `crates/sybil-api/tests/api_integration.rs` — new store-backed round-trip tests.
- `docker-compose.prod.yml` — set the two caps to `0` for prod.

---

## Task 1: Make the stored types serde-friendly + history DTO

**Files:**
- Modify: `crates/matching-sequencer/src/aggregates/equity_tracker.rs:19-25`
- Modify: `crates/matching-sequencer/src/aggregates/account_event_log.rs:14-25,51-67`
- Test: `crates/matching-sequencer/src/store.rs` (new `#[cfg(test)]` round-trip)

- [ ] **Step 1: Add serde derives to `EquityPoint`**

In `equity_tracker.rs`, change the derive on `EquityPoint`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EquityPoint {
    pub height: u64,
    pub timestamp_ms: u64,
    pub portfolio_value_nanos: i64,
    pub deposited_nanos: i64,
}
```

- [ ] **Step 2: Add serde derives to `HistoryKind`**

In `account_event_log.rs`, change the derive on `HistoryKind`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HistoryKind {
```

(`HistoryEvent` itself stays as-is — it keeps the `&'static str` fields; only the DTO below is serialized.)

- [ ] **Step 3: Add the storage DTO + conversions**

Append to `crates/matching-sequencer/src/aggregates/account_event_log.rs` (after `HistoryEvent`'s `impl`):

```rust
/// Owned, serde-safe mirror of [`HistoryEvent`] for persistence. The live type
/// keeps `Option<&'static str>` (cheap, no alloc) which serializes but cannot
/// deserialize; this DTO stores those as owned `String` and maps them back to
/// the known 'static literals on read.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StoredHistoryEvent {
    pub account_id: u64,
    pub seq: u64,
    pub block_height: u64,
    pub timestamp_ms: u64,
    pub kind: HistoryKind,
    pub market_id: Option<u32>,
    pub order_id: Option<u64>,
    pub side: Option<String>,
    pub outcome: Option<String>,
    pub qty: Option<u64>,
    pub price_nanos: Option<u64>,
    pub amount_nanos: Option<i64>,
    pub realized_pnl_nanos: Option<i64>,
    pub payout_outcome: Option<String>,
}

/// Map a stored side/outcome string back to its 'static literal. Returns `None`
/// for unknown values (defensive; only "BUY"/"SELL"/"YES"/"NO" are ever stored).
fn static_label(s: &str) -> Option<&'static str> {
    match s {
        "BUY" => Some("BUY"),
        "SELL" => Some("SELL"),
        "YES" => Some("YES"),
        "NO" => Some("NO"),
        _ => None,
    }
}

impl StoredHistoryEvent {
    pub fn from_event(e: &HistoryEvent) -> Self {
        Self {
            account_id: e.account_id.0,
            seq: e.seq,
            block_height: e.block_height,
            timestamp_ms: e.timestamp_ms,
            kind: e.kind,
            market_id: e.market_id.map(|m| m.0),
            order_id: e.order_id,
            side: e.side.map(|s| s.to_string()),
            outcome: e.outcome.map(|s| s.to_string()),
            qty: e.qty,
            price_nanos: e.price_nanos,
            amount_nanos: e.amount_nanos,
            realized_pnl_nanos: e.realized_pnl_nanos,
            payout_outcome: e.payout_outcome.map(|s| s.to_string()),
        }
    }

    pub fn into_event(self) -> HistoryEvent {
        HistoryEvent {
            account_id: AccountId(self.account_id),
            seq: self.seq,
            block_height: self.block_height,
            timestamp_ms: self.timestamp_ms,
            kind: self.kind,
            market_id: self.market_id.map(MarketId::new),
            order_id: self.order_id,
            side: self.side.as_deref().and_then(static_label),
            outcome: self.outcome.as_deref().and_then(static_label),
            qty: self.qty,
            price_nanos: self.price_nanos,
            amount_nanos: self.amount_nanos,
            realized_pnl_nanos: self.realized_pnl_nanos,
            payout_outcome: self.payout_outcome.as_deref().and_then(static_label),
        }
    }
}
```

- [ ] **Step 4: Run the existing aggregate tests to confirm nothing broke**

Run: `cargo test -p matching-sequencer aggregates::`
Expected: PASS (existing equity/history unit tests unaffected).

- [ ] **Step 5: Commit**

```bash
git add crates/matching-sequencer/src/aggregates/equity_tracker.rs crates/matching-sequencer/src/aggregates/account_event_log.rs
git commit -m "feat(persist): serde-enable EquityPoint/HistoryKind + StoredHistoryEvent DTO"
```

---

## Task 2: redb tables, key helpers, and store read/write methods

**Files:**
- Modify: `crates/matching-sequencer/src/store.rs` (table consts near `:141`; helpers near `:218`; pre-create in `open` near `:388`; new methods in `impl Store`)
- Test: `crates/matching-sequencer/src/store.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Add table definitions** (next to `FILL_HISTORY` at `store.rs:141`)

```rust
/// Per-account equity series. Key = account_id(8B BE) ++ height(8B BE); one
/// point per (account, block). Value = rmp-serde EquityPoint. Off-block.
const EQUITY_POINTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("equity_points");

/// Per-account history feed. Key = account_id(8B BE) ++ block_height(8B BE) ++
/// seq(8B BE). Value = rmp-serde StoredHistoryEvent. Off-block.
const HISTORY_EVENTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("history_events");
```

- [ ] **Step 2: Add key helpers** (next to `fill_history_key` at `store.rs:218`)

```rust
fn equity_key(account_id: AccountId, height: u64) -> [u8; 16] {
    let mut k = [0u8; 16];
    k[..8].copy_from_slice(&account_id.0.to_be_bytes());
    k[8..].copy_from_slice(&height.to_be_bytes());
    k
}

fn history_event_key(account_id: AccountId, block_height: u64, seq: u64) -> [u8; 24] {
    let mut k = [0u8; 24];
    k[..8].copy_from_slice(&account_id.0.to_be_bytes());
    k[8..16].copy_from_slice(&block_height.to_be_bytes());
    k[16..].copy_from_slice(&seq.to_be_bytes());
    k
}
```

- [ ] **Step 3: Pre-create the two tables in `Store::open`** (add after `store.rs:388` `txn.open_table(FILL_HISTORY)?;`)

```rust
        txn.open_table(EQUITY_POINTS)?;
        txn.open_table(HISTORY_EVENTS)?;
```

- [ ] **Step 4: Write the failing read-method test**

Add to the `#[cfg(test)] mod tests` block in `store.rs` (use the existing test imports/helpers for `Store::open`/`temp` path; mirror an existing store test's setup):

```rust
    #[test]
    fn equity_and_history_rows_roundtrip() {
        use crate::account::AccountId;
        use crate::aggregates::{EquityPoint, HistoryEvent, HistoryKind, StoredHistoryEvent};

        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("t.redb")).unwrap();
        let aid = AccountId(7);

        let pts = vec![
            EquityPoint { height: 1, timestamp_ms: 1_000, portfolio_value_nanos: 100, deposited_nanos: 100 },
            EquityPoint { height: 2, timestamp_ms: 2_000, portfolio_value_nanos: 150, deposited_nanos: 100 },
        ];
        let mut e1 = HistoryEvent::new(aid, HistoryKind::Placed, 1, 1_000);
        e1.seq = 0;
        let mut e2 = HistoryEvent::new(aid, HistoryKind::Filled, 2, 2_000);
        e2.seq = 1;
        let events: Vec<StoredHistoryEvent> =
            vec![StoredHistoryEvent::from_event(&e1), StoredHistoryEvent::from_event(&e2)];

        store
            .append_offblock_rows(
                &pts.iter().map(|p| (aid, *p)).collect::<Vec<_>>(),
                &events,
            )
            .unwrap();

        // Equity: oldest-first, all points.
        let got = store.equity_series(aid).unwrap();
        assert_eq!(got, pts);

        // History: newest-first, filtered + paged like AccountEventLog::query.
        let all = store.account_events(aid, 10, None, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].kind, HistoryKind::Filled); // newest first

        let trades = store.account_events(aid, 10, None, Some("trades".into())).unwrap();
        assert_eq!(trades.len(), 2);

        // Cursor before (2, 1) excludes the Filled@(2,1) event.
        let page = store.account_events(aid, 10, Some((2, 1)), None).unwrap();
        assert!(page.iter().all(|e| !(e.block_height == 2 && e.seq == 1)));

        // Unknown account → empty.
        assert!(store.equity_series(AccountId(99)).unwrap().is_empty());
        assert!(store.account_events(AccountId(99), 10, None, None).unwrap().is_empty());
    }
```

- [ ] **Step 5: Run it to confirm it fails to compile (methods missing)**

Run: `cargo test -p matching-sequencer store::tests::equity_and_history_rows_roundtrip`
Expected: FAIL — `no method named append_offblock_rows / equity_series / account_events`.

- [ ] **Step 6: Implement the three store methods** (in `impl Store`)

```rust
    /// Append this block's equity points and history events as individual rows.
    /// Append-only; called once per block from `save_block_with_witness`'s txn
    /// when standalone, or reuse the block txn (see Task 4). This standalone
    /// version is used by tests and as a fallback.
    pub fn append_offblock_rows(
        &self,
        equity: &[(AccountId, crate::aggregates::EquityPoint)],
        history: &[crate::aggregates::StoredHistoryEvent],
    ) -> Result<(), StoreError> {
        let txn = self.db.begin_write()?;
        {
            let mut t = txn.open_table(EQUITY_POINTS)?;
            for (aid, p) in equity {
                let key = equity_key(*aid, p.height);
                let bytes = rmp_serde::to_vec(p)?;
                t.insert(key.as_slice(), bytes.as_slice())?;
            }
            let mut h = txn.open_table(HISTORY_EVENTS)?;
            for ev in history {
                let key = history_event_key(AccountId(ev.account_id), ev.block_height, ev.seq);
                let bytes = rmp_serde::to_vec(ev)?;
                h.insert(key.as_slice(), bytes.as_slice())?;
            }
        }
        txn.commit()?;
        Ok(())
    }

    /// All equity points for an account, oldest-first (matches `EquityTracker::series`).
    pub fn equity_series(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<crate::aggregates::EquityPoint>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(EQUITY_POINTS)?;
        let lo = equity_key(account_id, 0);
        let hi = equity_key(account_id, u64::MAX);
        let mut out = Vec::new();
        for entry in table.range::<&[u8]>(lo.as_slice()..=hi.as_slice())? {
            let (_k, v) = entry?;
            out.push(rmp_serde::from_slice(v.value())?);
        }
        Ok(out)
    }

    /// Newest-first page of an account's history, replicating
    /// `AccountEventLog::query` (cursor `before = (block_height, seq)`,
    /// `category` filter via `HistoryKind::category`).
    pub fn account_events(
        &self,
        account_id: AccountId,
        limit: usize,
        before: Option<(u64, u64)>,
        category: Option<String>,
    ) -> Result<Vec<crate::aggregates::HistoryEvent>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(HISTORY_EVENTS)?;
        let lo = history_event_key(account_id, 0, 0);
        let hi = history_event_key(account_id, u64::MAX, u64::MAX);
        let mut out = Vec::new();
        for entry in table.range::<&[u8]>(lo.as_slice()..=hi.as_slice())?.rev() {
            let (_k, v) = entry?;
            let stored: crate::aggregates::StoredHistoryEvent = rmp_serde::from_slice(v.value())?;
            if let Some((b, s)) = before {
                if !((stored.block_height, stored.seq) < (b, s)) {
                    continue;
                }
            }
            if let Some(ref c) = category {
                if stored.kind.category() != c.as_str() {
                    continue;
                }
            }
            out.push(stored.into_event());
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }
```

- [ ] **Step 6b: Add the required re-exports** (MANDATORY — verified missing)

`EquityPoint`, `HistoryEvent`, `HistoryKind` are already re-exported from `aggregates/mod.rs` (lines ~26-30), but `MAX_EQUITY_POINTS`, `MAX_HISTORY_EVENTS_PER_ACCOUNT`, and the new `StoredHistoryEvent` are NOT. Extend the existing `pub use` lines:

```rust
pub use account_event_log::{
    AccountEventLog, HistoryEvent, HistoryKind, StoredHistoryEvent, MAX_HISTORY_EVENTS_PER_ACCOUNT,
    // ...keep existing names already listed here...
};
pub use equity_tracker::{EquityPoint, EquityTracker, MAX_EQUITY_POINTS};
```

Without these, `crate::aggregates::MAX_EQUITY_POINTS` / `…MAX_HISTORY_EVENTS_PER_ACCOUNT` (Task 4 Step 1) and `crate::aggregates::StoredHistoryEvent` (store.rs, analytics.rs) do not resolve.

- [ ] **Step 7: Run the test to confirm it passes**

Run: `cargo test -p matching-sequencer store::tests::equity_and_history_rows_roundtrip`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/matching-sequencer/src/store.rs crates/matching-sequencer/src/aggregates/mod.rs
git commit -m "feat(persist): redb equity_points/history_events tables + read/write methods"
```

---

## Task 3: Per-block `pending` buffers + configurable in-memory cap

**Files:**
- Modify: `crates/matching-sequencer/src/aggregates/equity_tracker.rs:27-106`
- Modify: `crates/matching-sequencer/src/aggregates/account_event_log.rs:99-119`
- Modify: `crates/matching-sequencer/src/analytics.rs:26-52,80-94,330-343` and add `clear_pending`

- [ ] **Step 1: Add cap + pending to `EquityTracker`**

Replace the `EquityTracker` struct and `new`/`record` in `equity_tracker.rs`:

```rust
#[derive(Clone)]
pub struct EquityTracker {
    points: HashMap<AccountId, VecDeque<EquityPoint>>,
    known: HashSet<AccountId>,
    last_sweep_ms: u64,
    max_points: usize,
    /// Points appended since the last `take_pending`. Drained per block by the
    /// persist path; cleared after commit.
    pending: Vec<(AccountId, EquityPoint)>,
}

impl Default for EquityTracker {
    fn default() -> Self {
        Self::with_retention(MAX_EQUITY_POINTS)
    }
}

impl EquityTracker {
    pub fn new() -> Self {
        Self::with_retention(MAX_EQUITY_POINTS)
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

    pub fn take_pending(&mut self) -> Vec<(AccountId, EquityPoint)> {
        std::mem::take(&mut self.pending)
    }

    pub fn pending(&self) -> &[(AccountId, EquityPoint)] {
        &self.pending
    }

    pub fn clear_pending(&mut self) {
        self.pending.clear();
    }
```

Then inside `record`, replace the push block (the `let ring = …; ring.push_back(point); while ring.len() > MAX_EQUITY_POINTS …`) with:

```rust
            self.pending.push((aid, point));
            if self.max_points > 0 {
                let ring = self.points.entry(aid).or_default();
                ring.push_back(point);
                while ring.len() > self.max_points {
                    ring.pop_front();
                }
            }
```

(The `if self.max_points > 0` guard matters: with cap = 0 in prod, skipping the `entry(aid).or_default()` avoids creating an empty `VecDeque` + HashMap entry **per account ever touched** — otherwise prod per-account RAM is small-but-nonzero. `pending` still carries the row to redb.)

- [ ] **Step 2: Add cap + pending to `AccountEventLog`**

Replace the struct + `append` in `account_event_log.rs`:

```rust
#[derive(Clone)]
pub struct AccountEventLog {
    events: HashMap<AccountId, VecDeque<HistoryEvent>>,
    next_seq: u64,
    max_events: usize,
    /// Events appended since the last `take_pending`.
    pending: Vec<HistoryEvent>,
}

impl Default for AccountEventLog {
    fn default() -> Self {
        Self::with_retention(MAX_HISTORY_EVENTS_PER_ACCOUNT)
    }
}

impl AccountEventLog {
    pub fn new() -> Self {
        Self::with_retention(MAX_HISTORY_EVENTS_PER_ACCOUNT)
    }

    pub fn with_retention(max_events: usize) -> Self {
        Self {
            events: HashMap::new(),
            next_seq: 0,
            max_events,
            pending: Vec::new(),
        }
    }

    pub fn take_pending(&mut self) -> Vec<HistoryEvent> {
        std::mem::take(&mut self.pending)
    }

    pub fn pending(&self) -> &[HistoryEvent] {
        &self.pending
    }

    pub fn clear_pending(&mut self) {
        self.pending.clear();
    }

    pub fn append(&mut self, mut event: HistoryEvent) {
        event.seq = self.next_seq;
        self.next_seq += 1;
        self.pending.push(event.clone());
        if self.max_events > 0 {
            let ring = self.events.entry(event.account_id).or_default();
            ring.push_back(event);
            while ring.len() > self.max_events {
                ring.pop_front();
            }
        }
    }
```

- [ ] **Step 3: Thread caps + pending through `AnalyticsState`**

In `analytics.rs`, change `new` and `restore` to build the trackers with retention from config, and add `clear_pending` + pending accessors. In `AnalyticsState::new` (`:40`) replace the two constructions:

```rust
            equity_tracker: EquityTracker::with_retention(config.max_equity_points_per_account),
            account_event_log: AccountEventLog::with_retention(config.max_history_events_per_account),
```

In `AnalyticsState::restore` (`:75`) replace the same two with the retention constructors (identical lines). Then add methods to `impl AnalyticsState`:

```rust
    pub fn take_offblock_pending(
        &mut self,
    ) -> (
        Vec<(AccountId, crate::aggregates::EquityPoint)>,
        Vec<crate::aggregates::HistoryEvent>,
    ) {
        (
            self.equity_tracker.take_pending(),
            self.account_event_log.take_pending(),
        )
    }

    pub fn offblock_pending(
        &self,
    ) -> (
        Vec<(AccountId, crate::aggregates::EquityPoint)>,
        Vec<crate::aggregates::HistoryEvent>,
    ) {
        (
            self.equity_tracker.pending().to_vec(),
            self.account_event_log.pending().to_vec(),
        )
    }

    pub fn clear_offblock_pending(&mut self) {
        self.equity_tracker.clear_pending();
        self.account_event_log.clear_pending();
    }

    pub fn seed_equity_known(&mut self, ids: impl IntoIterator<Item = AccountId>) {
        self.equity_tracker.seed_known(ids);
    }
```

- [ ] **Step 4: Run the aggregate + analytics tests**

Run: `cargo test -p matching-sequencer aggregates:: analytics`
Expected: PASS (existing unit tests use the default cap, so rings behave as before).

- [ ] **Step 5: Commit**

```bash
git add crates/matching-sequencer/src/aggregates/equity_tracker.rs crates/matching-sequencer/src/aggregates/account_event_log.rs crates/matching-sequencer/src/analytics.rs
git commit -m "feat(analytics): per-block pending buffers + configurable in-memory cap"
```

---

## Task 4: Config fields, snapshot wiring, persist write, clear-after-commit

**Files:**
- Modify: `crates/matching-sequencer/src/sequencer.rs:60-110` (`SequencerConfig`), `:726-740` (`snapshot`), `:1717-1722` (`commit_prepared_block`)
- Modify: `crates/matching-sequencer/src/store.rs:332-345` (`AnalyticsSnapshot`), and `save_block_with_witness` body
- Modify: `crates/sybil-api/src/config.rs` (env → `SequencerConfig`)

- [ ] **Step 1: Add cap fields to `SequencerConfig`**

In `sequencer.rs`, add to `SequencerConfig` next to `max_price_history_points_per_market`:

```rust
    /// In-memory equity points retained per account (serving fallback only;
    /// full series lives in redb). Set to 0 in prod.
    pub max_equity_points_per_account: usize,
    /// In-memory history events retained per account (serving fallback only).
    /// Set to 0 in prod.
    pub max_history_events_per_account: usize,
```

In the `Default for SequencerConfig` impl (`:107` area), add:

```rust
            max_equity_points_per_account: crate::aggregates::MAX_EQUITY_POINTS,
            max_history_events_per_account: crate::aggregates::MAX_HISTORY_EVENTS_PER_ACCOUNT,
```

(Both consts MUST be re-exported from `crate::aggregates` — done in Task 2 Step 6b. `MAX_EQUITY_POINTS = 43_200`, `MAX_HISTORY_EVENTS_PER_ACCOUNT = 5_000`.)

- [ ] **Step 2: Add pending deltas to `AnalyticsSnapshot`**

In `store.rs`, extend `AnalyticsSnapshot<'a>` (`:333`):

```rust
    pub equity_points_delta: Vec<(AccountId, crate::aggregates::EquityPoint)>,
    pub history_events_delta: Vec<crate::aggregates::StoredHistoryEvent>,
```

- [ ] **Step 3: Populate the deltas in `AnalyticsState::snapshot`**

In `analytics.rs::snapshot` (`:80`), add to the returned struct:

```rust
            equity_points_delta: self.equity_tracker.pending().to_vec(),
            history_events_delta: self
                .account_event_log
                .pending()
                .iter()
                .map(crate::aggregates::StoredHistoryEvent::from_event)
                .collect(),
```

- [ ] **Step 4: Write the rows inside `save_block_with_witness`**

In `store.rs`, inside the existing block-write transaction in `save_block_with_witness` (the same `txn` that writes `FILL_HISTORY` around `:579`), add a block that reuses `txn`:

```rust
        {
            let mut eq = txn.open_table(EQUITY_POINTS)?;
            for (aid, p) in &snapshot.analytics.equity_points_delta {
                let key = equity_key(*aid, p.height);
                let bytes = rmp_serde::to_vec(p)?;
                eq.insert(key.as_slice(), bytes.as_slice())?;
            }
            let mut hist = txn.open_table(HISTORY_EVENTS)?;
            for ev in &snapshot.analytics.history_events_delta {
                let key = history_event_key(AccountId(ev.account_id), ev.block_height, ev.seq);
                let bytes = rmp_serde::to_vec(ev)?;
                hist.insert(key.as_slice(), bytes.as_slice())?;
            }
        }
```

(Atomic with the rest of the block: if persist fails, these rows roll back with everything else.)

- [ ] **Step 5: Clear pending after commit**

In `sequencer.rs::commit_prepared_block` (`:1717`), after `*self = next_sequencer;` add:

```rust
        self.analytics.clear_offblock_pending();
```

> Why here: history events accrue on the live sequencer during admission AND on the clone during production; the clone (persisted snapshot) captures both. After `*self = next_sequencer` the live pending equals everything just persisted, so clearing here resets the buffer for the next cycle without losing admission-time events on a persist failure (no commit → no clear → retried next block).

- [ ] **Step 6: Add the two clap/env fields to `ApiConfig`** (`crates/sybil-api/src/config.rs`, next to `max_price_history_points_per_market` ~line 105; use the file's existing **string** `default_value` style, not `default_value_t`)

```rust
    #[arg(long, default_value = "0", env = "SYBIL_MAX_EQUITY_POINTS_PER_ACCOUNT")]
    pub max_equity_points_per_account: usize,
    #[arg(long, default_value = "0", env = "SYBIL_MAX_HISTORY_EVENTS_PER_ACCOUNT")]
    pub max_history_events_per_account: usize,
```

- [ ] **Step 6b: Add the same fields to `impl Default for ApiConfig`** (`config.rs:166-198` — REQUIRED or the crate won't compile, since that impl is an exhaustive literal)

```rust
            max_equity_points_per_account: 0,
            max_history_events_per_account: 0,
```

- [ ] **Step 6c: Map them in the `SequencerConfig` literal in `main.rs`** (the mapping lives at `crates/sybil-api/src/main.rs:183-200`, an exhaustive `SequencerConfig { … }` literal using `config.X`; the sibling line is `max_price_history_points_per_market: config.max_price_history_points_per_market` at ~`:195`)

```rust
        max_equity_points_per_account: config.max_equity_points_per_account,
        max_history_events_per_account: config.max_history_events_per_account,
```

> Note: the binary default is `0` (prod-safe: nothing retained in RAM). `SequencerConfig::default()` (used by unit/integration tests) keeps the full const caps, so test fallbacks still work.

- [ ] **Step 7: Build the workspace**

Run: `cargo build -p matching-sequencer -p sybil-api`
Expected: compiles clean.

- [ ] **Step 8: Commit**

```bash
git add crates/matching-sequencer/src/sequencer.rs crates/matching-sequencer/src/store.rs crates/matching-sequencer/src/analytics.rs crates/sybil-api/src/config.rs crates/sybil-api/src/main.rs
git commit -m "feat(persist): write equity/history rows in block txn + config caps + clear-after-commit"
```

---

## Task 5: Actor read handlers read from the store

**Files:**
- Modify: `crates/matching-sequencer/src/actor.rs:1494-1503` (the two handlers)

- [ ] **Step 1: Swap the equity handler to prefer the store**

Replace the `GetEquitySeries` arm (`actor.rs:1494`). Sibling arms bind the actor state as `state` (e.g. `state.sequencer.equity_series` at `:1495`), and `state.store` is `Option<Arc<Store>>`:

```rust
            SequencerMsg::GetEquitySeries(account_id, reply) => {
                let result = match &state.store {
                    Some(store) => store.equity_series(account_id).unwrap_or_else(|e| {
                        tracing::warn!(error = %e, "equity_series read failed; falling back to memory");
                        state.sequencer.equity_series(account_id)
                    }),
                    None => state.sequencer.equity_series(account_id),
                };
                let _ = reply.send(result);
            }
```

- [ ] **Step 2: Swap the history handler to prefer the store**

Replace the `GetAccountEvents` arm (`actor.rs:1497`):

```rust
            SequencerMsg::GetAccountEvents(account_id, limit, before, category, reply) => {
                let result = match &state.store {
                    Some(store) => store
                        .account_events(account_id, limit, before, category.clone())
                        .unwrap_or_else(|e| {
                            tracing::warn!(error = %e, "account_events read failed; falling back to memory");
                            state.sequencer.account_history(
                                account_id,
                                limit,
                                before,
                                category.as_deref(),
                            )
                        }),
                    None => state.sequencer.account_history(
                        account_id,
                        limit,
                        before,
                        category.as_deref(),
                    ),
                };
                let _ = reply.send(result);
            }
```

> Check the exact `category` type on `SequencerMsg::GetAccountEvents` (`actor.rs:116`) — it is `Option<String>`. `store.account_events` takes `Option<String>`; `sequencer.account_history` takes `Option<&str>` (hence `.as_deref()`). Keep the original signatures intact.

- [ ] **Step 3: Build**

Run: `cargo build -p matching-sequencer`
Expected: compiles.

- [ ] **Step 4: Run the no-store equity/history integration tests (fallback path)**

Run: `cargo test -p sybil-api account_equity_series_populates_after_trades account_history_shows_placed_then_cancelled`
Expected: PASS (these use `test_app` → no store → in-memory fallback with default caps).

- [ ] **Step 5: Commit**

```bash
git add crates/matching-sequencer/src/actor.rs
git commit -m "feat(actor): serve equity/history from redb when a store is attached"
```

---

## Task 6: Store-backed round-trip integration tests

**Files:**
- Modify: `crates/sybil-api/tests/api_integration.rs` (add two tests)

- [ ] **Step 1: Write the store-backed equity test**

Add to `api_integration.rs` (uses `test_app_with_store` so the redb write+read path is exercised; modeled on `account_equity_series_populates_after_trades` at `:1340`):

```rust
#[tokio::test]
async fn account_equity_series_persists_to_store() {
    let (app, handle) = test_app_with_store(true).await;

    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "EqDb?" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();
    let (_, body) = post_json(app.clone(), "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 })).await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();
    let (_, body) = post_json(app.clone(), "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 })).await;
    let account_b = parse_json(&body)["account_id"].as_u64().unwrap();

    post_json(app.clone(), "/v1/orders", json!({
        "account_id": account_id,
        "orders": [{ "type": "BuyYes", "market_id": market_id, "limit_price_nanos": 600_000_000u64, "quantity": 10 }]
    })).await;
    post_json(app.clone(), "/v1/orders", json!({
        "account_id": account_b,
        "orders": [{ "type": "BuyNo", "market_id": market_id, "limit_price_nanos": 500_000_000u64, "quantity": 10 }]
    })).await;

    let block = handle.produce_block().await.unwrap();
    assert!(!block.canonical.fills.is_empty(), "expected fills");

    let (status, body) = get(app, &format!("/v1/accounts/{account_id}/equity?range=all")).await;
    assert_eq!(status, StatusCode::OK);
    let v = parse_json(&body);
    assert!(
        !v["points"].as_array().unwrap().is_empty(),
        "equity must come back from redb: {v}"
    );
}
```

- [ ] **Step 2: Write the store-backed history test**

```rust
#[tokio::test]
async fn account_history_persists_to_store() {
    let (app, handle) = test_app_with_store(true).await;

    let (_, body) = post_json(app.clone(), "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 })).await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();
    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "HistDb" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();

    post_json(app.clone(), "/v1/orders", json!({
        "account_id": account_id,
        "orders": [{ "type": "BuyYes", "market_id": market_id, "limit_price_nanos": 500_000_000u64, "quantity": 5 }]
    })).await;
    handle.produce_block().await.unwrap();

    let (status, body) = get(app, &format!("/v1/accounts/{account_id}/events?limit=20")).await;
    assert_eq!(status, StatusCode::OK);
    let v = parse_json(&body);
    assert!(
        !v.as_array().unwrap().is_empty(),
        "history must come back from redb: {v}"
    );
}
```

> If `test_app_with_store` is `#[allow(dead_code)]` / not yet imported in this file's `use common::{…}` list, add it. The order JSON shapes mirror the existing `account_equity_series_populates_after_trades` test, so they match the current request schema.

- [ ] **Step 3: Run both new tests**

Run: `cargo test -p sybil-api account_equity_series_persists_to_store account_history_persists_to_store`
Expected: PASS.

- [ ] **Step 4: Run the full sequencer + api test suites**

Run: `cargo test -p matching-sequencer -p sybil-api`
Expected: PASS (no regressions).

- [ ] **Step 5: Commit**

```bash
git add crates/sybil-api/tests/api_integration.rs
git commit -m "test(persist): store-backed equity/history round-trip integration tests"
```

---

## Task 7: Seed swept-account set on restore (preserve sweep behavior)

**Files:**
- Modify: `crates/matching-sequencer/src/sequencer.rs` (`BlockSequencer::restore`)

- [ ] **Step 1: Seed `known` from restored accounts**

`BlockSequencer::restore` (`sequencer.rs:661-711`) builds the result **inline** as a `BlockSequencer { … analytics: AnalyticsState::restore(state.analytics, &config), … }` literal and **moves** `state.accounts` into it — there is no `analytics` or `accounts` local to reference. So bind the constructed value to a `mut` local, seed via its (private, but same-`impl`-accessible) fields, then return it. Also: `AccountStore::iter()` yields `(&AccountId, &Account)`, so deref the id.

Change the tail of `restore` from `BlockSequencer { … }` (returned directly) to:

```rust
        let mut sequencer = BlockSequencer { /* …existing fields… */ };
        let account_ids: Vec<AccountId> = sequencer.accounts.iter().map(|(id, _)| *id).collect();
        sequencer.analytics.seed_equity_known(account_ids);
        sequencer
```

> `restore` is an associated fn of `BlockSequencer`, so it can read the private `accounts`/`analytics` fields of the value it just built — no new accessor needed. Confirm the exact field names (`accounts`, `analytics`) on the struct literal at `sequencer.rs:683`/`:691`.

- [ ] **Step 2: Build + run sequencer tests**

Run: `cargo test -p matching-sequencer`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/matching-sequencer/src/sequencer.rs
git commit -m "feat(analytics): reseed equity sweep set on restore"
```

---

## Task 8: Prod config — disable in-memory retention

**Files:**
- Modify: `docker-compose.prod.yml` (sybil-api `environment`)

- [ ] **Step 1: Set both caps to 0 in prod**

Under the `sybil-api` service `environment:` in `docker-compose.prod.yml`, add:

```yaml
      SYBIL_MAX_EQUITY_POINTS_PER_ACCOUNT: "0"
      SYBIL_MAX_HISTORY_EVENTS_PER_ACCOUNT: "0"
```

> With these at 0, the in-memory rings stay empty in prod; equity/history are served entirely from redb. The per-block sequencer clone no longer copies these rings (CPU + transient-alloc win on top of the steady RAM win).

- [ ] **Step 2: Commit**

```bash
git add docker-compose.prod.yml
git commit -m "chore(prod): serve equity/history from redb only (in-memory caps = 0)"
```

---

## Task 9 (optional, recommended): Bounded disk retention

**Files:**
- Modify: `crates/matching-sequencer/src/store.rs` (new `prune_offblock_rows`, modeled on `prune_historical_block_rows` at `:231`)
- Modify: `crates/matching-sequencer/src/actor.rs` (call prune every N blocks in `on_tick`)

- [ ] **Step 1: Add a time-based prune for equity rows + per-account cap for history**

```rust
    /// Delete equity points older than `retain_ms` and trim each account's
    /// history to the newest `history_cap` rows. Best-effort; called periodically.
    pub fn prune_offblock_rows(
        &self,
        now_ms: u64,
        retain_ms: u64,
        history_cap: usize,
    ) -> Result<(), StoreError> {
        let cutoff = now_ms.saturating_sub(retain_ms);
        let txn = self.db.begin_write()?;
        {
            // Equity: scan all, collect keys whose point.timestamp_ms < cutoff.
            let mut to_delete: Vec<[u8; 16]> = Vec::new();
            {
                let eq = txn.open_table(EQUITY_POINTS)?;
                for entry in eq.iter()? {
                    let (k, v) = entry?;
                    let p: crate::aggregates::EquityPoint = rmp_serde::from_slice(v.value())?;
                    if p.timestamp_ms < cutoff {
                        let mut key = [0u8; 16];
                        key.copy_from_slice(k.value());
                        to_delete.push(key);
                    }
                }
            }
            let mut eq = txn.open_table(EQUITY_POINTS)?;
            for k in to_delete {
                eq.remove(k.as_slice())?;
            }
            // History: per-account count, drop oldest beyond history_cap.
            // (Iterate ascending; keys group by account prefix; track counts.)
            // Left as a straightforward extension if disk pressure warrants it.
            let _ = history_cap;
        }
        txn.commit()?;
        Ok(())
    }
```

- [ ] **Step 2: Call it every ~600 blocks in `on_tick`** (after `commit_prepared_block`)

```rust
        if let Some(store) = &self.store {
            if bp.block.header.height % 600 == 0 {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                if let Err(e) = store.prune_offblock_rows(now, 30 * 24 * 3_600_000, 5_000) {
                    tracing::warn!(error = %e, "offblock prune failed");
                }
            }
        }
```

- [ ] **Step 3: Build + test + commit**

Run: `cargo test -p matching-sequencer`
Then:
```bash
git add crates/matching-sequencer/src/store.rs crates/matching-sequencer/src/actor.rs
git commit -m "feat(persist): periodic disk retention for equity/history rows"
```

---

## Verification (whole-feature)

- [ ] `cargo test -p matching-sequencer -p sybil-api` — all green.
- [ ] `cargo clippy -p matching-sequencer -p sybil-api --all-targets` — no new warnings.
- [ ] Manual: run sybil-api with `SYBIL_DATA_DIR` set and the two caps at 0; create an account, place crossing orders, produce blocks; `GET /v1/accounts/{id}/equity?range=all` and `/events?limit=50` return data; restart the process and confirm the data is **still** there (previously it reset).
- [ ] Frontend smoke: portfolio equity curve + history feed + degen tracker still render (no schema/contract change — `HistoryEventResponse` / `EquitySeriesResponse` untouched).

## Risk notes / what this does NOT do

- **Accounts still accumulate** (arena recreates a cohort per restart; nothing deletes accounts). This moves their equity/history cost from RAM to disk (cheap, prunable). The durable fix (arena account reuse / idle-account eviction / per-container memory metrics) is separate follow-up.
- **The equity sweep still does O(accounts) work every 60 s** — now O(accounts) redb inserts/min inside the block txn instead of RAM pushes. Fine at current scale; if it becomes a commit-latency issue, batch the sweep into its own write txn off the block path.
- **redb is mmap-backed**; serving large histories pulls file pages into (reclaimable) page cache. Watch the `sybil-data` volume size and the container's cache usage after rollout.
