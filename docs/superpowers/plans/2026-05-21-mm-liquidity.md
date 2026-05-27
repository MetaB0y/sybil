# MM Liquidity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Include market-maker (MM) orders in the per-market liquidity score, which today reflects only resting-book depth.

**Architecture:** Liquidity is scored once per block by `LiquidityTracker::record_block`, which walks the post-settle `OrderBook` and sums `limit_price × max_fill` for single-market orders within ±band of the midprice. MM orders never enter the book (they're flash/IOC, solved one-shot per block from `pending_bundles`), so the walk can't see them — that's the bug. Fix: at the call site the solver `Problem` (still borrowed, not moved) holds every order including MM ones, identified by `mm_order_ids_set`; pass that MM slice into `record_block` and score it with the same band logic. Per decision, MM orders are scored by their quoted `max_fill` (the budget cap can make actual fills smaller — that's acceptable).

**Tech Stack:** Rust, integer nanos.

**Conventions:** jj VCS; `just fmt`/`just lint`; run one test with `cargo test -p matching-sequencer <name>`.

---

## File Structure

- Modify `crates/matching-sequencer/src/aggregates/liquidity_tracker.rs` — `record_block` gains an `mm_orders` param + a second scoring pass; `Order` import; new + updated tests.
- Modify `crates/matching-sequencer/src/analytics.rs:284` — `record_liquidity` forwards the MM slice.
- Modify `crates/matching-sequencer/src/sequencer.rs:2417` — build the MM `Order` slice from `problem.orders` and pass it.

---

## Task 1: Score MM orders in `LiquidityTracker::record_block`

**Files:**
- Modify/Test: `crates/matching-sequencer/src/aggregates/liquidity_tracker.rs`

- [ ] **Step 1: Write the failing unit test**

In `liquidity_tracker.rs`, add to the `tests` module:

```rust
    /// MM orders never sit in the book but must still count toward liquidity.
    /// A resting order (qty 4) + an MM order (qty 6), both in-band, score as
    /// mid*(4+6).
    #[test]
    fn record_block_includes_mm_orders() {
        let (markets, accounts, trader, m0, _m1) = two_market_setup();
        let mut book = OrderBook::new(1_000);
        let mid_yes = NANOS_PER_DOLLAR / 2;
        admit(
            &mut book,
            &accounts,
            outcome_buy(&markets, 1, m0, 0, mid_yes, 4),
            trader,
        );

        // Flash MM order — built but NOT accepted into the book.
        let mm = outcome_buy(&markets, 99, m0, 0, mid_yes, 6);
        let mm_slice: Vec<&matching_engine::Order> = vec![&mm];

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![mid_yes, NANOS_PER_DOLLAR - mid_yes]);

        tracker.record_block(&book, &mm_slice, &midprices, 50_000_000);

        assert_eq!(tracker.current(m0), mid_yes.saturating_mul(10));
    }

    /// An out-of-band MM order is excluded, same as resting orders.
    #[test]
    fn record_block_excludes_out_of_band_mm() {
        let (markets, accounts, _trader, m0, _m1) = two_market_setup();
        let book = OrderBook::new(1_000);
        let mid_yes = NANOS_PER_DOLLAR / 2;
        let mm = outcome_buy(&markets, 99, m0, 0, mid_yes - 100_000_000, 6);
        let mm_slice: Vec<&matching_engine::Order> = vec![&mm];

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![mid_yes, NANOS_PER_DOLLAR - mid_yes]);
        tracker.record_block(&book, &mm_slice, &midprices, 50_000_000);

        assert_eq!(tracker.current(m0), 0);
    }
```

- [ ] **Step 2: Run, verify it fails to compile**

Run: `cargo test -p matching-sequencer record_block_includes_mm_orders`
Expected: FAIL — `record_block` takes 3 args, not 4.

- [ ] **Step 3: Add the `Order` import**

In `liquidity_tracker.rs`, change line 18:

```rust
use matching_engine::{MarketId, Nanos, Order};
```

- [ ] **Step 4: Add the `mm_orders` parameter and second scoring pass**

Change `record_block` (line 66) to accept `mm_orders: &[&Order]` as the second parameter, and add an MM pass after the resting-book pass, before the ring-push pass:

```rust
    pub fn record_block(
        &mut self,
        book: &OrderBook,
        mm_orders: &[&Order],
        midprices: &HashMap<MarketId, Vec<Nanos>>,
        band_nanos: u64,
    ) {
        // First pass: aggregate near-the-money depth per market from the
        // resting book in O(N) over orders.
        let mut depth_by_market: HashMap<MarketId, u64> = HashMap::new();
        for (order, _account_id) in book.resting_orders() {
            if order.num_markets != 1 {
                continue;
            }
            let market = order.markets[0];
            let Some(prices) = midprices.get(&market) else {
                continue;
            };
            let mid = prices.first().copied().unwrap_or(0);
            if mid == 0 {
                continue;
            }
            let band_lo = mid.saturating_sub(band_nanos);
            let band_hi = mid.saturating_add(band_nanos);
            if order.limit_price >= band_lo && order.limit_price <= band_hi {
                let value = order.limit_price.saturating_mul(order.max_fill);
                let entry = depth_by_market.entry(market).or_insert(0);
                *entry = entry.saturating_add(value);
            }
        }

        // MM pass: flash MM orders never enter the book, but they provide
        // real near-the-money depth for this batch. Score them with the same
        // single-market band rule (by quoted `max_fill`).
        for order in mm_orders {
            if order.num_markets != 1 {
                continue;
            }
            let market = order.markets[0];
            let Some(prices) = midprices.get(&market) else {
                continue;
            };
            let mid = prices.first().copied().unwrap_or(0);
            if mid == 0 {
                continue;
            }
            let band_lo = mid.saturating_sub(band_nanos);
            let band_hi = mid.saturating_add(band_nanos);
            if order.limit_price >= band_lo && order.limit_price <= band_hi {
                let value = order.limit_price.saturating_mul(order.max_fill);
                let entry = depth_by_market.entry(market).or_insert(0);
                *entry = entry.saturating_add(value);
            }
        }

        // Second pass: push into per-market rings for every market that has
        // a clearing price (so the average for a quiet market stays low
        // rather than stuck on the last non-zero value).
        for &market in midprices.keys() {
            let depth = depth_by_market.get(&market).copied().unwrap_or(0);
            let ring = self.last_n_per_market.entry(market).or_default();
            ring.push_back(depth);
            while ring.len() > LIQUIDITY_RING_CAP {
                ring.pop_front();
            }
        }

        self.band_nanos_at_last_update = band_nanos;
    }
```

Also update the module doc (lines 10-14): the line claiming "MM orders never sit in the resting book … so no MM-specific gating is needed here" is now stale — replace with a note that MM orders are passed in separately and scored in the MM pass.

- [ ] **Step 5: Update existing `record_block` test call sites**

Every existing call in this file's tests passes 3 args; add an empty MM slice as the new 2nd arg. There are 5 call sites — in `record_block_excludes_multi_market`, `ring_caps_at_10` (in a loop), `order_outside_band_excluded`, `liquidity_tracker_snapshot_roundtrip` (in a loop), and `quiet_markets_push_zero` (in a loop). Each becomes:

```rust
        tracker.record_block(&book, &[], &midprices, 50_000_000);
```

- [ ] **Step 6: Run the new tests, verify they pass**

Run: `cargo test -p matching-sequencer -- record_block_includes_mm_orders record_block_excludes_out_of_band_mm`
Expected: PASS.

- [ ] **Step 7: Run the whole liquidity test module**

Run: `cargo test -p matching-sequencer liquidity`
Expected: PASS (the 5 updated tests + the 2 new ones).

- [ ] **Step 8: Commit**

```bash
just fmt && just lint
jj describe -m "feat(liquidity): score flash MM orders into the per-market liquidity ring"
```

---

## Task 2: Forward the MM slice through `record_liquidity`

**Files:**
- Modify: `crates/matching-sequencer/src/analytics.rs:284`

- [ ] **Step 1: Add the parameter and forward it**

Change `AnalyticsState::record_liquidity` (line 284):

```rust
    pub fn record_liquidity(
        &mut self,
        order_book: &OrderBook,
        mm_orders: &[&Order],
        clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
        band_nanos: u64,
    ) {
        self.liquidity_tracker
            .record_block(order_book, mm_orders, clearing_prices, band_nanos);
    }
```

> Implementer note: ensure `Order` is in scope in `analytics.rs` (add `Order` to the existing `matching_engine::{...}` import if absent).

- [ ] **Step 2: Build check (call site not updated yet — expect a failure)**

Run: `cargo build -p matching-sequencer`
Expected: FAIL at the `sequencer.rs:2417` call site (now 3 args vs 4) — fixed in Task 3.

---

## Task 3: Pass MM orders at the call site

**Files:**
- Modify: `crates/matching-sequencer/src/sequencer.rs:2414-2421`

- [ ] **Step 1: Build the MM slice and pass it**

`problem` is still borrowed (not moved) at this point — `solve_batch_phase(&problem, …)` and `finalize_block_state_phase(…, &problem, …)` both take references. Replace the `record_liquidity` call (lines 2417-2421):

```rust
        // Off-block liquidity tracker — score the post-settle resting book
        // PLUS this batch's flash MM orders against each market's midprice.
        // MM orders never enter the book, so pull them from the solver input.
        let mm_orders: Vec<&Order> = problem
            .orders
            .iter()
            .filter(|o| mm_order_ids_set.contains(&o.id))
            .collect();
        self.analytics.record_liquidity(
            &self.order_book,
            &mm_orders,
            &clearing_prices,
            self.config.liquidity_band_nanos,
        );
```

> Implementer note: `Order` is already used throughout `sequencer.rs`; no new import needed. `clearing_prices` here is the `SolvedBatch` field destructured at line 2381.

- [ ] **Step 2: Build + full sequencer suite**

Run: `cargo test -p matching-sequencer`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
just fmt && just lint
jj describe -m "feat(liquidity): pass batch MM orders into the liquidity tracker"
```

---

## Task 4: Manual local verification

- [ ] **Step 1: Run the API + Polymarket mirror (the mirror runs the MM)**

```bash
cargo run --release -p sybil-api -- --dev-mode --port 3001 &
cargo run --release -p sybil-polymarket -- --sybil-url=http://localhost:3001 --max-events=5 --mm-budget-dollars=5000 --mm-initial-balance-dollars=1000000
```

- [ ] **Step 2: Observe liquidity rising on a mirrored market**

Wait for a few blocks, then:

```bash
curl -s localhost:3001/v1/markets | jq '.[] | {market_id, liquidity_avg10_nanos, liquidity_band_nanos}' | head
```

Expected: `liquidity_avg10_nanos` is non-zero on markets the MM quotes — previously it would be ~0 for markets with only MM (flash) participation and no resting book.

---

## Self-Review Notes

- **Spec coverage:** #2 — MM orders now contribute to the liquidity score. Resting-book behavior is unchanged (MM scoring is additive; no double-count since MM orders are never in the book and resting orders are never in `mm_order_ids_set`).
- **Decision honored:** MM scored by `max_fill` (quoted depth); budget-capped actual fills may be smaller — accepted.
- **Type consistency:** `record_block(book, mm_orders: &[&Order], midprices, band)` and `record_liquidity(book, mm_orders: &[&Order], clearing_prices, band)` agree; call site passes `&Vec<&Order>` which coerces to `&[&Order]`.
- **No placeholders:** all code exact; the two implementer notes are import-scope confirmations.
