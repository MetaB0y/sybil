# Recent-Blocks Endpoint + `created_at_ms` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `GET /v1/blocks?limit=N` (last N blocks, newest-first) served from the in-memory block history, and add `created_at_ms` (wall-clock admit time) to `PendingOrderResponse`.

**Architecture:** Two independent additions. (A) A read-only actor RPC `GetRecentBlocks(n)` over the existing `block_history` deque, exposed at `GET /v1/blocks`. (B) Thread a wall-clock millisecond timestamp through the order-admit path so each `RestingOrder` carries `created_at_ms`, surfaced on the pending-orders read path. These ship together because the frontend's open-orders "Created" column (and the 10s-cadence work in plan #2) need a real timestamp instead of `block_height × cadence`.

**Tech Stack:** Rust, axum, ractor actor (`SequencerHandle` ⇄ `SequencerMsg`), serde, `tower::ServiceExt::oneshot` integration tests.

**Conventions:** jj for VCS (`jj` not `git`); integer nanos only; `just lint` = clippy, `just fmt` = rustfmt. Run a single integration test with `cargo test -p sybil-api --test api_integration <name>`.

---

## File Structure

**Part A — recent-blocks endpoint**
- Modify `crates/matching-sequencer/src/actor.rs` — add `SequencerMsg::GetRecentBlocks`, its match arm, and the `get_recent_blocks` handle method.
- Modify `crates/sybil-api/src/routes/blocks.rs` — add `RecentBlocksQuery` + `get_recent_blocks` handler.
- Modify `crates/sybil-api/src/app.rs` — register `GET /v1/blocks`.
- Test `crates/sybil-api/tests/api_integration.rs`.

**Part B — `created_at_ms`**
- Modify `crates/matching-sequencer/src/order_book.rs` — `RestingOrder.created_at_ms` field, `accept()` param, `resting_orders_full()` tuple.
- Modify `crates/matching-sequencer/src/sequencer.rs` — `try_admit_direct(now_ms)`, both `accept()` call sites, `PendingOrderInfo.created_at_ms`, `from_resting`, `pending_orders_info`/`market_orderbook` destructures.
- Modify `crates/matching-sequencer/src/actor.rs` — compute `now_ms` in `admit_or_defer` and pass it.
- Modify `crates/sybil-api-types/src/response.rs` — `PendingOrderResponse.created_at_ms`.
- Modify `crates/sybil-api/src/routes/orders.rs` — map it in `to_pending_response`.
- Test `crates/sybil-api/tests/api_integration.rs`.

---

## Part A — `GET /v1/blocks?limit=N`

### Task A1: Recent-blocks endpoint, end to end

**Files:**
- Test: `crates/sybil-api/tests/api_integration.rs`
- Modify: `crates/matching-sequencer/src/actor.rs` (enum ~line 40, match arm ~line 1393, handle ~line 1947)
- Modify: `crates/sybil-api/src/routes/blocks.rs`
- Modify: `crates/sybil-api/src/app.rs:653`

- [ ] **Step 1: Write the failing integration test**

Append to `crates/sybil-api/tests/api_integration.rs`:

```rust
#[tokio::test]
async fn recent_blocks_returns_newest_first() {
    let (app, handle) = test_app(true).await;

    let b0 = handle.produce_block().await.unwrap();
    let b1 = handle.produce_block().await.unwrap();
    let b2 = handle.produce_block().await.unwrap();
    assert!(b2.header.height > b1.header.height && b1.header.height > b0.header.height);

    // newest-first, clamped to the requested limit
    let (status, body) = get(app.clone(), "/v1/blocks?limit=2").await;
    assert_eq!(status, StatusCode::OK);
    let arr = parse_json(&body);
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 2, "got {arr:?}");
    assert_eq!(arr[0]["height"].as_u64().unwrap(), b2.header.height);
    assert_eq!(arr[1]["height"].as_u64().unwrap(), b1.header.height);

    // asking for more than exist returns all produced
    let (status, body) = get(app.clone(), "/v1/blocks?limit=1000").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parse_json(&body).as_array().unwrap().len(), 3);

    // limit=0 → empty
    let (status, body) = get(app, "/v1/blocks?limit=0").await;
    assert_eq!(status, StatusCode::OK);
    assert!(parse_json(&body).as_array().unwrap().is_empty());
}
```

- [ ] **Step 2: Run the test, verify it fails**

Run: `cargo test -p sybil-api --test api_integration recent_blocks_returns_newest_first`
Expected: FAIL — `/v1/blocks` returns 404 (route not registered), so the first `assert_eq!(status, OK)` fails.

- [ ] **Step 3: Add the `GetRecentBlocks` message variant**

In `crates/matching-sequencer/src/actor.rs`, in `enum SequencerMsg`, immediately after `GetLatestBlock(RpcReplyPort<Option<Block>>),` (line 40):

```rust
    GetRecentBlocks(usize, RpcReplyPort<Vec<Block>>),
```

- [ ] **Step 4: Add the match arm**

In `actor.rs`, immediately after the `SequencerMsg::GetBlock(height, reply) => { ... }` arm (ends ~line 1393):

```rust
            SequencerMsg::GetRecentBlocks(n, reply) => {
                let cap = state.sequencer.config.block_history_capacity;
                let take = n.min(cap);
                let blocks: Vec<Block> = state
                    .block_history
                    .iter()
                    .rev()
                    .take(take)
                    .cloned()
                    .collect();
                let _ = reply.send(blocks);
            }
```

- [ ] **Step 5: Add the handle method**

In `actor.rs`, immediately after the `get_block` handle method (ends ~line 1947):

```rust
    pub async fn get_recent_blocks(&self, n: usize) -> Result<Vec<Block>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetRecentBlocks(n, reply)).await
    }
```

- [ ] **Step 6: Add the route handler**

In `crates/sybil-api/src/routes/blocks.rs` (the `Query` extractor is already imported on line 2), add above `get_latest_block`:

```rust
#[derive(serde::Deserialize)]
pub struct RecentBlocksQuery {
    pub limit: Option<usize>,
}

/// GET /v1/blocks?limit=N — last N blocks, newest-first, from in-memory history.
#[utoipa::path(
    get,
    path = "/v1/blocks",
    params(("limit" = Option<usize>, Query, description = "Recent blocks, newest-first; clamped to history capacity (default 20)")),
    responses((status = 200, description = "Recent blocks, newest-first", body = [BlockResponse]))
)]
pub async fn get_recent_blocks(
    State(state): State<AppState>,
    Query(q): Query<RecentBlocksQuery>,
) -> Result<Json<Vec<BlockResponse>>, AppError> {
    let limit = q.limit.unwrap_or(20);
    let blocks = state.sequencer.get_recent_blocks(limit).await?;
    Ok(Json(blocks.iter().map(block_to_response).collect()))
}
```

- [ ] **Step 7: Register the route**

In `crates/sybil-api/src/app.rs`, in the `// Blocks` group (line 652), add as the first block route (axum's matchit handles `/v1/blocks` alongside `/v1/blocks/{height}` and `/v1/blocks/latest` without conflict):

```rust
        .route("/v1/blocks", axum::routing::get(routes::blocks::get_recent_blocks))
```

- [ ] **Step 8: Run the test, verify it passes**

Run: `cargo test -p sybil-api --test api_integration recent_blocks_returns_newest_first`
Expected: PASS.

- [ ] **Step 9: Lint, format, commit**

```bash
just fmt
just lint
jj describe -m "feat(api): GET /v1/blocks?limit=N recent-blocks range endpoint"
```

---

## Part B — `created_at_ms` on pending orders

### Task B1: Thread `created_at_ms` through admit → resting order → response

**Files:**
- Test: `crates/sybil-api/tests/api_integration.rs`
- Modify: `crates/matching-sequencer/src/order_book.rs` (`RestingOrder` ~line 57, `accept` ~line 205, `resting_orders_full` ~line 359, test call sites)
- Modify: `crates/matching-sequencer/src/sequencer.rs` (`PendingOrderInfo` ~line 164, `from_resting` ~line 262, `try_admit_direct` ~line 1273 + accept call ~1335, batch accept ~2209, `pending_orders_info`/`market_orderbook` ~1390/1409, test call sites ~4150-4212)
- Modify: `crates/matching-sequencer/src/actor.rs` (`admit_or_defer` ~line 840)
- Modify: `crates/sybil-api-types/src/response.rs` (`PendingOrderResponse` ~line 607)
- Modify: `crates/sybil-api/src/routes/orders.rs` (`to_pending_response` ~line 172)

- [ ] **Step 1: Write the failing integration test**

Append to `crates/sybil-api/tests/api_integration.rs`:

```rust
#[tokio::test]
async fn account_orders_include_created_at_ms() {
    let (app, _) = test_app(true).await;

    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "ts?" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();

    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();

    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let (status, _) = post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": account_id,
            "orders": [{ "type": "BuyYes", "market_id": market_id, "limit_price_nanos": 500_000_000u64, "quantity": 10 }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get(app, &format!("/v1/accounts/{account_id}/orders")).await;
    assert_eq!(status, StatusCode::OK);
    let pending = parse_json(&body);
    let pending = pending.as_array().unwrap();
    assert_eq!(pending.len(), 1, "got {pending:?}");
    let created_at_ms = pending[0]["created_at_ms"].as_u64().unwrap();
    assert!(created_at_ms >= before, "created_at_ms {created_at_ms} not >= submit time {before}");
}
```

- [ ] **Step 2: Run the test, verify it fails**

Run: `cargo test -p sybil-api --test api_integration account_orders_include_created_at_ms`
Expected: FAIL — `pending[0]["created_at_ms"]` is JSON `null` → `as_u64().unwrap()` panics.

- [ ] **Step 3: Add `created_at_ms` to `RestingOrder`**

In `crates/matching-sequencer/src/order_book.rs`, in `struct RestingOrder`, after the `original_max_fill` field (line 56):

```rust
    /// Wall-clock admit time, ms since epoch. Set once by `accept`, never
    /// mutated. `0` on snapshots written before this field (#[serde(default)]).
    /// Surfaced as `PendingOrderResponse.created_at_ms`.
    #[serde(default)]
    pub(crate) created_at_ms: u64,
```

- [ ] **Step 4: Add the `accept()` parameter and set the field**

In `order_book.rs`, change `accept`'s signature (line 205) to add a final parameter after `current_height: u64,`:

```rust
        current_height: u64,
        created_at_ms: u64,
```

and in the `RestingOrder { ... }` literal it builds (line 231), add after `original_max_fill: order.max_fill,`:

```rust
            created_at_ms,
```

- [ ] **Step 5: Widen `resting_orders_full()` to expose `created_at_ms`**

In `order_book.rs`, change `resting_orders_full` (line 359) so its item type and body include `created_at_ms` as the 6th element:

```rust
    pub fn resting_orders_full(
        &self,
    ) -> impl Iterator<Item = (&Order, AccountId, u64, u64, u64, u64)> {
        self.orders.iter().map(|ro| {
            (
                &ro.order,
                ro.account_id,
                ro.created_at,
                ro.expires_at_block,
                ro.original_max_fill,
                ro.created_at_ms,
            )
        })
    }
```

- [ ] **Step 6: Update every other `accept()` call site to pass a timestamp**

The signature change breaks all callers. Production callers get real values in later steps; for now make every call site compile.

In `crates/matching-sequencer/src/order_book.rs` test module, append `, 0` to the `current_height` argument of each `book.accept(...)` call (lines 686, 703, 707, 719, 736, 761, 795, 812, 864, 883, 902, 925, 969). Example — line 686:

```rust
        book.accept(order, aid, account, 1, 0).unwrap();
```

In `crates/matching-sequencer/src/store.rs` test module, do the same for lines 2044, 2098, 2404:

```rust
        book.accept(order, aid, accounts.get(aid).unwrap(), 1, 0)
```

In `crates/matching-sequencer/src/aggregates/liquidity_tracker.rs` line 183:

```rust
        book.accept(order, trader, account, 1, 0).expect("admit");
```

- [ ] **Step 7: Add `created_at_ms` to `try_admit_direct` and pass it to `accept()`**

In `crates/matching-sequencer/src/sequencer.rs`, change `try_admit_direct`'s signature (line 1273):

```rust
    pub fn try_admit_direct(&mut self, submission: OrderSubmission, now_ms: u64) -> AdmitOutcome {
```

and its `accept` call (line 1335):

```rust
            .accept(order, account_id, account, self.height, now_ms)
```

Update the test callers in the same file (lines 4150, 4157, 4171, 4181, 4196, 4212) to pass `0`, e.g.:

```rust
        let outcome = seq.try_admit_direct(first, 0);
```

- [ ] **Step 8: Compute `now_ms` in the actor and pass it into `try_admit_direct`**

In `crates/matching-sequencer/src/actor.rs`, in `admit_or_defer` (line 840): compute `now_ms` at the top (right after `check_account_submission_limits`) and pass it into `try_admit_direct`; then reuse it for the existing `record_trader_placement_analytics` call (delete the duplicate `now_ms` block at line 873).

```rust
    async fn admit_or_defer(&mut self, submission: OrderSubmission) -> Result<(), SequencerError> {
        self.check_account_submission_limits(&submission)?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        match self.sequencer.try_admit_direct(submission, now_ms) {
```

Then in the `Admitted { .. }` arm, remove the re-computation:

```rust
                // (delete) let now_ms = std::time::SystemTime::now()...as_millis() as u64;
                let markets: Vec<MarketId> = resting_order.order.active_markets().collect();
                self.sequencer.record_trader_placement_analytics(
                    resting_order.account_id,
                    markets.clone(),
                    now_ms,
                    false,
                );
```

- [ ] **Step 9: Pass the block timestamp in the batch-admit path**

In `crates/matching-sequencer/src/sequencer.rs`, the batch-admit `accept` call (line 2209) lives inside `produce_block_in_place`, which is called with the block's wall-clock timestamp. Pass that timestamp (the `timestamp_ms` value `produce_block_in_place` receives) as the new arg:

```rust
                        .accept(order.clone(), account_id, account, self.height, timestamp_ms)
```

> Implementer note: confirm the in-scope name of the block timestamp at the top of `produce_block_in_place` (`fn produce_block_in_place` ~line 1930); it is the ms-since-epoch the block is sealed with. If named differently, use that binding.

- [ ] **Step 10: Add `created_at_ms` to `PendingOrderInfo` + `from_resting` + its callers**

In `sequencer.rs`, add to `struct PendingOrderInfo` (after `original_quantity`, line 176):

```rust
    /// Wall-clock admit time, ms since epoch. `0` for pre-existing orders.
    pub created_at_ms: u64,
```

Add the parameter to `from_resting` (line 262) and set it on the struct it returns:

```rust
    fn from_resting(
        order: &Order,
        account_id: AccountId,
        created_at: u64,
        expires_at_block: u64,
        original_max_fill: u64,
        created_at_ms: u64,
    ) -> Self {
```

and in the returned `Self { ... }` add (after the `original_quantity` field):

```rust
            created_at_ms,
```

Update both callers to destructure the new 6-tuple and forward it — `pending_orders_info` (line 1390) and `market_orderbook` (line 1409):

```rust
            .map(
                |(order, aid, created_at, expires_at_block, original_max_fill, created_at_ms)| {
                    PendingOrderInfo::from_resting(
                        order,
                        aid,
                        created_at,
                        expires_at_block,
                        original_max_fill,
                        created_at_ms,
                    )
                },
            )
```

- [ ] **Step 11: Add `created_at_ms` to the response type and map it**

In `crates/sybil-api-types/src/response.rs`, in `struct PendingOrderResponse` (after `original_quantity`, line 621):

```rust
    /// Wall-clock admit time, ms since epoch. `0` for orders admitted before
    /// this field shipped (#[serde(default)] forward compat).
    #[serde(default)]
    pub created_at_ms: u64,
```

In `crates/sybil-api/src/routes/orders.rs`, in `to_pending_response` (line 172), add to the constructed struct:

```rust
        created_at_ms: info.created_at_ms,
```

- [ ] **Step 12: Run the test, verify it passes**

Run: `cargo test -p sybil-api --test api_integration account_orders_include_created_at_ms`
Expected: PASS.

- [ ] **Step 13: Run the affected crates' full suites**

Run: `cargo test -p matching-sequencer && cargo test -p sybil-api`
Expected: PASS (catches any missed `accept` / `try_admit_direct` call site).

- [ ] **Step 14: Lint, format, commit**

```bash
just fmt
just lint
jj describe -m "feat(orders): add created_at_ms (wall-clock admit time) to PendingOrderResponse"
```

---

## Task C: Manual local verification

- [ ] **Step 1: Build and run the API in dev mode**

```bash
cargo run --release -p sybil-api -- --dev-mode --port 3001
```

- [ ] **Step 2: Exercise the recent-blocks endpoint**

In another shell (the dev server seals a block every `--block-interval-ms`, default 500ms, so history fills on its own):

```bash
sleep 3
curl -s 'http://localhost:3001/v1/blocks?limit=5' | jq 'length, .[0].height, .[-1].height'
```

Expected: a length up to 5 and a strictly descending height range (newest first).

- [ ] **Step 3: Exercise `created_at_ms`**

```bash
MID=$(curl -s -XPOST localhost:3001/v1/markets -H 'content-type: application/json' -d '{"name":"verify"}' | jq .market_id)
AID=$(curl -s -XPOST localhost:3001/v1/accounts -H 'content-type: application/json' -d '{"initial_balance_nanos":10000000000}' | jq .account_id)
curl -s -XPOST localhost:3001/v1/orders -H 'content-type: application/json' \
  -d "{\"account_id\":$AID,\"orders\":[{\"type\":\"BuyYes\",\"market_id\":$MID,\"limit_price_nanos\":500000000,\"quantity\":10}]}" >/dev/null
curl -s "localhost:3001/v1/accounts/$AID/orders" | jq '.[0].created_at_ms'
```

Expected: a millisecond epoch timestamp (~now), not `0`.

---

## Self-Review Notes

- **Spec coverage:** #5b (`GET /v1/blocks?limit=N`) = Part A; #5a (`created_at_ms`) = Part B. Both covered.
- **Type consistency:** `get_recent_blocks(usize) -> Result<Vec<Block>, _>` used identically in handle (Step 5) and route (Step 6). `created_at_ms: u64` consistent across `RestingOrder`, the `resting_orders_full` tuple, `PendingOrderInfo`, `from_resting`, and `PendingOrderResponse`.
- **No placeholders:** every code site has exact text; the only judgement call is the block-timestamp binding name in Step 9 (note included).
- **Clamp behavior:** `n.min(block_history_capacity)` matches the existing cap (100); raise `SequencerConfig.block_history_capacity` if windows >100 are ever needed.
