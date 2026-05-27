/**
 * Frontend mock values for fields the backend doesn't expose. Every consumer
 * pulls from this single module so there's one delete-site as backend
 * coverage lands.
 *
 * - Batch-detail meta strip: sequencer identity, tx hash, clearing duration —
 *   none are tracked on the backend.
 */

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
