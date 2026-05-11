# Frontend known issues

Workarounds in place + open tickets for proper fixes.

---

## #1 — u64 / `_nanos` fields come over the wire as JSON numbers

**Status:** workaround active in frontend; backend fix pending.

**What it is.** Sybil represents money as nanos (1 dollar = 1,000,000,000 nanos). The Rust API serializes these `u64` fields as JSON numbers. JavaScript's `number` type can only represent integers up to **2^53 − 1 ≈ 9.007e15**. Once a value crosses that line, the precision is silently lost during JSON parsing — before our code ever sees it.

In practical Sybil terms, the danger zone in nanos is values above `~$9,007,199`. Per-trade prices are nowhere near; aggregate welfare totals, large account balances, and protocol-wide volume can cross it.

**What we did about it (frontend-only).**

1. `scripts/patch-bigints.mjs` rewrites the generated TypeScript schema so every `*_nanos: number` becomes `*_nanos: string`. Runs automatically after `pnpm types:generate`.
2. `src/lib/format/nanos.ts` exposes `parseNanos()`, `formatDollars()`, `formatProbability()`. `parseNanos` accepts `string | number | bigint` so it works whether the runtime value is a (possibly-rounded) JS number or a string.
3. All money arithmetic in app code must go through `bigint` and these helpers — **never** do `data.balance_nanos * 2` directly.

**Why this isn't enough.** The wire format is still JSON numbers, so a value of, say, `12,345,678,000,000,000` nanos (~$12.3M) is *already corrupted* by `JSON.parse` before our code receives it. We can't recover the lost digits client-side. The workaround prevents *further* corruption inside our code and forces correct arithmetic — that's the bound of what's possible from the frontend alone.

**The proper fix (backend ticket).** Configure `utoipa` in `crates/sybil-api` to emit `format: int64` u64 fields as JSON strings. When that lands:
- `pnpm types:generate` will produce `*_nanos: string` naturally → `scripts/patch-bigints.mjs` becomes a no-op and can be deleted.
- The wire format will be `"12345678000000000"` (string) instead of `12345678000000000` (number) → no precision loss in transit.
- `parseNanos` already handles both paths, so no app-code changes needed.

**Owner:** backend team. Track in the Rust repo, not here. This file should be updated when it ships.

---
