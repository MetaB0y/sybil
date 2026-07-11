# Backend implementation plan

> Source of truth for **what** to build: [`BACKEND_DATA_PLAN.md`](./BACKEND_DATA_PLAN.md). Its "Decisions" section (Q1-Q6) is committed; this plan does not relitigate any of them.
>
> This file is the source of truth for **how** to build it: 15 atomic, revertible steps grouped into five phases, each landed as one commit.
>
> **Revision history.** Three reviewer specialists (code reviewer, lead design architect, lead Rust dev) audited the first draft and flagged 14 findings (4 blockers, 5 major, 5 minor). All 14 are folded in here. Notable structural changes from the first draft: (i) old phase D moved before old phase C — `OrderCancelled` ships LAST so the riskier coordinated-deploy step doesn't gate the cost-basis surfaces; (ii) `OrderBook.cancel` joins the B5 return-type widening (4 methods total, not 3); (iii) a new `OrderDirection` enum gets added in matching-engine for the `OrderCancelled.side` field; (iv) D2 → C2's indicative scheduler is a separate `IndicativeTick` timer task, not an idle-tick branch.

---

## Execution status (2026-05-14)

Updated as steps land. Use this section to pick up after a context reset.

**Branch:** `r/dev` · git (not jj, by user choice during execution).

**Landed:**

| # | ID | Commit | Subject |
|---|----|--------|---------|
| 1 | A1 | `470029c` | aggregates: scaffold module + BlockMarketStats wire type |
| 2 | A2 | `3722745` | restart-caveat-badge: FE component for "since last restart" disclosure |
| 3 | B1 | `591afff` | traders: TraderTracker + open-batch + activity-overview endpoints |
| 4 | B2 | `1dfead3` | volume: 24h windows + per-block per-market split via PriceTracker |
| 5 | B3 | `eaedd1e` | price-24h: server-computed 24h-ago snapshot per market |
| 6 | B4 | `f270890` | liquidity: LiquidityTracker (last-10 ±band) + SequencerConfig.liquidity_band_nanos |
| 7 | B5 | `055ae04` | order-book: RestingOrder annotations + widen expire/revalidate/settle/cancel returns |
| 8 | B6 | `5cae0b7` | order-stats: OrderStatsTracker (placed/matched/unmatched) per-market + 24h |
| 9 | B7 | `e1e21ef` | welfare: per-market accumulator in solve_batch_phase |
| 10 | B8 | `2d2f01a` | portfolio: first_deposit_ms + total_fill_count + original_quantity wire |
| 11 | C1 | `e88f0b1` | cost-basis: WAC tracker + apply_fill/apply_resolution hooks |
| 12 | C2 | `df7e9d9` | indicative: separate IndicativeTick timer + spawn_blocking shadow-solve |
| 13 | BFE+CFE (schema) | `66ff50e` | schema: regenerate openapi types against Phase B+C sybil-api |
| 14 | D1 | `6f8ac60` | order-cancelled: SystemEvent + OrderDirection enum + verifier variant |

**Post-D1 status (2026-05-15):** D1 lands the on-chain `OrderCancelled` system event as the LAST point-of-no-return. Forward-additive serde encoding means historical `events_root` stays valid; new blocks need the variant on both sequencer and verifier. Coordinated deploy (rsync + docker build on prod) is deferred to 2026-05-16 per the user's "deploy tomorrow" instruction — D1 today is pure code work on `r/dev` against a green-green workspace. Next steps: E1 (Sybil console Aggregates tab — Alpine.js HTML only, no Rust) and E2 (integration smoke + STATUS/OPEN_QUESTIONS).

**C1 entry notes (when proceeding):**

- Scope: ~420 Rust LOC. New `aggregates/cost_basis_tracker.rs` + persistence + hook in `FillRecorder.record_fills` + hook in BOTH `Sequencer::resolve_market` and `Sequencer::resolve_market_attested`.
- Why point of no return: the apply_fill hook lives INSIDE `FillRecorder.record_fills` — reverting C1 alone is fine TODAY, but once any reader of `realized_pnl` / `cost_basis` lands the FillRecorder lifecycle and CostBasisTracker lifecycle must stay coupled. The persistence story (own redb table, sibling to FILL_HISTORY) makes the snapshot/restore footprint coordinated.
- `CostBasisTracker { basis: HashMap<(AccountId, MarketId, u8), i64>, realized: HashMap<AccountId, i64> }`. MINT early-return at the top of `apply_fill`. `apply_resolution(market, payout_nanos, affected_accounts)`. Methods: `cost_basis(account, market, outcome) -> i64`, `realized_pnl(account) -> i64`.
- Wire fields:
  - `PositionValueResponse`: `avg_entry_price_nanos: u64` (`#[serde(default)]`)
  - `PortfolioResponse`: `realized_pnl_nanos: i64`, `unrealized_pnl_nanos: i64` (`#[serde(default)]` each)
  - PnL split: `unrealized = (current_price - avg_entry) * quantity`, `realized` summed across the account's realized HashMap.
- Persistence: own redb table `COST_BASIS_TRACKER`. Single rmp_serde blob keyed `"snapshot"`. Missing-row → `Default::default()`. Same pattern as the four B-phase trackers.
- Hooks (verify line refs with `grep -n` before editing — drift is real):
  - `FillRecorder.record_fills` (`crates/matching-sequencer/src/fill_recorder.rs:66+`): right after the `position_deltas` is computed and the record is pushed, call `cost_basis_tracker.apply_fill(account_id, &position_deltas, fill.fill_price)`. **MINT early-return inside apply_fill**, not at caller.
  - `Sequencer::resolve_market` (find via `grep -n 'fn resolve_market\b' sequencer.rs`): after `settlement::resolve_market` applies payouts.
  - `Sequencer::resolve_market_attested` (same file): same hook, same args (just a different entrypoint).
- The CostBasisTracker is owned by `BlockSequencer` directly (NOT inside FillRecorder), but its `apply_fill` is invoked from inside `FillRecorder.record_fills` via a `&mut` reference. The cleanest pattern: pass `&mut CostBasisTracker` into `record_fills` as a parameter. Avoid making CostBasisTracker a field of FillRecorder — keeps the rollback footprint cleaner.
- 5 new inline tracker tests (per plan): `apply_fill_basic`, `apply_fill_excludes_mint`, `apply_resolution_realizes`, `cost_basis_snapshot_roundtrip`, `realized_pnl_after_resolution`.
- After C1 lands and before C2: end-of-Phase-C status ping is NOT required (single-step phase between gates, just continue to C2). End-of-Phase-C check-in happens after C2.

**B5 entry notes (when proceeding):**

- Scope: ~270 LOC. Pure mechanical refactor. No wire/FE change.
- Files to touch: `crates/matching-sequencer/src/order_book.rs` + `crates/matching-sequencer/src/sequencer.rs` (+ inline order_book tests).
- `RestingOrder` (order_book.rs:36) gets two `#[serde(default)]` fields: `has_been_matched: bool`, `original_max_fill: u64`. Pattern mirrors the existing `expires_at_block` field's `#[serde(default = "default_resting_expires_at_block")]`.
- `OrderBook.accept` (order_book.rs:196) populates `original_max_fill = order.max_fill` once at construction time; never mutated thereafter.
- `OrderBook.settle` (order_book.rs:396): when an order is filled (filled > 0), set `has_been_matched = true` on the resting order before it gets returned in the removed-orders Vec.
- Widen 4 method return types:
  - `expire`, `revalidate`, `settle`: `()` → `Vec<RestingOrder>` (orders removed).
  - `cancel`: `Result<(), CancelError>` → `Result<RestingOrder, CancelError>` (the cancelled order).
- 5 production call sites in sequencer.rs (verify line numbers against current code before editing — the plan's line refs may have drifted):
  - `cancel_pending_order` (around `self.order_book.cancel(account_id, order_id)?`) — bind result to a variable (B5: unused; D1 reads it).
  - Pre-solve `expire` + `revalidate` calls in `produce_block_in_place` — bind to `_expired` / `_revalidated`.
  - STP-undo phantom-fill `settle` call — bind to `_stp_undo`.
  - Post-solve `settle` (the existing one after `finalize_block_state_phase`) — bind to `_post_solve` (or rename `liquidity_tracker.record_block` flow if needed).
- 7 test sites in order_book.rs need binding updates (mechanical).
- New inline tests to add: `expire_returns_removed_orders`, `settle_marks_matched`, `cancel_returns_order`, `resting_order_serde_default`.
- After B5 commit lands, B6 + D1 can start consuming `has_been_matched` (B6) and `cancel`'s returned RestingOrder (D1). Reverting B5 alone is fine TODAY; once B6 lands, B5+B6 must revert together; once D1 lands, all three must revert together.
- Run `cargo test -p matching-sequencer order_book::tests` + the new tests; `cargo clippy -p matching-sequencer --no-deps -- -D warnings`.

**Plan amendments agreed during execution:**

1. **VCS:** git per step on `r/dev` (no jj backing in the repo; user opted for plain git over `jj git init`).
2. **Workspace fmt drift:** three files unrelated to the plan (`crates/sybil-api/src/routes/markets.rs`, `crates/sybil-polymarket/src/polymarket/types.rs`, `crates/sybil-polymarket/src/sync.rs`) fail `cargo fmt --check` at baseline. Each step runs targeted `cargo test` + `cargo clippy` on its touched crates instead of `just check-all`. Workspace fmt drift to be cleaned up in a separate session.
3. **FE wiring batched.** Each backend step (B1–B8, C1, C2) lands backend + endpoints + persistence + wire types only. FE consumption + `pnpm types:generate` happens once at the end of Phase B (and once at end of Phase C). Adds ~1–2 "BFE/CFE" steps at the appropriate phase boundaries. Manual smoke via `curl` covers each backend step until then.
4. **FE test infra:** vitest runs under node env (no jsdom / @testing-library/react installed). React component tests use `renderToStaticMarkup` from `react-dom/server` and assert against the HTML string.
5. **Schema regen deferred.** `frontend/web/src/lib/api/schema.d.ts` will be regenerated once at end of Phase B against a running `--dev-mode` API.

**Build state:** `cargo build -p sybil-api --release` completed (binary at `target/release/sybil-api`).

**B2 entry notes (pick up here):**

- `crates/matching-sequencer/src/price_tracker.rs` is the existing `PriceTracker` with `record_block` already computing a transient `per_market_volume: HashMap<MarketId, u64>` (currently discarded after updating cumulative).
- Plan: extend `PriceTracker` with three new fields:
  - `platform_volume: u64` (running total of all fill_price × fill_qty, not sum of per_market values — multi-market fills must NOT over-count)
  - `hourly_per_market: VecDeque<(u64 hour_start_ms, HashMap<MarketId, u64>)>` cap 25
  - `hourly_platform: VecDeque<(u64, u64)>` cap 25
- `record_block` extends to: bump `platform_volume` from `Σ fill_price * fill_qty` over fills (NOT sum of per_market_volume — that over-counts multi-market orders), route the existing per-market split into the current `hourly_per_market` bucket, bump `hourly_platform` current bucket.
- For `Block.volume_by_market: HashMap<MarketId, u64>`: cleanest path is to have `record_block` return the per-market split it already computes, and `finalize_block_state_phase` plumbs it into Block. Or compute per_market_volume separately in `solve_batch_phase` (which has `order_map`) and pass to both `record_block` and Block construction.
- New methods on `PriceTracker`: `market_volume_24h(m, now_ms)`, `platform_volume_24h(now_ms)`, `platform_volume_total()`.
- Persistence: combined blob `PriceTrackerVolumeSnapshot { platform_volume, hourly_per_market, hourly_platform }` into a new `PRICE_TRACKER_VOLUME_EXTENSIONS` redb table — single-row keyed "snapshot" — same pattern as `TRADER_TRACKER` from B1. Plumb through `SequencerSnapshot` + `RestoredState`. Extend `PriceTracker::with_state` to take the snapshot too (or add a separate `restore_volume_extensions` builder).
- Wire fields:
  - `MarketResponse.volume_24h_nanos: u64` + `MarketSummaryResponse.volume_24h_nanos` (`#[serde(default)]`)
  - `BlockMarketStats.volume_nanos: u64` (`#[serde(default)]` — second field on the struct)
  - `ActivityOverviewResponse.{all_time,last_24h}.total_volume_nanos` already exists as `OverviewBucketResponse.total_volume_nanos`; just populate.
- Route handler change: `routes/aggregates.rs::get_activity_overview` needs to fetch platform volumes. Add a SequencerMsg variant: `GetPlatformVolumes(now_ms) -> (u64 all_time, u64 last_24h)`.
- `routes/markets.rs` plumbing: extend `tokio::try_join!` in `list_markets`, `list_markets_summary`, `get_market`, `search_markets` to also fetch a per-market 24h volume map. Add `GetAllMarketVolumes24h(now_ms) -> HashMap<MarketId, u64>` SequencerMsg variant.
- Add `volume_24h_nanos: u64` to `BuildMarketResponseArgs` in markets.rs; populate in build_market_response.
- Inline tests in `price_tracker.rs`: bucket roll on hour boundary, cap-25 drop oldest, 24h window arithmetic, multi-market platform total correctness.
- See B2's section in this plan for the full spec.

**Reference points already verified against current code:**

- BlockSequencer struct: `crates/matching-sequencer/src/sequencer.rs:515`
- `try_admit_direct`: :1058 (single non-MM single-market admits only)
- Admission loop in `produce_block_in_place`: ~:1772+ with both MM and non-MM Ok branches
- Witness-orders capture site for per-block placers: my B1 edit added the capture block right before "Build Problem" in `produce_block_in_place`
- `solve_batch_phase`: :1487 (currently doesn't yield per_market_volume — record_block computes it transiently)
- `finalize_block_state_phase` calls `price_tracker.record_block` at :1570
- Block construction: ~:2118 (now includes `unique_placers` and `placers_by_market` from B1)
- `RestoredState`: `crates/matching-sequencer/src/store.rs:200`
- `SequencerSnapshot`: ~:240 (now includes `trader_tracker: TraderTrackerSnapshot` from B1)
- Existing redb tables: :72-160 + `TRADER_TRACKER` added by B1 at the bottom of that block.
- `BlockMarketStats` currently has one field: `placers: u32` (B1). B2 appends `volume_nanos: u64` with `#[serde(default)]`.
- `BuildMarketResponseArgs` in `routes/markets.rs` already takes `trader_count: u32` (B1).
- New `SequencerHandle` methods added in B1: `get_all_trader_counts`, `get_platform_trader_counts`, `get_event_trader_count`, `get_open_batch_placers`.

**Three points of no return ahead (need user approval before starting):**

- B5 (4 OrderBook return-type widenings)
- C1 (CostBasisTracker persistence co-located with FillRecorder lifecycle)
- D1 (coordinated sequencer+verifier deploy for OrderCancelled)

---

## How this plan is structured

Each step is a self-contained section an AI agent can execute without rereading the source plan. Steps are ordered so that each one builds on a green main: foundations → off-block trackers → cost basis + indicative → the single on-chain `OrderCancelled` change → console tab + signoff.

Reverting any step (or contiguous range from the head) leaves a working build. **Three steps are flagged as "point of no return"** for their subsystem:

- **B5** widens `OrderBook.expire / revalidate / settle / cancel` return types from `()` / `Result<(), _>` to `Vec<RestingOrder>` / `Result<RestingOrder, _>`. Later steps (B6 and D1) depend on this signal.
- **C1** (CostBasisTracker) persists alongside `FillRecorder`-shared state; once a snapshot has been written, reverting requires either a snapshot wipe or carrying a dead-field decoder on the `FillRecorder` snapshot for one cycle. The persistence layout puts CostBasisTracker in its own redb table (sibling, not co-mingled with `FillRecorder`'s existing tables) to keep this revert clean — see C1's "Rollback" note.
- **D1** lands the on-chain `OrderCancelled` event and requires a coordinated sequencer + verifier deploy.

Off-block sidecars are individually revertible at any time — reverting one leaves the others intact.

---

## Conventions

### Version control

This project uses **jj (Jujutsu)**, not git. Each step is one `jj` change with a `jj describe` message.

- Start a new change: `jj new -m "<subject>"`
- Refine the change: edit, then `jj describe`
- Show diff: `jj diff --git`
- Squash absorbed work back into a step: `jj squash`

`AGENTS.md` at the repo root has the project-wide jj conventions.

### Test commands

- Workspace: `just test` (= `cargo test --workspace`)
- Single crate: `cargo test -p matching-sequencer` / `cargo test -p sybil-api` / `cargo test -p sybil-verifier`
- Single test: `cargo test -p <crate> <test_name>`
- Clippy: `just lint`
- Frontend unit tests: `cd frontend/web && pnpm test` (vitest; existing `vitest.config.ts` is on the branch)
- Frontend type check: `cd frontend/web && pnpm tsc --noEmit`

A step is "done" when:
1. Its listed cargo + pnpm commands pass.
2. `just check-all` (= fmt + clippy + workspace tests) is green.
3. The acceptance criteria stated in the step are satisfied.

### Test module convention

Per the existing convention in this workspace (e.g. `crates/matching-sequencer/src/fill_recorder.rs` has inline `#[cfg(test)] mod tests { ... }`), new tracker tests live **inline at the bottom of the tracker file**, not in separate `*_tests.rs` files. Test names use `module_path::test_name` form so they remain `cargo test -p <crate> module_path::test_name`-filterable.

### Wire schema regeneration

Every step that changes the API response shape requires regenerating the frontend's typed schema:

```
cd frontend/web && pnpm types:generate
```

This hits the running API (default `http://localhost:3001` for dev, `https://172-104-31-54.nip.io` for live) and rewrites `frontend/web/src/lib/api/schema.d.ts`. The `scripts/patch-bigints.mjs` postprocess rewrites `*_nanos: number` → `string` automatically (per `frontend/CLAUDE.md`). For local-only iteration: start the API with `cargo run --release -p sybil-api -- --dev-mode --port 3001` first.

Each wire-touching step lists `pnpm types:generate` as a required test step.

### Commit message style

Match the recent r/dev commit log: `<scope>: <imperative summary>`. Examples from `git log`:

- `mock-value: add pill + tint variants; mark every mock visibly`
- `activity: lift /activity-dev to /activity, drop prototype`

Each step below specifies its subject line. The body should state the rationale (one short paragraph) and list the touched files.

### Persistence

The redb layout version (`crates/matching-sequencer/src/store.rs:160` — `STORE_LAYOUT_VERSION = 1`) **stays at 1** for this entire iteration. The forward-compatibility story is two-part:

1. **`SequencerSnapshot` and `RestoredState` are NOT serde-encoded** — each field is mapped to its own redb `TableDefinition` (the file already has 14 such tables; see `store.rs:72-148`). Each new tracker adds a new table. On `load_state`, missing tables yield `Default::default()` for the tracker (cold start until activity accumulates). Tracker steps do NOT add `#[serde(default)]` to `SequencerSnapshot`/`RestoredState` fields; that would be a category error.

2. **`RestingOrder` (which IS serde-encoded into a single redb blob via `resting_orders` table) uses `#[serde(default)]` for new fields** — that's how the existing `expires_at_block` field handles old-snapshot rounding (`order_book.rs:42`). B5's new `has_been_matched` and `original_max_fill` follow the same pattern.

So **per-tracker persistence plumbing has 4 sites**: a `SequencerSnapshot` field, a `RestoredState` field, a `TableDefinition` + write path in `save_block_inner` (`store.rs:304`), and a read path in `load_state` (`store.rs:592` / `RestoredState` assembly at `:800`). Layout version bumps are deferred to a future iteration that introduces a non-additive change.

### Frontend deploy

The branch is `r/dev`. Backend changes that originate on `r/dev` should be cherry-picked or rebased onto a short branch off `main`, pushed to `main`, deployed (rsync to prod box + docker build there per `frontend/CLAUDE.md`), then `r/dev` absorbs the new schema via `git pull origin main` + `pnpm types:generate`. **Plan-wide convention:** the AI agent executes the backend work on a feature branch off `main`, lands all 15 commits, then `r/dev` integrates.

---

## Invariants (restated from `BACKEND_DATA_PLAN.md`)

The agent must preserve these without exception. If a step appears to violate one, stop and surface the conflict.

1. **Off-block sidecars only**, except `OrderCancelled` (step D1).
2. **Core engine untouched.** No edits to: `crates/matching-solver/`, settlement math (`compute_fill_settlement`, `settle_batch`, minting, payouts in `crates/matching-sequencer/src/settlement.rs`), MM budget/quoting logic (`crates/sybil-polymarket/src/mm.rs`), admission validation, or witness/verification structure beyond the `OrderCancelled` variant additions.
3. **`OrderCancelled.side` uses a new `OrderDirection` enum** `{ BuyYes, SellYes, BuyNo, SellNo }` (declared in `crates/matching-engine/src/types.rs` next to the existing `Side { Bid, Ask }` enum — they're distinct concepts; `Side` is order-book bid/ask, `OrderDirection` is user-facing outcome×buy/sell). NOT `String`, NOT existing `Side`. Derivation function lives next to the enum; takes `&Order` + primary market, returns `OrderDirection`. For multi-market cancels (rare), the primary market is `order.payoffs.iter().next()`; documented edge case in D1.
4. **`OrderCancelled` propagates to 5 sites** (sequencer + verifier in lockstep, single D1 commit):
   - `SystemEvent::OrderCancelled` variant in `crates/matching-sequencer/src/system_event.rs`
   - `SystemEventWitness::OrderCancelled` variant in `crates/sybil-verifier/src/event_schema.rs` (~:109)
   - `system_event_leaf_value` arm (`crates/sybil-verifier/src/event_schema.rs:24`) at tag byte 5 (0-4 are taken; ordering matters for `events_root`)
   - `digest::encode_order_cancelled_event` in `crates/matching-sequencer/src/digest.rs` (mirrors the existing `encode_*_event` family at `:10-100`). Note: `encode_mint_event` (`digest.rs:84`) is NOT a SystemEvent — it folds per-account mint/burn into `events_digest`; leave it untouched.
   - `convert_system_event` arm in `crates/matching-sequencer/src/sequencer.rs:355` AND the per-account `events_digest` 6th arm at `sequencer.rs:1641-1708`
   - `Sequencer::cancel_pending_order` (`sequencer.rs:1192`) stages the event into `pending_system_events` using the `RestingOrder` returned by the widened `OrderBook.cancel` (B5)
   - `SystemEventResponse::OrderCancelled` API convert in `crates/sybil-api/src/convert.rs`
5. **`RestingOrder` annotation fields ship combined (B5)** with `#[serde(default)]`: `has_been_matched: bool`, `original_max_fill: u64`. Same struct, same migration, same review cycle.
6. **`OrderBook.expire / revalidate / settle / cancel` return-type widening (B5)** — FOUR methods, not three:
   - `expire` `()` → `Vec<RestingOrder>` (expired orders)
   - `revalidate` `()` → `Vec<RestingOrder>` (evicted orders)
   - `settle` `()` → `Vec<RestingOrder>` (removed-this-batch orders)
   - `cancel` `Result<(), CancelError>` → `Result<RestingOrder, CancelError>` (the cancelled order)
   Touches **5 production sites**: `sequencer.rs:1192-1197` (cancel caller), `:1749` (expire), `:1750` (revalidate), `:1873` (STP-undo settle), `:2007-2008` (post-solve settle — the method call begins `:2008` after a leading-newline chain) + **7 test sites in `order_book.rs`** around `:670, :694, :720, :744, :761, :764, :810`.
7. **`CostBasisTracker.apply_fill` is called INSIDE `FillRecorder.record_fills`** (`fill_recorder.rs:76`, shares the `position_deltas` walk from `compute_fill_settlement`). MINT early-return guard at the top of `apply_fill`. **Persistence**: CostBasisTracker has its own redb `TableDefinition` (sibling to FillRecorder's tables), NOT co-mingled with `FillRecorder.fills_by_account`. The struct field on `FillRecorder` exists for sharing the walk; the snapshot story is independent.
8. **`CostBasisTracker.apply_resolution`** hooks from inside `Sequencer::resolve_market` (`sequencer.rs:1212`), immediately AFTER `settlement::resolve_market` (`settlement.rs:80`) applies payouts to `account.balance`, and BEFORE the `SystemEvent::MarketResolved` is staged into `pending_system_events`. Resolution bypasses the Fill stream; `settlement::resolve_market` is extended to return the set of affected accounts so the tracker can iterate them. **NOT in `convert_system_event`** — that runs at block-emission time, after payouts have already been applied and would double-fire on the same resolution.
9. **Indicative scheduler** = a **separate `IndicativeTick` timer task** registered at actor `post_start` (`actor.rs:947`, mirroring the existing block-ticker pattern at `:956`). The handler is `SequencerMsg::IndicativeTick`, which clones `myself: ActorRef<SequencerMsg>`, builds a speculative `Problem` from a snapshot of the resting book, and dispatches `tokio::task::spawn_blocking` that sends an `IndicativeUpdate` self-message on completion. No shared lock, no `AtomicBool`. `indicative_cache: HashMap<MarketId, IndicativeSnapshot>` lives on `SequencerActorState`, not `BlockSequencer`. **`Problem: Clone` is already true** (`matching-engine/src/problem.rs:51`).
10. **Trade count lives on `FillRecorder`** (decision Q3). Add `total_count: HashMap<AccountId, u64>` to `FillRecorder`, bump in `record_fills`.
11. **`BlockResponse.by_market: HashMap<String, BlockMarketStats>`** — one nested map (decision Q1), not six parallel maps. `BlockMarketStats` is added in A1 with zero fields and grows append-only (each new field gets `#[serde(default)]` so partial reverts stay clean).
12. **Trackers co-located** under `crates/matching-sequencer/src/aggregates/` (decision Q2). No shared `HourlyBuckets<T>` helper yet.
13. **Per-tracker persistence plumbing is mandatory** (4 sites per tracker, see "Persistence" above). Every tracker step lands its persistence plumbing.
14. **UI "all-time" labels** gate on production persistence; a single `<RestartCaveatBadge />` FE component covers caveats until then (decision Q5). Every FE surface that consumes an "all-time" tracker field renders the badge inline. A2 lands the component; B1/B2/B6/B8 are its consumers (the badge does NOT appear on B3 24h delta, B4 liquidity, B7 per-block welfare — all are window-bounded or per-block).
15. **Multi-market attribution.** Per-market counters credit each active market; platform totals are independent (not a sum). Documented in API docs on the relevant endpoint.

---

## Summary

| # | ID | Subject | Phase | Est. LOC (Rust + TS) | Prereqs | Revertibility |
|---|----|---------|-------|----------------------|---------|---------------|
| 1 | A1 | Scaffold `aggregates/` module + `BlockMarketStats` wire type | A | 80 + 20 | — | Clean revert; no producers yet |
| 2 | A2 | Add `<RestartCaveatBadge />` FE component | A | 0 + 80 | — | Clean revert; pure FE |
| 3 | B1 | `TraderTracker` + `/v1/markets/{id}/open-batch` endpoint (indicative stubbed) | B | 350 + 80 | A1, A2 | Clean; tracker disappears, endpoint removed |
| 4 | B2 | `PriceTracker` volume extensions (24h + platform) + `/v1/activity/overview` | B | 360 + 60 | A1, A2, B1 | Clean; wire fields default to 0 |
| 5 | B3 | `PriceTracker` price-24h-ago extension | B | 220 + 50 | A1, B2 | Clean; FE falls back to `useCardHistory` |
| 6 | B4 | `LiquidityTracker` + `SequencerConfig.liquidity_band_nanos` | B | 320 + 40 | A1, A2 | Clean; band config defaults back; see B4 revert protocol |
| 7 | B5 | `RestingOrder` annotations + `OrderBook` 4-method return-type widening | B | 270 + 0 | — | **Point of no return** for OrderBook signatures; downstream B6 and D1 depend on it |
| 8 | B6 | `OrderStatsTracker` consumes B5's signal | B | 380 + 60 | A1, A2, B5 | Clean; annotations remain harmlessly |
| 9 | B7 | Per-market welfare in `solve_batch_phase` → `by_market[m].welfare_nanos` | B | 80 + 40 | A1 | Clean; field defaults to `{}` |
| 10 | B8 | Small additions: `first_deposit_ms` (#13) + `total_fill_count` (#14) + `original_quantity` wire (#16) | B | 150 + 50 | A1, A2, B5 | Clean; three small wire fields default-zero |
| 11 | C1 | `CostBasisTracker` (`apply_fill` inside `FillRecorder.record_fills` + `apply_resolution` in `Sequencer::resolve_market`) | C | 420 + 60 | A1, A2 | **Point of no return** (persistence co-mingles with FillRecorder lifecycle); see C1 Rollback |
| 12 | C2 | Indicative scheduler (separate `IndicativeTick` timer + `spawn_blocking`) lights up open-batch fields | C | 300 + 40 | B1 | Clean; fields go back to `None` |
| 13 | D1 | `OrderCancelled` SystemEvent (sequencer + verifier in lockstep) | D | 360 + 80 | B5 | **Coordinated revert**: sequencer + verifier together |
| 14 | E1 | Sybil console new "Aggregates" tab (Alpine.js) | E | 0 + 600 (HTML) | B1, B2, B3, B4, B6, B7, B8, C1, C2, D1 | Pure HTML revert |
| 15 | E2 | Integration smoke + `STATUS.md` / `OPEN_QUESTIONS.md` updates | E | 80 + 0 | All prior | Pure docs/script revert |

**Totals:** ~3,370 Rust LOC + ~1,260 TS/HTML LOC (~800 TS, ~600 HTML, ~80 docs).

---

## Phase A — Foundations

### Step A1: Scaffold `aggregates/` module and `BlockMarketStats` wire type

**Phase:** A
**Prereqs:** none
**Est. LOC:** ~80 Rust + ~20 TS

**Goal.** Land the empty `aggregates/` module that every tracker step will populate, plus the `BlockMarketStats` struct that becomes `BlockResponse.by_market` — wire-additive, no producer yet.

**Files.**

New:
- `crates/matching-sequencer/src/aggregates/mod.rs` — `// trackers live here; see BACKEND_IMPLEMENTATION_PLAN.md`. Empty module body (no exports until B1).

Modified:
- `crates/matching-sequencer/src/lib.rs` — add `pub mod aggregates;` declaration alongside the existing `pub mod price_tracker;` / `pub mod fill_recorder;` / etc.
- `crates/sybil-api-types/src/response.rs` — add `BlockMarketStats` struct (empty body — fields land in subsequent steps); add `pub by_market: HashMap<String, BlockMarketStats>` field with `#[serde(default, skip_serializing_if = "HashMap::is_empty")]` to `BlockResponse` (around line 187).
- `frontend/web/src/lib/api/schema.d.ts` — regenerated by `pnpm types:generate`.

**Changes.**

- The new `aggregates/mod.rs` is intentionally empty (one comment line). It establishes the import path so every tracker step that follows lands strictly under `crates/matching-sequencer/src/aggregates/`.
- `BlockMarketStats` is declared with `#[derive(Default, Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]` and zero fields yet. Subsequent steps (B2, B6, B7) add fields one at a time, each with `#[serde(default)]` so the struct stays forward-additive.
- `BlockResponse.by_market` defaults to an empty map; existing API consumers see no change.
- `convert.rs` (engine → API) writes an empty map for `by_market` until producers wire in.

**Tests.**

- `cargo test -p sybil-api-types` — existing tests pass.
- `cargo test -p sybil-api-types response::block_response_serde_roundtrip` (NEW inline test in `crates/sybil-api-types/src/response.rs`) — round-trip `BlockResponse` with `by_market: HashMap::new()` vs old shape, assert serde compatible.
- `cd frontend/web && pnpm types:generate` against a locally-running dev server; verify `schema.d.ts` now contains `BlockMarketStats` and `by_market?: Record<string, BlockMarketStats>` on `BlockResponse`.
- `cd frontend/web && pnpm tsc --noEmit` — no type errors.

**Acceptance criteria.**

- `cargo build --workspace` passes with the new module declaration.
- `BlockResponse` JSON serializes the same as before when `by_market` is empty.
- Generated FE schema has `BlockMarketStats` declared.

**Commit.**

Subject: `aggregates: scaffold module + BlockMarketStats wire type`

Body:
```
Establishes crates/matching-sequencer/src/aggregates/ as the home for off-block
trackers per BACKEND_DATA_PLAN.md decision Q2. Lands BlockMarketStats as the
nested per-market sidecar on BlockResponse (decision Q1); fields are added by
each subsequent tracker step.

Touches:
  crates/matching-sequencer/src/aggregates/mod.rs (new)
  crates/matching-sequencer/src/lib.rs
  crates/sybil-api-types/src/response.rs
  frontend/web/src/lib/api/schema.d.ts (regenerated)
```

**Rollback.** Pure revert. No persisted state. Old `BlockResponse` JSON is byte-identical because `by_market` is `skip_serializing_if = "HashMap::is_empty"`.

**Non-goals.** No tracker types added. No producer hooks. No FE rendering of `by_market` (FE consumers start in B1+).

---

### Step A2: Add `<RestartCaveatBadge />` FE component

**Phase:** A
**Prereqs:** none
**Est. LOC:** ~80 TS

**Goal.** Land the single FE component used to flag "all-time" fields as caveat-bearing until production persistence is on (decision Q5 part 2). No surfaces consume it yet — first consumer lands in B1.

**Files.**

New:
- `frontend/web/src/components/restart-caveat-badge.tsx` — small inline component, accepts no props (or one optional `hint` prop), renders a tiny pill `"since last restart"` with tooltip pointing to the persistence story.
- Tests: inline `vitest` describe block at the bottom of the same `.tsx` file (or a `.test.tsx` sibling if vitest config prefers — agent matches existing convention used by `mock-value.test.tsx` if present).

Modified:
- `frontend/web/src/styles/sybil-tokens.css` — only if a new caveat-specific token is needed; otherwise re-use existing `--fg-3` / `--accent` tokens.

**Changes.**

- Component matches the visual register of `<MockValue variant="pill">` (defined in `frontend/web/src/components/mock-value.tsx`) but with a distinct color and text. The intent is **honesty about a known limitation**, not "this number is fake."
- Component is pure (no hooks); renderable inline next to a numeric value or as a header chip.
- A README-style block at the top of the file explains: "Render this inline next to any 'all-time' figure backed by an in-memory tracker. Drop once `SYBIL_DATA_DIR` is populated in prod."
- **Eventual consumer list** (documented at top of the component for traceability — agent updates as it lands the consumers):
  - B1 — `binary-card.tsx` trader count, `multi-card.tsx` trader count, `activity/page.tsx` all-time unique traders
  - B2 — `activity/page.tsx` all-time total volume
  - B6 — `activity/page.tsx` all-time orders placed/matched/unmatched, `m/[id]/page.tsx` orders totals
  - B8 — `portfolio/portfolio-hero.tsx` first deposit / total fill count
  - C1 — `portfolio/portfolio-hero.tsx` realized PnL (cumulative, restart-sensitive)
  - NOT used on: B3 (24h is window-bounded), B4 (liquidity is current state), B7 (per-block), C2 (real-time), D1 (event-based)

**Tests.**

- `cd frontend/web && pnpm test restart-caveat-badge` — snapshot matches; props variant renders both modes.
- `cd frontend/web && pnpm tsc --noEmit` — clean.

**Acceptance criteria.**

- Importable from `@/components/restart-caveat-badge` with no runtime cost.
- Visual smoke: render once in `/smoke` page (optional, agent's call) to eyeball the design.

**Commit.**

Subject: `restart-caveat-badge: FE component for "since last restart" disclosure`

Body:
```
Single FE component used inline next to any all-time figure backed by an
in-memory tracker. Per BACKEND_DATA_PLAN.md decision Q5: production
persistence is not yet on; until it is, every aggregate is "since last
restart" — one badge across all surfaces, not 12 per-field disclaimers.

Eventual consumer list (B1, B2, B6, B8, C1) documented at the top of the
component file.

Touches:
  frontend/web/src/components/restart-caveat-badge.tsx (new)
```

**Rollback.** Pure FE revert; no consumers yet.

**Non-goals.** No surface wires the badge in this step. B1 is the first consumer.

---

## Phase B — Off-block trackers

Each tracker step lands a complete vertical slice: tracker type + persistence plumbing (4 sites) + producer hook(s) + wire field(s) on response types + FE consumer wired up + tests. After each B-step the relevant FE surface stops rendering `<MockValue>` and starts rendering real data with `<RestartCaveatBadge />` where applicable.

### Step B1: `TraderTracker` + `/v1/markets/{id}/open-batch` endpoint

**Phase:** B
**Prereqs:** A1, A2
**Est. LOC:** ~350 Rust + ~80 TS

**Goal.** Lights up 5 trader surfaces (a/c/d/e/f from the Traders entry). Surface (b) per-event count comes via `/v1/events/{event_id}/traders` (added in this step too — small endpoint, fits here).

**Files.**

New:
- `crates/matching-sequencer/src/aggregates/trader_tracker.rs` — `TraderTracker` struct with `per_market: HashMap<MarketId, HashSet<AccountId>>`, `platform: HashSet<AccountId>`, `hourly_buckets: VecDeque<(u64, HashSet<AccountId>)>` (cap 25). Methods: `record_placed(account, market, ts_ms)`, `per_market_count(m)`, `platform_count()`, `platform_24h_count(now_ms)`, `event_count(market_ids)`. **Tests live inline** at the bottom: `#[cfg(test)] mod tests { ... }` (admit twice → counts as 1; bucket roll on hour boundary; event union; MM/MINT skipped).
- `crates/sybil-api/src/routes/aggregates.rs` — new routes file housing `/v1/activity/overview` (just `unique_traders` populated in this step; volume + orders join in B2/B6), `/v1/markets/{id}/open-batch` (with `unique_placers` real + indicative fields stubbed `None`/`0`), `/v1/events/{event_id}/traders`. Registered in `crates/sybil-api/src/routes/mod.rs`.

Modified:
- `crates/matching-sequencer/src/aggregates/mod.rs` — `pub mod trader_tracker;` and `pub use trader_tracker::TraderTracker;`.
- `crates/matching-sequencer/src/sequencer.rs` — add `trader_tracker: TraderTracker` to `BlockSequencer`. Hook at `try_admit_direct` (`:1058`) — call `trader_tracker.record_placed` after a successful direct admit. Hook at the admission loop inside `produce_block_in_place` (the per-submission loop around `:1772+`; agent verifies exact line) — same call after each successful admit. MM/MINT exclusion in `TraderTracker::record_placed` (no-op early-return for those `AccountId`s).
- `crates/matching-sequencer/src/sequencer.rs` — during `produce_block_in_place`, capture `unique_placers: u32` from `witness_orders` (set semantics across `account_id` field) and feed into `Block.unique_placers`; also capture `placers_by_market: HashMap<MarketId, u32>` for `by_market[m].placers`.
- `crates/matching-sequencer/src/block.rs` — add `pub unique_placers: u32` and `pub placers_by_market: HashMap<MarketId, u32>` to `Block`.
- `crates/sybil-api-types/src/response.rs` — add `pub unique_placers: u32` to `BlockResponse` (around :187), add `pub placers: u32` field to `BlockMarketStats` (the first field added to the struct from A1), add `pub trader_count: u32` to `MarketResponse` (:26) and `MarketSummaryResponse` (:91), define new `OpenBatchResponse` / `ActivityOverviewResponse` / `EventTradersResponse` shapes.
- `crates/sybil-api/src/convert.rs` — populate `BlockResponse.unique_placers` and `by_market[m].placers`.
- `crates/matching-sequencer/src/store.rs` — persistence plumbing (4 sites):
  1. `SequencerSnapshot` gains `trader_tracker: &'a TraderTracker`.
  2. `RestoredState` gains `trader_tracker: TraderTracker`.
  3. New `TableDefinition` for the tracker (single table holding a serialized `TraderTrackerSnapshot` payload — agent picks bincode/cbor; the existing pattern uses bincode).
  4. `save_block_inner` writes the tracker; `load_state` reads it (missing table → `Default::default()`).
- `frontend/web/src/lib/markets/use-markets.ts` — type for `trader_count` already in the regenerated schema; just stop reading the mock.
- `frontend/web/src/components/binary-card.tsx` — at the mock site (~:421) replace `<MockValue hint="trader count">` with the real `market.trader_count` rendered with `<RestartCaveatBadge />`.
- `frontend/web/src/components/multi-card.tsx` — similar replacement at ~:502.
- `frontend/web/src/components/market-rail/last-batches-disclosure.tsx` — replace `<MockValue hint="placed-trader counts not on the wire">` with `block.by_market[mid].placers` (or fall back to block-level `unique_placers` if per-market not present).
- `frontend/web/src/components/market-rail/next-batch-banner.tsx` and `batch-hero.tsx` — wire `tradersInBatch` from a new hook `useOpenBatch(market_id)` that polls `/v1/markets/{id}/open-batch` every 2 s while the batch is open.
- `frontend/web/src/app/activity/page.tsx` (or its `useActivityOverview` hook) — wire `unique_traders` from `/v1/activity/overview` with `<RestartCaveatBadge />`.

**Changes.**

- `TraderTracker::record_placed`: early-return if `account_id` is the project-wide MM account or `AccountId::MINT.0`; otherwise insert into `per_market[market]`, `platform`, and the current `hourly_buckets[-1]` (rolling if `hour_start_ms` of `ts_ms` differs from the latest bucket's). Bucket cap = 25 (drop oldest).
- `TraderTracker::event_count(market_ids)`: union over `per_market[m]` for each market in the event; `len()`. Heavy if called per request — cache in the API layer (~30s TTL) per the plan's "cache hot events" note.
- `/v1/markets/{id}/open-batch`:
  ```jsonc
  {
    "unique_placers": <iter over order_book.market_orderbook(id) and pending_bundles, dedupe account_ids>,
    "indicative_yes_price_nanos": null,
    "indicative_no_price_nanos": null,
    "indicative_volume_nanos": 0,
    "indicative_computed_at_ms": 0
  }
  ```
  Indicative fields are stubbed — C2 lights them up without a schema change.

**Tests.**

- `cargo test -p matching-sequencer aggregates::trader_tracker::tests` — bucket roll, MM/MINT skip, event union.
- `cargo test -p matching-sequencer trader_tracker_snapshot_roundtrip` — write tracker via `SequencerSnapshot`, load via `RestoredState`, assert equal.
- `cargo test -p sybil-api activity_overview_endpoint` — start in-memory sequencer, place orders, GET `/v1/activity/overview`, assert `unique_traders` matches expected (3 distinct accounts).
- `cargo test -p sybil-api open_batch_endpoint_unique_placers` — same harness, GET `/v1/markets/{id}/open-batch`, assert `unique_placers` reflects open-batch state.
- `cd frontend/web && pnpm types:generate`
- `cd frontend/web && pnpm test market-cards` (existing tests + one new test that mounts a card with `trader_count: 42` and asserts the number renders next to the badge).

**Acceptance criteria.**

- All trader surfaces stop showing `<MockValue>` and show real numbers with `<RestartCaveatBadge />` where applicable.
- `/v1/activity/overview.all_time.unique_traders` and `last_24h.unique_traders` populate with real numbers (volume + orders blocks return zeros until B2/B6 — partial-but-honest payload).
- `/v1/markets/{id}/open-batch` returns a real `unique_placers`; indicative fields are null/zero.

**Commit.**

Subject: `traders: TraderTracker + open-batch + activity-overview endpoints`

Body:
```
Five trader surfaces (a/c/d/e/f from BACKEND_DATA_PLAN.md Traders entry) light
up against a new TraderTracker under crates/matching-sequencer/src/aggregates/.
Persistence plugs into SequencerSnapshot/RestoredState. New endpoints:
  GET /v1/activity/overview     (unique_traders populated; volume/orders 0)
  GET /v1/markets/{id}/open-batch (unique_placers real; indicative stubbed)
  GET /v1/events/{eid}/traders   (per-event union, ~30s API cache)

Touches: see file list in BACKEND_IMPLEMENTATION_PLAN.md step B1.
```

**Rollback.** Remove the tracker, drop the endpoints, restore mocks on the FE. Persisted redb tables for `TRADER_TRACKER_*` remain harmlessly (no code reads them after revert).

**Non-goals.** No volume / orders / liquidity fields populate yet. The activity-overview endpoint returns zeros for those (they land in B2/B6). The open-batch endpoint's indicative fields stay stubbed until C2.

---

### Step B2: Extend `PriceTracker` with platform + hourly volume; land activity-overview volume

**Phase:** B
**Prereqs:** A1, A2, B1
**Est. LOC:** ~360 Rust + ~60 TS

**Goal.** Light up volume 24h (per-market + platform) and per-block per-market volume. Closes Volume surfaces (a/b/c/d/e/f from the Volume entry).

**Files.**

Modified:
- `crates/matching-sequencer/src/price_tracker.rs` — extend `PriceTracker` (struct at :18) with `platform_volume: u64`, `hourly_per_market: VecDeque<(u64, HashMap<MarketId, u64>)>` (cap 25), `hourly_platform: VecDeque<(u64, u64)>` (cap 25). Extend `record_block` (:85) to: (a) bump `platform_volume`; (b) route the existing per-market split (already computed transiently in `record_block`) into `hourly_per_market`'s current bucket; (c) bump `hourly_platform`'s current bucket. Add `market_volume_24h(m, now_ms)`, `platform_volume_24h(now_ms)`, `platform_volume_total()` methods. **Inline tests** at the bottom of the file (`#[cfg(test)] mod tests`).
- `crates/matching-sequencer/src/sequencer.rs` — `solve_batch_phase` (:1430) already computes `per_market_volume` (~:1469); plumb it onto `Block.volume_by_market: HashMap<MarketId, u64>` (new field on `Block`).
- `crates/sybil-api-types/src/response.rs` — add `pub volume_24h_nanos: u64` to `MarketResponse` + `MarketSummaryResponse`. Add `pub volume_nanos: u64` field to `BlockMarketStats`. Extend `ActivityOverviewResponse.all_time` with `total_volume_nanos: u64` and `last_24h` with `total_volume_nanos: u64`.
- `crates/sybil-api/src/convert.rs` — populate `MarketResponse.volume_24h_nanos` from `price_tracker.market_volume_24h(m, now_ms)`. Populate `by_market[m].volume_nanos` from `Block.volume_by_market`.
- `crates/sybil-api/src/routes/aggregates.rs` — extend the `/v1/activity/overview` response to populate volume fields.
- `crates/matching-sequencer/src/store.rs` — **full 4-site persistence plumbing** for the new fields (the existing `PRICE_TRACKER`/`MARKET_VOLUMES` tables persist only `last_clearing_prices` + `market_volumes`; the new `platform_volume`, `hourly_per_market`, `hourly_platform` need their own plumbing):
  1. `SequencerSnapshot` gains 3 new borrowed fields (or one combined `&PriceTrackerExtensions`).
  2. `RestoredState` gains the owned counterparts.
  3. New `TableDefinition`(s): either a single `PRICE_TRACKER_VOLUME_HOURLY` table with bincode-serialized payload, or three tables — agent picks the lower-friction option matching existing patterns.
  4. `save_block_inner` writes; `load_state` reads (missing tables → defaults).
- `frontend/web/src/components/binary-card.tsx`, `multi-card.tsx` — drop the volume `<MockValue>` and render `market.volume_24h_nanos`.
- `frontend/web/src/app/activity/page.tsx` (or `useActivityOverview`) — wire volume into the activity hero next to `<RestartCaveatBadge />`.

**Changes.**

- "First-of-bucket wins" applies for the hour roll: when `record_block` is called and the current `hour_start_ms` differs from the latest bucket, push a fresh bucket and drop the head if over cap.
- `market_volume_24h(m, now_ms)`: sum `bucket[m]` across buckets where `now_ms - bucket.hour_start_ms < 24 * 3_600_000`. ±1h resolution acceptable per the plan.
- `platform_volume_total()`: returns running sum (constant time).
- Memory check: 25 × N markets × 16B ≈ 2 MB at 5K markets.

**Tests.**

- `cargo test -p matching-sequencer price_tracker::tests::volume_extensions_bucket_roll`
- `cargo test -p matching-sequencer price_tracker::tests::volume_24h_window_arithmetic`
- `cargo test -p matching-sequencer price_tracker::tests::volume_cap_25_drop_oldest`
- `cargo test -p matching-sequencer price_tracker_volume_snapshot_roundtrip` — write extended state via SequencerSnapshot, load via RestoredState, assert equal.
- `cargo test -p sybil-api volume_24h_response` — place fills, advance simulated time, assert `volume_24h_nanos` and `total_volume_nanos` correct.
- `cd frontend/web && pnpm types:generate`
- `cd frontend/web && pnpm test market-cards` — extend test with `volume_24h_nanos: 1_000_000_000` and assert "$1.00" renders.

**Acceptance criteria.**

- `MarketResponse.volume_24h_nanos` returns non-zero after a fill within the last hour.
- `BlockResponse.by_market[mid].volume_nanos` matches `sum(fills in m, this block)` exactly.
- `/v1/activity/overview.all_time.total_volume_nanos` and `last_24h.total_volume_nanos` both populate.
- FE renders real numbers with `<RestartCaveatBadge />` on the all-time figure.
- New redb tables survive a save+load cycle in the snapshot-roundtrip test.

**Commit.**

Subject: `volume: 24h windows + per-block per-market split via PriceTracker`

Body:
```
Extends PriceTracker with platform_volume + hourly_per_market + hourly_platform
(VecDeque buckets, cap 25). Plumbs per-block per-market volume onto
BlockResponse.by_market. Populates the volume slots in /v1/activity/overview.

Each new field gets full 4-site persistence plumbing (the existing
PriceTracker snapshot path only covers last_clearing_prices + market_volumes;
these extensions need new tables).

Touches: see file list in BACKEND_IMPLEMENTATION_PLAN.md step B2.
```

**Rollback.** Field defaults zero everywhere. No new endpoints. Revert leaves the FE rendering `<MockValue>` again where it was. Persisted volume-extension tables remain harmlessly.

**Non-goals.** No new tracker file. No price-history changes (B3 handles 24h-ago snapshots). No orders / liquidity fields populated.

---

### Step B3: `PriceTracker` price-24h-ago extension

**Phase:** B
**Prereqs:** A1, B2
**Est. LOC:** ~220 Rust + ~50 TS

**Goal.** Ship server-computed "price 24h ago" per market so the FE 24h delta is one subtraction client-side. Fixes the silent bug where `useCardHistory` returns < 24h on busy markets (`price_history` cap 2000 = ~67 min at 2s cadence).

**Files.**

Modified:
- `crates/matching-sequencer/src/price_tracker.rs` — add `hourly_clearing_prices: HashMap<MarketId, VecDeque<(u64, Vec<u64>)>>` (cap 25 per market). In `record_block`: if the current hour bucket has no entry yet, insert `merged_clearing_prices.get(m).cloned()` for each settled market. **First-of-hour wins** — subsequent prices same hour leave bucket untouched. Add `price_n_hours_ago(m, n, now_ms) -> Option<(u64, u64)>` returning `(yes_price, no_price)` from the bucket whose `hour_start_ms` brackets `now_ms - n*3_600_000`. Inline tests.
- `crates/sybil-api-types/src/response.rs` — add `pub yes_price_24h_ago_nanos: Option<u64>` and `pub no_price_24h_ago_nanos: Option<u64>` to `MarketResponse` + `MarketSummaryResponse`.
- `crates/sybil-api/src/convert.rs` — populate via `price_tracker.price_n_hours_ago(m, 24, now_ms)`.
- `crates/matching-sequencer/src/store.rs` — **full 4-site persistence plumbing** for the new field (separate table from the volume extensions in B2). Missing-table → `Default::default()`.
- `frontend/web/src/lib/markets/use-card-history.ts` — drop the from-history derivation of the 24h delta (sparkline still uses history). Compute delta as `(current_yes - yes_price_24h_ago) / yes_price_24h_ago` when `yes_price_24h_ago_nanos != null`; show "—" when null.
- `frontend/web/src/components/binary-card.tsx` (~:253) and `multi-card.tsx` (~:333, ~:438) — drop sibling-row `<MockValue hint="24h delta">`; render real delta.
- `frontend/web/src/components/outcome-legend.tsx` (~:84) — same fix.

**Changes.**

- Markets younger than 24h → `price_n_hours_ago` returns `None` → FE renders "—".
- Markets with no clearing price yet → no buckets → `None`.

**Tests.**

- `cargo test -p matching-sequencer price_tracker::tests::hourly_clearing_prices_first_wins` — multiple `record_block` in same hour, assert first stuck.
- `cargo test -p matching-sequencer price_tracker::tests::price_24h_ago_lookup` — advance 24h+ of synthetic time, assert bucket bracketing.
- `cargo test -p matching-sequencer price_tracker_clearing_history_snapshot_roundtrip`
- `cargo test -p sybil-api market_response_includes_24h_ago` — fill then advance time, assert non-null in response.
- `cd frontend/web && pnpm types:generate`
- `cd frontend/web && pnpm test use-card-history` — null branch (24h-ago missing) renders "—"; non-null branch matches expected delta.

**Acceptance criteria.**

- `MarketResponse.yes_price_24h_ago_nanos` and `no_price_24h_ago_nanos` populate for markets older than 24h with prior clearing.
- BinaryCard 24h delta on busy markets stops being silently truncated.
- MultiCard sibling rows show real deltas.
- OutcomeLegend shows real per-outcome deltas.

**Commit.**

Subject: `price-24h: server-computed 24h-ago snapshot per market`

Body:
```
Extends PriceTracker with hourly_clearing_prices (first-of-hour wins, cap 25
per market) plus full 4-site persistence plumbing. Adds yes_price_24h_ago_nanos
/ no_price_24h_ago_nanos on MarketResponse so FE computes delta = current −
snapshot in one subtraction. Fixes the silent bug at use-card-history.ts:30
where price_history's 2000-cap gives ~67min not 24h on busy markets.

Touches: see file list in BACKEND_IMPLEMENTATION_PLAN.md step B3.
```

**Rollback.** Fields default `None`; FE falls back to `useCardHistory`'s buggy-but-existing path. (Silent bug returns; no new bug introduced.) Persisted clearing-history table remains harmlessly.

**Non-goals.** No price-history endpoint change. No retroactive computation (markets without 24h of buckets return null).

---

### Step B4: `LiquidityTracker` + `SequencerConfig.liquidity_band_nanos`

**Phase:** B
**Prereqs:** A1, A2
**Est. LOC:** ~320 Rust + ~40 TS

**Goal.** Light up the `liq` metric on cards. Per-market, ±band-around-midprice, averaged across last 10 batches. Excludes multi-market orders and MM flash liquidity.

**Files.**

New:
- `crates/matching-sequencer/src/aggregates/liquidity_tracker.rs` — `LiquidityTracker` with `last_n_per_market: HashMap<MarketId, VecDeque<u64>>` (cap 10 per market) + `band_nanos_at_last_update: u64`. Methods: `record_block(book: &OrderBook, midprices: &HashMap<MarketId, u64>, band_nanos: u64)`, `avg_last_n(m: MarketId, n: usize) -> u64`, `current(m: MarketId) -> u64`. Inline tests.

Modified:
- `crates/matching-sequencer/src/aggregates/mod.rs` — `pub mod liquidity_tracker;` and re-export.
- `crates/matching-sequencer/src/sequencer.rs` — add `liquidity_tracker: LiquidityTracker` and `liquidity_band_nanos: u64` field to `BlockSequencer`. Hook at end of `produce_block_in_place` right after `self.order_book.settle(...)` (`:2007-2008`). Compute midprices from `merged_clearing_prices` (binary: YES price ≈ midprice in our model) and pass to `liquidity_tracker.record_block(...)`.
- Where `SequencerConfig` is defined (search: `pub struct SequencerConfig` — likely in `sequencer.rs` or a sibling) — add `pub liquidity_band_nanos: u64` with `Default = 50_000_000` (= $0.05 at 1e9 nanos/dollar). Field uses `#[serde(default = ...)]` if `SequencerConfig` participates in any serialized form.
- `crates/sybil-api-types/src/response.rs` — add `pub liquidity_avg10_nanos: u64` and `pub liquidity_band_nanos: u64` to `MarketResponse` + `MarketSummaryResponse`.
- `crates/sybil-api/src/convert.rs` — populate from `liquidity_tracker.avg_last_n(m, 10)` + `self.liquidity_band_nanos`.
- `crates/matching-sequencer/src/store.rs` — 4 persistence sites (new `LIQUIDITY_TRACKER` table).
- `frontend/web/src/components/binary-card.tsx`, `multi-card.tsx` — replace `<MockValue hint="liq metric">` with `formatDollars(market.liquidity_avg10_nanos) + " (±" + formatDollars(market.liquidity_band_nanos) + ")"`.

**Changes.**

- `record_block`: one pass over `order_book.resting_orders()`. For each `RestingOrder` where `order.num_markets() == 1`, fetch its market's midprice, check `(mid - band) <= limit_price <= (mid + band)`, accumulate `limit_price × max_fill`. Push the per-market scalar into the ring (drop oldest if at cap 10). Multi-market orders excluded entirely (per plan's deliberate divergence).
- `avg_last_n(m, n)`: sum / count; returns 0 if empty (cold start until 10 blocks).
- `band_nanos_at_last_update` snapshotted alongside the ring so readers can detect band changes (FE compares wire `liquidity_band_nanos` against this; if they diverge, label shows the old band).

**Revert protocol.** If B4 is reverted post-snapshot (production has saved a tracker including `band_nanos_at_last_update`), the old `LiquidityTracker` table simply becomes orphaned in redb. Add a `#[serde(default)]` to the `band_nanos_at_last_update` field at landing time so future schema additions can read partial old snapshots; alternatively, on revert, drop the orphaned table via a one-shot migration. The plan-wide convention is to leave orphaned tables alone — they're harmless.

**Tests.**

- `cargo test -p matching-sequencer liquidity_tracker::tests::record_block_excludes_multi_market` — place spread (2-market order) + single-market order, assert spread's value not in the sum.
- `cargo test -p matching-sequencer liquidity_tracker::tests::ring_caps_at_10` — record 12 blocks, assert ring is 10 long with the latest 10 values.
- `cargo test -p matching-sequencer liquidity_tracker_snapshot_roundtrip`
- `cargo test -p sybil-api liquidity_in_market_response` — place orders, produce block, assert response field non-zero.
- `cd frontend/web && pnpm types:generate`
- `cd frontend/web && pnpm test market-cards` — render with `liquidity_avg10_nanos: 50_000_000_000` + band `50_000_000`, assert "$50.00 (±$0.05)".

**Acceptance criteria.**

- Cards render `liq $X.XX (±$0.05)` from real data; mock vanishes.
- Markets with no clearing price yet → `liquidity_avg10_nanos = 0` and FE shows "—" via existing format guard.

**Commit.**

Subject: `liquidity: LiquidityTracker (last-10 ±band) + SequencerConfig.liquidity_band_nanos`

Body:
```
New off-block LiquidityTracker. Per-market last-10-batch average of single-
market resting depth within ±band of midprice. Hook at end of
produce_block_in_place after order_book.settle(). Multi-market orders
excluded entirely (their limit_price is the bundle total, not attributable to
one market). Band ships on the wire alongside the average so FE labels it.

Touches: see file list in BACKEND_IMPLEMENTATION_PLAN.md step B4.
```

**Rollback.** Tracker disappears, fields default zero. Cards re-mock. See "Revert protocol" above for the snapshot-coupling story.

**Non-goals.** No per-side / per-account / per-order depth exposure. No histogram. No deliberate suppression for thin books.

---

### Step B5: `RestingOrder` annotations + `OrderBook` 4-method return-type widening

**Phase:** B
**Prereqs:** none (independent of A1/A2 — pure backend mechanical refactor)
**Est. LOC:** ~270 Rust + 0 TS

**Goal.** Add `has_been_matched: bool` + `original_max_fill: u64` to `RestingOrder` with `#[serde(default)]`. Widen FOUR `OrderBook` removal methods — `expire`, `revalidate`, `settle`, `cancel` — from their current `()` / `Result<(), _>` returns to `Vec<RestingOrder>` / `Result<RestingOrder, _>` so downstream B6 (OrderStatsTracker) and D1 (OrderCancelled) have clean signals.

**This step has no producer for the new fields** other than `OrderBook.accept` (which populates `original_max_fill` once at admit time) and `OrderBook.settle` (which sets `has_been_matched = true` when fill > 0). The fields are dead-but-correct until B6 / D1 consume them.

**Files.**

Modified:
- `crates/matching-sequencer/src/order_book.rs`:
  - `RestingOrder` struct (top of file, near `:25-50` where `expires_at_block` already uses `#[serde(default)]`) gains:
    - `#[serde(default)] pub has_been_matched: bool` (default `false`)
    - `#[serde(default)] pub original_max_fill: u64` (default `0`)
  - `accept` (:196): populate `original_max_fill = order.max_fill` at construction time. Never mutated thereafter.
  - `expire` (:240): change return type from `()` to `Vec<RestingOrder>`. Collect removed orders into a `Vec` and return them.
  - `revalidate` (:261): same widening.
  - `cancel` (:370): change return type from `Result<(), CancelError>` to `Result<RestingOrder, CancelError>`. The cancelled `RestingOrder` (currently dropped internally) is returned to the caller.
  - `settle` (:396): change return type from `()` to `Vec<RestingOrder>`. Inside settle, when an order gets filled (existing logic at the filled branch around `:420`), set `has_been_matched = true` on the now-mutated `RestingOrder` before it's persisted back or collected as removed.
- `crates/matching-sequencer/src/sequencer.rs`:
  - 5 production call sites updated to bind the new return values:
    - `:1197` `let ro = self.order_book.cancel(account_id, order_id)?;` (inside `cancel_pending_order` at `:1192`). The `ro` binding is unused in B5 — D1 will consume it.
    - `:1749` `let _expired = self.order_book.expire(self.height);`
    - `:1750` `let _revalidated = self.order_book.revalidate(&self.accounts, &active_markets);`
    - `:1873` `let _stp_undo = self.order_book.settle(...)` (the STP-undo phantom-fill settle)
    - `:2007-2008` `let _post_solve = self.order_book.settle(&fills, &mm_order_ids_set, self.height);`
- `crates/matching-sequencer/src/order_book.rs` test sites (7 calls; agent runs `cargo test -p matching-sequencer order_book::tests` after the edit to verify):
  - ~:670 `book.expire(5);`
  - ~:694, ~:720, ~:744 `book.settle(...)`
  - ~:761, ~:764 `book.expire(...)`
  - ~:810 `book.cancel(aid, accepted.order.id).unwrap();` — now needs `let _ = book.cancel(...).unwrap();` or similar binding to handle the widened return type.

**Changes.**

- `has_been_matched` defaults `false`; flips to `true` exactly in `OrderBook.settle` when `filled > 0` for the order. Survives in-place mutations.
- `original_max_fill` defaults `0` for old snapshots; set once in `accept` for new orders. **B8 reads this field for `PendingOrderResponse.original_quantity`.**
- Return values:
  - `expire / revalidate / settle` return `Vec<RestingOrder>` = orders removed from the book by that call. Empty `Vec` when nothing removed.
  - `cancel` returns `Result<RestingOrder, CancelError>` = the cancelled order (Ok branch) or the original error (Err branch).
- Refactor is mechanical: every production site currently ignores the return value; after this step they bind to `_` or a variable that B6/D1 will start reading.

**Tests.**

- `cargo test -p matching-sequencer order_book::tests` — the existing 7 test sites compile and pass without behavior change.
- `cargo test -p matching-sequencer order_book::tests::expire_returns_removed_orders` — NEW: place 3 orders with `expires_at_block = 1`, `expire(2)` returns a `Vec` of length 3.
- `cargo test -p matching-sequencer order_book::tests::settle_marks_matched` — NEW: place order, settle with a matching fill, then peek into the removed list and assert `has_been_matched == true`.
- `cargo test -p matching-sequencer order_book::tests::cancel_returns_order` — NEW: place order, cancel, assert the returned `RestingOrder.original_max_fill` matches what was admitted.
- `cargo test -p matching-sequencer order_book::tests::resting_order_serde_default` — NEW: deserialize a `RestingOrder` JSON missing the two new fields, assert `has_been_matched == false` and `original_max_fill == 0`.
- `just check-all` — workspace passes.

**Acceptance criteria.**

- `RestingOrder` JSON round-trips with old snapshots (`#[serde(default)]` works).
- All 5 production sites bind the return value (compiler-enforced).
- No behavior change on any user-visible surface (no new wire fields, no FE change).

**Commit.**

Subject: `order-book: RestingOrder annotations + widen expire/revalidate/settle/cancel returns`

Body:
```
Adds has_been_matched: bool + original_max_fill: u64 to RestingOrder, both
with #[serde(default)] for old-snapshot compat (matches existing
expires_at_block pattern at order_book.rs:42). Widens 4 OrderBook removal
methods to return the removed RestingOrder(s): expire / revalidate / settle
→ Vec<RestingOrder>; cancel → Result<RestingOrder, CancelError>. Touches 5
production sites in sequencer.rs + 7 test sites in order_book.rs.

No producer consumes the new fields yet — B6 (OrderStatsTracker) consumes
has_been_matched; D1 (OrderCancelled) consumes the cancel return value.

Point of no return: the return-type signatures of these four methods.
Reverting this commit also reverts B6 and D1.

Touches:
  crates/matching-sequencer/src/order_book.rs
  crates/matching-sequencer/src/sequencer.rs
```

**Rollback.** Revertible only if B6 and D1 have also been reverted. The new fields then disappear with `#[serde(default)]` covering the snapshot-format transition.

**Non-goals.** No wire change. No FE change. The new return values are bound to `_` until B6/D1 land.

---

### Step B6: `OrderStatsTracker` consumes B5's signal

**Phase:** B
**Prereqs:** A1, A2, B5
**Est. LOC:** ~380 Rust + ~60 TS

**Goal.** Light up the orders-placed/matched/unmatched surfaces (a/b/c/d/e from the Orders entry). Per-market + platform + 24h.

**Files.**

New:
- `crates/matching-sequencer/src/aggregates/order_stats_tracker.rs` — `OrderStats { placed: u64, matched: u64, unmatched: u64 }` + `OrderStatsTracker { per_market: HashMap<MarketId, OrderStats>, platform: OrderStats, hourly_platform: VecDeque<(u64, OrderStats)> }` (cap 25). Methods: `record_placed(account, markets, ts_ms)`, `record_matched(order: &RestingOrder, ts_ms)` (idempotent — only first matched per order counts), `record_unmatched(order: &RestingOrder, ts_ms)`, `per_market(m)`, `platform()`, `platform_24h(now_ms)`. Inline tests.

Modified:
- `crates/matching-sequencer/src/aggregates/mod.rs` — re-export.
- `crates/matching-sequencer/src/sequencer.rs`:
  - `BlockSequencer` gains `order_stats_tracker: OrderStatsTracker`.
  - `try_admit_direct` (`:1058`) and the admission loop in `produce_block_in_place` (`:1772+`) — call `order_stats_tracker.record_placed(...)` after every successful admit. Per-market attribution: for an `N`-market order, increment `placed` in each active market (platform increments once).
  - Right after `self.order_book.expire(...)` (now returning `Vec<RestingOrder>` per B5) — for each removed: `if !o.has_been_matched { order_stats_tracker.record_unmatched(&o, ts_ms) }`.
  - Right after `self.order_book.revalidate(...)` — same.
  - Right after `self.order_book.settle(...)` at `:2007-2008` — for each removed: if `o.has_been_matched`, treat as matched (already counted once when first matched — idempotent), else if removed via the expired-this-batch branch with `filled > 0`, count as matched, else unmatched.
  - **Cancellation NOT counted here.** Cancel removes flow through a separate path; cancels are surfaced via `OrderCancelled` in D1.
- `crates/matching-sequencer/src/order_book.rs` — `settle` (`:396`): the filled-branch logic gets a one-liner inside the inner loop: `o.has_been_matched = true;` before pushing to removed.
- `crates/matching-sequencer/src/block.rs` — add `pub orders_by_market: HashMap<MarketId, (u32, u32, u32)>` to `Block` (placed, matched, unmatched per block). Per-block platform `(placed, matched, unmatched)` are already covered by existing scalars (`order_count` + `orders_filled`); add `unmatched_this_block: u32` if not present.
- `crates/sybil-api-types/src/response.rs`:
  - `MarketResponse` + `MarketSummaryResponse`: `orders_placed_total: u64`, `orders_matched_total: u64`, `orders_unmatched_total: u64` (all `#[serde(default)]`).
  - `BlockMarketStats` (the struct from A1, growing again): `placed: u32`, `matched: u32`, `unmatched: u32`.
  - `ActivityOverviewResponse.all_time.orders` + `last_24h.orders` shells lit up.
- `crates/sybil-api/src/convert.rs` — populate the new fields.
- `crates/matching-sequencer/src/store.rs` — 4 persistence sites (new tables for the tracker).
- `frontend/web/src/app/m/[id]/page.tsx` — wire `orders_placed_total / matched_total / unmatched_total` in the market detail metric panel.
- `frontend/web/src/components/market-rail/last-batches-disclosure.tsx` (~:106) — drop the unmatched mock; surface block-level matched/unmatched per market.
- `frontend/web/src/app/activity/page.tsx` — wire `all_time.orders.*` and `last_24h.orders.*` in the activity hero (with `<RestartCaveatBadge />` on the all-time entries).

**Changes.**

- `record_matched` is idempotent per order: an order matched in batch 7 then matched again in batch 12 is still one matched event. Tracker checks `if order.has_been_matched` (which B5 sets) — if it was already true on entry to this call site, skip.
- Revalidate-evicted-then-never-matched counts as unmatched (flag in API docs comment near the endpoint route).
- Per-market hourly is not added in first pass (per plan; deferred); only platform hourly tracked.

**Tests.**

- `cargo test -p matching-sequencer order_stats_tracker::tests::placed_matched_unmatched_basic`
- `cargo test -p matching-sequencer order_stats_tracker::tests::matched_idempotent`
- `cargo test -p matching-sequencer order_stats_tracker::tests::multi_market_attribution`
- `cargo test -p matching-sequencer order_stats_tracker::tests::hourly_24h_window`
- `cargo test -p matching-sequencer order_stats_tracker_snapshot_roundtrip`
- `cargo test -p sybil-api order_stats_response`
- `cd frontend/web && pnpm types:generate`
- `cd frontend/web && pnpm test market-detail`

**Acceptance criteria.**

- All 5 Orders surfaces show real data.
- `BlockResponse.by_market[mid]` includes `placed / matched / unmatched`.
- `/v1/activity/overview.all_time.orders.placed/matched/unmatched` populate.
- Cancels do NOT appear in any of these counters.

**Commit.**

Subject: `order-stats: OrderStatsTracker (placed/matched/unmatched) per-market + 24h`

Body:
```
New OrderStatsTracker under aggregates/. Consumes B5's Vec<RestingOrder>
returns from expire/revalidate/settle to categorize exits as matched vs
unmatched. record_placed hooks at both admission sites. Cancellations are
NOT counted here (they flow through OrderCancelled in D1).

Per-market attribution: each active market gets +1 (platform once);
sum-of-per-market over-counts for multi-market orders, platform is
authoritative.

Touches: see file list in BACKEND_IMPLEMENTATION_PLAN.md step B6.
```

**Rollback.** Tracker disappears, fields default zero. B5's annotations remain harmlessly.

**Non-goals.** No per-market 24h windows (deferred). No cancel counting (D1).

---

### Step B7: Per-market welfare in `solve_batch_phase`

**Phase:** B
**Prereqs:** A1
**Est. LOC:** ~80 Rust + ~40 TS

**Goal.** Surface per-market welfare on `BlockResponse.by_market[m].welfare_nanos`. Pure data extension; no new tracker.

**Files.**

Modified:
- `crates/matching-sequencer/src/sequencer.rs` — `solve_batch_phase` (`:1430`), near the existing `total_volume` / `total_welfare` computation around `:1468`. Add a parallel per-market accumulator:
  ```
  let mut welfare_by_market: HashMap<MarketId, i64> = HashMap::new();
  for fill in &fills {
      if fill.fill_qty == 0 { continue; }
      let Some(order) = order_map.get(&fill.order_id) else { continue; };
      let w = order.welfare_contribution(fill.fill_price, fill.fill_qty);
      for m in order.active_markets() {
          *welfare_by_market.entry(m).or_insert(0) += w;
      }
  }
  ```
  Plumb onto the `SolvedBatch` return shape and store on `Block.welfare_by_market: HashMap<MarketId, i64>`.
- `crates/matching-sequencer/src/block.rs` — add `pub welfare_by_market: HashMap<MarketId, i64>` field on `Block`.
- `crates/sybil-api-types/src/response.rs` — `BlockMarketStats` gets `pub welfare_nanos: i64` (`#[serde(default)]`).
- `crates/sybil-api/src/convert.rs` — populate `by_market[m].welfare_nanos`.
- `frontend/web/src/app/activity/page.tsx` (or its `BatchDetail` component) — render per-market welfare in the expanded batch detail.

**Changes.**

- `welfare_contribution` is the existing `Order::welfare_contribution(fill_price, fill_qty)` from `crates/matching-engine/`. No new math.
- Sum-of-per-market over-counts vs. `total_welfare_nanos` (the existing scalar) for multi-market orders. Per the plan's attribution rule, `total_welfare_nanos` stays authoritative.
- Signed i64 to handle rare solver-rounding negatives.

**Tests.**

- `cargo test -p matching-sequencer welfare_by_market_basic` — single-market fill, assert `welfare_by_market[m] == total_welfare`.
- `cargo test -p matching-sequencer welfare_by_market_multi_market_attribution` — 2-market order, assert each market gets the contribution credited and sum > platform total (over-count).
- `cargo test -p sybil-api block_response_welfare_per_market`.
- `cd frontend/web && pnpm types:generate`
- `cd frontend/web && pnpm test batch-detail`

**Acceptance criteria.**

- `BlockResponse.by_market[mid].welfare_nanos: i64` populates per fill.
- Activity page batch-detail shows per-market welfare instead of mock.

**Commit.**

Subject: `welfare: per-market accumulator in solve_batch_phase`

Body:
```
Surfaces per-market welfare as BlockResponse.by_market[mid].welfare_nanos.
One extra HashMap accumulation alongside the existing total_welfare
computation in solve_batch_phase. Off-block per ground rules; total_welfare
stays the on-block authoritative scalar.

Touches:
  crates/matching-sequencer/src/sequencer.rs
  crates/matching-sequencer/src/block.rs
  crates/sybil-api-types/src/response.rs
  crates/sybil-api/src/convert.rs
  frontend/web/src/app/activity/page.tsx
```

**Rollback.** Field defaults `{}`. No FE regression.

**Non-goals.** No solver edits. No witness change. No per-event welfare endpoint.

---

### Step B8: Small additions — `first_deposit_ms` (#13), `total_fill_count` (#14), `original_quantity` wire (#16)

**Phase:** B
**Prereqs:** A1, A2, B5 (B5 added `original_max_fill` to RestingOrder; this step just surfaces it)
**Est. LOC:** ~150 Rust + ~50 TS

**Goal.** Three trivial wire additions. All off-block.

**Files.**

Modified:
- `crates/matching-sequencer/src/sequencer.rs`:
  - Add `first_deposit_ms: HashMap<AccountId, u64>` to `BlockSequencer` (off-block sidecar; NOT added to `Account` which would touch `state_root`).
  - `fund_account` (`:857`) and `ingest_l1_deposit` (`:929`) — `self.first_deposit_ms.entry(account_id).or_insert(ts_ms);`
- `crates/matching-sequencer/src/fill_recorder.rs`:
  - Add `total_count: HashMap<AccountId, u64>` to `FillRecorder` (per Q3).
  - `record_fills` (`:59`) — bump per fill. MINT exclusion at the top of `record_fills` (existing logic). Multi-market fills bump once per account (not per market).
  - Methods: `total_fills(account) -> u64`.
- `crates/matching-sequencer/src/store.rs` — extend persistence for both `first_deposit_ms` (new `FIRST_DEPOSIT_MS` table, full 4-site plumbing — small map but the canonical pattern) and `FillRecorder.total_count` (extension of the existing `FILL_HISTORY` / `FillRecorder` snapshot — the existing snapshot path covers it as part of `FillRecorder`'s state).
- `crates/sybil-api-types/src/response.rs`:
  - `PortfolioResponse` (~:398) gains `pub first_deposit_ms: u64` and `pub total_fill_count: u64` (both `#[serde(default)]`).
  - `PendingOrderResponse` (~:460) gains `pub original_quantity: u64` (`#[serde(default)]`).
- `crates/sybil-api/src/convert.rs` — populate all three.
- `frontend/web/src/components/portfolio/portfolio-hero.tsx` — render `formatDate(first_deposit_ms)` in the "since first deposit" copy on the ALL range (with `<RestartCaveatBadge />`); render `total_fill_count` instead of capped `fills.length` (`200+` goes away).
- `frontend/web/src/components/portfolio/open-orders.tsx` (or its equivalent — find the file rendering `<MockValue hint=`partial-fill progress`>`) — render `filled / size` bar using `(original_quantity - remaining_quantity) / original_quantity`.

**Changes.**

- `first_deposit_ms` is a `HashMap`; in `convert.rs` lookup returns `0` for accounts never deposited (UI shows `—`).
- `total_fill_count` MINT exclusion matches existing fill-recorder MINT handling.
- `original_quantity` reads from B5's `RestingOrder.original_max_fill` (which `OrderBook.accept` populates).

**Tests.**

- `cargo test -p matching-sequencer first_deposit_records_once`
- `cargo test -p matching-sequencer fill_recorder::tests::total_count_bumps_per_fill_not_per_market`
- `cargo test -p matching-sequencer fill_recorder::tests::total_count_excludes_mint`
- `cargo test -p sybil-api portfolio_response_first_deposit_and_count`
- `cd frontend/web && pnpm types:generate`
- `cd frontend/web && pnpm test portfolio-hero`

**Acceptance criteria.**

- Portfolio hero shows real first-deposit date + real (uncapped) trade count (both with the caveat badge).
- Open-orders row shows partial-fill progress bar.

**Commit.**

Subject: `portfolio: first_deposit_ms + total_fill_count + original_quantity wire`

Body:
```
Three trivial single-field wire additions per BACKEND_DATA_PLAN.md "Small
additions" (#13, #14, #16). first_deposit_ms is an off-block sidecar on
BlockSequencer (adding to Account would touch state_root). total_fill_count
lives on FillRecorder (decision Q3). original_quantity reads from B5's
RestingOrder.original_max_fill.

Touches: see file list in BACKEND_IMPLEMENTATION_PLAN.md step B8.
```

**Rollback.** Three fields default zero; FE shows date/count placeholders again.

**Non-goals.** No equity curve (OPEN_QUESTIONS #12; explicitly Not Now).

---

## Phase C — Cost basis + indicative

### Step C1: `CostBasisTracker` (apply_fill inside `FillRecorder.record_fills` + apply_resolution inside `Sequencer::resolve_market`)

**Phase:** C
**Prereqs:** A1, A2
**Est. LOC:** ~420 Rust + ~60 TS

**Goal.** Light up realized + unrealized PnL split + `avg_entry_price_nanos` on positions. Closes OPEN_QUESTIONS #10/#11.

**Files.**

New:
- `crates/matching-sequencer/src/aggregates/cost_basis_tracker.rs` — `CostBasisTracker { basis: HashMap<(AccountId, MarketId, u8), i64>, realized: HashMap<AccountId, i64> }`. Methods: `apply_fill(account: AccountId, deltas: &[(MarketId, u8, i64)], fill_price: u64)` (MINT early-return at the top), `apply_resolution(market: MarketId, payout_nanos: u64, affected_accounts: &[AccountId])`, `cost_basis(account, market, outcome) -> i64`, `realized_pnl(account) -> i64`. **Inline tests** at the bottom of the file.

Modified:
- `crates/matching-sequencer/src/aggregates/mod.rs` — re-export.
- `crates/matching-sequencer/src/fill_recorder.rs`:
  - `FillRecorder` gains `cost_basis_tracker: CostBasisTracker` field (the field exists ON `FillRecorder` so `apply_fill` can be called inline inside `record_fills` — sharing the `position_deltas` walk).
  - Inside `record_fills` (`:59`), inside the existing `position_deltas` loop at `:76`, call `self.cost_basis_tracker.apply_fill(account_id, &position_deltas, fill.fill_price)`.
  - **Persistence:** CostBasisTracker is snapshotted as its OWN redb table (`COST_BASIS_TRACKER`), NOT co-mingled with the existing `FillRecorder` snapshot. This keeps revert clean — see Rollback. The `FillRecorder` snapshot path stays untouched; a sibling save/load arm handles CostBasisTracker.
- `crates/matching-sequencer/src/settlement.rs` — `resolve_market` (`:80`) extended to return the set of `affected_accounts` (accounts whose balance changed due to the resolution payout). Currently the function returns `Result<ResolutionRecord, _>` or similar; extend the return type to include the affected-accounts set.
- `crates/matching-sequencer/src/sequencer.rs`:
  - `Sequencer::resolve_market` (`:1212`): after the call to `settlement::resolve_market` returns the `ResolutionRecord` + `affected_accounts`, call `self.fill_recorder.cost_basis_tracker.apply_resolution(market_id, payout_nanos, &affected_accounts)` immediately, BEFORE the `SystemEvent::MarketResolved` is staged into `pending_system_events`.
  - Same hook in `Sequencer::resolve_market_attested` (`:1270`) — both resolution paths must fire the cost-basis update.
- `crates/matching-sequencer/src/portfolio.rs` (`:31`):
  - `compute_portfolio` derives `unrealized_pnl_nanos` from `cost_basis_tracker.basis` + `last_clearing_prices`, and `realized_pnl_nanos` from `cost_basis_tracker.realized`. `pnl_nanos = unrealized + realized` (the existing scalar stays = sum).
- `crates/sybil-api-types/src/response.rs`:
  - `PortfolioResponse` gains `pub unrealized_pnl_nanos: i64` + `pub realized_pnl_nanos: i64` (`#[serde(default)]`).
  - `PositionValueResponse` (`:410`) gains `pub avg_entry_price_nanos: u64` (`#[serde(default)]`) — the cost basis as a positive price (sign already in `quantity`).
- `crates/sybil-api/src/convert.rs` — populate the new fields.
- `crates/matching-sequencer/src/store.rs` — 4 NEW persistence sites for the CostBasisTracker sibling table (separate from FillRecorder's existing plumbing):
  1. `SequencerSnapshot` gains `cost_basis_tracker: &'a CostBasisTracker`.
  2. `RestoredState` gains `cost_basis_tracker: CostBasisTracker`.
  3. New `COST_BASIS_TRACKER` `TableDefinition`.
  4. `save_block_inner` writes; `load_state` reads (missing table → `Default::default()`).
- `frontend/web/src/components/portfolio/portfolio-hero.tsx` — drop the `<MockValue>` wrappers around realized/unrealized PnL; render real values. Render `<RestartCaveatBadge />` next to realized PnL (cumulative, restart-sensitive). Trade count + first-deposit already live (from B8).
- `frontend/web/src/lib/account/use-portfolio.ts` — surface `realized_pnl_nanos` + `unrealized_pnl_nanos` + `avg_entry_price_nanos` to consumers.

**Changes.**

- WAC update rules (per the plan):
  - Opening/scaling same-sign: `new_basis = (old_basis × old_qty + fill_price × Δqty) / (old_qty + Δqty)`. No realized.
  - Reducing toward zero: realize `(fill_price - basis) × Δqty` for longs (flip for shorts). Basis unchanged.
  - Closing to zero: realize same; **reset basis to 0** (fixes the flip bug).
  - Flipping through zero: split — realize against prior position, start fresh basis at `fill_price` with the remainder.
- `apply_resolution`: for every `(account, market, outcome)` entry with non-zero qty, realize `(payout_nanos - basis) × qty`; zero out the basis entry.
- MINT early-return at the top of `apply_fill`.

**Tests.**

- `cargo test -p matching-sequencer cost_basis_tracker::tests::wac_scaling`
- `cargo test -p matching-sequencer cost_basis_tracker::tests::wac_reduction`
- `cargo test -p matching-sequencer cost_basis_tracker::tests::wac_close_resets`
- `cargo test -p matching-sequencer cost_basis_tracker::tests::wac_flip_through_zero`
- `cargo test -p matching-sequencer cost_basis_tracker::tests::apply_resolution_realizes`
- `cargo test -p matching-sequencer cost_basis_tracker::tests::mint_excluded`
- `cargo test -p matching-sequencer cost_basis_tracker_snapshot_roundtrip` — write tracker via sibling table, load via `RestoredState`, assert equal.
- `cargo test -p matching-sequencer resolve_market_hook` — call `Sequencer::resolve_market`, assert `cost_basis_tracker.realized` updated for affected accounts.
- `cargo test -p sybil-api portfolio_pnl_split`
- `cd frontend/web && pnpm types:generate`
- `cd frontend/web && pnpm test portfolio-hero`

**Acceptance criteria.**

- Realized + unrealized PnL render real numbers on portfolio hero.
- `avg_entry_price_nanos` populates per position.
- Resolved markets zero out cost basis correctly.
- `(realized + unrealized) == existing pnl_nanos` (invariant).
- `cost_basis_tracker_snapshot_roundtrip` test confirms sibling-table persistence works.

**Commit.**

Subject: `cost-basis: CostBasisTracker (WAC) + apply_fill inside FillRecorder + apply_resolution inside resolve_market`

Body:
```
New CostBasisTracker under aggregates/, lives as a field on FillRecorder so
apply_fill shares the position_deltas walk from compute_fill_settlement
(no parallel walk in settle_batch). Persistence is a SIBLING redb table —
not co-mingled with FillRecorder's existing snapshot — so revert stays clean.

Separate apply_resolution hook inside Sequencer::resolve_market (sequencer.rs
:1212) and Sequencer::resolve_market_attested (:1270), immediately after
settlement::resolve_market applies payouts. Resolution bypasses the Fill
stream so the fill hook above doesn't see it.

Lights up realized + unrealized PnL split + avg_entry_price_nanos.
Closes OPEN_QUESTIONS #10 / #11.

Point of no return: once a snapshot has been written with COST_BASIS_TRACKER
table populated, reverting requires accepting cold-start on next load. See
Rollback for the protocol.

Touches: see file list in BACKEND_IMPLEMENTATION_PLAN.md step C1.
```

**Rollback.** Fields default zero; FE re-mocks. The persisted `COST_BASIS_TRACKER` redb table becomes orphaned but harmless (no code reads it after revert; redb tolerates ignored tables). If C1 is reverted then re-applied later, the orphaned table is re-attached as long as no schema-breaking change happened in between.

**Non-goals.** No durable fill history (the 200-cap stays until a separate persistence iteration). No `cost_basis_after_nanos` / `realized_pnl_delta_nanos` on `AccountFillResponse` — deferred to a later step (the plan marks these as "optional convenience").

---

### Step C2: Indicative scheduler (separate `IndicativeTick` timer + `spawn_blocking`)

**Phase:** C
**Prereqs:** B1
**Est. LOC:** ~300 Rust + ~40 TS

**Goal.** Light up the indicative price/volume fields that B1 stubbed on `/v1/markets/{id}/open-batch`.

**Files.**

Modified:
- `crates/matching-sequencer/src/actor.rs`:
  - Add `indicative_cache: HashMap<MarketId, IndicativeSnapshot>` to `SequencerActorState` (`:344`), next to `latest_block` etc.
  - Define `pub struct IndicativeSnapshot { yes_price_nanos: Option<u64>, no_price_nanos: Option<u64>, volume_nanos: u64, computed_at_ms: u64 }` in this file or a sibling.
  - Define `SequencerMsg::IndicativeTick` (unit payload; fires from a dedicated timer task).
  - Define `SequencerMsg::IndicativeUpdate { snapshots: HashMap<MarketId, IndicativeSnapshot> }`.
  - Define `SequencerMsg::GetIndicative { market_id: MarketId, reply: RpcReplyPort<IndicativeSnapshot> }` mirroring the existing `GetMarketPrices` RPC pattern.
  - In `post_start` (`:947`, near the existing block-ticker setup at `:956`): register a SECOND `interval_at` ticker that fires `SequencerMsg::IndicativeTick` every 500ms-1s (specific cadence: 750ms — under one block period, more than enough to refresh between blocks). The existing block-ticker pattern is the template; duplicate it with the new message.
  - In `Actor::handle`'s match arm: add `SequencerMsg::IndicativeTick => state.on_indicative_tick(myself.clone()).await,` — `myself` is the `ActorRef<SequencerMsg>` available in `handle`'s scope (NOT in `on_tick`).
  - Add `async fn on_indicative_tick(&mut self, myself: ActorRef<SequencerMsg>)` to `SequencerActorState`:
    1. Build a speculative `Problem` from `self.sequencer.order_book.resting_orders()` (Tier 1: no pending bundles, no MM flash).
    2. Clone `myself`.
    3. `tokio::task::spawn_blocking(move || { let result = LpSolver::default().solve(&problem); let snapshots = extract_per_market(result); let _ = myself.send_message(SequencerMsg::IndicativeUpdate { snapshots }); });`
  - In `Actor::handle` add arms:
    - `SequencerMsg::IndicativeUpdate { snapshots } => { state.indicative_cache = snapshots; }`
    - `SequencerMsg::GetIndicative { market_id, reply } => { let snap = state.indicative_cache.get(&market_id).cloned().unwrap_or_default(); let _ = reply.send(snap); }`
  - **Cache lives on `SequencerActorState`** (NOT `BlockSequencer`); pure-core stays pure.
- `crates/sybil-api/src/routes/aggregates.rs` — `/v1/markets/{id}/open-batch` handler now calls `GetIndicative` via the `SequencerHandle` to populate the indicative fields. The stub-zeros from B1 disappear.
- `frontend/web/src/lib/markets/use-open-batch.ts` (the hook B1 created) — type-only update via `pnpm types:generate`; render path unchanged (consumer was already wired to optional fields).
- `frontend/web/src/app/m/[id]/page.tsx` or its pro-trading section — drop `<MockValue hint="indicative">`.

**Changes.**

- Fallback semantics (per the plan):
  - Empty resting book or solver infeasible → `yes_price = None`, `no_price = None`, `volume = 0`.
  - Book has orders but no matchable cross → `yes_price = last_clearing_prices[m].first()`, `no_price = symmetric`, `volume = 0`.
- MM flash liquidity excluded (Tier 1). Pending bundles excluded (Tier 1). MM-quoted-market bias documented in API docs comment.
- `Problem: Clone` is already true (`matching-engine/src/problem.rs:51`). No lifetime issues.
- The dedicated timer task fires `IndicativeTick` regardless of whether a block was just produced — block production and indicative refresh are now decoupled. The actor's mailbox still serializes the cache write against block-production ticks.

**Tests.**

- `cargo test -p matching-sequencer actor::tests::indicative_update_message_overwrites_cache`
- `cargo test -p matching-sequencer actor::tests::get_indicative_returns_cached_or_default`
- `cargo test -p matching-sequencer indicative_snapshot_fallback_to_last_clearing` — book with no cross, assert prices fall back to `last_clearing_prices`.
- `cargo test -p matching-sequencer actor::tests::no_deadlock_under_100_ticks` — exercise interleavings of Tick + IndicativeTick + IndicativeUpdate + ProduceBlock; assert the actor mailbox stays responsive.
- `cargo test -p sybil-api open_batch_indicative_real` — start sequencer, add orders, wait one IndicativeTick (~750ms), GET `/v1/markets/{id}/open-batch`, assert indicative fields non-null.
- `cd frontend/web && pnpm types:generate`
- `cd frontend/web && pnpm test market-detail`

**Acceptance criteria.**

- Indicative price + volume populate for markets with a matchable cross.
- Fallback to last clearing when no cross; null when book empty.
- Indicative refreshes mid-batch (not gated on block production).
- Actor does not deadlock under any sequence of `Tick` + `IndicativeTick` + `IndicativeUpdate` + block production.

**Commit.**

Subject: `indicative: separate IndicativeTick timer + spawn_blocking-scheduled solve`

Body:
```
Lights up the indicative fields stubbed in B1. Speculative solver call runs
on its own 750ms cadence via a dedicated IndicativeTick timer task
registered at actor post_start (mirroring the existing block-ticker pattern).
spawn_blocking with a cloned Problem; result flows back as an
IndicativeUpdate self-message that overwrites indicative_cache on
SequencerActorState. No shared lock, no AtomicBool — the actor mailbox
serializes cache writes against block-production ticks.

Decoupled from on_tick because on_tick's ActorRef isn't accessible at the
current trait boundary and because every tick currently produces a block
(no idle branch).

Tier 1: resting orders only. Pending bundles + MM flash deferred to Tier 2/3.

Touches: see file list in BACKEND_IMPLEMENTATION_PLAN.md step C2.
```

**Rollback.** Indicative fields go back to stubbed `None`/`0` from B1. The indicative timer task is removed; actor reverts to single-ticker. Mechanical.

**Non-goals.** No Tier 2 (pending bundles in speculative solve). No Tier 3 (deterministic MM snapshot). No per-request solving.

---

## Phase D — On-chain change

### Step D1: `OrderCancelled` SystemEvent

**Phase:** D — **the only on-chain change in this iteration**
**Prereqs:** B5
**Est. LOC:** ~360 Rust + ~80 TS

**Goal.** Plumb a new `OrderCancelled` SystemEvent end to end: sequencer + verifier in lockstep, with events_root containing the new variant. Then the portfolio activity feed renders cancellations.

**Coordinated deploy note.** This step is the only one in the plan that requires the sequencer and verifier to be deployed together. A serde enum addition is forward-additive under externally-tagged encoding (the default) — historical blocks encode identically and historical digests stay valid — but **new blocks need the new variant on both sides**. In this repo today: single sequencer, no third-party verifiers in production → straightforward. Document the deploy ordering in the commit body.

**Files.**

Modified:
- `crates/matching-engine/src/types.rs` — add a NEW `OrderDirection` enum next to the existing `Side`:
  ```
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  pub enum OrderDirection {
      BuyYes,
      SellYes,
      BuyNo,
      SellNo,
  }

  pub fn derive_order_direction(order: &Order, primary_market: MarketId) -> OrderDirection { ... }
  ```
  `derive_order_direction` reads `order.payoffs[primary_market]` (or equivalent), determines outcome bit + sign, returns the right variant. **Document the multi-market edge case**: `primary_market` is conventionally `order.payoffs.iter().next().map(|(m, _)| *m)` — first entry. For binary single-market orders this is unambiguous; for spreads / baskets the derivation reflects the first market only.
- `crates/matching-sequencer/src/system_event.rs` — add the 6th variant:
  ```
  OrderCancelled {
      account_id: AccountId,
      order_id: u64,
      market_ids: Vec<MarketId>,
      side: matching_engine::OrderDirection,
      remaining_quantity: u64,
  },
  ```
- `crates/sybil-verifier/src/event_schema.rs`:
  - Add `OrderCancelled { ... same fields ... }` variant to `SystemEventWitness` (~:109).
  - Add the leaf-encoding arm in `system_event_leaf_value` (`:24`) at **tag byte 5** (existing tags are 0-4, sorted; 5 is the next free slot; leaf-tag ordering matters for the `events_root` hash). Bytes encode: `[5_u8, account_id_be, order_id_be, market_ids.len() le32, market_ids[..] le32 each, side_byte, remaining_quantity_be]`. The `side_byte` maps `OrderDirection` to `{ BuyYes: 0, SellYes: 1, BuyNo: 2, SellNo: 3 }` (stable ordering, documented in a comment).
- `crates/matching-sequencer/src/digest.rs` — add `encode_order_cancelled_event(account_id, order_id, market_ids, side, remaining_quantity, block_height) -> Vec<u8>` mirroring the existing `encode_*_event` family (`:10-100`). This is what `account.events_digest` gets folded with via `update_digest`. **Note:** `encode_mint_event` (`digest.rs:84`) is NOT a SystemEvent — leave it untouched.
- `crates/matching-sequencer/src/sequencer.rs`:
  - `convert_system_event` arm at `:355`: `SystemEvent::OrderCancelled { ... } => SystemEventWitness::OrderCancelled { ... }`.
  - Per-account `events_digest` 6th arm at `:1641-1708`: after the existing 5 arms (Deposit `:1652`, L1Deposit `:1659`, WithdrawalCreated `:1675`, CreateAccount `:1691`, MarketResolved `:1707`), add a 6th match arm for `SystemEvent::OrderCancelled` that calls `encode_order_cancelled_event` and folds it into the cancelling account's `events_digest`.
  - `cancel_pending_order` (`:1192-1197`): the `OrderBook.cancel` call (now returning `Result<RestingOrder, CancelError>` per B5) yields the cancelled order. Use `derive_order_direction(&ro.order, primary_market)` to compute the `side`, then stage:
    ```
    self.pending_system_events.push(SystemEvent::OrderCancelled {
        account_id,
        order_id,
        market_ids: ro.order.active_markets().collect(),
        side: derive_order_direction(&ro.order, primary_market),
        remaining_quantity: ro.order.max_fill,
    });
    ```
- `crates/sybil-api-types/src/response.rs` — extend `SystemEventResponse` (~:148) with the new variant:
  ```jsonc
  { "type": "order_cancelled", "account_id": u64, "order_id": u64, "market_ids": [u32], "side": "BuyYes" | "SellYes" | "BuyNo" | "SellNo", "remaining_quantity": u64 }
  ```
- `crates/sybil-api/src/convert.rs` — `SystemEventWitness::OrderCancelled` → `SystemEventResponse::OrderCancelled`. `OrderDirection` serializes via its existing `Serialize` impl.
- `frontend/web/src/components/portfolio/activity-feed.tsx` (or equivalent — locate the file rendering cancellations in the activity tab) — wire `SystemEventResponse::OrderCancelled` entries into the feed. The localStorage browser-only cancellation hack disappears.

**Changes.**

- Historical blocks (pre-deploy) encode identically. Their `events_root` stays valid.
- New blocks with cancellations encode the cancellation into the cancelling `account.events_digest`. The block's `events_root` Merkle aggregates these per-account digests via the existing pipeline.
- The wire `SystemEventResponse` is externally-tagged (uses `#[serde(tag = "type")]`); old clients that don't know the `"order_cancelled"` discriminator simply skip it.
- `OrderDirection` is a new user-facing enum distinct from the existing `Side { Bid, Ask }` (which describes order-book bid/ask, not buy/sell of YES/NO outcomes).

**Tests.**

- `cargo test -p matching-engine types::tests::order_direction_derivation` — single-market BuyYes order, derive correctly; same for SellNo etc.
- `cargo test -p matching-sequencer system_event::tests::order_cancelled_witness_roundtrip`
- `cargo test -p sybil-verifier event_schema::tests::order_cancelled_tag_byte_5` — assert the encoded leaf starts with `0x05`.
- `cargo test -p matching-sequencer cancel_emits_order_cancelled` — place order, cancel, produce block, assert `block.system_events` contains an `OrderCancelled` matching the cancellation, and the cancelling account's `events_digest` advanced.
- `cargo test -p sybil-verifier verify_block_with_order_cancelled` — run the existing block-verification pipeline against a block containing an `OrderCancelled` event; assert verification succeeds.
- `cd frontend/web && pnpm types:generate`
- `cd frontend/web && pnpm test activity-feed`

**Acceptance criteria.**

- Sequencer emits `OrderCancelled` in `block.system_events` for every cancellation.
- Verifier accepts and verifies blocks containing the new variant.
- Portfolio activity feed shows cancellations across sessions (not just localStorage).
- `events_root` for blocks without cancellations is byte-identical to pre-deploy.

**Commit.**

Subject: `order-cancelled: SystemEvent + OrderDirection enum + verifier variant + portfolio activity feed`

Body:
```
The plan's only on-chain change. New OrderCancelled SystemEvent (variant 6 of
6 on SystemEvent / SystemEventWitness; leaf tag byte 5). Coordinated deploy:
sequencer and sybil-verifier ship together.

Introduces a new OrderDirection enum (BuyYes/SellYes/BuyNo/SellNo) in
matching-engine for the user-facing event payload. Distinct from the
existing Side { Bid, Ask } which describes order-book bid/ask.

Forward-additive under externally-tagged serde encoding: historical blocks
encode identically, historical events_root stays valid. New blocks with
cancellations fold into the cancelling account's events_digest via a new
encode_order_cancelled_event in digest.rs. encode_mint_event is unrelated
and stays untouched.

Cancel staging happens in Sequencer::cancel_pending_order using the
RestingOrder returned by B5's widened OrderBook.cancel.

Closes OPEN_QUESTIONS #15.

Touches: see file list in BACKEND_IMPLEMENTATION_PLAN.md step D1.
```

**Rollback.** Coordinated revert (sequencer + verifier together). Once deployed in prod, reverting requires ensuring no blocks-with-cancellations are still being replayed.

**Non-goals.** No admin-cancel event (separate future addition). No "cancelled by whom" disambiguation (only self-cancel exists today).

---

## Phase E — Console + signoff

### Step E1: Sybil console new "Aggregates" tab

**Phase:** E
**Prereqs:** B1, B2, B3, B4, B6, B7, B8, C1, C2, D1
**Est. LOC:** ~600 HTML (Alpine.js)

**Goal.** Surface every metric this iteration adds as a new tab in the existing Sybil console (`crates/sybil-api/static/index.html`, served from port 3000 by sybil-api).

**Files.**

Modified:
- `crates/sybil-api/static/index.html` (~1338 LOC today) — add a new tab button next to the existing tabs (Markets, MM, Blocks per the AGENTS.md description). New tab id `aggregates`. Tab content surfaces:
  - **Platform** (top of tab, `/v1/activity/overview`):
    - All-time: unique traders, total volume, orders placed/matched/unmatched. Each rendered with a small `since last restart` tag.
    - Last 24h: same fields, no tag (24h is well-defined regardless of restart).
  - **Per-market table** (one row per active market, polled from `/v1/markets` every 2s):
    - Trader count, 24h volume, liquidity (with band), 24h price delta (yes/no), orders placed/matched/unmatched total.
  - **Latest block** (`/v1/blocks/latest` or WS-streamed):
    - `unique_placers`, then a sub-table per `by_market[m]` showing volume, welfare, placed/matched/unmatched, placers.
  - **Open batch panel** (one market at a time, picker; polls `/v1/markets/{id}/open-batch`):
    - `unique_placers`, indicative yes/no/volume, `computed_at_ms` staleness indicator.
  - **Cost-basis sample** (picks a fixed AccountId — e.g., MM account or a configured "demo" account; polls `/v1/accounts/{id}/portfolio`):
    - Realized PnL, unrealized PnL, position list with `avg_entry_price_nanos`.
  - **OrderCancelled stream** (subscribes to WS block stream or polls `/v1/system-events/recent` if present; otherwise scans last 50 blocks for `system_events` of type `order_cancelled`):
    - Scrolling list: timestamp, account_id, order_id, side, remaining_quantity.
- All sections use the existing Alpine.js / CSS-variable styling already established in the file (do not introduce a new framework).

**Graceful degradation.** Each tab section guards its render with `data && data.<field> != null` checks so that if any single upstream tracker step is reverted post-E1, the rest of the tab still functions. Sections that have lost their data source render a small `"unavailable — upstream tracker not deployed"` placeholder instead of erroring.

**Tests.**

- Visual smoke against running API: `cargo run --release -p sybil-api -- --dev-mode --port 3001` then open `http://localhost:3001/` (which serves the static file) and click the new tab. Every section populates without errors in the browser console.
- No automated test for this step — Alpine.js console is not unit-tested. Manual checklist:
  - [ ] Trader counts non-zero after 1 order placed
  - [ ] Volume populates after 1 fill
  - [ ] Liquidity populates after 10 blocks of resting depth
  - [ ] Per-block by_market entries appear in the latest-block panel
  - [ ] Indicative fields populate within ~1s after orders arrive (C2 cadence is 750ms)
  - [ ] OrderCancelled stream picks up cancellations across page reload (proves it's wire, not localStorage)
  - [ ] If a section's upstream tracker is reverted, that section shows `"unavailable"` without breaking adjacent sections

**Acceptance criteria.**

- New tab renders without errors in Chrome + Safari.
- Every metric this iteration adds is visible.
- No regressions to existing tabs (Markets / MM / Blocks).
- Graceful degradation: a single tracker revert doesn't blank the tab.

**Commit.**

Subject: `console: Aggregates tab surfacing every new metric this iteration adds`

Body:
```
New "Aggregates" tab in crates/sybil-api/static/index.html surfacing all
iteration outputs: platform totals (24h + all-time) from /v1/activity/
overview, per-market table from /v1/markets, latest-block per-market
breakdown from BlockResponse.by_market, open-batch indicative for a
picked market, cost-basis sample for a demo account, and a scrolling
OrderCancelled event stream.

Each section guards with `data && data.field != null` so partial reverts
of upstream tracker steps degrade gracefully (the section shows
"unavailable" instead of erroring the whole tab).

Pure HTML edit; re-uses Alpine.js + the existing fetch/WS helpers.

Touches: crates/sybil-api/static/index.html
```

**Rollback.** Revert the HTML edit.

**Non-goals.** No new framework. No per-account drill-down beyond the single demo-account cost-basis. No charts beyond what existing tabs already render.

---

### Step E2: Integration smoke + `STATUS.md` / `OPEN_QUESTIONS.md` updates

**Phase:** E
**Prereqs:** all prior steps
**Est. LOC:** ~80 Rust/shell + docs

**Goal.** Close the iteration with a thin smoke script + housekeeping. No new product features.

**Files.**

New:
- `scripts/smoke-aggregates.sh` (or a Rust integration test in `crates/sybil-api/tests/`) — script that:
  1. Starts `sybil-api --dev-mode --port 3001` in the background.
  2. POST a few orders + fills to drive the trackers.
  3. GETs each new endpoint: `/v1/activity/overview`, `/v1/markets/{id}/open-batch`, `/v1/events/{eid}/traders`, `/v1/markets/{id}` (to check `trader_count` / `volume_24h_nanos` / `liquidity_avg10_nanos` / `yes_price_24h_ago_nanos` / `orders_*_total`), `/v1/blocks/latest` (to check `by_market[m]`), `/v1/accounts/{id}/portfolio` (to check `realized_pnl_nanos` / `unrealized_pnl_nanos` / `first_deposit_ms` / `total_fill_count`).
  4. Asserts every field this iteration adds returns a non-default value (or the documented null).
  5. Issues a cancel and verifies a corresponding `SystemEventResponse::OrderCancelled` appears in the block stream.

Modified:
- `frontend/STATUS.md` — update "what's built" / "backend backlog" sections; flip the trackers from `BACKEND_DATA_PLAN.md`-tracked to landed. Note `<RestartCaveatBadge>` is rendered on N surfaces; will be dropped when prod persistence is on.
- `frontend/OPEN_QUESTIONS.md` — close out #1, #2, #3, #4, #5, #7, #8, #10, #11, #13, #14, #15, #16 (per the entries in `BACKEND_DATA_PLAN.md`). Leave #6, #9, #12 open (NOT NOW items).
- `frontend/BACKEND_DATA_PLAN.md` — append a final log entry under "Log": `2026-XX-XX — implementation landed via BACKEND_IMPLEMENTATION_PLAN.md (15 commits, A1 through E2).`

**Changes.**

- Script is executable but not run by CI by default; agent runs once locally to verify, commits the script, then optionally `chmod +x`.

**Tests.**

- `bash scripts/smoke-aggregates.sh` — script exits 0 against a fresh `--dev-mode` server.
- `just check-all` — workspace remains green.

**Acceptance criteria.**

- Smoke script exercises every new endpoint and asserts non-default values.
- STATUS.md and OPEN_QUESTIONS.md reflect landed state.
- `BACKEND_DATA_PLAN.md` has a closing log entry.

**Commit.**

Subject: `signoff: smoke-aggregates script + STATUS / OPEN_QUESTIONS updates`

Body:
```
Thin closeout for the backend-data iteration. New scripts/smoke-aggregates.sh
walks every new endpoint and asserts non-default values. STATUS.md and
OPEN_QUESTIONS.md updated to reflect landed state; BACKEND_DATA_PLAN.md
gets a final log entry.

Touches:
  scripts/smoke-aggregates.sh (new)
  frontend/STATUS.md
  frontend/OPEN_QUESTIONS.md
  frontend/BACKEND_DATA_PLAN.md
```

**Rollback.** Pure docs/script revert.

**Non-goals.** No CI integration of the smoke script (separate decision). No persistence flip (still gated on `SYBIL_DATA_DIR` in prod).

---

## After all 15 land

Before declaring the iteration done:

1. Run `just check-all` from the repo root. Must be green.
2. Run `bash scripts/smoke-aggregates.sh` against a fresh `--dev-mode` server. Must exit 0.
3. Open `http://localhost:3001/` and click through the new "Aggregates" tab. Every section populates.
4. Spot-check: `<RestartCaveatBadge />` renders on every surface that consumes an "all-time" tracker field (binary-card trader_count, multi-card trader_count, activity hero all-time figures, portfolio hero first_deposit_ms + total_fill_count + realized PnL). The badge will be removed in a future iteration once `SYBIL_DATA_DIR` is set in prod.
5. Confirm the OrderCancelled events appear in `/v1/blocks/latest.system_events` after a cancellation. Confirm verifier (`cargo test -p sybil-verifier`) accepts blocks with cancellations.

Then dispatch the three reviewers (code reviewer, lead design architect, lead Rust dev) on the **landed work**, not on this plan — a separate review pass.

---

## Plan-review pass (already completed)

This plan went through a three-reviewer specialist pass after the first draft (recorded above in "Revision history"). All 14 findings (4 blockers, 5 major, 5 minor) have been folded in. Key changes from first-draft to this version:

- **C1 / D1 swap.** OrderCancelled (originally `C1`) is now `D1` and ships LAST; cost-basis + indicative work was originally `D1` / `D2` and is now `C1` / `C2`. Rationale: D1's coordinated sequencer+verifier deploy is the riskiest commit; shipping the off-block portfolio surfaces first lets the FE PnL split land regardless of D1's deploy timing.
- **B5 widens FOUR OrderBook methods**, not three. `cancel` is widened too (returns `Result<RestingOrder, CancelError>`) so D1 can read the cancelled order's metadata. This was a unanimous reviewer flag — `OrderBook.cancel` currently drops the order internally; the caller had no way to stage the event.
- **`OrderDirection` enum is new.** The existing `Side { Bid, Ask }` is for order-book bid/ask, not buy/sell of YES/NO outcomes. `OrderDirection { BuyYes, SellYes, BuyNo, SellNo }` is added to `matching-engine` for the user-facing event payload.
- **Indicative scheduler is a separate timer task**, not an idle-tick branch. Every tick currently produces a block (no idle branch in `on_tick`); a dedicated `IndicativeTick` ticker at `actor.rs` post_start decouples cadence from block production.
- **D1 cost-basis hook is in `Sequencer::resolve_market`**, not `convert_system_event`. The original plan hooked at events-digest update time, but payouts are applied earlier (in `Sequencer::resolve_market` → `settlement::resolve_market`); the cost-basis update must fire alongside payouts.
- **CostBasisTracker has its own redb table** (sibling to FillRecorder's tables), not co-mingled. Keeps revert clean despite the field living on `FillRecorder` for the shared `position_deltas` walk.
- **B2 / B3 carry full 4-site persistence plumbing.** The original plan claimed PriceTracker "already has snapshot plumbing"; in fact only `last_clearing_prices` + `market_volumes` are persisted today. The new fields each need their own tables.
- **3 points-of-no-return**, not 2: B5 (4 OrderBook signatures), C1 (CostBasisTracker persistence), D1 (coordinated deploy).
