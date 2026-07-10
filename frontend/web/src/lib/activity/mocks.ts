/**
 * Frontend mock values for fields the backend doesn't expose. Every consumer
 * pulls from this single module so there's one delete-site as backend
 * coverage lands.
 *
 * - Batch-detail proof tx: blocks seal a real `events_root`, but nothing
 *   anchors it to a chain, so there's no transaction hash to show.
 */

/**
 * Mocked tx hash, elided head···tail. Derived from height so two views of the
 * same batch agree. Kept to 8+8 hex so it fits the 280px detail sidebar beside
 * its label and the `mock` pill.
 */
export function mockTxHash(height: number): string {
  const head = hash32(height * 3 + 1).toString(16).padStart(8, "0");
  const tail = hash32(height * 3 + 3).toString(16).padStart(8, "0");
  return `0x${head}···${tail}`;
}

/** Cheap 32-bit hash for deterministic mocks. xmur3 variant. */
function hash32(n: number): number {
  let h = (n | 0) ^ 0x9e3779b9;
  h = Math.imul(h ^ (h >>> 16), 0x85ebca6b);
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35);
  h ^= h >>> 16;
  return h >>> 0;
}
