# Polymarket Link + Start Dates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the FE a stable per-market link to the Polymarket event JSON (`condition_id`) plus event/market start dates, so the markets page can show real outcome labels (#3) and a correct "New" badge/sort (#4) — and read any future Polymarket field from the JSON without more backend changes.

**Architecture:** Three small off-block fields ride the *existing* mirror → `MarketRefData` → `MarketResponse` pipeline (the same path `*_end_date_ms` already uses). `condition_id` lets the FE join `/v1/markets` rows to `/v1/events/{id}/raw` (store 3) for per-market fields like `groupItemTitle`; the two date scalars sit on the list so #4 can sort the whole grid without fetching every event JSON. A one-time mirror-startup metadata refresh backfills existing markets so no volume is wiped.

**Tech Stack:** Rust (3 crates: `sybil-polymarket`, `sybil-api`, `sybil-api-types`), Next.js 16 / React 19 FE, Docker (single `sybil-api:latest` image, all services), colima+Rosetta off-box build, prod Linode via `docker compose`.

**Branch & ordering strategy (important):**
- FE-only fixes already done this session (#1 batch-pill jitter, #2 "every 10s", #5 "±0¢") are **uncommitted on `r/dev`** → push first (Phase 0).
- Backend changes go on a **branch off `main`** (backend trunk; never on `r/dev`), then merge to `main` and deploy (Phases 1–2).
- FE wiring for #3/#4 needs the regenerated schema from the **deployed** API, so it comes **after** deploy, on `r/dev` (Phase 3).

---

## File structure

**Phase 0 (FE push):** no new files — commit existing edits.

**Phase 1 (backend, branch off `main`):**
- `crates/sybil-polymarket/src/polymarket/types.rs` — add `start_date` to `GammaMarket` (event already has it).
- `crates/sybil-api-types/src/request.rs` — add 3 fields to `SetMarketMetadataRequest`.
- `crates/sybil-api-types/src/response.rs:26` — add 3 fields to `MarketResponse`.
- `crates/sybil-polymarket/src/sync.rs` — set fields in `build_metadata_request` (~334) + one-time startup metadata backfill.
- `crates/sybil-api/src/state.rs:22` — add 3 fields to `MarketRefData`.
- `crates/sybil-api/src/routes/markets.rs` — metadata POST handler writes them; response merge reads them.

**Phase 2 (deploy):** `Dockerfile`, `/opt/sybil/docker-compose*.yml` (prod) — no edits, just build/ship.

**Phase 3 (FE, on `r/dev`):**
- `frontend/web/src/lib/api/schema.d.ts` — regenerated (not hand-edited).
- `frontend/web/src/lib/markets/use-event-raw.ts` — **new** hook: fetch + cache `/v1/events/{id}/raw`, expose `conditionId → json market` map.
- `frontend/web/src/components/multi-card.tsx` — labels via `groupItemTitle` (#3).
- `frontend/web/src/app/page.tsx` + `frontend/web/src/lib/markets/use-markets.ts` — "New" sort/badge from start dates (#4).
- `frontend/web/src/components/binary-card.tsx` / `multi-card.tsx` — render the "New" badge in the eyebrow.

---

## Phase 0 — Push the FE fixes already made

### Task 0.1: Commit + push #1/#2/#5 to `r/dev`

**Files:** already edited — `batch-pill.tsx`, `app/page.tsx`, `lib/format/nanos.ts`, `multi-card.tsx`, `binary-card.tsx`.

- [ ] **Step 1: Confirm we're on `r/dev` and review the diff**
```bash
cd /Users/r/pr/Sybil
git branch --show-current   # expect r/dev
git status --short          # expect the 5 files above, all FE
git diff --stat
```

- [ ] **Step 2: Verify FE still green**
```bash
cd /Users/r/pr/Sybil/frontend/web && pnpm exec tsc --noEmit && pnpm exec eslint src
```
Expected: no output, exit 0.

- [ ] **Step 3: Commit + push**
```bash
cd /Users/r/pr/Sybil
git add frontend/web/src
git commit -m "$(cat <<'EOF'
fe: static batch-pill width, 10s clearing copy, ±0¢ flat delta

- batch-pill: reserve fixed countdown width (no jitter on 9.9→10.0 reset)
- markets page: derive "uniform clearing every Ns" from BLOCK_INTERVAL_MS
- delta cells: flat renders ±0¢ (grey); no-data — keeps a tooltip

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git push origin r/dev
```

---

## Phase 1 — Backend fields (branch off `main`)

### Task 1.1: Start the backend branch from latest `main`

- [ ] **Step 1: Fetch + branch (do NOT base on r/dev)**
```bash
cd /Users/r/pr/Sybil
git stash list   # ensure nothing important stashed
git fetch origin
git switch -c r/poly-link-startdates origin/main
```
Expected: new branch tracking `origin/main` (so MetaB0y's trunk is the base).

### Task 1.2: Add `start_date` to `GammaMarket`

**Files:** Modify `crates/sybil-polymarket/src/polymarket/types.rs` (struct at line 206; `end_date` is the template at ~242).

- [ ] **Step 1: Add the field** next to `end_date`:
```rust
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
```
(`GammaEvent.start_date` already exists at types.rs:127 — no change there.)

- [ ] **Step 2: Compile the crate**
```bash
cargo build -p sybil-polymarket
```
Expected: builds clean.

### Task 1.3: Add the 3 fields to `SetMarketMetadataRequest`

**Files:** Modify `crates/sybil-api-types/src/request.rs` (the `SetMarketMetadataRequest` struct).

- [ ] **Step 1: Add fields** (mirror the existing `event_end_date_ms: Option<u64>` style; trailing, defaulted):
```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polymarket_condition_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_start_date_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_start_date_ms: Option<u64>,
```

- [ ] **Step 2: Compile**
```bash
cargo build -p sybil-api-types
```

### Task 1.4: Populate the fields in the mirror (`build_metadata_request`)

**Files:** Modify `crates/sybil-polymarket/src/sync.rs` (`build_metadata_request`, ~334; `parse_iso8601_to_ms` lives in `types.rs` and is already imported and used for `end_date`).

- [ ] **Step 1: Parse the start dates** alongside the existing end-date parsing:
```rust
    let event_start_date_ms = event
        .start_date
        .as_deref()
        .and_then(parse_iso8601_to_ms)
        .and_then(|ms| u64::try_from(ms).ok());
    let market_start_date_ms = market
        .start_date
        .as_deref()
        .and_then(parse_iso8601_to_ms)
        .and_then(|ms| u64::try_from(ms).ok());
```

- [ ] **Step 2: Set them on the returned struct** (add to the `SetMarketMetadataRequest { .. }` literal):
```rust
        polymarket_condition_id: Some(market.condition_id.clone()),
        event_start_date_ms,
        market_start_date_ms,
```

- [ ] **Step 3: Compile**
```bash
cargo build -p sybil-polymarket
```

### Task 1.5: One-time startup metadata backfill in the mirror

**Why:** existing events are skipped by `is_event_synced` (sync.rs:113), so their `MarketRefData` would never receive the new fields. Re-POST metadata for all *already-mapped* markets once per process start. This avoids wiping `market_ref_data.json`.

**Files:** Modify `crates/sybil-polymarket/src/sync.rs` (the `SyncActor` struct + `sync_once`).

- [ ] **Step 1: Add a one-shot flag** to `SyncActor` (default `true` where it's constructed):
```rust
    /// Re-push metadata for all mapped markets on the first cycle after start,
    /// so schema additions backfill onto existing markets without a wipe.
    first_sync: bool,
```

- [ ] **Step 2: Add the backfill pass** in `sync_once`, after the event-JSON push loop (~line 108) and before the per-event creation loop. Collect under the lock, POST after dropping it:
```rust
    if self.first_sync {
        let refresh: Vec<(u32, SetMarketMetadataRequest)> = {
            let map = self.mapping.read().await;
            events
                .iter()
                .flat_map(|event| {
                    event
                        .markets
                        .iter()
                        .filter(|m| m.active && !m.closed)
                        .filter_map(|m| {
                            map.sybil_market_id(&m.condition_id)
                                .map(|sid| (sid, build_metadata_request(event, m)))
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        info!(count = refresh.len(), "backfilling market metadata (one-time)");
        for (sid, req) in refresh {
            if let Err(e) = self.sybil_client.set_market_metadata(sid, &req).await {
                warn!(sybil_id = sid, error = %e, "metadata backfill failed (will not retry)");
            }
        }
        self.first_sync = false;
    }
```

- [ ] **Step 3: Compile**
```bash
cargo build -p sybil-polymarket
```
Expected: clean. (If `SetMarketMetadataRequest` isn't already in scope in sync.rs, add the `use` — it's the return type of `build_metadata_request`, so it already is.)

### Task 1.6: Add the 3 fields to `MarketRefData` + the API write/merge

**Files:** Modify `crates/sybil-api/src/state.rs` (struct at line 22) and `crates/sybil-api/src/routes/markets.rs` (metadata POST handler + the response builder that already maps `event_end_date_ms`).

- [ ] **Step 1: `MarketRefData`** — add trailing, defaulted fields:
```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polymarket_condition_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_start_date_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_start_date_ms: Option<u64>,
```

- [ ] **Step 2: Metadata POST handler** (markets.rs) — copy from request into the entry, mirroring the existing `if let Some(v) = req.event_end_date_ms { entry.event_end_date_ms = Some(v); }` pattern:
```rust
    if let Some(v) = req.polymarket_condition_id { entry.polymarket_condition_id = Some(v); }
    if let Some(v) = req.event_start_date_ms { entry.event_start_date_ms = Some(v); }
    if let Some(v) = req.market_start_date_ms { entry.market_start_date_ms = Some(v); }
```

- [ ] **Step 3: Response merge** (markets.rs, where `MarketResponse` is built from `ref_data`) — add, mirroring `event_end_date_ms: args.ref_data.and_then(|r| r.event_end_date_ms)`:
```rust
    polymarket_condition_id: args.ref_data.and_then(|r| r.polymarket_condition_id.clone()),
    event_start_date_ms: args.ref_data.and_then(|r| r.event_start_date_ms),
    market_start_date_ms: args.ref_data.and_then(|r| r.market_start_date_ms),
```

### Task 1.7: Add the 3 fields to `MarketResponse`

**Files:** Modify `crates/sybil-api-types/src/response.rs` (struct at line 26).

- [ ] **Step 1: Add fields** (trailing; same `#[serde(default, skip_serializing_if = "Option::is_none")]` + doc-comment style as the other off-block fields, so `utoipa` emits them into `/openapi.json`):
```rust
    /// Polymarket on-chain condition id — the FE join key into
    /// `GET /v1/events/{event_id}/raw` `markets[].conditionId`. Off-block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polymarket_condition_id: Option<String>,
    /// Parent event start date (epoch ms) from Polymarket. Display/sort only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_start_date_ms: Option<u64>,
    /// Per-market start date (epoch ms) from Polymarket. Display/sort only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_start_date_ms: Option<u64>,
```

- [ ] **Step 2: Workspace build + smoke**
```bash
cargo build --workspace
cargo test -p sybil-api-types -p sybil-api 2>&1 | tail -20   # if tests exist
```
Expected: builds clean.

### Task 1.8: Merge to `main`

- [ ] **Step 1: Commit**
```bash
cd /Users/r/pr/Sybil
git add crates/
git commit -m "$(cat <<'EOF'
mirror: expose polymarket_condition_id + event/market start dates

Off-block fields on MarketRefData → MarketResponse so the FE can join
/v1/markets to /v1/events/{id}/raw (condition_id) and sort/badge "New"
from real start dates. One-time mirror-startup backfill repopulates
existing markets (no volume wipe).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 2: Rebase onto latest main (don't clobber MetaB0y) + push**
```bash
git fetch origin
git rebase origin/main         # resolve if MetaB0y pushed meanwhile
git switch main && git pull --ff-only origin main
git merge --ff-only r/poly-link-startdates
git push origin main
```

---

## Phase 2 — Build the image off-box + deploy to prod

> Build on the Mac via colima+Rosetta (building on the 1-vCPU Linode OOMs). **Non-destructive: do NOT wipe `sybil.redb`, `sybil.qmdb`, `market_ref_data.json`, or `polymarket_mapping.json`** — the Task 1.5 backfill repopulates the new fields.

### Task 2.1: Build `sybil-api:latest` for linux/amd64

- [ ] **Step 1: Ensure colima is up with Rosetta**
```bash
colima status || colima start --vm-type vz --vz-rosetta --cpu 6 --memory 10 --disk 60
```

- [ ] **Step 2: Build (tag with the merge SHA for rollback clarity)**
```bash
cd /Users/r/pr/Sybil
SHA=$(git rev-parse --short HEAD)
docker build --platform linux/amd64 -t sybil-api:latest -t "sybil-api:$SHA" .
```
Expected: image built; `docker images sybil-api` shows `latest` + `$SHA`. (~20 min cold.)

### Task 2.2: Ship the image to prod

- [ ] **Step 1: Snapshot the current prod image as rollback, then load the new one**
```bash
ssh root@172.104.31.54 'docker tag sybil-api:latest sybil-api:rollback-$(date +%Y%m%d-%H%M) || true'
docker save sybil-api:latest | gzip | ssh root@172.104.31.54 'gunzip | docker load'
```
Expected: `Loaded image: sybil-api:latest` on prod.

### Task 2.3: Recreate the two services (reload state from disk)

- [ ] **Step 1: Bring up api + mirror with ALL three overlays** (omitting an overlay drops `SYBIL_DATA_DIR` → in-memory):
```bash
ssh root@172.104.31.54 'cd /opt/sybil && docker compose \
  -f docker-compose.yml -f docker-compose.prod.yml -f docker-compose.telegram.yml \
  up -d sybil-api sybil-polymarket'
```
Expected: both recreated; healthy in ~50s.

- [ ] **Step 2: Confirm health + height resumed (not 0)**
```bash
curl -s https://172-104-31-54.nip.io/v1/health   # {"status":"ok","height":>0}
```

### Task 2.4: Verify the new fields land (after one mirror cycle, ≤2 min)

- [ ] **Step 1: Check a market carries the new fields**
```bash
curl -s https://172-104-31-54.nip.io/v1/markets | python3 -c "import sys,json;ms=json.load(sys.stdin);m=ms[0];print({k:m.get(k) for k in ['market_id','polymarket_condition_id','event_start_date_ms','market_start_date_ms']})"
```
Expected: `polymarket_condition_id` is a `0x…` hex; both dates are populated (epoch ms). If still null, wait one sync cycle (the backfill runs on the mirror's first post-restart cycle) and re-check.

- [ ] **Step 2: Confirm the join works end-to-end**
```bash
EID=$(curl -s https://172-104-31-54.nip.io/v1/markets | python3 -c "import sys,json;print(json.load(sys.stdin)[0]['event_id'])")
curl -s "https://172-104-31-54.nip.io/v1/events/$EID/raw" | python3 -c "import sys,json;ms=json.load(sys.stdin)['markets'];print('conditionIds in JSON:',[m['conditionId'][:14] for m in ms[:3]])"
```
Expected: the market's `polymarket_condition_id` appears among the JSON `conditionId`s.

---

## Phase 3 — FE wiring (on `r/dev`, after deploy)

### Task 3.1: Absorb schema + regenerate types

- [ ] **Step 1: Update r/dev and regen types from the live API**
```bash
cd /Users/r/pr/Sybil
git switch r/dev && git pull origin main          # absorb backend change + any MetaB0y work
cd frontend/web && pnpm types:generate            # hits prod /openapi.json; patch-bigints runs after
git diff --stat src/lib/api/schema.d.ts           # expect the 3 new MarketResponse fields
```
Expected: `polymarket_condition_id`, `event_start_date_ms`, `market_start_date_ms` present on `MarketResponse`.

### Task 3.2: #4 — "New" sort + badge from start dates

**Files:** `frontend/web/src/app/page.tsx` (`maxCreated` → newness), the card eyebrows in `binary-card.tsx` / `multi-card.tsx`.

- [ ] **Step 1: Add a newness helper** in `page.tsx` (replace `maxCreated` usage). Newest of event-start / market-start, falling back to `created_at_ms`:
```ts
const NEW_WINDOW_MS = 7 * 24 * 60 * 60 * 1000; // 7d

function marketNewnessMs(m: Market): number {
  return Math.max(
    m.event_start_date_ms ?? 0,
    m.market_start_date_ms ?? 0,
    m.created_at_ms ?? 0, // fallback only
  );
}
function eventNewnessMs(ms: Market[]): number {
  return ms.reduce((acc, m) => Math.max(acc, marketNewnessMs(m)), 0);
}
```
Use `eventNewnessMs(g.markets)` for multi `createdMs` and `marketNewnessMs(m)` for binary. (Sort already does `b.createdMs - a.createdMs`, so it just gets correct values.)

- [ ] **Step 2: Pass an `isNew` flag to the cards** (compute `Date.now() - newness < NEW_WINDOW_MS`) and render a small badge in the eyebrow row of `BinaryCard`/`MultiCard` (reuse the existing eyebrow `text-mono` styling; a `var(--accent)` pill reading `NEW`).

- [ ] **Step 3: Verify**
```bash
cd /Users/r/pr/Sybil/frontend/web && pnpm exec tsc --noEmit && pnpm exec eslint src
```
Then in the browser: sort=New surfaces recently-started events; older bulk-mirrored markets no longer all read as new.

### Task 3.3: #3 — multi-card outcome labels from `groupItemTitle`

**Files:** new `frontend/web/src/lib/markets/use-event-raw.ts`; modify `frontend/web/src/components/multi-card.tsx`.

- [ ] **Step 1: New hook** — fetch + cache the event JSON, expose a `conditionId → json market` lookup (untyped; the `/raw` endpoint has no OpenAPI schema). Lazy via an `enabled` flag so only in-view multi-cards fetch:
```ts
import { useQuery } from "@tanstack/react-query";

type RawMarket = { conditionId?: string; groupItemTitle?: string; startDate?: string };

export function useEventRaw(eventId: string | undefined, enabled: boolean) {
  return useQuery({
    queryKey: ["event-raw", eventId],
    enabled: enabled && !!eventId,
    staleTime: 30 * 60_000,
    queryFn: async (): Promise<Map<string, RawMarket>> => {
      const base = process.env.NEXT_PUBLIC_API_BASE!;
      const res = await fetch(`${base}/v1/events/${eventId}/raw`);
      if (!res.ok) return new Map();
      const ev = await res.json();
      const map = new Map<string, RawMarket>();
      for (const m of ev.markets ?? []) if (m.conditionId) map.set(m.conditionId, m);
      return map;
    },
  });
}
```

- [ ] **Step 2: Use it in `MultiCard`** — gate on `inView` (already computed). Label each outcome by its `groupItemTitle`, falling back to the current `trimOutcomeLabel(name)`:
```ts
const rawQ = useEventRaw(markets[0]?.event_id ?? undefined, inView);
const labelFor = (m: Market) =>
  (m.polymarket_condition_id && rawQ.data?.get(m.polymarket_condition_id)?.groupItemTitle)
  || trimOutcomeLabel(m.name);
```
Replace `trimOutcomeLabel(leader.name)` / `trimOutcomeLabel(market.name)` in `FeaturedOutcome` and `SecondaryRow` with `labelFor(...)`. (NegRisk events keep working via the fallback; this fixes non-NegRisk events like Bitcoin → `"↑ 200,000"`.)

- [ ] **Step 3: Verify**
```bash
cd /Users/r/pr/Sybil/frontend/web && pnpm exec tsc --noEmit && pnpm exec eslint src
```
Browser: a non-NegRisk multi event (e.g. "What price will Bitcoin hit in 2026?") shows short outcome labels, not full questions.

### Task 3.4: Commit + push FE wiring

- [ ] **Step 1:**
```bash
cd /Users/r/pr/Sybil
git add frontend/web/src
git commit -m "$(cat <<'EOF'
fe: real outcome labels (#3) + start-date New badge/sort (#4)

- multi-card: join /v1/events/{id}/raw by polymarket_condition_id for
  groupItemTitle labels (lazy, cached), fallback to trimmed name
- markets page: "New" sort/badge from event/market start dates

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git push origin r/dev
```

---

## Rollback

If the deploy misbehaves:
```bash
ssh root@172.104.31.54 'cd /opt/sybil && docker tag sybil-api:rollback-<stamp> sybil-api:latest && \
  docker compose -f docker-compose.yml -f docker-compose.prod.yml -f docker-compose.telegram.yml up -d sybil-api sybil-polymarket'
```
State is untouched (no wipes), so rollback is just re-tagging the previous image and recreating.

## Optional optimization (not in scope unless you say so)

If #3's per-card JSON fetch feels heavy, add a 4th field `group_item_title` to the same pipeline (it's already in scope at `build_metadata_request` as `market.group_item_title`). Then `MultiCard` reads it directly from `MarketResponse` and Task 3.3's hook/fetch disappears. Costs one extra field; saves ~8–12 JSON fetches per index page.

## Self-review notes
- Spec coverage: #1/#2/#5 (Phase 0), #3 (Task 3.3), #4 (Task 3.2), build (2.1), deploy (2.2–2.3), FE push (0.1 + 3.4). ✓
- Persistence: all new persisted-struct fields are trailing + `#[serde(default)]` (old `market_ref_data.json` loads, new keys default to `None`, backfill fills them). ✓
- Non-destructive deploy + one-time backfill avoids the duplicate-market trap from wiping the mapping. ✓
- Type consistency: field names identical across `SetMarketMetadataRequest` / `MarketRefData` / `MarketResponse` (`polymarket_condition_id`, `event_start_date_ms`, `market_start_date_ms`). ✓
