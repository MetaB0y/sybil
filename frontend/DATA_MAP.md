# Frontend ↔ Backend data map

Per-page inventory of every piece of backend-sourced data the Sybil frontend
displays — direct REST reads, REST + client-side processing, and data derived
from the live WebSocket block stream. Living doc; iterate freely.

_Generated 2026-06-29 by tracing each page's component tree → hooks → endpoints._

## How the data flows (foundation)

Two channels feed everything:

- **REST** — `openapi-fetch` client (`src/lib/api/client.ts`) against
  `https://172-104-31-54.nip.io`, base `/v1/...`, wrapped in React Query hooks.
- **WebSocket** `/v1/blocks/ws` — the live block stream (`BlockResponse` every
  ~2s). On mount the app **hydrates** with `GET /v1/blocks/latest` +
  `GET /v1/markets/prices` (`src/lib/ws/realtime-provider.tsx`), seeds the
  Zustand store (`src/lib/store/index.ts`), then the WS keeps `latestBlock`,
  `recentBlocks` (ring buffer ~80) and `pricesByMarketId` (from
  `clearing_prices_nanos`) live. Most "live price / live mark / countdown" data
  is derived from this store, not re-fetched.

Legend: ⚠️ = mocked / not real backend yet · FE-derived = computed client-side.

---

## Global shell (renders on every page)

| Sybil page | Frontend data (displayed) | Backend data (source) |
|---|---|---|
| Global nav (all pages) | Account chip: live portfolio total | `GET /v1/accounts/{id}/portfolio` → `portfolio_value_nanos` / positions marked at live WS prices; invalidated each block |
| Global nav (all pages) | Account chip: available cash | `GET /v1/accounts/{id}/portfolio` → `balance_nanos`, minus cash reserved by open buys (`GET /v1/accounts/{id}/orders`); FE-derived |
| Global nav (all pages) | Account chip: account ID / alias / pubkey | Local session (no fetch) |
| Global nav (all pages) | Batch pill: latest block height, 2s countdown, connection state | WS `/v1/blocks/ws` → `BlockResponse.height` + `perf.now()` anchor; hydrated by `GET /v1/blocks/latest` |
| Global nav (all pages) | Nav search dropdown (names, YES odds, volume, outcome count, category dot) | `GET /v1/markets` filtered **client-side** (`/v1/markets/search` exists in schema but is unused) + live prices from store |

---

## Home — market index (`/`)

| Sybil page | Frontend data (displayed) | Backend data (source) |
|---|---|---|
| Home (/) | Card grid + binary/multi grouping by event | `GET /v1/markets` → `MarketResponse[]`, grouped by `event_id` |
| Home (/) | Card category badge | `MarketResponse.categories[]` → FE priority pick (`pickDisplayCategory`) |
| Home (/) | Card title | `MarketResponse.name` (binary) / `event_title` (multi) |
| Home (/) | Card thumbnail | `market_image_url` / `market_icon_url` / `event_image_url` / `event_icon_url` |
| Home (/) | Binary card YES/NO price (%) | WS `/v1/blocks/ws` `clearing_prices_nanos` + `GET /v1/markets/prices` seed (store `pricesByMarketId`) |
| Home (/) | Binary card 24h delta + sparkline | `GET /v1/markets/{id}/prices/history?from_ms=now-24h` (lazy, on viewport); delta = last−first FE-derived |
| Home (/) | Card volume / liquidity | `MarketResponse.volume_nanos` / `liquidity_avg10_nanos` |
| Home (/) | Card trader count (binary) | `MarketResponse.trader_count` |
| Home (/) | Multi-card event trader count (union) | `GET /v1/events/{event_id}/traders` (lazy, only when sorting by traders) |
| Home (/) | Multi-card outcome short labels ("↑ 200,000") | `GET /v1/events/{event_id}/raw` (raw Gamma JSON), matched by `polymarket_condition_id` (lazy) |
| Home (/) | Multi-card featured outcome + "+N more" | `GET /v1/markets` ranked by volume/closed FE-side |
| Home (/) | Clearing ticker strip (name, vol, price change, age) | WS `/v1/blocks/ws` → `clearing_prices_nanos`, `by_market[id].volume_nanos`, `timestamp_ms` |
| Home (/) | Sort tabs / pagination | Local UI state (no fetch) |

---

## Activity (`/activity`)

| Sybil page | Frontend data (displayed) | Backend data (source) |
|---|---|---|
| Activity | Hero all-time: matched volume, welfare, active traders, orders placed/matched | `GET /v1/activity/overview` → `all_time.*` (unmatched = FE-derived) |
| Activity | Hero last-24h: same metrics | `GET /v1/activity/overview` → `last_24h.*` |
| Activity | Total batches count | store `latestBlock.height` (WS / `GET /v1/blocks/latest`) |
| Activity | Live markets count | `GET /v1/markets/summary` → count `status="active"` |
| Activity | Batches table (height, time, volume, welfare, orders, markets, traders) | `GET /v1/blocks?limit=60` REST backfill + WS `/v1/blocks/ws` tail → `BlockResponse` fields |
| Activity | Batch detail per-market rows (title, category, clearing price, Δ vs prev batch, matched vol, welfare, placed/matched) | `BlockResponse.by_market[mid]` + `GET /v1/markets` for names/categories; Δ vs previous block FE-derived |
| Activity | Batch composition (donut, KV: markets/traders/processed/matched/unmatched) | FE-derived from the expanded `BlockResponse` |
| Activity | Current batch number + countdown | FE-derived (`height+1`, 2s cadence) |
| Activity | Batch detail TX hash / sequencer / clearing duration | ⚠️ Mocked (not from backend) |

---

## Market detail — public (`/m/[id]`)

| Sybil page | Frontend data (displayed) | Backend data (source) |
|---|---|---|
| Market detail (/m/[id]) | Header: name, status pill, category, resolve date, thumbnail | `GET /v1/markets/{id}` → `name`, `status`/`closed`, `categories`, `market_end_date_ms`, image URLs |
| Market detail (/m/[id]) | Header stats: total volume, 24h volume, trader count, liquidity | `GET /v1/markets/{id}` → `volume_nanos`, `volume_24h_nanos`, `trader_count`, `liquidity_avg10_nanos` |
| Market detail (/m/[id]) | Header: market age | `created_at_ms` + WS latest block `timestamp_ms` (FE-derived) |
| Market detail (/m/[id]) | Price chart (per-outcome series) | `GET /v1/markets/{id}/prices/history` + WS `clearing_prices_nanos`; `buildChartSeries` merges onto shared time grid |
| Market detail (/m/[id]) | Chart legend (outcomes, colors, current prices) | `GET /v1/markets` (event group) + live store prices; short labels FE-derived |
| Market detail (/m/[id]) | Chart mode (area/stacked/lines) | `GET /v1/events/{id}/raw` (negRisk flag) + `detectStackable` heuristic |
| Market detail (/m/[id]) | Description, resolution criteria, source link | `GET /v1/markets/{id}` (`description`, `resolution_criteria`, `external_url`) + `GET /v1/events/{id}/raw` (preferred for Polymarket: `description`, `resolutionSource`) |
| Market detail (/m/[id]) | Event holdings: user positions (shares, entry→mark, value, P&L) | `GET /v1/accounts/{id}/portfolio` + live marks (WS) + `GET /v1/accounts/{id}/fills` for avg entry (FE-derived) |
| Market detail (/m/[id]) | Event holdings: open orders | `GET /v1/accounts/{id}/orders` |
| Market detail (/m/[id]) | Event holdings: closed orders (avg fill, realized PnL) | `GET /v1/accounts/{id}/events` → reconstructed FE-side from event log |
| Market detail (/m/[id]) | Degen rail: outcome picker + prices | `GET /v1/markets` (group) + live store prices |
| Market detail (/m/[id]) | Degen rail: available balance | `GET /v1/accounts/{id}/portfolio` − reserved (`GET /v1/accounts/{id}/orders`) FE-derived |
| Market detail (/m/[id]) | Degen rail: mark price for bet sizing, expiry block | `GET /v1/markets/{id}/prices/history` last point (fallback live WS) + WS latest height |

---

## Market detail — dev (`/m-dev/[id]`)

| Sybil page | Frontend data (displayed) | Backend data (source) |
|---|---|---|
| Market detail DEV (/m-dev/[id]) | Stats panel: total/24h volume, trader count, liquidity, age | `GET /v1/markets/{id}` (same fields as public) + WS for age |
| Market detail DEV (/m-dev/[id]) | Recent batches panel (placed/matched/volume over 1/5/10/50 windows, avg/batch) | WS `/v1/blocks/ws` `recentBlocks` → `by_market[mid].{placed,matched,volume_nanos}`; `deriveBatchWindowStats` sums FE-side |
| Market detail DEV (/m-dev/[id]) | Debug panel: hydration state, WS connection, latest height, last block ts | Store state (WS-driven) |
| Market detail DEV (/m-dev/[id]) | Open-batch panel: traders, indicative YES, volume, imbalance bps | ⚠️ Mocked (`deriveOpenBatchSnapshot`, seeded by id+height+live YES). Note: a real `GET /v1/markets/{id}/open-batch` exists and is used on Dev › Aggregates. |

---

## Portfolio (`/portfolio`)

| Sybil page | Frontend data (displayed) | Backend data (source) |
|---|---|---|
| Portfolio › Hero | Portfolio value + delta/% over 24H/7D/30D/ALL range | `GET /v1/accounts/{id}/portfolio` (`portfolio_value_nanos`) + `GET /v1/accounts/{id}/equity?range=` for the delta |
| Portfolio › Hero | Positions value + open count, cash | `GET /v1/accounts/{id}/portfolio` → `total_position_value_nanos`, positions length, `balance_nanos` |
| Portfolio › Hero | Unrealized / realized P&L (+ trade count) | `GET /v1/accounts/{id}/portfolio` (`unrealized_pnl_nanos`, `realized_pnl_nanos`); trade count from `GET /v1/accounts/{id}/events` |
| Portfolio › Equity chart | Equity curve (time × value, live tip, crosshair) | `GET /v1/accounts/{id}/equity?range=` → points; live tip appended from WS block + portfolio value |
| Portfolio › Positions | Per-position: thumbnail, name, side, shares, entry/mark ¢, 7d sparkline, value, PnL, resolve date | `GET /v1/accounts/{id}/portfolio` + `GET /v1/accounts/{id}/fills` (entry) + `GET /v1/markets` (names/images/dates) + `GET /v1/markets/{id}/prices/history` (sparkline); PnL FE-computed |
| Portfolio › Orders | Open orders: action/side, placed/filled/remaining qty, limit ¢, avg fill ¢, value, age, TIF, cancel | `GET /v1/accounts/{id}/orders` + `GET /v1/accounts/{id}/fills` (avg fill) + `GET /v1/markets` (names) |
| Portfolio › Trades | Executed fills: action/side, exec price, requested price, welfare edge, value, realized PnL, time | `GET /v1/accounts/{id}/events` (filled/partial_fill) + `GET /v1/markets`; welfare/edge FE-computed |
| Portfolio › History | Full event timeline (created/placed/fill/cancel/expire/reject/deposit/withdraw/resolved + cash impact, block height); filters | `GET /v1/accounts/{id}/events` → `HistoryEventResponse[]` (200, newest-first); self-contained |
| Portfolio (all tabs) | Live refresh as blocks land | WS `/v1/blocks/ws` invalidates the React Query caches above |

**Mutations on this page:** `POST /v1/orders/signed` (place), `POST /v1/orders/cancel/signed` (cancel).

---

## Dev tooling (`/dev/*`)

| Sybil page | Frontend data (displayed) | Backend data (source) |
|---|---|---|
| Dev shell (all /dev) | Connection state, latest height, state-root shorthand | Store (WS `/v1/blocks/ws`) |
| Dev › Overview | Markets cleared/no-clear split, ref-price coverage | `GET /v1/markets/summary` (`useDevMarkets`) |
| Dev › Overview | Pending orders total + markets-with-pending | `GET /v1/orders/pending` |
| Dev › Overview | Recent volume/fills/orders window + block bar chart | `GET /v1/blocks/latest` + `GET /v1/blocks/{height}` backfill + WS `recentBlocks` |
| Dev › Overview | MM reference PnL, position/active-account counts, insights, quick answers | `GET /v1/accounts/{id}/portfolio` (ids 0–47 + pending ids) → `accountAggregates`/`buildInsights`/`buildQuickAnswer` (derive.ts) |
| Dev › Markets | Markets table (id, name, state, ref/yes/no, volume, pending, price gap) | `GET /v1/markets/summary` + `GET /v1/orders/pending`; `filterMarkets`/`priceState`/`priceGap` |
| Dev › Markets | Group filter dropdown | `GET /v1/markets/groups` → `mergeGroupsByName` (FE dedupe) |
| Dev › Accounts | Account chips/selector, scope stats (pending, cash, ref portfolio, ref PnL) | `GET /v1/accounts/{id}/portfolio` (ids 0–47 + pending) → `accountAggregates` |
| Dev › Accounts | Top positions table | `GET /v1/accounts/{id}/portfolio` positions + `GET /v1/markets/summary` index |
| Dev › Accounts | Participants table (cash, portfolio, PnL, positions, pending, recent fills) | `GET /v1/accounts/{id}/portfolio` + `GET /v1/orders/pending` + `GET /v1/accounts/{id}/fills?limit=25` |
| Dev › Accounts | Pending concentration table | `GET /v1/orders/pending` (`pendingIndex`) + `GET /v1/markets/summary` |
| Dev › Aggregates | Platform aggregates (traders, volume, welfare, matched, cancellations — all-time/24h) | `GET /v1/activity/overview` + block-window system events |
| Dev › Aggregates | Per-market table (top 80 by 24h vol: traders, vol, liquidity, placed/matched/unmatched, 24h Δ) | `GET /v1/markets/summary` → `topMarketsByVolume24h` |
| Dev › Aggregates | Latest-block per-market sidecar | `GET /v1/blocks/latest` `by_market` → `latestBlockByMarketRows` |
| Dev › Aggregates | Recent cancellations table | block-window `order_cancelled` system events → `recentCancellations` |
| Dev › Aggregates | Open-batch indicative snapshot (real) | `GET /v1/markets/{id}/open-batch` (`useDevOpenBatch`, on demand) |
| Dev › Aggregates | Cost-basis / portfolio mark panel | `GET /v1/accounts/{id}/portfolio` (`useDevPortfolio`, on demand) |
| Dev › Blocks | Chain blocks list + selected block detail (orders, fills, volume, root/parent/prices JSON, rejections) | `GET /v1/blocks/latest` + `GET /v1/blocks/{height}` + WS `recentBlocks` → `mergeBlocks`; detail uses `clearing_prices_nanos`, `fills`, `rejections` |
| Dev › Bots | Bot decision feed: stats, summaries table, reasoning cards (edge, FV, market price, positions, LLM usage, article links) | `GET /v1/bots/decisions?limit=80` (`useDevBots`; `db_available` flag) |

---

## Caveats

- **Mocked (no real backend yet):** Activity batch-detail TX hash / sequencer /
  clearing-duration; the `/m-dev` open-batch panel (a real
  `/v1/markets/{id}/open-batch` *is* used on Dev › Aggregates).
- **In schema but unused by the FE:** `/v1/markets/search`,
  `/v1/markets/{id}/orderbook`, `/v1/markets/{id}/resolution`,
  `/v1/markets/prices/reference`, `/v1/state-root`, `/v1/proofs/state/*`,
  `/v1/bridge/*`, `/v1/feeds`.
- **Heavily FE-derived, not direct reads:** all PnL / welfare / available-balance
  figures, equity-curve live tip, 24h deltas, chart series, dev-page aggregates
  (`src/lib/dev/derive.ts`).
- **Prod quirk:** `/v1/accounts/{id}/fills` is now **store-first** off the durable
  redb `FILL_HISTORY` table (uncapped) per the source on `main` — but the old
  "returns `[]` in prod" behavior was a recorder-only path, so this still needs a
  live prod curl to confirm the *deployed* binary has the store-first read. See
  the stability section below.

---

## Data stability / survivability

What survives a restart vs. what is capped or rebuilt-from-zero, per backend data
source. **All statuses describe PROD**, where `docker-compose.prod.yml:11` sets
`SYBIL_DATA_DIR=/data` so redb + qMDB persist to the `sybil-data` named volume.
⚠️ In **dev/base** (no `SYBIL_DATA_DIR`) there is no store and every
redb/qmdb-backed row below degrades to 🔴 restart-lost.

**Legend** — 🟢 **Persistent**: survives restart effectively forever (durable on
disk, insert-only or overwrite-in-place, never trimmed). 🟠 **Capped**: durable
while running but older data is evicted after N items/blocks by design (a true
retention cap). 🔴 **Restart-lost**: RAM-only, no disk backing, gone /
rebuilt-from-zero on restart. 🔵 **External**: a separate datastore outside the
engine (arena SQLite on its own volume); sybil-api is a read-through. 🟣
**Mixed**: read the detail — either (a) restart-lost ring + a while-running cap,
or (b) persistent across restart but an inherently rolling time-window that ages
out by design, or (c) one durable half + one volatile half.

**Headline:** most of the data is safe in prod. Balances, positions, cost basis,
**open _and_ closed/cancelled/expired/rejected orders**, fills, the equity curve,
and the full account history all persist forever (durable redb tables, never
trimmed). The genuine restart-loss is: **per-market price-history charts** (🔴 —
the redb table is commented out), the **block-stream ring** (🟣 — only the last
~100 blocks are queryable, lost on restart though chain height survives), and
**raw Polymarket event JSON** (🔴 — the snapshot dir is wiped on every boot).

### Account-scoped (your portfolio data)

| Datapoint | Endpoint(s) | Status | What survives / what's lost (exact caps) |
|---|---|---|---|
| Portfolio: balance, positions, deposited, value, realized/unrealized PnL, cost basis | `GET /v1/accounts/{id}/portfolio` | 🟢 Persistent | No cap. balance/positions = fence-recovered qMDB; cost basis + realized PnL = rewritten redb `cost_basis_tracker` snapshot. value/PnL/unrealized recomputed live from persisted positions × marks (marks reseeded from `CLEARING_PRICES`; missing → 50¢). Survives fully. |
| Open / pending orders | `GET /v1/accounts/{id}/orders` · `/v1/orders/pending` (dev) · `/v1/markets/{id}/orderbook` (dev) | 🟢 Persistent | Full book = single-row redb `RESTING_ORDERS` rewritten each block; between-block admits WAL-logged (`ADMIT_LOG`/`PENDING_BUNDLES`) before the 200 OK. Every acked order survives. Lost: only mempool that never got a 200 OK. Lifecycle limits (not retention): 1000 open/account, TTL 63,072,000 blocks. |
| Account fills | `GET /v1/accounts/{id}/fills` | 🟢 Persistent | Store-first from redb `FILL_HISTORY` (insert-only, **uncapped** full history); RAM recorder is a 5000/account fallback, rehydrated at startup. Survives incl. >5000 lifetime fills. **Caveat**: depends on the deployed binary having the store-first path — verify with a live prod curl (stale memory said `[]`). |
| Account event / history feed (Portfolio History) | `GET /v1/accounts/{id}/events` | 🟢 Persistent | DISK **uncapped** insert-only redb `HISTORY_EVENTS`, served store-first, cursor-paged (page cap: limit 50, max 500). RAM ring = 0 in prod but `append()` still writes the per-block delta, so cap 0 = "served from redb", not lost. Disk grows unbounded. |
| Closed / cancelled / expired / rejected orders | derived from `GET /v1/accounts/{id}/events` | 🟢 Persistent | **NOT dropped after N batches in prod** (your top concern). All lifecycle events land in `HISTORY_EVENTS` (never trimmed). `/v1/accounts/{id}/orders` carries open orders only — closed records live solely in `/events`. Restart-loss + the 5000 cap apply to dev only. (`deploy-reset-state CONFIRM` wipes the volume — intentional, not retention.) |
| Account equity curve | `GET /v1/accounts/{id}/equity` | 🟢 Persistent | DISK redb `EQUITY_POINTS` insert-only, **never trimmed**, served store-first oldest-first. RAM cap 0 in prod (default 43,200) but every sample is written to the per-block delta. Cadence: every trading account + a 60s full sweep ⇒ ≥1 pt/~60s/account. |

### Market-scoped

| Datapoint | Endpoint(s) | Status | What survives / what's lost (exact caps) |
|---|---|---|---|
| Live clearing / current market price | `GET /v1/markets/prices` · `/v1/markets/{id}` · WS `clearing_prices_nanos` | 🟢 Persistent | redb `CLEARING_PRICES`, 1 row/market overwritten each block (no cap). Survives. Never-traded markets → 50¢ default. The WS/SSE `clearing_prices_nanos` *stream* is live-only — historical block messages are not replayed on restart (clients get GET snapshot + forward stream). |
| Market / event metadata (titles, images, dates, categories, event_id, condition_id, group_item_title, closed) | `GET /v1/markets`, `/v1/markets/{id}` | 🟢 Persistent | Doubly durable: in-RAM `market_ref_data` persisted to `/data/market_ref_data.json` + reloaded at startup; no cap, no eviction. 2nd layer: mirror re-POSTs every 120s. Off-block (display-only, not in state_root). |
| On-block market fields: `created_at_ms` (market age), `description`, `resolution_criteria`, `external_url` | `GET /v1/markets/{id}` | 🟢 Persistent | On-block `MarketMetadata` in the redb `MARKETS` table — part of `state_root`, restored at startup. Survives. (FE may prefer the Polymarket raw JSON for description/source — see that row.) |
| Markets list: existence, `volume_nanos`, `trader_count` | `GET /v1/markets`, `/v1/markets/summary` | 🟢 Persistent | All cumulative/forever, snapshotted to redb every block + restored. `trader_count` = all-time **unbounded** HashSet (memory-growth vector, not a cap). The FE `RestartCaveatBadge` comments (markets.rs:31-48) are **stale**. |
| Market groups | `GET /v1/markets/groups` (dev) | 🟢 Persistent | redb `MARKET_GROUPS`, written in the block sidecar + folded into `state_root`, restored at startup. (Distinct from the off-block `event_id` grouping the FE uses for cards.) |
| Markets list: `volume_24h_nanos`, `liquidity_avg10_nanos` | `GET /v1/markets`, `/v1/markets/summary` | 🟣 Mixed | **Persistent across restart**, but inherently **rolling**: `volume_24h` = ≤25 hourly buckets; `liquidity_avg10` = last-10-block ring (and is actually a *sum*, not an avg). Ages out by design while running, not restart loss. |
| Per-market price history (charts / sparklines) | `GET /v1/markets/{id}/prices/history` | 🟣 Mixed | **In-RAM only** (redb `PRICE_HISTORY` table is commented out, `store.rs:316`) → **restart-lost**: returns `[]` after restart and rebuilds one point per price-moving block. While-running cap = **2000 points/market**. `SYBIL_DATA_DIR` does NOT save this series. |

### Platform / activity-scoped

| Datapoint | Endpoint(s) | Status | What survives / what's lost (exact caps) |
|---|---|---|---|
| Activity overview — all-time volume / welfare / traders / orders | `GET /v1/activity/overview` (`all_time.*`) | 🟢 Persistent | Uncapped/forever: unbounded trader HashSet, i64 welfare sum, u64 counters; tracker snapshots written to redb every block + restored. **Concern**: trader sets serialized in full every block (memory/IO growth). |
| Activity overview — last-24h | `GET /v1/activity/overview` (`last_24h.*`) | 🟣 Mixed | **Persistent across restart**, inherently **rolling** ≤25 hourly buckets/tracker (cap 25 each), summed over `[now-24h, now]`. Reads its own persisted buckets, not the block ring. |
| Event trader count | `GET /v1/events/{id}/traders` | 🟢 Persistent | All-time **unbounded** union of per-market placer sets (the 25-bucket cap is only the 24h platform count); redb-backed + restored. Correct immediately after restart. |
| Raw Polymarket event JSON | `GET /v1/events/{id}/raw` | 🔴 Restart-lost | Files on the persistent volume, but the `event_snapshots` dir is `remove_dir_all`'d + recreated on **every startup** (`main.rs:127-142`) → 404 for up to ~2 min until the mirror re-syncs (120s). No cap, ~zero cross-restart retention. |
| Block stream / batches / heights | `GET /v1/blocks*`, `/v1/blocks/ws` | 🟣 Mixed | In-RAM ring, **cap 100** (~16.7 min @ 10s blocks), FIFO. **Restart-lost**: after restart `/v1/blocks=[]`, `/latest`+`/{height}` 404, WS replay fails until ~100 new blocks accrue. redb keeps only the latest header (hash-chaining, never served). Chain **height** IS preserved → the "Total batches" count resumes, does not zero out. (Project-memory "10 deep" note is stale; it's 100.) |
| Open-batch indicative snapshot | `GET /v1/markets/{id}/open-batch` (dev) | 🔴 Restart-lost | In-memory intra-block placer state; resets to the fresh open batch on restart. Loss is inherent (the in-flight, not-yet-sealed batch) — acceptable. |

### External

| Datapoint | Endpoint(s) | Status | What survives / what's lost (exact caps) |
|---|---|---|---|
| Bot decision feed | `GET /v1/bots/decisions` | 🔵 External | SQLite `decisions.db` on the dedicated `arena-data` volume, written by sybil-arena, read-only per request by the API. Survives restart of both services (writer is insert-only — no DROP/DELETE/VACUUM). **Uncapped** (grows forever); only a read-time page limit (default 50, max 200). `db_available=false` is a liveness probe (still HTTP 200), not data loss. |

### Backend fixes (prioritized to-improve list)

1. **🔴 high — Persist the price-history chart series.** It's in-RAM only (the
   `PRICE_HISTORY` redb table is commented out at `store.rs:316`), so charts
   rebuild from zero on every restart — your explicit worry. Re-enable the table:
   write the per-block `PricePoint` delta (insert-only, `market_id||height`) in
   `save_block` and load it on restore (or serve store-first like equity/fills).
   Converts 🟣 → 🟢.
2. **🟣 high — Persist the block stream.** Only a 100-deep RAM ring exists; after
   restart `/v1/blocks` is empty and WS replay fails for ~16.7 min. Add a redb
   `BLOCKS` table keyed by height + a store fallback in
   `GetBlock`/`GetRecentBlocks`/`GetLatestBlock` and the WS replay path. (Chain
   height already survives and the ring self-heals in minutes, so below charts.)
3. **🔴 medium — Stop wiping `event_snapshots` on startup.** `main.rs:127-142`
   `remove_dir_all`s raw event JSON on every boot, causing ~2 min of 404s. Drop
   the wipe (keep ensure-exists) so files on the persistent volume survive, or
   shorten the mirror sync interval. Makes the raw-JSON half 🟢.
4. **🟢 medium — Verify the deployed fills binary.** `/fills` persistence assumes
   the prod binary has the store-first read (`actor.rs:1513-1531`); stale memory
   recorded `[]` in prod. Curl prod `/v1/accounts/{id}/fills` for an account with
   >5000 lifetime fills after a restart; redeploy current `main` if empty. Ops
   check, not a code fix.
5. **🟢 low — Offload all-time trader sets from per-block blobs.** Not data loss,
   but `trader_tracker` HashSets grow unbounded and are serialized in full to redb
   every block (memory + write amplification — matches the known off-block
   aggregate leak). Move to incremental per-account / per-(market,account) redb
   rows with an O(1) RAM cardinality counter.
6. **🟣 low — Clean up the rolling-window rows.** Delete the stale
   `RestartCaveatBadge` comments at `markets.rs:31-48` (persistence is wired),
   relabel the 24h/liquidity rows as "persistent (rolling window)" so 🟣 isn't read
   as restart-lost, and fix the `liquidity_avg10` name (it's a *sum* of the last
   10 block depths, not an average).
7. **🔴 low — Open-batch snapshot.** Restart-lost by nature (the in-flight batch);
   no persistence needed. If continuity is ever wanted, reconstruct from the
   already-durable `ADMIT_LOG`/`PENDING_BUNDLES` WAL.

> All "persistent" rows hinge on prod keeping `SYBIL_DATA_DIR=/data`. The
> `frontend/CLAUDE.md` note claiming prod runs `SYBIL_DATA_DIR=""` (in-memory) is
> **stale and wrong** — if trusted it would flip every 🟢 account/market row to
> 🔴. Recommend correcting that note.

---

## Write / mutation endpoints (reference)

| Endpoint | Used by | Purpose |
|---|---|---|
| `POST /v1/accounts` | connect / create demo account | Create account |
| `POST /v1/accounts/{id}/keys` | connect flow | Register signer pubkey |
| `POST /v1/accounts/{id}/fund` | funding | Fund account |
| `POST /v1/orders/signed` | Portfolio, trade rail | Place signed order |
| `POST /v1/orders/cancel/signed` | Portfolio, trade rail | Cancel open order |
</content>
</invoke>
