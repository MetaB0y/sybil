# Hide Closed Polymarket Markets/Events Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Closed Polymarket events/markets (e.g. "Eurovision Winner 2026") stop appearing on the markets index; partially-closed events show their resolved outcomes greyed; a closed market's detail page renders read-only.

**Architecture:** The off-block `MarketRefData.closed` flag already exists end-to-end but is only written by the mirror's one-time first-sync backfill. The resolution actor already fetches closed Polymarket events every tick and matches them to mapped markets — we add a *mark-once* write of `closed: true` there so closing self-heals in steady state (no restart needed). The frontend stops hard-dropping closed markets from the shared bundle; instead it keeps them (so detail pages and multi-cards can read them) and applies visibility only at the index layer.

**Tech Stack:** Rust (sybil-polymarket actor + sybil-api-types), TypeScript/React (Next.js 16 frontend), vitest.

**Workflow (per user):** All changes — backend *and* frontend — land on the `r/dev` branch. Backend is deployed from `r/dev` source via the off-box build (rsync → `docker build` on prod), then a PR is opened to `main`. This deviates from `frontend/CLAUDE.md` ("don't put backend on r/dev") by explicit user instruction.

**Key facts verified during planning:**
- Frontend filter today: `frontend/web/src/lib/markets/use-markets.ts:62` — `allMarkets.filter((m) => m.closed !== true)`.
- Backend write today: only `crates/sybil-polymarket/src/sync.rs:119-160` (first-sync). `resolution.rs` never writes `closed`.
- `SetMarketMetadataRequest` derives `Default` and is all-`Option` (`crates/sybil-api-types/src/request.rs:319-382`), so `SetMarketMetadataRequest { closed: Some(true), ..Default::default() }` is valid.
- The metadata handler merges field-by-field; `None` fields are left untouched (`crates/sybil-api/src/routes/markets.rs:847-896`).
- `fetch_closed_events` returns closed events ordered by `endDate` desc (`crates/sybil-polymarket/src/polymarket/gamma.rs:184-190`) → recently-closed events are in-window.
- `useMarket` returns the full `MarketResponse` incl. `closed` (`frontend/web/src/lib/markets/use-market.ts`).
- Test runner: `vitest run` (script `test` in `frontend/web/package.json`). Tests colocate as `*.test.ts`.

---

## File Structure

**Backend (Rust):**
- Modify: `crates/sybil-polymarket/src/resolution.rs` — add `flagged_closed` set + `pending_close_flags` pure helper + tick wiring + unit test.

**Frontend (TS/React):**
- Modify: `frontend/web/src/lib/markets/use-markets.ts` — stop filtering closed; add visibility helpers.
- Create: `frontend/web/src/lib/markets/use-markets.test.ts` — unit tests for the new pure helpers + `assemble`.
- Modify: `frontend/web/src/app/page.tsx` — apply index visibility, fix counts, ticker uses open-only map.
- Modify: `frontend/web/src/components/multi-card.tsx` — grey closed outcome rows, sort closed last.
- Modify: `frontend/web/src/lib/market-detail/use-event-group.ts` — add `closed` to `EventOutcome`, sort closed last.
- Modify: `frontend/web/src/app/m/[id]/page.tsx` — closed banner keyed on `market.closed`.
- Modify: `frontend/web/src/components/market-rail/index.tsx` — read-only rail when the selected outcome is closed.

---

## Phase A — Backend: mark-once close flag (on `r/dev`)

### Task A1: Pure helper `pending_close_flags` + unit test

**Files:**
- Modify: `crates/sybil-polymarket/src/resolution.rs`

- [ ] **Step 1: Add the failing unit test**

Append to the end of `crates/sybil-polymarket/src/resolution.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::pending_close_flags;
    use crate::polymarket::types::{GammaEvent, GammaMarket};
    use std::collections::{HashMap, HashSet};

    fn market(condition_id: &str, closed: bool) -> GammaMarket {
        GammaMarket {
            condition_id: condition_id.into(),
            question: "Q?".into(),
            outcomes: String::new(),
            outcome_prices: String::new(),
            clob_token_ids: String::new(),
            active: !closed,
            closed,
            neg_risk: false,
            group_item_title: None,
            best_bid: None,
            best_ask: None,
            last_trade_price: None,
            volume: None,
            liquidity: None,
            slug: None,
            description: None,
            start_date: None,
            end_date: None,
            resolution_source: None,
            image: None,
            icon: None,
            umared: None,
            resolved_by: None,
            extra: Default::default(),
        }
    }

    fn event(markets: Vec<GammaMarket>) -> GammaEvent {
        GammaEvent {
            id: "e1".into(),
            title: "T".into(),
            description: String::new(),
            slug: String::new(),
            active: false,
            closed: true,
            enable_neg_risk: false,
            neg_risk: false,
            markets,
            tags: Vec::new(),
            volume: None,
            liquidity: None,
            start_date: None,
            end_date: None,
            created_at: None,
            image: None,
            icon: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn flags_mapped_closed_markets_once() {
        let events = vec![event(vec![
            market("0xaaa", true),  // mapped + closed -> flag
            market("0xbbb", false), // mapped but still open -> skip
            market("0xccc", true),  // closed but NOT mapped -> skip
        ])];
        let mut mirrors = HashMap::new();
        mirrors.insert("0xaaa".to_string(), 10u32);
        mirrors.insert("0xbbb".to_string(), 11u32);
        let already = HashSet::new();

        let out = pending_close_flags(&events, &mirrors, &already);
        assert_eq!(out, vec![(10u32, "0xaaa".to_string())]);

        // Once 0xaaa is flagged, it is not returned again.
        let already: HashSet<String> = ["0xaaa".to_string()].into_iter().collect();
        let out = pending_close_flags(&events, &mirrors, &already);
        assert!(out.is_empty());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p sybil-polymarket pending_close_flags`
Expected: FAIL — `cannot find function pending_close_flags in this scope` / module `pending_close_flags` not found.

- [ ] **Step 3: Implement the pure helper**

Add this free function in `crates/sybil-polymarket/src/resolution.rs`, immediately after the `impl ResolutionActor { ... }` block (before the `#[cfg(test)]` module):

```rust
/// Pure decision: which mapped markets in `events` still need their off-block
/// `closed` flag written. A market qualifies when Polymarket reports it
/// `closed`, it is in the `mirrors` map (condition_id -> sybil_market_id), and
/// it has not already been flagged this process lifetime. Returns
/// `(sybil_market_id, condition_id)` pairs. No I/O — unit-tested in isolation.
fn pending_close_flags(
    events: &[GammaEvent],
    mirrors: &std::collections::HashMap<String, u32>,
    already_flagged: &std::collections::HashSet<String>,
) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for event in events {
        for market in &event.markets {
            if !market.closed {
                continue;
            }
            let Some(&sybil_id) = mirrors.get(&market.condition_id) else {
                continue;
            };
            if already_flagged.contains(&market.condition_id) {
                continue;
            }
            out.push((sybil_id, market.condition_id.clone()));
        }
    }
    out
}
```

Also add `GammaEvent` to the existing types import at the top of the file. Change:

```rust
use crate::polymarket::types::GammaMarket;
```

to:

```rust
use crate::polymarket::types::{GammaEvent, GammaMarket};
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p sybil-polymarket pending_close_flags`
Expected: PASS (1 test).

- [ ] **Step 5: Commit**

```bash
git add crates/sybil-polymarket/src/resolution.rs
git commit -m "feat(mirror): pending_close_flags helper for off-block close marking

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task A2: Wire mark-once close flagging into `tick()`

**Files:**
- Modify: `crates/sybil-polymarket/src/resolution.rs`

- [ ] **Step 1: Add the `flagged_closed` field to the actor struct**

In the `pub struct ResolutionActor { ... }` definition, add a field:

```rust
    signer: ResolutionSigner,
    /// Condition ids already pushed `closed: true` this process lifetime, so we
    /// write the off-block flag once per market instead of every tick. In-memory
    /// only — a restart re-flags each once via this path (and the sync actor's
    /// first-sync backfill), which is harmless and idempotent on the API side.
    flagged_closed: std::sync::Mutex<std::collections::HashSet<String>>,
```

- [ ] **Step 2: Initialize the field in `ResolutionActor::new`**

In `pub fn new(...) -> Self`, add the field to the returned struct literal:

```rust
        Self {
            config,
            gamma,
            sybil,
            mapping,
            signer,
            flagged_closed: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
```

- [ ] **Step 3: Write the off-block flags in `tick()`**

In `async fn tick(&self)`, immediately after the `let events = self.gamma.fetch_closed_events(...).await?;` line and BEFORE the `let mut resolved = 0usize;` loop, insert:

```rust
        // Mark-once: push `closed: true` off-block for any mapped market
        // Polymarket reports closed, so the frontend hides/greys it. Independent
        // of settlement (`maybe_resolve` only handles clean binary payouts);
        // this fires for every close, clean or not. Skips ids already flagged
        // this lifetime so we don't re-POST every tick.
        let to_flag = {
            let flagged = self.flagged_closed.lock().expect("flagged_closed poisoned");
            pending_close_flags(&events, &mirrors, &flagged)
        };
        if !to_flag.is_empty() {
            let req = SetMarketMetadataRequest {
                closed: Some(true),
                ..Default::default()
            };
            for (sybil_id, condition_id) in to_flag {
                match self.sybil.set_market_metadata(sybil_id, &req).await {
                    Ok(()) => {
                        self.flagged_closed
                            .lock()
                            .expect("flagged_closed poisoned")
                            .insert(condition_id);
                    }
                    Err(e) => {
                        warn!(sybil_id, error = %e, "failed to flag market closed")
                    }
                }
            }
        }
```

Note: `mirrors` is the `HashMap<String, u32>` already built at the top of `tick()`. `SetMarketMetadataRequest` is already in scope via the file's `use sybil_api_types::*;`? Verify — if not, add `use sybil_api_types::SetMarketMetadataRequest;`. (The file already imports `use crate::sybil::client::SybilClient;`; the request type comes from `sybil_api_types`. Check the top of the file and add the import if the build complains in Step 4.)

- [ ] **Step 4: Build to verify it compiles**

Run: `cargo build -p sybil-polymarket`
Expected: compiles clean (warnings OK). If `SetMarketMetadataRequest` is unresolved, add `use sybil_api_types::SetMarketMetadataRequest;` near the other `use` lines and rebuild.

- [ ] **Step 5: Run the crate tests**

Run: `cargo test -p sybil-polymarket`
Expected: all pass, including `pending_close_flags`.

- [ ] **Step 6: Commit**

```bash
git add crates/sybil-polymarket/src/resolution.rs
git commit -m "feat(mirror): flag mirrored markets closed off-block in resolution tick

Self-heals closed markets in steady state (no restart needed): the resolution
actor already fetches closed Polymarket events each tick; now it also writes
MarketRefData.closed=true once per market so the frontend hides them.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task A3: Deploy to prod from `r/dev` and verify

**Files:** none (ops).

Background: only `crates/sybil-polymarket` changed. `sybil-api` and `sybil-api-types` are untouched (the `closed` field already exists). So restart **only** `sybil-polymarket`. Do **NOT** wipe any volume (per memory `project_prod_persists_engine_state`).

- [ ] **Step 1: Push `r/dev`**

```bash
git push origin r/dev
```

- [ ] **Step 2: Rsync source to prod build dir**

```bash
rsync -az \
  --exclude target/ --exclude .git/ --exclude node_modules/ \
  --exclude frontend/ --exclude '*.log' --exclude zk/ \
  --exclude arena/markets/ --exclude arena/decisions/ \
  /Users/r/pr/Sybil/ root@172.104.31.54:/opt/sybil-build/
```

- [ ] **Step 3: Build the image on prod (detached)**

```bash
ssh root@172.104.31.54 'cd /opt/sybil-build && rm -f /tmp/sybil-build.log && \
  nohup bash -c "DOCKER_BUILDKIT=1 docker build -t sybil-api:latest -t sybil-api:closeflag ." \
    > /tmp/sybil-build.log 2>&1 < /dev/null & disown'
```

- [ ] **Step 4: Wait for the build to finish**

```bash
ssh root@172.104.31.54 'BUILD_PID=$(pgrep -f "docker build -t sybil-api:latest" | head -1); \
  while kill -0 $BUILD_PID 2>/dev/null; do sleep 60; done; \
  tail -25 /tmp/sybil-build.log; docker images sybil-api'
```
Expected: build log ends with a successful image write; `docker images` shows a fresh `sybil-api:latest`.

- [ ] **Step 5: Restart only the mirror (no volume wipe)**

```bash
ssh root@172.104.31.54 'cd /opt/sybil && \
  docker compose -f docker-compose.yml -f docker-compose.prod.yml up -d --no-deps sybil-polymarket'
```

- [ ] **Step 6: Verify the flag is being written**

Watch the mirror logs for one resolution tick (interval = `resolution_poll_interval_secs`):

```bash
ssh root@172.104.31.54 'cd /opt/sybil && \
  docker compose -f docker-compose.yml -f docker-compose.prod.yml logs --since=5m sybil-polymarket | tail -60'
```
Expected: no repeated `failed to flag market closed` warnings.

Then confirm a known Eurovision market now reports `closed: true` from the API:

```bash
curl -s https://172-104-31-54.nip.io/v1/markets | \
  python3 -c "import sys,json; ms=json.load(sys.stdin); \
  print([ (m['market_id'], m['name'], m.get('closed')) for m in ms \
  if 'eurovision' in (m.get('event_title') or m.get('name') or '').lower() ])"
```
Expected: every Eurovision row shows `closed: True`. If the list is empty, the markets already dropped from the engine (also acceptable — they won't render). Load `https://172-104-31-54.nip.io/` (or the markets page) and confirm Eurovision is gone.

- [ ] **Step 7: If Eurovision still shows `closed: null`** (markets fell out of the closed-events window): manually flag them once via the metadata endpoint, e.g.:

```bash
curl -sX POST https://172-104-31-54.nip.io/v1/markets/<MARKET_ID>/metadata \
  -H 'content-type: application/json' -d '{"closed": true}'
```
(Repeat per Eurovision `market_id`. Not expected — `order=endDate desc` keeps recently-closed events in-window.)

---

### Task A4: Open PR to `main`

**Files:** none (git/gh).

- [ ] **Step 1: Open the PR**

The backend commits are on `r/dev`. Open a PR to `main`:

```bash
gh pr create --base main --head r/dev \
  --title "feat(mirror): self-heal closed markets off-block" \
  --body "$(cat <<'EOF'
## Summary
- Resolution actor now writes `MarketRefData.closed=true` (mark-once) for any mirrored market Polymarket reports closed, every tick.
- Fixes closed events (e.g. Eurovision Winner 2026) lingering on the markets page; previously only the one-time first-sync backfill ever wrote the flag.

## Test
- `cargo test -p sybil-polymarket` (new `pending_close_flags` unit test).
- Deployed to prod; verified Eurovision markets report `closed: true` and are hidden.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

If a clean backend-only PR is preferred (so `main` doesn't absorb frontend WIP from `r/dev`), instead cherry-pick the two backend commits onto a branch off `main` and PR that:

```bash
git switch main && git pull origin main
git switch -c backend/hide-closed
git cherry-pick <A1-sha> <A2-sha>
git push -u origin backend/hide-closed
gh pr create --base main --head backend/hide-closed --title "feat(mirror): self-heal closed markets off-block" --body "<as above>"
git switch r/dev
```

---

## Phase B — Frontend: keep closed in bundle + display states (on `r/dev`)

### Task B1: Stop dropping closed markets from the bundle + visibility helpers

**Files:**
- Modify: `frontend/web/src/lib/markets/use-markets.ts`
- Create: `frontend/web/src/lib/markets/use-markets.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `frontend/web/src/lib/markets/use-markets.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import {
  assemble,
  eventVisibleOnIndex,
  isClosed,
  type Market,
} from "./use-markets";

function mk(partial: Partial<Market> & { market_id: number }): Market {
  return {
    market_id: partial.market_id,
    name: partial.name ?? `m${partial.market_id}`,
    status: "active",
    ...partial,
  } as Market;
}

describe("markets/use-markets helpers", () => {
  it("isClosed only true for explicit closed===true", () => {
    expect(isClosed(mk({ market_id: 1, closed: true }))).toBe(true);
    expect(isClosed(mk({ market_id: 2, closed: false }))).toBe(false);
    expect(isClosed(mk({ market_id: 3 }))).toBe(false);
  });

  it("eventVisibleOnIndex hides only when every market is closed", () => {
    expect(
      eventVisibleOnIndex([
        mk({ market_id: 1, closed: true }),
        mk({ market_id: 2, closed: false }),
      ]),
    ).toBe(true);
    expect(
      eventVisibleOnIndex([
        mk({ market_id: 1, closed: true }),
        mk({ market_id: 2, closed: true }),
      ]),
    ).toBe(false);
  });

  it("assemble keeps closed markets in byId and groups", () => {
    const bundle = assemble([
      mk({ market_id: 1, event_id: "e1", event_title: "E1", closed: true }),
      mk({ market_id: 2, event_id: "e1", event_title: "E1", closed: false }),
      mk({ market_id: 3, closed: true }),
    ]);
    expect(bundle.byId.has(1)).toBe(true); // closed retained
    expect(bundle.byId.has(3)).toBe(true);
    const e1 = bundle.groups.find((g) => g.eventId === "e1");
    expect(e1?.markets.length).toBe(2); // both, incl. closed
    expect(bundle.total).toBe(3);
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend/web && npx vitest run src/lib/markets/use-markets.test.ts`
Expected: FAIL — `assemble`, `eventVisibleOnIndex`, `isClosed` are not exported.

- [ ] **Step 3: Update `use-markets.ts`**

In `frontend/web/src/lib/markets/use-markets.ts`:

(a) Export the visibility helpers — add after the `Market` type export (after line 20):

```ts
/** A market Polymarket has closed (resolved / past deadline). */
export function isClosed(m: Market): boolean {
  return m.closed === true;
}

/** An event is shown on the index if at least one of its markets is still open. */
export function eventVisibleOnIndex(markets: Market[]): boolean {
  return markets.some((m) => !isClosed(m));
}
```

(b) Export `assemble` and remove the hard filter. Change the function signature line:

```ts
function assemble(allMarkets: Market[]): MarketsListBundle {
```
to:
```ts
export function assemble(allMarkets: Market[]): MarketsListBundle {
```

(c) Delete the filter and use all markets. Replace lines 57-65:

```ts
function assemble(allMarkets: Market[]): MarketsListBundle {
  // Hide markets Polymarket has closed (resolved or past their deadline). The
  // mirror flags these off-block, so closed markets carry `closed: true`; an
  // absent/false flag (e.g. pre-deploy, or sybil-native markets) shows
  // everything as before.
  const markets = allMarkets.filter((m) => m.closed !== true);

  const byId = new Map<number, Market>();
  for (const m of markets) byId.set(m.market_id, m);
```

with:

```ts
export function assemble(allMarkets: Market[]): MarketsListBundle {
  // Keep ALL markets (open + closed) in the bundle. Closed markets are needed
  // by the detail page (read-only state) and by multi-cards (greyed outcome
  // rows). Index-level visibility — hiding fully-closed events and standalone
  // closed binaries — is applied by the markets page, not here. Each market
  // carries its own `closed` flag (`isClosed`) for downstream display logic.
  const markets = allMarkets;

  const byId = new Map<number, Market>();
  for (const m of markets) byId.set(m.market_id, m);
```

Leave the rest of `assemble` (grouping loop, return) unchanged — it now naturally retains closed markets.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd frontend/web && npx vitest run src/lib/markets/use-markets.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add frontend/web/src/lib/markets/use-markets.ts frontend/web/src/lib/markets/use-markets.test.ts
git commit -m "feat(markets): keep closed markets in bundle; add visibility helpers

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task B2: Apply index visibility + fix counts + ticker on the markets page

**Files:**
- Modify: `frontend/web/src/app/page.tsx`

- [ ] **Step 1: Import the helpers**

In `frontend/web/src/app/page.tsx`, update the `use-markets` import (line 14):

```ts
import { useMarketsList, type Market } from "@/lib/markets/use-markets";
```
to:
```ts
import {
  useMarketsList,
  eventVisibleOnIndex,
  isClosed,
  type Market,
} from "@/lib/markets/use-markets";
```

- [ ] **Step 2: Skip fully-closed events and closed standalone binaries when building items**

In the `items` useMemo (lines 55-98), add visibility guards. Replace the group loop body (lines 58-86):

```ts
    for (const g of bundle.groups) {
      if (g.markets.length >= 2) {
        // Group-level category: use any market's categories (they share an
        // event so the buckets are the same; pick the first market's).
        const first = g.markets[0]!;
        const primary = pickDisplayCategory(first.categories, first.category).primary;
        all.push({
          kind: "multi",
          name: g.name,
          eventId: g.eventId,
          markets: g.markets,
          volumeNanos: sumVolume(g.markets),
          sortKey: g.name.toLowerCase(),
          createdMs: eventNewnessMs(g.markets),
          primaryCategory: primary,
        });
      } else {
        for (const m of g.markets) {
          all.push({
            kind: "binary",
            market: m,
            volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
            sortKey: m.name.toLowerCase(),
            createdMs: marketNewnessMs(m),
            primaryCategory: pickDisplayCategory(m.categories, m.category).primary,
          });
        }
      }
    }
    for (const m of bundle.ungrouped) {
      all.push({
        kind: "binary",
        market: m,
        volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
        sortKey: m.name.toLowerCase(),
        createdMs: m.created_at_ms ?? 0,
        primaryCategory: pickDisplayCategory(m.categories, m.category).primary,
      });
    }
```

with (note the two `continue` guards):

```ts
    for (const g of bundle.groups) {
      if (g.markets.length >= 2) {
        // Hide a multi-outcome event only when every outcome is closed; a
        // partially-closed event stays (its closed rows render greyed).
        if (!eventVisibleOnIndex(g.markets)) continue;
        // Group-level category: use any market's categories (they share an
        // event so the buckets are the same; pick the first market's).
        const first = g.markets[0]!;
        const primary = pickDisplayCategory(first.categories, first.category).primary;
        all.push({
          kind: "multi",
          name: g.name,
          eventId: g.eventId,
          markets: g.markets,
          volumeNanos: sumVolume(g.markets),
          sortKey: g.name.toLowerCase(),
          createdMs: eventNewnessMs(g.markets),
          primaryCategory: primary,
        });
      } else {
        for (const m of g.markets) {
          if (isClosed(m)) continue; // closed standalone binary -> hide
          all.push({
            kind: "binary",
            market: m,
            volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
            sortKey: m.name.toLowerCase(),
            createdMs: marketNewnessMs(m),
            primaryCategory: pickDisplayCategory(m.categories, m.category).primary,
          });
        }
      }
    }
    for (const m of bundle.ungrouped) {
      if (isClosed(m)) continue; // closed standalone binary -> hide
      all.push({
        kind: "binary",
        market: m,
        volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
        sortKey: m.name.toLowerCase(),
        createdMs: m.created_at_ms ?? 0,
        primaryCategory: pickDisplayCategory(m.categories, m.category).primary,
      });
    }
```

- [ ] **Step 3: Fix the header counts to reflect visible items**

`bundle.total` and `bundle.groups.length + bundle.ungrouped.length` now include closed markets. Replace the header count expression (lines 209-215):

```tsx
          <p className="text-annotation">
            {bundle == null
              ? "loading…"
              : filtered && filtered.length !== items?.length
                ? `${filtered.length} of ${items?.length ?? 0} cards · uniform clearing every ${BLOCK_INTERVAL_MS / 1000}s`
                : `${bundle.total} markets · ${bundle.groups.length + bundle.ungrouped.length} events · uniform clearing every ${BLOCK_INTERVAL_MS / 1000}s`}
          </p>
```

with:

```tsx
          <p className="text-annotation">
            {bundle == null
              ? "loading…"
              : filtered && filtered.length !== items?.length
                ? `${filtered.length} of ${items?.length ?? 0} cards · uniform clearing every ${BLOCK_INTERVAL_MS / 1000}s`
                : `${visibleMarketCount} markets · ${items?.length ?? 0} events · uniform clearing every ${BLOCK_INTERVAL_MS / 1000}s`}
          </p>
```

And add the derived count next to the `items` memo (right after the `items` useMemo, before `multiEventIds`):

```ts
  // Markets actually shown on the index (each multi card may carry greyed
  // closed outcomes — count them as shown). Drives the header tally now that
  // the bundle retains closed markets.
  const visibleMarketCount = useMemo(
    () =>
      items?.reduce(
        (n, it) => n + (it.kind === "multi" ? it.markets.length : 1),
        0,
      ) ?? 0,
    [items],
  );
```

- [ ] **Step 4: Keep closed markets out of the clearing ticker**

Replace the ticker line (line 179):

```tsx
      {bundle && <ClearingTicker marketsById={bundle.byId} />}
```

with an open-only map:

```tsx
      {openById && <ClearingTicker marketsById={openById} />}
```

And add the memo right after `const prices = useStore(selectPricesByMarketId);` (line 52):

```ts
  // The clearing ticker is an active-board readout — exclude closed markets,
  // which the bundle now retains for detail/multi-card use.
  const openById = useMemo(() => {
    if (!bundle) return null;
    const m = new Map<number, Market>();
    for (const [id, mk] of bundle.byId) {
      if (mk.closed !== true) m.set(id, mk);
    }
    return m;
  }, [bundle]);
```

- [ ] **Step 5: Type-check / lint**

Run: `cd frontend/web && npx tsc --noEmit && npx eslint src/app/page.tsx`
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add frontend/web/src/app/page.tsx
git commit -m "feat(markets): hide fully-closed events & closed binaries from index

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task B3: Grey closed outcome rows in MultiCard, sort closed last

**Files:**
- Modify: `frontend/web/src/components/multi-card.tsx`

- [ ] **Step 1: Sort closed outcomes last**

In `MultiCard` (the `ranked` sort, lines 47-54), make closed outcomes sink to the bottom so the leader/featured outcome is always open when possible. Replace:

```ts
  const ranked = [...markets].sort((a, b) => {
    const va = a.volume_nanos ? BigInt(a.volume_nanos) : 0n;
    const vb = b.volume_nanos ? BigInt(b.volume_nanos) : 0n;
    if (va !== vb) return va > vb ? -1 : 1;
    const pa = prices[a.market_id]?.yes ?? -1n;
    const pb = prices[b.market_id]?.yes ?? -1n;
    return pa > pb ? -1 : pa < pb ? 1 : 0;
  });
```

with:

```ts
  const ranked = [...markets].sort((a, b) => {
    // Closed outcomes always sink below open ones (still shown, just greyed).
    const ca = a.closed === true ? 1 : 0;
    const cb = b.closed === true ? 1 : 0;
    if (ca !== cb) return ca - cb;
    const va = a.volume_nanos ? BigInt(a.volume_nanos) : 0n;
    const vb = b.volume_nanos ? BigInt(b.volume_nanos) : 0n;
    if (va !== vb) return va > vb ? -1 : 1;
    const pa = prices[a.market_id]?.yes ?? -1n;
    const pb = prices[b.market_id]?.yes ?? -1n;
    return pa > pb ? -1 : pa < pb ? 1 : 0;
  });
```

- [ ] **Step 2: Grey the closed secondary rows**

In `SecondaryRow` (lines 400-488), dim the row and append a "closed" marker when `market.closed === true`. Replace the opening of the returned `<Link>`'s label `<span>` block. Specifically, change the label span (lines 439-452):

```tsx
      <span
        style={{
          fontFamily: "var(--font-sans)",
          fontSize: "var(--fs-13)",
          color: "var(--fg-2)",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          userSelect: "text",
          cursor: "pointer",
        }}
      >
        {label}
      </span>
```

with:

```tsx
      <span
        style={{
          fontFamily: "var(--font-sans)",
          fontSize: "var(--fs-13)",
          color: market.closed === true ? "var(--fg-4)" : "var(--fg-2)",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          userSelect: "text",
          cursor: "pointer",
        }}
      >
        {label}
        {market.closed === true && (
          <span
            className="text-mono"
            style={{
              marginLeft: 6,
              fontSize: "9px",
              letterSpacing: "var(--track-wide)",
              textTransform: "uppercase",
              color: "var(--fg-4)",
            }}
          >
            closed
          </span>
        )}
      </span>
```

And dim the whole row by adding `opacity` to the `<Link>` style. In `SecondaryRow`'s `<Link style={{ ... }}>` (lines 428-437), add `opacity: market.closed === true ? 0.5 : 1,` to the style object.

- [ ] **Step 3: Reflect closed in the featured (leader) outcome**

The leader can be closed only when the whole event is closed (those are hidden on the index) OR all higher-ranked outcomes are also closed. With closed-last sorting the leader is open whenever any outcome is open, so no featured-row change is required. Leave `FeaturedOutcome` as-is.

- [ ] **Step 4: Manual verification (no unit test — pure presentational)**

Run the dev server: `cd frontend/web && pnpm dev`. Open a partially-closed multi-outcome event card (or temporarily mark one market `closed` via the API in Step A3). Confirm: closed outcomes appear greyed with a "closed" tag, sorted to the bottom, and the featured outcome is an open one.

- [ ] **Step 5: Type-check + commit**

```bash
cd frontend/web && npx tsc --noEmit && npx eslint src/components/multi-card.tsx
git add frontend/web/src/components/multi-card.tsx
git commit -m "feat(markets): grey closed outcomes in MultiCard, sort them last

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task B4: Expose `closed` on event-group outcomes, sort closed last

**Files:**
- Modify: `frontend/web/src/lib/market-detail/use-event-group.ts`

- [ ] **Step 1: Add `closed` to the `EventOutcome` type**

In `frontend/web/src/lib/market-detail/use-event-group.ts`, add to the `EventOutcome` type (after `marketId`, around line 28):

```ts
export type EventOutcome = {
  marketId: number;
  /** Polymarket has closed/resolved this outcome — render read-only / greyed. */
  closed: boolean;
```

- [ ] **Step 2: Populate `closed` when mapping outcomes**

In the `outcomes` map (lines 80-105), add `closed` to each returned object. In the `return { ... }` inside `siblings.map(...)`, add after `marketId: m.market_id,`:

```ts
        marketId: m.market_id,
        closed: m.closed === true,
```

- [ ] **Step 3: Sort closed outcomes last**

The outcomes are sorted by YES probability descending (lines 109-111). Make closed sink to the bottom. Replace:

```ts
    outcomes.sort(
      (a, b) => (b.yesCents ?? -1) - (a.yesCents ?? -1),
    );
```

with:

```ts
    outcomes.sort((a, b) => {
      // Closed outcomes always sort below open ones, so the picker/chart
      // default lands on a tradeable outcome.
      if (a.closed !== b.closed) return a.closed ? 1 : -1;
      return (b.yesCents ?? -1) - (a.yesCents ?? -1);
    });
```

- [ ] **Step 4: Type-check**

Run: `cd frontend/web && npx tsc --noEmit`
Expected: no errors. (`EventOutcome` consumers — `pro-rail.tsx`, `degen-rail.tsx`, `batch-hero.tsx`, `outcome-legend.tsx`, `degen-outcome-picker.tsx` — only read existing fields; adding a field is non-breaking.)

- [ ] **Step 5: Commit**

```bash
git add frontend/web/src/lib/market-detail/use-event-group.ts
git commit -m "feat(market-detail): expose closed on outcomes, sort closed last

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task B5: Read-only closed state on the detail page + rail

**Files:**
- Modify: `frontend/web/src/app/m/[id]/page.tsx`
- Modify: `frontend/web/src/components/market-rail/index.tsx`

- [ ] **Step 1: Add a closed banner on the detail page**

In `frontend/web/src/app/m/[id]/page.tsx`, render a banner above the header when the market is closed. In the `{market && ( <> ... </> )}` block (line 63), insert the banner just before `<Header ... />`:

```tsx
      {market && (
        <>
          {market.closed === true && <ClosedBanner />}
          {/* Header spans the full width; the chart/rail split sits below it. */}
          <Header marketId={marketId} market={market} />
```

Add the `ClosedBanner` component near the other helpers (e.g. before `function Placeholder`):

```tsx
/**
 * Shown atop a market whose Polymarket event has closed/resolved. Trading is
 * disabled in the rail; this explains why. Best-effort — the market is still
 * mirrored in the engine; after a sybil-api restart closed markets drop out
 * entirely and this page 404s (acceptable per product decision).
 */
function ClosedBanner() {
  return (
    <div
      role="status"
      className="text-mono"
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-2)",
        padding: "var(--space-3) var(--space-4)",
        borderRadius: "var(--radius-md)",
        background: "var(--warn-soft)",
        color: "var(--warn)",
        border: "1px solid var(--warn-soft)",
        fontSize: "var(--fs-12)",
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
      }}
    >
      market closed · trading disabled · view only
    </div>
  );
}
```

- [ ] **Step 2: Gate the rail to read-only when the selected outcome is closed**

In `frontend/web/src/components/market-rail/index.tsx`, compute the selected outcome's closed state and replace the trade rails with a notice. Replace the component body:

```tsx
export function MarketRail({ marketId }: { marketId: number }) {
  const [mode, setMode] = useRailMode();
  const { group, isPending } = useEventGroup(marketId);

  return (
    <aside
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 14,
      }}
    >
      <ModeTabs value={mode} onChange={setMode} />
      {isPending && (
        <div
          style={{
            padding: "24px 12px",
            color: "var(--fg-3)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            textAlign: "center",
          }}
        >
          loading rail…
        </div>
      )}
      {group && mode === "degen" && <DegenRail group={group} />}
      {group && mode === "pro" && <ProRail group={group} />}
    </aside>
  );
}
```

with:

```tsx
export function MarketRail({ marketId }: { marketId: number }) {
  const [mode, setMode] = useRailMode();
  const { group, isPending } = useEventGroup(marketId);

  const selected = group
    ? group.outcomes.find((o) => o.marketId === group.currentMarketId) ??
      group.outcomes[0]
    : undefined;
  const closed = selected?.closed === true;

  return (
    <aside
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 14,
      }}
    >
      {!closed && <ModeTabs value={mode} onChange={setMode} />}
      {isPending && (
        <div
          style={{
            padding: "24px 12px",
            color: "var(--fg-3)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            textAlign: "center",
          }}
        >
          loading rail…
        </div>
      )}
      {group && closed && <ClosedRail />}
      {group && !closed && mode === "degen" && <DegenRail group={group} />}
      {group && !closed && mode === "pro" && <ProRail group={group} />}
    </aside>
  );
}

/** Read-only replacement for the trade rail on a closed/resolved market. */
function ClosedRail() {
  return (
    <div
      className="text-mono"
      style={{
        padding: "24px 16px",
        borderRadius: "var(--radius-md)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        color: "var(--fg-3)",
        fontSize: 12,
        lineHeight: "18px",
        textAlign: "center",
      }}
    >
      This market has closed. Trading is disabled.
    </div>
  );
}
```

- [ ] **Step 3: Manual verification**

With the dev server running, open the detail page of a closed market (use a market flagged `closed` from Step A3, or a `?` override). Confirm: the closed banner shows above the header, the mode tabs and Degen/Pro trade forms are replaced by the "market has closed" notice, and the chart still renders.

- [ ] **Step 4: Type-check + lint + commit**

```bash
cd frontend/web && npx tsc --noEmit && npx eslint src/app/m/[id]/page.tsx src/components/market-rail/index.tsx
git add frontend/web/src/app/m/[id]/page.tsx frontend/web/src/components/market-rail/index.tsx
git commit -m "feat(market-detail): read-only closed state (banner + disabled rail)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task B6: Full frontend verification

**Files:** none.

- [ ] **Step 1: Run the full frontend test suite**

Run: `cd frontend/web && pnpm test`
Expected: all pass (incl. new `use-markets.test.ts`).

- [ ] **Step 2: Type-check the whole app**

Run: `cd frontend/web && npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Manual end-to-end against prod data**

With backend deployed (Phase A) and `pnpm dev` running against the prod API:
- Markets index: Eurovision Winner 2026 is gone.
- A partially-closed event (if any): card present, resolved outcomes greyed at the bottom.
- Direct-navigate to a closed market's `/m/<id>`: banner + read-only rail, chart renders.

- [ ] **Step 4: Push**

```bash
git push origin r/dev
```

---

## Self-Review

**Spec coverage:**
- #1 hide closed from index → Task A2 (sets flag) + B1/B2 (index visibility). ✓
- #2 partial events show all outcomes, closed greyed → B1 (retain in bundle) + B3 (MultiCard grey) + B4 (outcome closed flag). ✓
- #3 read-only closed detail → B1 (retain in byId) + B4 (closed flag) + B5 (banner + disabled rail). ✓
- Backend already-knows mechanism → A1/A2 (resolution.rs mark-once). ✓
- Efficiency (#1 from discussion: not re-writing every cycle) → A2 `flagged_closed` mark-once set. ✓
- Backend on r/dev, deploy, PR to main → A3/A4. ✓
- Best-effort durability accepted → noted in `ClosedBanner` doc + plan header. ✓

**Placeholder scan:** none — every step has concrete code/commands.

**Type consistency:** `isClosed`/`eventVisibleOnIndex`/`assemble` exported from `use-markets.ts` and imported in `page.tsx` + test. `EventOutcome.closed` added in B4, read in B5. `pending_close_flags(events, mirrors, already_flagged)` signature identical in A1 (helper + test) and A2 (call site). `flagged_closed` field name consistent across struct/new/tick.

**Known limitations (by design):** #2 greyed rows and #3 read-only detail are best-effort — after a `sybil-api` restart, closed-only markets aren't recreated (mirror only recreates `active && !closed`), so they vanish. Accepted by the user.
