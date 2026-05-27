/**
 * Mocks for fields the backend doesn't expose yet.
 *
 * Everything here is DETERMINISTIC from a seed (usually market_id or a
 * stable string) so it doesn't shuffle on re-render. Each call site
 * should also wrap the rendered value in `<MockValue>` so the user can
 * see at a glance which numbers are placeholders.
 *
 * When backend exposes the real fields:
 *   - 24h delta → fetch per-outcome history, drop mockDelta()
 *
 * liquidity and trader counts are now real — cards read
 * `market.liquidity_avg10_nanos` / `market.trader_count`, and MultiCard
 * fetches the per-event trader union via `useEventTraders`.
 *
 * Categories already come from backend; see lib/categorize.ts.
 */

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

/** Compact trader count formatter: 4.2K · 1.8K · 240 · 8. */
export function formatTraders(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toString();
}
