/**
 * Mocks for fields the backend doesn't expose yet.
 *
 * Everything here is DETERMINISTIC from a seed (usually market_id or a
 * stable string) so it doesn't shuffle on re-render. Each call site
 * should also wrap the rendered value in `<MockValue>` so the user can
 * see at a glance which numbers are placeholders.
 *
 * When backend exposes the real fields:
 *   - category → swap mockCategory() for market.category, drop the dot/MockValue wrap
 *   - 24h delta → fetch per-outcome history, drop mockDelta()
 *   - liquidity → market.liq_nanos? drop mockLiq()
 *   - trader count → market.trader_count? drop mockTraders()
 *
 * grep this file's exports to find every mocked surface.
 */

const CATEGORIES = [
  { name: "Politics", color: "#9F8FE8" },
  { name: "Elections", color: "#5BC4E0" },
  { name: "Economy", color: "#E8B447" },
  { name: "Tech", color: "#7E9AE8" },
  { name: "Finance", color: "#5BD99A" },
  { name: "Culture", color: "#E89D9F" },
  { name: "Climate", color: "#4FB5A8" },
  { name: "Mentions", color: "#C49AE8" },
  { name: "World", color: "#E89A6B" },
  { name: "Crypto", color: "#F2B244" },
  { name: "Sports", color: "#5BD99A" },
] as const;

export type MockCategory = (typeof CATEGORIES)[number];

/** Hash a string to a non-negative integer. */
function hashSeed(seed: string | number): number {
  if (typeof seed === "number") return Math.abs(seed | 0);
  let h = 2166136261;
  for (let i = 0; i < seed.length; i++) {
    h ^= seed.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return Math.abs(h | 0);
}

export function mockCategory(seed: string | number): MockCategory {
  const h = hashSeed(seed);
  return CATEGORIES[h % CATEGORIES.length]!;
}

/**
 * Mock a 24h delta in cents. Range -8..+8, biased toward the
 * direction the current price implies (above 50% → slightly positive).
 * Stable across renders for a given seed.
 */
export function mockDelta(seed: string | number, yesPct?: number | null): number {
  const h = hashSeed(seed);
  // Normalize 0..1 then map to -5..5
  const base = ((h % 1000) / 1000) * 10 - 5;
  // Bias by current price: high prices → small positive bias, low → negative
  const bias = yesPct != null ? (yesPct - 50) / 25 : 0; // -2..+2
  return Math.max(-12, Math.min(12, Math.round(base + bias)));
}

/** Mocked liquidity, expressed as nanos. Derived as ~30% of volume so it
 *  scales with real activity rather than being a flat number. */
export function mockLiq(volumeNanos: bigint, seed: string | number): bigint {
  if (volumeNanos === 0n) return 0n;
  const h = hashSeed(seed);
  const pct = 20 + (h % 25); // 20..44%
  return (volumeNanos * BigInt(pct)) / 100n;
}

/**
 * Mocked trader count. Roughly proportional to volume (a $1M-volume
 * market has more traders than a $1K-volume market) with jitter.
 */
export function mockTraders(seed: string | number, volumeNanos: bigint): number {
  if (volumeNanos === 0n) return 0;
  const h = hashSeed(seed);
  const vol = Number(volumeNanos / 1_000_000_000n); // dollars
  const base = Math.max(8, Math.round(Math.sqrt(vol) * 1.4));
  const jitter = (h % 40) - 20; // ±20
  return Math.max(1, base + jitter);
}

/** Compact trader count formatter: 4.2K · 1.8K · 240 · 8. */
export function formatTraders(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toString();
}
