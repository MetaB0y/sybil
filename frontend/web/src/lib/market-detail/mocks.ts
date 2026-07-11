/**
 * Deterministic values for the legacy `/m-dev/[id]` diagnostic route. These
 * do not describe the product page or the current API surface.
 *
 * Each call site is expected to wrap the rendered value in a <MockValue>-style
 * marker so the placeholder is visible to the user.
 */

/** Cheap 32-bit hash for deterministic mocks. Same shape as activity/mocks.ts. */
function hash32(n: number): number {
  let h = (n | 0) ^ 0x9e3779b9;
  h = Math.imul(h ^ (h >>> 16), 0x85ebca6b);
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35);
  h ^= h >>> 16;
  return h >>> 0;
}

// ── Lifetime / market-stats mocks ───────────────────────────────────────

/**
 * Mocked last-24h volume in nanos. Sized as a fraction of lifetime volume so
 * busier markets show bigger numbers; deterministic from (marketId, latestHeight).
 * Retained only for the legacy diagnostic presentation.
 */
export function mock24hVolumeNanos(
  marketId: number,
  totalVolumeNanos: bigint,
  latestHeight: number,
): bigint {
  if (totalVolumeNanos === 0n) return 0n;
  const h = hash32(marketId * 31 + latestHeight);
  const pct = 4 + (h % 12); // 4..15% of lifetime volume
  return (totalVolumeNanos * BigInt(pct)) / 100n;
}

/**
 * Mocked lifetime unique trader count. Roughly `sqrt(dollars) * 1.4 ± jitter`
 * to match the shape of `mockTraders` in `lib/mock.ts`.
 */
export function mockLifetimeTraders(
  marketId: number,
  totalVolumeNanos: bigint,
): number {
  if (totalVolumeNanos === 0n) return 0;
  const dollars = Number(totalVolumeNanos / 1_000_000_000n);
  const base = Math.max(8, Math.round(Math.sqrt(dollars) * 1.4));
  const jitter = (hash32(marketId * 17 + 3) % 40) - 20;
  return Math.max(1, base + jitter);
}

/**
 * Mocked liquidity in nanos. ~20-44% of lifetime volume, deterministic per
 * market.
 */
export function mockLiquidityNanos(
  marketId: number,
  totalVolumeNanos: bigint,
): bigint {
  if (totalVolumeNanos === 0n) return 0n;
  const pct = 20 + (hash32(marketId * 7 + 11) % 25); // 20..44%
  return (totalVolumeNanos * BigInt(pct)) / 100n;
}

// ── Open-batch mocks ────────────────────────────────────────────────────

/**
 * Mocked count of traders that placed orders in the currently-open batch.
 * Range 0..30.
 */
export function mockTradersInOpenBatch(
  marketId: number,
  latestHeight: number,
): number {
  return hash32(marketId * 113 + latestHeight) % 31;
}

/**
 * Mocked indicative YES clearing price in nanos for the open batch.
 * Anchored near the latest clearing price (if provided) ± a small drift so
 * the value stays plausible.
 *
 * `currentYesPriceNanos` should be the most recent committed price for the
 * market; the indicative drift is ±5¢ around it. Falls back to 50¢ if
 * unknown.
 */
export function mockIndicativeYesPriceNanos(
  marketId: number,
  latestHeight: number,
  currentYesPriceNanos: bigint | null,
): bigint {
  const anchor = currentYesPriceNanos ?? 500_000_000n; // 50¢
  // Drift range: ±5¢ = ±50_000_000 nanos
  const driftNanos = BigInt(
    (hash32(marketId * 211 + latestHeight) % 100_000_001) - 50_000_000,
  );
  const out = anchor + driftNanos;
  if (out < 0n) return 0n;
  if (out > 1_000_000_000n) return 1_000_000_000n;
  return out;
}

/**
 * Mocked indicative total volume that would clear in the open batch (nanos).
 * Sized to look like a single-batch slice of activity.
 */
export function mockIndicativeVolumeNanos(
  marketId: number,
  latestHeight: number,
): bigint {
  // Range: 0..$50_000 in nanos.
  const dollars = hash32(marketId * 53 + latestHeight) % 50_001;
  return BigInt(dollars) * 1_000_000_000n;
}

/**
 * Deterministic imbalance in basis points (-1000..+1000 ≙ ±10%). Same shape
 * as `activity/mocks.ts:mockImbalanceBps`. Re-implemented here so the
 * market-detail module is self-contained.
 */
export function mockImbalanceBps(
  marketId: number,
  latestHeight: number,
): number {
  return (hash32(marketId * 911 + latestHeight) % 2001) - 1000;
}
