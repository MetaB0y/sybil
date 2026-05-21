# Per-Account Equity Series Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record a throttled per-account equity series `(timestamp, portfolio_value, deposited)` and serve it at `GET /v1/accounts/{id}/equity?range=`, so the Portfolio equity chart and 24H/7D/30D hero deltas use real data instead of mocks.

**Architecture:** A new off-block `EquityTracker` sidecar in `AnalyticsState` (same pattern as `LiquidityTracker`/`CostBasisTracker`). At each block finalize it samples equity for accounts that traded that block, plus a periodic sweep (every `EQUITY_SAMPLE_INTERVAL_MS`) over all known accounts so price-driven equity changes between trades still land on the chart. Portfolio value is `balance + Σ qty·clearing_price` — no cost basis needed. In-memory only for now (resets on restart, same documented "since last restart" caveat as the other off-block aggregates); persistence can be added later via the `AnalyticsSnapshot` plumbing.

**Tech Stack:** Rust, ractor actor RPC, axum, serde.

**Conventions:** jj VCS; `just fmt`/`just lint`; tests via `cargo test -p <crate>`.

---

## File Structure

- Create `crates/matching-sequencer/src/aggregates/equity_tracker.rs` — `EquityTracker`, `EquityPoint`, retention constants, unit tests.
- Modify `crates/matching-sequencer/src/aggregates/mod.rs` — export `EquityTracker`, `EquityPoint`.
- Modify `crates/matching-sequencer/src/analytics.rs` — hold the tracker; `record_equity` method; init in `new`/`restore` (volatile).
- Modify `crates/matching-sequencer/src/sequencer.rs` — call `record_equity` in `produce_block_in_place`; add a `get_equity_series` accessor.
- Modify `crates/matching-sequencer/src/actor.rs` — `GetEquitySeries` message, match arm, handle method.
- Modify `crates/sybil-api-types/src/response.rs` — `EquityPointResponse`, `EquitySeriesResponse`.
- Modify `crates/sybil-api/src/routes/accounts.rs` — `get_equity` handler + `EquityRangeParams`.
- Modify `crates/sybil-api/src/app.rs` — register `GET /v1/accounts/{id}/equity`.
- Test `crates/sybil-api/tests/api_integration.rs`.

---

## Task 1: The `EquityTracker` sidecar

**Files:**
- Create/Test: `crates/matching-sequencer/src/aggregates/equity_tracker.rs`
- Modify: `crates/matching-sequencer/src/aggregates/mod.rs`

- [ ] **Step 1: Write the failing unit test**

Create `crates/matching-sequencer/src/aggregates/equity_tracker.rs`:

```rust
//! Off-block per-account equity series (t, portfolio_value, deposited).
//!
//! Sampled at block finalize: always for accounts that traded this block,
//! plus a periodic sweep over known accounts so price-driven equity changes
//! land between trades. In-memory only (resets on restart) — same caveat as
//! the other off-block aggregates.

use std::collections::{HashMap, HashSet, VecDeque};

use matching_engine::{MarketId, Nanos};

use crate::account::{AccountId, AccountStore};

/// Minimum wall-clock gap between periodic full sweeps (ms).
pub const EQUITY_SAMPLE_INTERVAL_MS: u64 = 60_000;
/// Max points retained per account (~30 days at one point/minute).
pub const MAX_EQUITY_POINTS: usize = 43_200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EquityPoint {
    pub height: u64,
    pub timestamp_ms: u64,
    pub portfolio_value_nanos: i64,
    pub deposited_nanos: i64,
}

#[derive(Clone, Default)]
pub struct EquityTracker {
    points: HashMap<AccountId, VecDeque<EquityPoint>>,
    known: HashSet<AccountId>,
    last_sweep_ms: u64,
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
            .unwrap_or(matching_engine::NANOS_PER_DOLLAR / 2);
        total += qty as i128 * price as i128;
    }
    total as i64
}

impl EquityTracker {
    pub fn new() -> Self {
        Self::default()
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
        let sweep_due = timestamp_ms.saturating_sub(self.last_sweep_ms) >= EQUITY_SAMPLE_INTERVAL_MS;
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
            let ring = self.points.entry(aid).or_default();
            ring.push_back(point);
            while ring.len() > MAX_EQUITY_POINTS {
                ring.pop_front();
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
        assert_eq!(t.series(aid)[0].portfolio_value_nanos, 1_000 * NANOS_PER_DOLLAR as i64);

        // Next block, not due, not touched → no new point.
        t.record(&HashSet::new(), &accounts, &prices, 2, 2_000);
        assert_eq!(t.series(aid).len(), 1);

        // Past the sweep interval → known account sampled even though untouched.
        t.record(&HashSet::new(), &accounts, &prices, 3, 1_000 + EQUITY_SAMPLE_INTERVAL_MS);
        assert_eq!(t.series(aid).len(), 2);
    }
}
```

- [ ] **Step 2: Run, verify failure**

Run: `cargo test -p matching-sequencer samples_touched_then_sweeps`
Expected: FAIL — module not declared in `mod.rs`.

- [ ] **Step 3: Export the module**

In `crates/matching-sequencer/src/aggregates/mod.rs`, add the module declaration and re-export (match the existing style for `cost_basis_tracker`):

```rust
pub mod equity_tracker;
pub use equity_tracker::{EquityPoint, EquityTracker};
```

> Implementer note: confirm `AccountStore::create_account`, `AccountStore::get`, and `Account.positions/balance/total_deposited` signatures match (they are used in `cost_basis_tracker.rs` tests and `portfolio.rs`). Adjust the test constructor if `create_account` differs.

- [ ] **Step 4: Run, verify pass**

Run: `cargo test -p matching-sequencer samples_touched_then_sweeps`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
just fmt && just lint
jj describe -m "feat(analytics): EquityTracker sidecar (per-account equity series)"
```

---

## Task 2: Wire the tracker into AnalyticsState + the block hook

**Files:**
- Modify: `crates/matching-sequencer/src/analytics.rs` (struct ~line 33; `new` ~line 45; `restore` ~line 69; new method)
- Modify: `crates/matching-sequencer/src/sequencer.rs` (`produce_block_in_place`, after `record_finalized_block`; new accessor)

- [ ] **Step 1: Add the field + init (volatile)**

In `analytics.rs`: import `EquityTracker` (extend the `crate::aggregates::{...}` use), add the field to `AnalyticsState` (after `first_deposit_ms`, line 33):

```rust
    equity_tracker: EquityTracker,
```

In `new` (line 45 area), add:

```rust
            equity_tracker: EquityTracker::new(),
```

In `restore` (line 69 area), add (volatile — no snapshot plumbing):

```rust
            equity_tracker: EquityTracker::new(),
```

- [ ] **Step 2: Add the `record_equity` forwarder + a series accessor**

In `analytics.rs`, near `record_finalized_block`:

```rust
    pub fn record_equity(
        &mut self,
        touched: &std::collections::HashSet<AccountId>,
        accounts: &AccountStore,
        prices: &HashMap<MarketId, Vec<Nanos>>,
        height: u64,
        timestamp_ms: u64,
    ) {
        self.equity_tracker
            .record(touched, accounts, prices, height, timestamp_ms);
    }

    pub fn equity_series(&self, account_id: AccountId) -> Vec<crate::aggregates::EquityPoint> {
        self.equity_tracker.series(account_id)
    }
```

- [ ] **Step 3: Call it from `produce_block_in_place`**

In `sequencer.rs`, right after the `record_finalized_block(...)` call in `finalize_block_state_phase`/`produce_block_in_place` (the analytics finalize hook), build the touched-accounts set from this block's fills via `order_account_map` (in scope; built at line ~2188) and record equity:

```rust
        let touched: std::collections::HashSet<AccountId> = fills
            .iter()
            .filter_map(|f| order_account_map.get(&f.order_id).copied())
            .collect();
        self.analytics
            .record_equity(&touched, &self.accounts, &clearing_prices, self.height, timestamp_ms);
```

> Implementer note: place this where `order_account_map`, `fills`, `clearing_prices`, `self.accounts`, `self.height`, and `timestamp_ms` are all in scope — i.e. in `produce_block_in_place` after `finalize_block_state_phase` (post-fill account state). If `record_finalized_block` is invoked inside `finalize_block_state_phase`, add the `record_equity` call right after that phase returns, where `order_account_map` is still live.

- [ ] **Step 4: Add the sequencer accessor**

In `sequencer.rs` (a `BlockSequencer` read method, near other analytics accessors):

```rust
    pub fn equity_series(&self, account_id: AccountId) -> Vec<crate::aggregates::EquityPoint> {
        self.analytics.equity_series(account_id)
    }
```

- [ ] **Step 5: Build + sequencer suite**

Run: `cargo test -p matching-sequencer`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
just fmt && just lint
jj describe -m "feat(analytics): sample equity at block finalize"
```

---

## Task 3: Actor RPC + API endpoint

**Files:**
- Modify: `crates/matching-sequencer/src/actor.rs` (enum near `GetAccountFills` line 107; arm near line 1421; handle near line 2000)
- Modify: `crates/sybil-api-types/src/response.rs`
- Modify: `crates/sybil-api/src/routes/accounts.rs`
- Modify: `crates/sybil-api/src/app.rs`
- Test: `crates/sybil-api/tests/api_integration.rs`

- [ ] **Step 1: Write the failing integration test**

In `api_integration.rs`:

```rust
#[tokio::test]
async fn account_equity_series_populates_after_trades() {
    let (app, handle) = test_app(true).await;

    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "Eq?" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();
    let (_, body) = post_json(app.clone(), "/v1/accounts", json!({ "initial_balance_nanos": 10_000_000_000u64 })).await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();

    post_json(app.clone(), "/v1/orders", json!({
        "account_id": account_id,
        "orders": [{ "type": "BuyYes", "market_id": market_id, "limit_price_nanos": 500_000_000u64, "quantity": 10 }]
    })).await;

    // Produce a block so the order fills/settles and equity samples.
    handle.produce_block().await.unwrap();

    let (status, body) = get(app, &format!("/v1/accounts/{account_id}/equity?range=all")).await;
    assert_eq!(status, StatusCode::OK);
    let v = parse_json(&body);
    assert_eq!(v["account_id"].as_u64().unwrap(), account_id);
    assert!(!v["points"].as_array().unwrap().is_empty(), "expected >=1 equity point: {v}");
}
```

- [ ] **Step 2: Run, verify failure**

Run: `cargo test -p sybil-api --test api_integration account_equity_series_populates_after_trades`
Expected: FAIL — route 404.

- [ ] **Step 3: Add response types**

In `crates/sybil-api-types/src/response.rs` (near the Portfolio section):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EquityPointResponse {
    pub timestamp_ms: u64,
    pub height: u64,
    pub portfolio_value_nanos: i64,
    pub deposited_nanos: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EquitySeriesResponse {
    pub account_id: u64,
    pub points: Vec<EquityPointResponse>,
}
```

- [ ] **Step 4: Add the actor message, arm, and handle**

In `actor.rs`, in `SequencerMsg` (after `GetAccountFills`, line 107):

```rust
    GetEquitySeries(AccountId, RpcReplyPort<Vec<crate::aggregates::EquityPoint>>),
```

Match arm (near the `GetAccountFills` arm, line 1421):

```rust
            SequencerMsg::GetEquitySeries(account_id, reply) => {
                let _ = reply.send(state.sequencer.equity_series(account_id));
            }
```

Handle method (near `get_account_fills`, line 2000):

```rust
    pub async fn get_equity_series(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<crate::aggregates::EquityPoint>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetEquitySeries(account_id, reply)).await
    }
```

- [ ] **Step 5: Add the route handler**

In `crates/sybil-api/src/routes/accounts.rs` (after `get_account_fills`):

```rust
#[derive(Debug, serde::Deserialize)]
pub struct EquityRangeParams {
    /// "24h" | "7d" | "30d" | "all" (default "all").
    pub range: Option<String>,
}

/// GET /v1/accounts/{id}/equity?range=
pub async fn get_equity(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Query(params): Query<EquityRangeParams>,
) -> Result<Json<EquitySeriesResponse>, AppError> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let since_ms = match params.range.as_deref() {
        Some("24h") => now_ms.saturating_sub(24 * 3_600_000),
        Some("7d") => now_ms.saturating_sub(7 * 24 * 3_600_000),
        Some("30d") => now_ms.saturating_sub(30 * 24 * 3_600_000),
        _ => 0,
    };
    let points = state.sequencer.get_equity_series(AccountId(id)).await?;
    let points: Vec<EquityPointResponse> = points
        .into_iter()
        .filter(|p| p.timestamp_ms >= since_ms)
        .map(|p| EquityPointResponse {
            timestamp_ms: p.timestamp_ms,
            height: p.height,
            portfolio_value_nanos: p.portfolio_value_nanos,
            deposited_nanos: p.deposited_nanos,
        })
        .collect();
    Ok(Json(EquitySeriesResponse { account_id: id, points }))
}
```

> Implementer note: add `EquitySeriesResponse, EquityPointResponse` to the `response` imports at the top of `accounts.rs` (the file already imports `PortfolioResponse`, etc.). `Query`, `Path`, `State` are already imported.

- [ ] **Step 6: Register the route**

In `crates/sybil-api/src/app.rs`, near the other `/v1/accounts/{id}/...` routes:

```rust
        .route(
            "/v1/accounts/{id}/equity",
            axum::routing::get(routes::accounts::get_equity),
        )
```

- [ ] **Step 7: Run, verify pass**

Run: `cargo test -p sybil-api --test api_integration account_equity_series_populates_after_trades`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
just fmt && just lint
jj describe -m "feat(api): GET /v1/accounts/{id}/equity?range= equity series"
```

---

## Task 4: Manual local verification

- [ ] **Step 1: Run, trade, produce blocks**

```bash
cargo run --release -p sybil-api -- --dev-mode --port 3001 &
MID=$(curl -s -XPOST localhost:3001/v1/markets -H 'content-type: application/json' -d '{"name":"eq"}' | jq .market_id)
AID=$(curl -s -XPOST localhost:3001/v1/accounts -H 'content-type: application/json' -d '{"initial_balance_nanos":10000000000}' | jq .account_id)
curl -s -XPOST localhost:3001/v1/orders -H 'content-type: application/json' \
  -d "{\"account_id\":$AID,\"orders\":[{\"type\":\"BuyYes\",\"market_id\":$MID,\"limit_price_nanos\":500000000,\"quantity\":10}]}" >/dev/null
sleep 3
curl -s "localhost:3001/v1/accounts/$AID/equity?range=all" | jq '{account_id, n: (.points|length), first: .points[0], last: .points[-1]}'
```

Expected: a non-empty `points` array with `portfolio_value_nanos` and `deposited_nanos`.

---

## Self-Review Notes

- **Spec coverage:** #4a — per-block (throttled) equity snapshots behind `GET /v1/accounts/{id}/equity?range=`, de-mocking the chart and hero deltas (FE computes 24H/7D/30D from the series).
- **Throttle:** touched accounts sampled every block; all known accounts every `EQUITY_SAMPLE_INTERVAL_MS` (60s) — bounds cost while keeping price-driven changes visible. Retention `MAX_EQUITY_POINTS` (~30d at 1/min).
- **Caveat:** in-memory only (resets on restart) — same documented behavior as other off-block aggregates; add `AnalyticsSnapshot` plumbing later for persistence.
- **Type consistency:** `EquityPoint` (engine) ↔ `get_equity_series` ↔ `EquityPointResponse`/`EquitySeriesResponse` agree on field names.
- **No placeholders:** code is exact; two implementer notes confirm in-scope bindings (the `record_equity` call site and `AccountStore` API).
