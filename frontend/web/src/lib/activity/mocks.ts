/**
 * Frontend mock values for fields the backend doesn't expose. Every consumer
 * pulls from this single module so there's one delete-site as backend
 * coverage lands.
 *
 * - 24h pulse-strip deltas: mocked ±% vs prior 24h (no prior_24h bucket).
 * - Genesis age: the chain runs in-memory — no persisted genesis timestamp.
 * - Batch-detail meta strip: sequencer identity, tx hash, clearing duration —
 *   none are tracked on the backend.
 */

/**
 * Mocked ±% deltas for the 24h pulse strip — change vs the prior 24h. The
 * `/v1/activity/overview` response has no `prior_24h` bucket and the backend
 * trackers keep only ~25 hourly buckets, so a real prior-window comparison
 * needs a backend change. The strip wraps these in a <MockValue> pill.
 */
export const MOCK_24H_DELTAS = {
  matchedVolumeDeltaPct: 12.4,
  tradersDeltaPct: 6.1,
};

/**
 * Genesis age for the Activity hero — the chain runs in-memory and has no
 * persisted genesis timestamp, so this stays a handoff-equivalent placeholder.
 * Every other hero figure is now real (see use-activity-overview.ts).
 */
export const MOCK_GENESIS_AGE = "6 mo 17 d";

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

/** Cheap 32-bit hash for deterministic mocks. xmur3 variant. */
function hash32(n: number): number {
  let h = (n | 0) ^ 0x9e3779b9;
  h = Math.imul(h ^ (h >>> 16), 0x85ebca6b);
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35);
  h ^= h >>> 16;
  return h >>> 0;
}
