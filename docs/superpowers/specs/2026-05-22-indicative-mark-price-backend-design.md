# Indicative / Mark Price — Backend Design

**Date:** 2026-05-22
**Scope:** Backend only (Rust crates). Frontend plumbing (API response fields, store,
cards, activity) is explicitly deferred to a later effort.
**Status:** Approved design, ready for implementation planning.

## Problem

A market only has a price when **trades match** in a batch. When no volume crosses for a
while, the price freezes at the last trade — even though resting limit orders keep moving
(quotes placed/pulled clearly show the price has shifted). Consequences today:

- **Charts** flatline between trades (`price_tracker.record_block` only appends a
  `PricePoint` for markets that had fills — `price_tracker.rs:229`).
- **Price deltas** are computed off that same trade-only series, so "change" looks stuck.
- **Liquidity** reads **zero** for any market with a healthy book but no recent cross
  (`liquidity_tracker` uses the clearing price as the band center and skips markets without
  one — `liquidity_tracker.rs:81`).
- **Portfolio** unrealized PnL / equity values open positions at the fills-only last
  clearing price (default $0.50), so "what's my position worth now" is stale
  (`equity_tracker.rs`, portfolio endpoint).

The matcher *already computes* a book-derived dual price for every market with orders on
every solve, but the block pipeline **deliberately discards it** for no-cross markets:
`merge_prices` only writes the solver price into `last_clearing_prices` for markets that had
fills (`price_tracker.rs:181-187`). The only place a fresh book-derived price escapes today
is the off-block C2 shadow-solve, surfaced per-market via `GET /v1/markets/{id}/open-batch`
(`actor.rs` `IndicativeSnapshot`). It is **not** recorded into any time series, **not** used
for liquidity, and **not** used for portfolio.

## Solution: one "mark price" per market, threaded through the serving layer

Introduce a single value — the **mark price** — computed per market per block by a simple
ladder:

1. **Volume matched this batch** → use the real uniform **clearing price** (unchanged).
2. **No volume matched** → **book midpoint** (the new part).
3. **Book too thin for a midpoint** → **carry over** the last mark.
4. **Cold start, never observed** → seed from persisted `last_clearing_prices`, else $0.50.

That one number feeds charts, deltas, liquidity, and portfolio unrealized PnL/equity.

### Hard invariant — serving layer only

The mark price **never** enters `Block.clearing_prices`, `state_root`, `events_root`, the
`BlockWitness`, matching, or settlement. It lives only in off-block serving aggregates
(which already do not enter consensus). This keeps the change off the consensus/critical
path and is the primary risk control. `last_clearing_prices` (trade-anchored) stays exactly
as-is; the mark is a **sibling** value.

## The book midpoint primitive

`book_midprice(market, &resting_orders) -> Option<Nanos>`: a pure function of the resting
**single-market** order book (multi-market/bundle orders excluded — their `limit_price` is a
bundle total, same restriction `liquidity_tracker` already applies at
`liquidity_tracker.rs:77`).

Reconstruct a synthetic YES book from each single-market order using `derive_order_direction`
(`order.rs:257`, exact for single-market binary orders) plus the NO↔YES complement:

- **YES bid** candidates: `BuyYes` @ `limit_price`, `SellNo` @ `NANOS_PER_DOLLAR - limit_price`
- **YES ask** candidates: `SellYes` @ `limit_price`, `BuyNo` @ `NANOS_PER_DOLLAR - limit_price`

Then:

- `best_bid = max(bid candidates)`, `best_ask = min(ask candidates)`
- If both sides present and `best_bid < best_ask` → **mid = (best_bid + best_ask) / 2**
- If one-sided or empty → `None` (caller carries over last mark)

**Definition chosen:** plain touch midpoint (no size floor). Simplest and most responsive; a
size floor is a trivial later addition if dust-quoting manipulation ever appears in practice.
Manipulation is bounded by batch-auction mechanics: a quote that moves the midpoint cannot be
cancelled within the batch window and risks being filled.

Computed at block finalization on the post-match (residual) book. For no-cross markets nothing
matched, so the residual equals the full book — timing is immaterial for the markets that use
the midpoint.

## Phased design

### Phase 0 — Pure primitives (full TDD, no wiring)
- `book_midprice(market, &resting_orders) -> Option<Nanos>`.
- `mark_price(volume, clearing, midpoint, last_mark, last_clearing) -> Nanos` ladder.
- Unit tests over hand-built books: two-sided cross, two-sided no-cross, one-sided,
  empty, BuyNo/SellNo complement correctness, multi-market exclusion.

### Phase 1 — Mark series + charts/deltas (#1, #2)
- At block finalization compute the mark for **all active markets** (today only filled
  markets are recorded). Inputs — per-market fill volume, clearing prices, resting book — are
  all available at that point.
- Add a sibling `last_mark_prices: HashMap<MarketId, Vec<Nanos>>`. Seeds from persisted
  `last_clearing_prices` on cold start; rebuilt each block ⇒ **no new persisted table**.
- `price_tracker.record_block` appends a mark `PricePoint` (with `volume_nanos = 0` tagging
  it indicative), **coalescing flat runs** (skip append when the mark equals the previous
  point) so idle markets don't drain the bounded (2000-pt) buffer.
- Deltas ride the same series automatically.

### Phase 2 — Liquidity (#3)
- Band center: clearing → **mark** (markets with a book but no cross now score > 0).
- Aggregation: `avg_last_n` → **sum** over the existing ring window (default ~10 blocks,
  kept as a tunable). All existing ±band machinery reused.
- Ensure the persisted `LiquidityTracker` snapshot stays backward-compatible on restart (the
  stored ring is per-block depth values; avg→sum is a read-time change, band center is
  recomputed each block — format should be unchanged, but verify cold-start/restore).

### Phase 3 — Portfolio (#5)
- `equity_tracker` and the portfolio endpoint's **unrealized** PnL value open positions at
  `last_mark_prices` (was fills-only last clearing / $0.50 default).
- **Midpoint fair-value** mark — one consistent number across charts, liquidity, and
  portfolio. It is an estimate of fair value, not liquidation proceeds (selling hits the bid
  and slips with size); standard broker behavior.
- **Realized PnL and settlement are untouched** — they remain driven by actual fills and
  resolution outcomes.

## Out of scope (deferred)
- All API response / schema changes (`MarketResponse`, `PricePoint`, portfolio
  `current_price_nanos`, an `is_indicative` flag).
- Frontend store tier, cards, activity, headline-price semantics.
- Extending the live open-batch shadow-solve to return the midpoint on no-cross.
- Liquidation-value (depth-walked) position marking.
- Size-floored / depth-weighted midpoint.

## Risks & mitigations
- **Manipulability of a no-volume price** — bounded by batch mechanics; mark is serving-only
  and never touches realized PnL, collateral, or settlement. Size floor available later.
- **Midpoint stability** — touch midpoint can step when the best order on a side changes;
  acceptable for a display/estimate mark; coalescing avoids buffer churn.
- **Persistence compatibility** — only `LiquidityTracker` is persisted among the touched
  aggregates; keep its snapshot format stable or tolerate a one-time rebuild (it is a
  non-canonical off-block aggregate that reconstructs from blocks).
- **Consensus safety** — guaranteed by the serving-layer-only invariant.

## Verification strategy
- `cargo test -p matching-engine` (or wherever the primitive lands) for Phase 0.
- `just test` (`cargo test --workspace`) green after each phase.
- `just check-all` (fmt-check + clippy + test) before PR.
- Manual: drive a market with resting orders but no cross, confirm the chart line moves,
  liquidity reads > 0, and portfolio unrealized updates — while realized PnL stays fixed.

## Delivery (final steps)
- Implement in a **git worktree** (branch off current HEAD) so the FE dev server on `r/dev`
  and its uncommitted changes are untouched.
- Interim commits per phase (git, not jj, despite AGENTS.md).
- Push branch + open **PR to `main` (do not merge)**.
- **Local container build + deploy:** `just deploy-api` (off-box build on Mac via
  colima+Rosetta; the prod Linode OOMs on build). **Never** `just deploy-reset-state` — it
  wipes `sybil-data` engine state. `deploy-api` preserves volumes; the touched in-memory
  caches (price history, mark map, equity) rebuild on restart.
