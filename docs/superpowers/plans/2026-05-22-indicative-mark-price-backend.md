# Indicative / Mark Price (Backend) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every market a continuously-updating "mark price" (real clearing price when a batch trades, book touch-midpoint when it doesn't) and use it for the price-history charts, price deltas, liquidity scoring, and unrealized-PnL/equity — without touching consensus, settlement, or realized PnL.

**Architecture:** Add one pure primitive in `matching-engine` that derives a per-market YES touch-midpoint from the resting single-market book. At block finalize the `PriceTracker` computes a per-market **mark** via the ladder *clearing-if-volume>0 → midpoint → carry-over last mark*, records it into the price-history series (coalescing flat ticks), and exposes a sibling `last_mark_prices` map. Liquidity uses the mark as its band center and sums (instead of averages) its ring; the portfolio/equity path values open positions at the mark. The mark lives entirely in the off-block serving layer — it never enters `Block.clearing_prices`, `state_root`, the witness, or settlement.

**Tech Stack:** Rust workspace (`matching-engine`, `matching-sequencer`, `sybil-api`), `cargo`/`just`, Docker + `kamal`-free `just deploy-api` (build on Mac via colima+Rosetta, ship to Linode).

**Spec:** `docs/superpowers/specs/2026-05-22-indicative-mark-price-backend-design.md`

---

## Pre-flight: isolated worktree

This is backend work with a container build. The FE dev server runs on `r/dev` with uncommitted changes and `main` lacks `lightweight-charts`, so do NOT switch branches in the main checkout. Work in a dedicated worktree.

- [ ] **Step 1: Create the worktree off the current branch**

The design spec was committed on `r/dev` (commit `d798eb2`). Branch from current `HEAD` so the spec travels with the work.

Run:
```bash
cd /Users/r/pr/Sybil
git worktree add -b r/indicative-mark-price ../Sybil-mark r/dev
cd ../Sybil-mark
git log --oneline -1
```
Expected: new worktree at `../Sybil-mark`, HEAD = the design-spec commit.

- [ ] **Step 2: Baseline build/test green before changes**

Run:
```bash
cargo build -p matching-engine -p matching-sequencer -p sybil-api 2>&1 | tail -5
```
Expected: `Finished` (no errors). If it fails, stop — the tree is not clean.

> All remaining steps run inside `../Sybil-mark`.

---

## Task 1: Pure primitive — `book_midprices` + `mark_yes_no`

**Files:**
- Create: `crates/matching-engine/src/midprice.rs`
- Modify: `crates/matching-engine/src/lib.rs` (add `pub mod midprice;` + re-export)

The synthetic YES book from single-market orders:
- `BuyYes` @ `limit_price` → YES **bid**
- `SellNo` @ `NANOS_PER_DOLLAR - limit_price` → YES **bid**
- `SellYes` @ `limit_price` → YES **ask**
- `BuyNo` @ `NANOS_PER_DOLLAR - limit_price` → YES **ask**

`best_bid = max(bids)`, `best_ask = min(asks)`; midpoint exists only when both sides are present and `best_bid < best_ask`.

- [ ] **Step 1: Write the failing tests**

Create `crates/matching-engine/src/midprice.rs`:

```rust
//! Book-derived "indicative" pricing for batches that do not cross.
//!
//! When a batch matches volume, the uniform clearing price is the market's
//! price. When nothing crosses, this module derives a touch midpoint from the
//! resting *single-market* order book — the analogue of an order-book mid.
//! Multi-market (bundle/spread) orders are excluded: their `limit_price` is a
//! bundle total, not attributable to one market (same rule the liquidity
//! tracker applies).
//!
//! Everything here is serving-layer only. It never feeds blocks, the witness,
//! settlement, or realized PnL.

use std::collections::HashMap;

use crate::order::derive_order_direction;
use crate::types::{MarketId, Nanos, NANOS_PER_DOLLAR};
use crate::OrderDirection;

/// Per-market YES touch midpoint over the resting single-market book.
///
/// Returns an entry only for markets with a two-sided, non-crossed book
/// (`best_bid < best_ask`). One-sided, empty, or crossed books are omitted —
/// the caller carries over the last mark for those.
pub fn book_midprices<'a>(
    orders: impl IntoIterator<Item = &'a crate::Order>,
) -> HashMap<MarketId, Nanos> {
    let mut best_bid: HashMap<MarketId, Nanos> = HashMap::new();
    let mut best_ask: HashMap<MarketId, Nanos> = HashMap::new();

    for order in orders {
        if order.num_markets != 1 {
            continue;
        }
        let market = order.markets[0];
        if market.is_none() {
            continue;
        }
        let (is_bid, price) = match derive_order_direction(order, market) {
            OrderDirection::BuyYes => (true, order.limit_price),
            OrderDirection::SellNo => (true, NANOS_PER_DOLLAR.saturating_sub(order.limit_price)),
            OrderDirection::SellYes => (false, order.limit_price),
            OrderDirection::BuyNo => (false, NANOS_PER_DOLLAR.saturating_sub(order.limit_price)),
        };
        if is_bid {
            best_bid
                .entry(market)
                .and_modify(|b| {
                    if price > *b {
                        *b = price;
                    }
                })
                .or_insert(price);
        } else {
            best_ask
                .entry(market)
                .and_modify(|a| {
                    if price < *a {
                        *a = price;
                    }
                })
                .or_insert(price);
        }
    }

    let mut mids = HashMap::new();
    for (&market, &bid) in &best_bid {
        if let Some(&ask) = best_ask.get(&market) {
            if bid < ask {
                mids.insert(market, (bid + ask) / 2);
            }
        }
    }
    mids
}

/// Resolve a market's `[yes, no]` mark via the ladder:
/// clearing-if-filled → midpoint → previous mark → last clearing → 50/50.
pub fn mark_yes_no(
    had_fill: bool,
    clearing: Option<&Vec<Nanos>>,
    midpoint: Option<Nanos>,
    last_mark: Option<&Vec<Nanos>>,
) -> Vec<Nanos> {
    let half = NANOS_PER_DOLLAR / 2;
    if had_fill {
        if let Some(c) = clearing {
            return c.clone();
        }
    }
    if let Some(mid) = midpoint {
        return vec![mid, NANOS_PER_DOLLAR.saturating_sub(mid)];
    }
    if let Some(prev) = last_mark {
        return prev.clone();
    }
    if let Some(c) = clearing {
        return c.clone();
    }
    vec![half, half]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order_builder::{outcome_buy, outcome_sell, spread};
    use crate::MarketSet;

    fn markets() -> (MarketSet, MarketId, MarketId) {
        let mut m = MarketSet::new();
        let m0 = m.add_binary("mid_m0");
        let m1 = m.add_binary("mid_m1");
        (m, m0, m1)
    }

    // BuyYes @ 40c (bid) + SellYes @ 60c (ask) → mid 50c.
    #[test]
    fn two_sided_no_cross_yields_midpoint() {
        let (ms, m0, _) = markets();
        let bid = outcome_buy(&ms, 1, m0, 0, 400_000_000, 5);
        let ask = outcome_sell(&ms, 2, m0, 0, 600_000_000, 5);
        let orders = vec![bid, ask];
        let mids = book_midprices(orders.iter());
        assert_eq!(mids.get(&m0).copied(), Some(500_000_000));
    }

    // BuyNo @ 30c is a YES ask at 70c; SellNo @ 80c is a YES bid at 20c.
    // bid 20c, ask 70c → mid 45c.
    #[test]
    fn no_side_orders_map_into_yes_book() {
        let (ms, m0, _) = markets();
        let yes_ask_via_no = outcome_buy(&ms, 1, m0, 1, 300_000_000, 5); // BuyNo @30c -> ask 70c
        let yes_bid_via_no = outcome_sell(&ms, 2, m0, 1, 800_000_000, 5); // SellNo @80c -> bid 20c
        let orders = vec![yes_ask_via_no, yes_bid_via_no];
        let mids = book_midprices(orders.iter());
        assert_eq!(mids.get(&m0).copied(), Some(450_000_000));
    }

    // Only bids, no asks → no midpoint.
    #[test]
    fn one_sided_book_has_no_midpoint() {
        let (ms, m0, _) = markets();
        let bid = outcome_buy(&ms, 1, m0, 0, 400_000_000, 5);
        let orders = vec![bid];
        let mids = book_midprices(orders.iter());
        assert!(mids.get(&m0).is_none());
    }

    // Multi-market spread orders are ignored entirely.
    #[test]
    fn multi_market_orders_excluded() {
        let (ms, m0, m1) = markets();
        let sp = spread(&ms, 1, m0, m1, 500_000_000, 5);
        let orders = vec![sp];
        let mids = book_midprices(orders.iter());
        assert!(mids.is_empty());
    }

    // Crossed book (bid >= ask) yields no midpoint (a real batch would match).
    #[test]
    fn crossed_book_has_no_midpoint() {
        let (ms, m0, _) = markets();
        let bid = outcome_buy(&ms, 1, m0, 0, 700_000_000, 5);
        let ask = outcome_sell(&ms, 2, m0, 0, 300_000_000, 5);
        let orders = vec![bid, ask];
        let mids = book_midprices(orders.iter());
        assert!(mids.get(&m0).is_none());
    }

    #[test]
    fn mark_ladder_prefers_clearing_when_filled() {
        let clearing = vec![620_000_000, 380_000_000];
        let got = mark_yes_no(true, Some(&clearing), Some(500_000_000), None);
        assert_eq!(got, clearing);
    }

    #[test]
    fn mark_ladder_uses_midpoint_when_not_filled() {
        let clearing = vec![620_000_000, 380_000_000];
        let got = mark_yes_no(false, Some(&clearing), Some(500_000_000), None);
        assert_eq!(got, vec![500_000_000, 500_000_000]);
    }

    #[test]
    fn mark_ladder_carries_over_when_no_midpoint() {
        let last_mark = vec![510_000_000, 490_000_000];
        let got = mark_yes_no(false, None, None, Some(&last_mark));
        assert_eq!(got, last_mark);
    }

    #[test]
    fn mark_ladder_defaults_to_half() {
        let got = mark_yes_no(false, None, None, None);
        assert_eq!(got, vec![NANOS_PER_DOLLAR / 2, NANOS_PER_DOLLAR / 2]);
    }
}
```

- [ ] **Step 2: Wire the module into the crate**

In `crates/matching-engine/src/lib.rs`, add the module declaration after `pub mod market;` (keep alphabetical-ish ordering near the others):

```rust
pub mod midprice;
```

And add a re-export after the `pub use market::{...};` line:

```rust
pub use midprice::{book_midprices, mark_yes_no};
```

- [ ] **Step 3: Run the tests, expect FAIL first**

Run:
```bash
cargo test -p matching-engine midprice:: 2>&1 | tail -20
```
Expected (before Step 2 is saved, or if the module body has a typo): compile error. After Steps 1–2: all `midprice::tests::*` PASS.

- [ ] **Step 4: Confirm green**

Run:
```bash
cargo test -p matching-engine midprice:: 2>&1 | tail -20
```
Expected: `test result: ok.` with 9 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/matching-engine/src/midprice.rs crates/matching-engine/src/lib.rs
git commit -m "feat(engine): book touch-midpoint + mark-price ladder primitives

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `PriceTracker` mark series (charts + deltas)

**Files:**
- Modify: `crates/matching-sequencer/src/price_tracker.rs` (struct field, constructors, `record_block`, accessor)
- Modify: `crates/matching-sequencer/src/analytics.rs` (`record_finalized_block` signature + `last_mark_prices` accessor)
- Modify: `crates/matching-sequencer/src/sequencer.rs` (`finalize_block_state_phase`: compute midpoints, thread mark map; `FinalizedBlockState`; `last_mark_prices()` accessor)

### 2a. Add `last_mark_prices` to `PriceTracker`

- [ ] **Step 1: Add the field**

In `crates/matching-sequencer/src/price_tracker.rs`, inside `struct PriceTracker` (after the `last_clearing_prices` field at line 54), add:

```rust
    /// Sibling of `last_clearing_prices`: the most recent **mark** per market
    /// (clearing when traded, else book midpoint, else carry-over). Serving
    /// layer only — never persisted or sent to consensus. Seeded from
    /// `last_clearing_prices` on restore so the portfolio has a mark before the
    /// first post-restart block.
    last_mark_prices: HashMap<MarketId, Vec<Nanos>>,
```

- [ ] **Step 2: Initialize in all three constructors**

In `with_retention` (the `Self { ... }` at lines 89–98), add after `last_clearing_prices: HashMap::new(),`:
```rust
            last_mark_prices: HashMap::new(),
```

In `with_state_and_retention` (the `Self { ... }` at lines 119–128), add after `last_clearing_prices,`:
```rust
            last_mark_prices: last_clearing_prices_seed,
```
and change the function body to compute the seed before constructing. Replace the existing `with_state_and_retention` body opening so the struct literal can reference a clone — i.e. insert this line immediately after the `pub fn with_state_and_retention(...) -> Self {` signature, before `Self {`:
```rust
        let last_clearing_prices_seed = last_clearing_prices.clone();
```

- [ ] **Step 3: Add the accessor**

After the existing `last_clearing_prices()` accessor (lines 162–165), add:
```rust
    /// Current mark prices (clearing-or-indicative). Always at least as
    /// populated as `last_clearing_prices` after the first block.
    pub fn last_mark_prices(&self) -> &HashMap<MarketId, Vec<Nanos>> {
        &self.last_mark_prices
    }
```

- [ ] **Step 4: Build to confirm field wiring compiles**

Run:
```bash
cargo build -p matching-sequencer 2>&1 | tail -15
```
Expected: compiles (the field is unused-warning only; we use it next).

### 2b. Rewrite `record_block` to emit the mark series

- [ ] **Step 5: Write the failing test first**

In `crates/matching-sequencer/src/price_tracker.rs`, add to the existing `#[cfg(test)] mod tests` (or create one if absent) a test that drives a no-fill block with a midpoint and asserts a `PricePoint` lands at the midpoint with `volume_nanos == 0`. Append:

```rust
    #[test]
    fn record_block_emits_midpoint_point_for_no_cross_market() {
        use matching_engine::{MarketId, NANOS_PER_DOLLAR};
        let mut pt = PriceTracker::new();

        let m0 = MarketId::new(0);
        let clearing: HashMap<MarketId, Vec<Nanos>> = HashMap::new(); // never traded
        let mut midpoints: HashMap<MarketId, Nanos> = HashMap::new();
        midpoints.insert(m0, 450_000_000);
        let orders: HashMap<u64, &Order> = HashMap::new();

        let (vol, mark) = pt.record_block(&[], &orders, &clearing, &midpoints, 1, 1_000);

        assert!(vol.is_empty(), "no fills => no volume");
        assert_eq!(mark.get(&m0).cloned(), Some(vec![450_000_000, NANOS_PER_DOLLAR - 450_000_000]));

        let hist = pt.price_history(m0, None, None);
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].yes_price, 450_000_000);
        assert_eq!(hist[0].volume_nanos, 0);

        // A second identical no-cross block coalesces (no new flat point).
        pt.record_block(&[], &orders, &clearing, &midpoints, 2, 2_000);
        assert_eq!(pt.price_history(m0, None, None).len(), 1, "flat tick coalesced");

        // Midpoint moves => new point.
        midpoints.insert(m0, 470_000_000);
        pt.record_block(&[], &orders, &clearing, &midpoints, 3, 3_000);
        assert_eq!(pt.price_history(m0, None, None).len(), 2);
    }
```

> Note: `MarketId::new` and `PricePoint { yes_price, no_price, volume_nanos, height, timestamp_ms }` match `market_info.rs`. If `MarketId::new` does not exist, use `MarketId(0)`.

- [ ] **Step 6: Replace the `record_block` body**

In `crates/matching-sequencer/src/price_tracker.rs`, change the import line at the top (line 5) to also pull in the mark helper and `HashSet` (HashSet is already imported on line 3):
```rust
use matching_engine::{book_midprices as _book_midprices_unused, mark_yes_no, Fill, MarketId, Nanos, Order, NANOS_PER_DOLLAR};
```
(Then drop the unused alias — simplest is:)
```rust
use matching_engine::{mark_yes_no, Fill, MarketId, Nanos, Order, NANOS_PER_DOLLAR};
```

Replace the entire `record_block` method (current lines 200–286) with:

```rust
    /// Record the per-block price series, volumes, and mark prices. Returns
    /// `(per_market_volume, mark_prices)`. The mark series powers live charts
    /// and 24h deltas; `mark_prices` is also reused by the liquidity and
    /// equity trackers so they value markets that have a book but no cross.
    pub fn record_block(
        &mut self,
        fills: &[Fill],
        orders: &HashMap<u64, &Order>,
        clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
        midpoints: &HashMap<MarketId, Nanos>,
        height: u64,
        timestamp_ms: u64,
    ) -> (HashMap<MarketId, u64>, HashMap<MarketId, Vec<Nanos>>) {
        // Per-market and platform volume from raw fills (multi-market orders
        // credit each active market; the platform total counts each fill once).
        let mut per_market_volume: HashMap<MarketId, u64> = HashMap::new();
        let mut platform_block_volume: u64 = 0;
        for fill in fills {
            if fill.fill_qty == 0 {
                continue;
            }
            let vol = fill.fill_price.saturating_mul(fill.fill_qty);
            platform_block_volume = platform_block_volume.saturating_add(vol);
            if let Some(order) = orders.get(&fill.order_id) {
                for mid in order.active_markets() {
                    *per_market_volume.entry(mid).or_insert(0) += vol;
                }
            }
        }

        // Universe of markets to mark this block: anything with a (carry-over)
        // clearing price, anything with a fresh midpoint, plus filled markets.
        let mut universe: HashSet<MarketId> = clearing_prices.keys().copied().collect();
        universe.extend(midpoints.keys().copied());
        universe.extend(per_market_volume.keys().copied());

        let mut mark_prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();
        for &mid in &universe {
            let vol = per_market_volume.get(&mid).copied().unwrap_or(0);
            let had_fill = vol > 0;
            let mark = mark_yes_no(
                had_fill,
                clearing_prices.get(&mid),
                midpoints.get(&mid).copied(),
                self.last_mark_prices.get(&mid),
            );
            let yes_price = mark.first().copied().unwrap_or(NANOS_PER_DOLLAR / 2);
            let no_price = mark
                .get(1)
                .copied()
                .unwrap_or_else(|| NANOS_PER_DOLLAR.saturating_sub(yes_price));

            // Coalesce flat no-trade ticks: skip the append when the price is
            // unchanged AND nothing traded. Trades always produce a point.
            let unchanged = vol == 0
                && self
                    .price_history
                    .get(&mid)
                    .and_then(|h| h.last())
                    .map(|p| p.yes_price == yes_price && p.no_price == no_price)
                    .unwrap_or(false);
            if !unchanged {
                let history = self.price_history.entry(mid).or_default();
                history.push(PricePoint {
                    height,
                    timestamp_ms,
                    yes_price,
                    no_price,
                    volume_nanos: vol,
                });
                let overflow = history.len().saturating_sub(self.max_history_points_per_market);
                if overflow > 0 {
                    history.drain(0..overflow);
                }
            }

            if vol > 0 {
                *self.market_volumes.entry(mid).or_insert(0) += vol;
            }
            self.last_mark_prices.insert(mid, mark.clone());
            mark_prices.insert(mid, mark);
        }

        // Volume extensions: running platform total + current hourly buckets.
        self.platform_volume = self.platform_volume.saturating_add(platform_block_volume);
        let hour_start_ms = timestamp_ms - (timestamp_ms % HOUR_MS);
        self.ensure_current_volume_bucket(hour_start_ms);
        if let Some((_, market_bucket)) = self.hourly_per_market.back_mut() {
            for (&mid, &vol) in &per_market_volume {
                let entry = market_bucket.entry(mid).or_insert(0);
                *entry = entry.saturating_add(vol);
            }
        }
        if let Some((_, platform_bucket)) = self.hourly_platform.back_mut() {
            *platform_bucket = platform_bucket.saturating_add(platform_block_volume);
        }

        // Hourly clearing/mark history (24h delta anchor): first observation per
        // hour wins. Use the mark so deltas reflect indicative movement too.
        for (&mid, mark) in &mark_prices {
            let bucket = self.hourly_clearing_prices.entry(mid).or_default();
            let need_new = bucket
                .back()
                .map(|(t, _)| *t != hour_start_ms)
                .unwrap_or(true);
            if need_new {
                bucket.push_back((hour_start_ms, mark.clone()));
                while bucket.len() > HOURLY_CLEARING_HISTORY_CAP {
                    bucket.pop_front();
                }
            }
        }

        (per_market_volume, mark_prices)
    }
```

- [ ] **Step 7: Run the new test, expect PASS**

Run:
```bash
cargo test -p matching-sequencer price_tracker::tests::record_block_emits_midpoint 2>&1 | tail -20
```
Expected: PASS. (If it fails to compile because other tests/callers use the old `record_block` signature, that's fixed in 2c — proceed and re-run after.)

### 2c. Thread midpoints + mark map through analytics and the sequencer

- [ ] **Step 8: Update `record_finalized_block` in `analytics.rs`**

In `crates/matching-sequencer/src/analytics.rs`, replace the `record_finalized_block` method (lines 243–265) with:

```rust
    #[allow(clippy::too_many_arguments)]
    pub fn record_finalized_block(
        &mut self,
        fills: &[Fill],
        orders: &HashMap<u64, &Order>,
        clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
        midpoints: &HashMap<MarketId, Nanos>,
        height: u64,
        timestamp_ms: u64,
        accounts: &AccountStore,
    ) -> (HashMap<MarketId, u64>, HashMap<MarketId, Vec<Nanos>>) {
        let (volume_by_market, mark_prices) = self.price_tracker.record_block(
            fills,
            orders,
            clearing_prices,
            midpoints,
            height,
            timestamp_ms,
        );
        self.fill_recorder.record_fills(
            fills,
            orders,
            height,
            timestamp_ms,
            &mut self.cost_basis_tracker,
            accounts,
            &mut self.account_event_log,
        );
        (volume_by_market, mark_prices)
    }
```

Add a mark accessor next to `last_clearing_prices` (after lines 96–98):
```rust
    pub fn last_mark_prices(&self) -> &HashMap<MarketId, Vec<Nanos>> {
        self.price_tracker.last_mark_prices()
    }
```

- [ ] **Step 9: Update `FinalizedBlockState` + `finalize_block_state_phase` in `sequencer.rs`**

Find the `FinalizedBlockState` struct (search `struct FinalizedBlockState`) and add a field:
```rust
    mark_prices: HashMap<MarketId, Vec<Nanos>>,
```

In `finalize_block_state_phase` (lines 1825–1856 region), replace the `record_finalized_block` call block (lines 1848–1856) with:

```rust
        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();
        // Touch midpoints from the resting single-market book for markets that
        // did not cross this batch. Scoped so the immutable book borrow is
        // released before the &mut self.analytics call below.
        let midpoints = {
            let resting: Vec<&Order> =
                self.order_book.resting_orders().map(|(o, _)| o).collect();
            matching_engine::book_midprices(resting.iter().copied())
        };
        let (volume_by_market, mark_prices) = self.analytics.record_finalized_block(
            fills,
            &order_map,
            clearing_prices,
            &midpoints,
            self.height,
            timestamp_ms,
            &self.accounts,
        );
```

Then find the `FinalizedBlockState { post_state, volume_by_market }` construction at the end of `finalize_block_state_phase` and change it to include the mark map:
```rust
        FinalizedBlockState {
            post_state,
            volume_by_market,
            mark_prices,
        }
```

- [ ] **Step 10: Capture `mark_prices` in `produce_block`**

At the `finalize_block_state_phase` call site (lines 2547–2550), change the destructure to bind the new field:
```rust
        let FinalizedBlockState {
            post_state,
            volume_by_market,
            mark_prices,
        } = self.finalize_block_state_phase(&fills, &problem, &clearing_prices, timestamp_ms);
```

> `mark_prices` is now in scope in `produce_block`. Tasks 3 and 4 consume it. For this task it may be unused — add `let _ = &mark_prices;` right after the destructure if the compiler errors on unused, and remove that line in Task 3.

- [ ] **Step 11: Add `last_mark_prices()` on `BlockSequencer`**

Near the existing `last_clearing_prices` delegation in `sequencer.rs` (search `fn last_clearing_prices`), add a sibling. If `BlockSequencer` does not already expose `last_clearing_prices`, add this method on its `impl` block (same impl that holds `portfolio_summary`):
```rust
    pub fn last_mark_prices(&self) -> &HashMap<MarketId, Vec<Nanos>> {
        self.analytics.last_mark_prices()
    }
```

- [ ] **Step 12: Build + run the full sequencer test suite**

Run:
```bash
cargo build -p matching-sequencer 2>&1 | tail -20
```
Fix any remaining call sites of the old `record_block`/`record_finalized_block` signatures (e.g. tests using `price_tracker_mut().record_block(...)`): add the `&midpoints` argument (`&HashMap::new()` where no book is relevant) and adjust the destructure to `(vol, _mark)`.

Run:
```bash
cargo test -p matching-sequencer price_tracker:: 2>&1 | tail -20
```
Expected: all price_tracker tests PASS, including the new midpoint test.

- [ ] **Step 13: Commit**

```bash
git add crates/matching-sequencer/src/price_tracker.rs crates/matching-sequencer/src/analytics.rs crates/matching-sequencer/src/sequencer.rs
git commit -m "feat(prices): record clearing-or-midpoint mark series for charts/deltas

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Liquidity uses the mark + sums the ring

**Files:**
- Modify: `crates/matching-sequencer/src/aggregates/liquidity_tracker.rs` (`avg_last_n` → sum semantics; doc)
- Modify: `crates/matching-sequencer/src/sequencer.rs` (`record_liquidity` arg: pass `mark_prices`)

The band-center swap is free: `record_liquidity`/`record_block` already take a `midprices: &HashMap<MarketId, Vec<Nanos>>` — we just pass the mark map instead of `clearing_prices`. The aggregation change converts the ring average into a sum.

- [ ] **Step 1: Write the failing test (sum, not average)**

In `crates/matching-sequencer/src/aggregates/liquidity_tracker.rs` tests module, add:

```rust
    /// `sum_last_n` totals the ring instead of averaging it.
    #[test]
    fn sum_last_n_totals_the_ring() {
        let (markets, accounts, trader, m0, _m1) = two_market_setup();
        let mut book = OrderBook::new(1_000);
        let mid_yes = NANOS_PER_DOLLAR / 2;
        admit(
            &mut book,
            &accounts,
            outcome_buy(&markets, 1, m0, 0, mid_yes, 2),
            trader,
        );

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![mid_yes, NANOS_PER_DOLLAR - mid_yes]);

        for _ in 0..3 {
            tracker.record_block(&book, &[], &midprices, 50_000_000);
        }
        let per_block = mid_yes.saturating_mul(2);
        assert_eq!(tracker.sum_last_n(m0, 10), per_block * 3);
    }
```

- [ ] **Step 2: Run it, expect FAIL (method missing)**

Run:
```bash
cargo test -p matching-sequencer liquidity_tracker::tests::sum_last_n 2>&1 | tail -15
```
Expected: FAIL — `no method named sum_last_n`.

- [ ] **Step 3: Add `sum_last_n` and switch the bulk view to it**

In `liquidity_tracker.rs`, add after `avg_last_n` (line 153):

```rust
    /// Sum over the last `n` ring entries (capped at the ring length). This is
    /// the windowed near-the-money depth across recent blocks — the headline
    /// liquidity metric. Returns 0 when the market has never been recorded.
    pub fn sum_last_n(&self, market_id: MarketId, n: usize) -> u64 {
        let Some(ring) = self.last_n_per_market.get(&market_id) else {
            return 0;
        };
        if ring.is_empty() || n == 0 {
            return 0;
        }
        let take = n.min(ring.len());
        ring.iter()
            .rev()
            .take(take)
            .copied()
            .fold(0u64, |acc, v| acc.saturating_add(v))
    }
```

Change `all_avg_last_n` (lines 172–177) to total instead of average — rename to `all_sum_last_n` and use `sum_last_n`:

```rust
    /// Bulk view: `sum_last_n(m, n)` for every market the tracker knows about.
    pub fn all_sum_last_n(&self, n: usize) -> HashMap<MarketId, u64> {
        self.last_n_per_market
            .keys()
            .map(|&m| (m, self.sum_last_n(m, n)))
            .collect()
    }
```

- [ ] **Step 4: Update the analytics passthroughs**

In `crates/matching-sequencer/src/analytics.rs`, change `liquidity_avg10` / `all_liquidity_avg10` (lines 192–198) to use the sum (keep the public method names so the API wire field is unchanged — only the math changes):

```rust
    pub fn liquidity_avg10(&self, market_id: MarketId) -> u64 {
        self.liquidity_tracker.sum_last_n(market_id, 10)
    }

    pub fn all_liquidity_avg10(&self) -> HashMap<MarketId, u64> {
        self.liquidity_tracker.all_sum_last_n(10)
    }
```

> Keeping the `avg10` method/field names avoids an API rename in this backend-only pass; the FE-phase effort can rename `liquidity_avg10_nanos` if desired.

- [ ] **Step 5: Pass the mark map as the liquidity band center**

In `crates/matching-sequencer/src/sequencer.rs`, at the `record_liquidity` call (lines 2577–2582), replace `&clearing_prices` with `&mark_prices`:

```rust
        self.analytics.record_liquidity(
            &self.order_book,
            &mm_orders,
            &mark_prices,
            self.config.liquidity_band_nanos,
        );
```

- [ ] **Step 6: Run liquidity tests + build**

Run:
```bash
cargo test -p matching-sequencer liquidity_tracker:: 2>&1 | tail -20
cargo build -p matching-sequencer -p sybil-api 2>&1 | tail -10
```
Expected: liquidity tests PASS (the existing `avg_last_n` tests still pass — that method is retained); build clean.

- [ ] **Step 7: Commit**

```bash
git add crates/matching-sequencer/src/aggregates/liquidity_tracker.rs crates/matching-sequencer/src/analytics.rs crates/matching-sequencer/src/sequencer.rs
git commit -m "feat(liquidity): center band on mark price and sum the ring window

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Portfolio + equity value positions at the mark

**Files:**
- Modify: `crates/matching-sequencer/src/sequencer.rs` (`portfolio_summary`: pass mark; `record_equity` call: pass mark)

`compute_portfolio` and `EquityTracker::record` already take a `prices`/`last_prices` map — we only change which map flows in. Realized PnL (`cost_basis_tracker`) and settlement are untouched.

- [ ] **Step 1: Point `portfolio_summary` at the mark map**

In `crates/matching-sequencer/src/sequencer.rs`, in `portfolio_summary` (line 1104–1110), change the prices argument:

```rust
        Ok(crate::portfolio::compute_portfolio(
            account,
            self.last_mark_prices(),
            self.analytics.first_deposit_ms(account_id).unwrap_or(0),
            self.analytics.total_fills(account_id),
            self.analytics.cost_basis_tracker(),
        ))
```

- [ ] **Step 2: Point `record_equity` at the mark map**

At the `record_equity` call (lines 2590–2596), replace `&clearing_prices` with `&mark_prices` (and remove the temporary `let _ = &mark_prices;` from Task 2 Step 10 if it was added):

```rust
        self.analytics.record_equity(
            &touched,
            &self.accounts,
            &mark_prices,
            self.height,
            timestamp_ms,
        );
```

- [ ] **Step 3: Add an integration assertion**

Add a focused test next to the existing sequencer tests (search the `#[cfg(test)] mod tests` in `sequencer.rs` for a helper that builds a `BlockSequencer` with a market and account). Model it on an existing test that places orders and produces a block. The assertion: place a resting BuyYes @ 40c and a resting SellYes @ 60c on a fresh market (no cross), produce one block, then assert `portfolio_summary` values a held YES position at ~50c rather than the 50/50 default — i.e. unrealized PnL reflects the midpoint.

```rust
    #[test]
    fn unrealized_pnl_uses_book_midpoint_when_no_cross() {
        // Reuse the crate's existing sequencer test harness/builders. Steps:
        //  1. Create one binary market.
        //  2. Give an account a YES position (e.g. via a prior crossed batch
        //     or direct position seed used by sibling tests).
        //  3. Place a two-sided, non-crossing book (BuyYes 40c, SellYes 60c).
        //  4. produce_block().
        //  5. let p = seq.portfolio_summary(acct).unwrap();
        //     assert that the position's current_price_nanos == 500_000_000.
        // Concrete builder calls mirror the nearest existing produce_block test
        // in this module; copy that setup verbatim and add the assertion above.
    }
```

> This is the one test whose exact setup depends on the local sequencer test harness. During execution, copy the closest existing `produce_block` test's scaffolding (account creation, market creation, order submission, `produce_block`) and add the midpoint assertion. Do not invent new harness helpers.

- [ ] **Step 4: Build + run sequencer tests**

Run:
```bash
cargo test -p matching-sequencer 2>&1 | tail -25
```
Expected: all PASS, including the new unrealized-PnL test.

- [ ] **Step 5: Commit**

```bash
git add crates/matching-sequencer/src/sequencer.rs
git commit -m "feat(portfolio): value open positions at mark price (unrealized only)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Full verification

- [ ] **Step 1: Workspace test + lint + fmt (CI equivalent)**

Run:
```bash
just check-all 2>&1 | tail -30
```
Expected: fmt-check clean, clippy clean, `cargo test --workspace` all green. Fix any clippy/fmt issues and re-run until clean.

- [ ] **Step 2: Smoke test the API end-to-end**

Run:
```bash
./scripts/smoke-test.sh 2>&1 | tail -30
```
Expected: server starts, API exercised, teardown — exit 0.

- [ ] **Step 3: Manual indicative-price sanity check**

Start a dev server, create a market, place a non-crossing two-sided book (no trades), and confirm the new behavior:
```bash
cargo run --release -p sybil-api -- --dev-mode &
SERVER=http://localhost:3000
# create market + place BuyYes 40c and SellYes 60c via the dev/order API,
# then after one block interval:
curl -s "$SERVER/v1/markets/0/prices/history" | tail -c 400   # expect a moving point at ~50c, volume 0
curl -s "$SERVER/v1/markets" | python3 -m json.tool | grep -i liquidity   # expect > 0
kill %1
```
Expected: a `PricePoint` near 50c with `volume_nanos: 0`; non-zero liquidity for the market despite no trades.

- [ ] **Step 4: Commit any fixes from verification**

```bash
git add -A && git commit -m "test: verification fixes for mark-price backend" || echo "nothing to fix"
```

---

## Task 6: Push, PR (no merge), build + deploy

- [ ] **Step 1: Push the branch**

Run:
```bash
git push -u origin r/indicative-mark-price
```

- [ ] **Step 2: Open the PR to `main` (do NOT merge)**

Run:
```bash
gh pr create --base main --head r/indicative-mark-price \
  --title "Indicative / mark price (backend)" \
  --body "$(cat <<'EOF'
## Summary
Introduce a per-market **mark price** (clearing when a batch trades, book touch-midpoint when it does not) and use it for price-history charts, 24h deltas, liquidity scoring, and unrealized-PnL/equity.

- New pure primitives in `matching-engine`: `book_midprices`, `mark_yes_no`.
- `PriceTracker` records a mark series (coalescing flat ticks) and exposes `last_mark_prices`.
- Liquidity centers its ±band on the mark and **sums** its ring window.
- Portfolio/equity value open positions at the mark (unrealized only).

## Safety
Mark price is **serving-layer only** — never enters `Block.clearing_prices`, `state_root`, the witness, settlement, or realized PnL. All touched aggregates are off-block; equity/history rebuild on restart.

## Spec
`docs/superpowers/specs/2026-05-22-indicative-mark-price-backend-design.md`

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```
Expected: PR URL printed. **Do not merge.**

- [ ] **Step 3: Build the prod image locally (off-box) and deploy — preserving state**

> Build runs on the Mac (colima + Rosetta produces the linux/amd64 image); building on the Linode OOMs. `just deploy-api` builds `sybil-api`, ships it via `docker save | ssh docker load`, and restarts services. It does **NOT** touch volumes. **NEVER** run `just deploy-reset-state` — that wipes `sybil-data` (persisted engine state).

Confirm colima is up first:
```bash
colima status 2>&1 | tail -3   # if not running: colima start
```

Deploy:
```bash
just deploy-api 2>&1 | tail -40
```
Expected: image builds, loads on the server, `sybil-api` + accessories come up.

- [ ] **Step 4: Verify the deploy is healthy**

Run:
```bash
curl -s http://172.104.31.54:3000/v1/health
just deploy-logs sybil-api 2>&1 | tail -20
```
Expected: health OK; logs show block production with no panics. Spot-check a quiet market's `/v1/markets/{id}/prices/history` shows moving indicative points.

---

## Self-review notes
- **Spec coverage:** charts/deltas (Task 2), liquidity sum + mark center (Task 3), portfolio unrealized + equity (Task 4), pure midpoint primitive (Task 1), serving-layer-only invariant (Tasks 2–4 never write `Block`/witness), worktree + PR + off-box deploy + no-state-wipe (Pre-flight, Task 6). FE/schema work is explicitly out of scope per the spec.
- **Type consistency:** `record_block` returns `(HashMap<MarketId,u64>, HashMap<MarketId,Vec<Nanos>>)`; `record_finalized_block` mirrors it; `FinalizedBlockState.mark_prices` and `produce_block`'s destructure match. `book_midprices(impl IntoIterator<Item=&Order>) -> HashMap<MarketId,Nanos>` and `mark_yes_no(...) -> Vec<Nanos>` are used identically in `price_tracker.rs`. Liquidity keeps `avg_last_n` (still tested) and adds `sum_last_n`/`all_sum_last_n`; analytics `liquidity_avg10*` names retained (wire field unchanged) but now sum.
- **Known execution-time dependency:** Task 4 Step 3's test setup must be copied from the nearest existing `produce_block` test in `sequencer.rs` (harness-specific); Task 2 Step 5 assumes `MarketId::new` or falls back to `MarketId(0)`.
