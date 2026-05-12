/**
 * Mock values + deterministic per-market splits for fields the backend
 * doesn't expose yet. Every consumer pulls from this single module so
 * there's one delete-site when OPEN_QUESTIONS #3–#6 land.
 *
 * - All-time stats (#3): handoff-equivalent constants, replaced when a
 *   /v1/activity/overview rollup endpoint exists.
 * - Per-market welfare (#4): proportional split of block-level welfare.
 * - Per-market placed/matched (#5): proportional split of block counts.
 * - Per-market imbalance (#6): deterministic hash → ±10% basis points.
 */

import type { AllTimeStats } from "./types";

/** Handoff-equivalent all-time figures. Replace when the backend lands a rollup. */
export const MOCK_ALL_TIME: AllTimeStats = {
  matchedVolume: "$487.2M",
  traders: 18_402,
  ordersPlaced: 2_104_877,
  ordersMatched: 1_682_711,
  ordersUnmatched: 422_166,
  totalBatches: 0, // overridden in the hook with latestBlock.height
  liveMarkets: 0, // overridden with the markets-summary count
  uptime: "99.97%",
  genesisAge: "6 mo 17 d",
  mocked: {
    matchedVolume: true,
    traders: true,
    orders: true,
    uptime: true,
    genesisAge: true,
  },
};

/** Mocked sequencer identity shown in expanded batch detail. */
export const MOCK_SEQUENCER = "0x4f2c···7a91";

/** Mocked clearing duration (ms). Range matches the handoff. */
export function mockClearingMs(height: number): number {
  // Deterministic for a given height so the value doesn't flicker on rerenders.
  return 180 + (hash32(height + 17) % 240);
}

/** Mocked tx hash. Derived from height so two views of the same batch agree. */
export function mockTxHash(height: number): string {
  const a = hash32(height * 3 + 1).toString(16).padStart(8, "0");
  const b = hash32(height * 3 + 2).toString(16).padStart(8, "0");
  const c = hash32(height * 3 + 3).toString(16).padStart(8, "0");
  return `0x${a}${b}···${c}`;
}

/** Deterministic imbalance in basis points (-1000..+1000 ≙ ±10%). */
export function mockImbalanceBps(marketId: number, height: number): number {
  return (hash32(marketId * 911 + height) % 2001) - 1000;
}

/**
 * Split a block-level total proportionally across markets that cleared this batch.
 * Used for per-market volume / welfare / order counts until the backend
 * denormalizes `market_id` onto FillResponse and breaks welfare down by market.
 *
 * The split is uniform across cleared markets — we have no signal to weight
 * by today. Caller passes the list of cleared market_ids; we return a Map
 * with `marketId → share` (bigint for money, integer for counts).
 */
export function splitBigintByMarket(
  total: bigint,
  marketIds: number[]
): Map<number, bigint> {
  const out = new Map<number, bigint>();
  if (marketIds.length === 0) return out;
  const each = total / BigInt(marketIds.length);
  let remainder = total - each * BigInt(marketIds.length);
  for (const id of marketIds) {
    // Drip the remainder onto the first markets so the sum is exact.
    const add = remainder > 0n ? 1n : 0n;
    if (remainder > 0n) remainder -= 1n;
    out.set(id, each + add);
  }
  return out;
}

export function splitIntByMarket(
  total: number,
  marketIds: number[]
): Map<number, number> {
  const out = new Map<number, number>();
  if (marketIds.length === 0) return out;
  const each = Math.floor(total / marketIds.length);
  let remainder = total - each * marketIds.length;
  for (const id of marketIds) {
    const add = remainder > 0 ? 1 : 0;
    if (remainder > 0) remainder -= 1;
    out.set(id, each + add);
  }
  return out;
}

/** Cheap 32-bit hash for deterministic mocks. xmur3 variant. */
function hash32(n: number): number {
  let h = (n | 0) ^ 0x9e3779b9;
  h = Math.imul(h ^ (h >>> 16), 0x85ebca6b);
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35);
  h ^= h >>> 16;
  return h >>> 0;
}
