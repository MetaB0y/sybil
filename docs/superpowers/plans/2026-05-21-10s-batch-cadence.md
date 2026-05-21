# 10s Batch Cadence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the production block cadence from 2s to 10s, adjust the block-count-based durations that would otherwise silently 5×, and fix the frontend countdowns/age displays that hardcode the 2s cadence.

**Architecture:** The cadence is an env var (`SYBIL_BLOCK_INTERVAL_MS`) consumed by the actor's block-production timer — no engine code changes. The work is (1) the env value, (2) the *block-count* durations coupled to it (bridge withdrawal expiry), and (3) the frontend, which hardcodes `2000` in three countdown components and approximates order-creation time as `block_height × cadence` (replaced by the real `created_at_ms` from plan #1).

**Tech Stack:** Docker Compose env, Rust const, Next.js/React (vitest), `openapi-typescript` codegen.

**Depends on:** Plan #1 (`2026-05-21-recent-blocks-and-created-at-ms`) — Task 3 here consumes `PendingOrderResponse.created_at_ms`.

**Conventions:** jj VCS; `just fmt`/`just lint` for Rust; `pnpm lint`/`pnpm build` in `frontend/web`.

---

## File Structure

- Modify `docker-compose.yml:11` — `SYBIL_BLOCK_INTERVAL_MS` 2000 → 10000 (base file; prod inherits, no override in `docker-compose.prod.yml`).
- Modify `crates/matching-sequencer/src/bridge.rs:11` — `DEFAULT_WITHDRAWAL_EXPIRY_BLOCKS` to preserve ~14 days wall-clock; add a guard test.
- Create `frontend/web/src/lib/constants.ts` — single `BLOCK_INTERVAL_MS` source of truth.
- Modify `frontend/web/src/components/batch-pill.tsx`, `.../market-rail/use-batch-countdown.ts`, `.../activity/batch-chip.tsx` — use the shared constant.
- Modify `frontend/web/src/components/portfolio/open-orders-list.tsx` — use `created_at_ms`, drop the cadence approximation.

> **Not changed (documented decisions):**
> - `config.rs:23` `order_ttl_blocks` default `63072000` is GTC ("effectively forever"); at 10s that's ~20y. It is already env-configurable (`SYBIL_ORDER_TTL_BLOCKS`) — leave the default, set the env only if a specific wall-clock GTC horizon is wanted.
> - Polymarket MM windows (`sybil-polymarket/config.rs`: `mm_sync_interval_blocks=5`, `mm_vol_window=30`) are blocks-based → MM position sync 10s→50s, variance window 60s→300s. Benign; adjust later if MM behavior needs tuning.
> - `frontend/web/src/lib/account/use-account-history.ts` hardcodes a cadence too, but it's the **mock** generator being replaced in plan #6 — left for that plan.

---

## Task 1: Backend cadence + bridge withdrawal expiry

**Files:**
- Modify: `docker-compose.yml:11`
- Modify: `crates/matching-sequencer/src/bridge.rs:11`
- Test: `crates/matching-sequencer/src/bridge.rs` (test module)

- [ ] **Step 1: Write the failing guard test for the wall-clock target**

In `crates/matching-sequencer/src/bridge.rs`, add (or extend) a test module at the end of the file:

```rust
#[cfg(test)]
mod cadence_tests {
    use super::DEFAULT_WITHDRAWAL_EXPIRY_BLOCKS;

    /// Withdrawal expiry is block-count-based, so it must track the block
    /// cadence. At the 10s production cadence the default must still equal a
    /// 14-day wall-clock challenge window (the value in effect at the prior
    /// 2s cadence). If the cadence changes again, change this constant.
    #[test]
    fn withdrawal_expiry_is_14_days_at_10s_cadence() {
        const CADENCE_S: u64 = 10;
        const FOURTEEN_DAYS_S: u64 = 14 * 86_400;
        assert_eq!(DEFAULT_WITHDRAWAL_EXPIRY_BLOCKS * CADENCE_S, FOURTEEN_DAYS_S);
    }
}
```

- [ ] **Step 2: Run the test, verify it fails**

Run: `cargo test -p matching-sequencer withdrawal_expiry_is_14_days_at_10s_cadence`
Expected: FAIL — `604_800 * 10 = 6_048_000 ≠ 1_209_600`.

- [ ] **Step 3: Update the bridge expiry constant**

In `crates/matching-sequencer/src/bridge.rs`, line 11:

```rust
/// Withdrawal challenge window, in blocks. 14 days of wall-clock at the 10s
/// block cadence (14 * 86_400 / 10). Block-count-based — keep in sync with
/// `SYBIL_BLOCK_INTERVAL_MS` (see `cadence_tests`).
pub const DEFAULT_WITHDRAWAL_EXPIRY_BLOCKS: u64 = 120_960;
```

- [ ] **Step 4: Run the test, verify it passes**

Run: `cargo test -p matching-sequencer withdrawal_expiry_is_14_days_at_10s_cadence`
Expected: PASS.

- [ ] **Step 5: Set the production cadence**

In `docker-compose.yml`, line 11:

```yaml
      SYBIL_BLOCK_INTERVAL_MS: "10000"
```

- [ ] **Step 6: Run the bridge suite, lint, format, commit**

Run: `cargo test -p matching-sequencer bridge && just lint && just fmt`
Expected: PASS.

```bash
jj describe -m "feat(cadence): 10s blocks; keep withdrawal expiry at 14d wall-clock"
```

---

## Task 2: Frontend — single cadence constant for countdowns

**Files:**
- Create: `frontend/web/src/lib/constants.ts`
- Modify: `frontend/web/src/components/batch-pill.tsx:7`
- Modify: `frontend/web/src/components/market-rail/use-batch-countdown.ts:18`
- Modify: `frontend/web/src/components/activity/batch-chip.tsx:20`

- [ ] **Step 1: Create the shared constant**

Create `frontend/web/src/lib/constants.ts`:

```ts
/**
 * Block cadence in ms. Mirrors the backend `SYBIL_BLOCK_INTERVAL_MS`
 * (docker-compose.yml). Single source of truth for batch countdown timers —
 * keep in sync if the backend cadence changes.
 */
export const BLOCK_INTERVAL_MS = 10_000;
```

- [ ] **Step 2: Use it in `batch-pill.tsx`**

Replace line 7 (`const BLOCK_MS = 2000; // ...`) with an import (top of file, with the other imports) and a local alias so the rest of the file is untouched:

```ts
import { BLOCK_INTERVAL_MS } from "@/lib/constants";
```

and where `BLOCK_MS` was defined, replace the constant with:

```ts
const BLOCK_MS = BLOCK_INTERVAL_MS;
```

- [ ] **Step 3: Use it in `use-batch-countdown.ts`**

Add the import at the top, and replace line 18 (`const BATCH_MS = 2000;`) with:

```ts
const BATCH_MS = BLOCK_INTERVAL_MS;
```

(import: `import { BLOCK_INTERVAL_MS } from "@/lib/constants";`)

- [ ] **Step 4: Use it in `batch-chip.tsx`**

Add the import at the top, and replace line 20 (`const BLOCK_MS = 2000;`) with:

```ts
const BLOCK_MS = BLOCK_INTERVAL_MS;
```

(import: `import { BLOCK_INTERVAL_MS } from "@/lib/constants";`)

- [ ] **Step 5: Typecheck + lint**

Run: `cd frontend/web && pnpm lint && pnpm build`
Expected: PASS (no type errors).

- [ ] **Step 6: Commit**

```bash
jj describe -m "fix(web): centralize batch cadence in BLOCK_INTERVAL_MS (10s)"
```

---

## Task 3: Frontend — open-orders "Created" uses real `created_at_ms`

**Files:**
- Regenerate: `frontend/web/src/lib/api/schema.d.ts` (via `pnpm types:generate`)
- Modify: `frontend/web/src/components/portfolio/open-orders-list.tsx` (lines 13-14, 37, 154-160, 263-270)

**Precondition:** plan #1 is deployed/running on the backend you point codegen at, so `/openapi.json` includes `PendingOrderResponse.created_at_ms`.

- [ ] **Step 1: Regenerate API types against a backend with plan #1**

Run a local backend that includes plan #1, then:

```bash
cd frontend/web && NEXT_PUBLIC_API_BASE=http://localhost:3001 pnpm types:generate
```

Verify `created_at_ms` now exists:

```bash
grep -n "created_at_ms" frontend/web/src/lib/api/schema.d.ts
```

Expected: a hit under `PendingOrderResponse`.

- [ ] **Step 2: Replace the cadence approximation with `created_at_ms`**

In `frontend/web/src/components/portfolio/open-orders-list.tsx`:

Remove the `CADENCE_MS` constant (line 37). Replace the `createdMs` block (lines 154-160):

```ts
  // Exact created time from the backend (created_at_ms on PendingOrderResponse).
  // Falls back to null for orders admitted before the field shipped (0).
  const createdMs = order.created_at_ms && order.created_at_ms > 0 ? order.created_at_ms : null;
```

- [ ] **Step 3: De-mock the CreatedCell**

The cell is no longer an approximation. Replace the `CreatedCell` (lines 263-270) so it renders the real value without the `MockValue` wrapper:

```tsx
/** Created-time cell — exact wall-clock from backend created_at_ms. */
function CreatedCell({ ms, block }: { ms: number | null; block: number }) {
  if (ms == null) {
    return <span className="text-muted-foreground">#{formatInt(block)}</span>;
  }
  return <span title={new Date(ms).toLocaleString()}>{formatRelativeMs(ms)}</span>;
}
```

> Implementer note: reuse whatever relative-time formatter the file already imports for other timestamps (e.g. `formatRelativeMs`/`formatRelative`); match the existing import. Keep the `block` fallback for pre-field orders. Remove the now-unused `MockValue` import if nothing else in the file uses it.

- [ ] **Step 4: Update the file's header doc**

Replace the stale lines 13-14 ("Created time is approximated… until the backend exposes `created_at_ms`") with:

```ts
 * - Created time is the exact `created_at_ms` from `PendingOrderResponse`
 *   (falls back to the block height for orders admitted before that field).
```

- [ ] **Step 5: Typecheck + lint**

Run: `cd frontend/web && pnpm lint && pnpm build`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
jj describe -m "fix(web): open-orders Created uses backend created_at_ms, drop cadence mock"
```

---

## Task 4: Manual local verification

- [ ] **Step 1: Run the backend at the new cadence**

```bash
cargo run --release -p sybil-api -- --dev-mode --port 3001 --block-interval-ms 10000
```

- [ ] **Step 2: Confirm blocks seal every ~10s**

```bash
curl -s localhost:3001/v1/blocks/latest | jq .height ; sleep 11 ; curl -s localhost:3001/v1/blocks/latest | jq .height
```

Expected: height increases by ~1 over 11s.

- [ ] **Step 3: Run the frontend against it and eyeball the timers**

```bash
cd frontend/web && NEXT_PUBLIC_API_BASE=http://localhost:3001 pnpm dev
```

Open the app and confirm: the batch pill / market-rail / activity countdowns count down from ~10s (not snapping to 0 at 2s), and the Portfolio → Open Orders "Created" column shows a real relative time (no MockValue underline).

---

## Self-Review Notes

- **Spec coverage:** cadence 2s→10s (Task 1, Step 5); "hardcoded frontend countdown must be fixed" (Task 2, all three components + Task 3 open-orders). Coupled blocks-based durations addressed (bridge expiry changed; order TTL + MM windows documented as deliberate no-ops).
- **Dependency:** Task 3 needs plan #1's `created_at_ms`; Step 1 regenerates types against it.
- **Decision flagged:** `DEFAULT_WITHDRAWAL_EXPIRY_BLOCKS` is security-relevant (withdrawal challenge window). Plan preserves the *current prod* 14-day wall-clock; the guard test documents intent. Adjust the constant if a different window is wanted.
- **No placeholders:** every edit has exact text; the only judgement call is the relative-time formatter name in Task 3 Step 3 (note included — match the file's existing import).
