import { describe, expect, it } from "vitest";
import { deriveBatchWindowStats } from "./derive-batch-windows";
import type { Block } from "./types";

/**
 * Minimal block builder. We only populate the per-market sidecar and the
 * fields the derivation actually reads, then cast to the full Block shape.
 */
type Sidecar = { placed?: number; matched?: number; volume_nanos?: string };
const blk = (
  height: number,
  byMarket: Record<number, Sidecar>,
): Block => {
  const by_market: Record<string, Sidecar> = {};
  for (const [id, stats] of Object.entries(byMarket)) by_market[id] = stats;
  return { height, by_market } as unknown as Block;
};

describe("deriveBatchWindowStats", () => {
  // Newest-first, like the store's ring buffer.
  const blocks: Block[] = [
    blk(30, { 230: { placed: 5, matched: 3, volume_nanos: "1000" }, 999: { placed: 9, matched: 9, volume_nanos: "9999" } }),
    blk(20, { 230: { placed: 2, matched: 1, volume_nanos: "500" }, 999: { placed: 9, matched: 9, volume_nanos: "9999" } }),
    blk(10, { 230: { placed: 4, matched: 4, volume_nanos: "300" }, 999: { placed: 9, matched: 9, volume_nanos: "9999" } }),
  ];

  it("sums placed, matched, and volume for the target market across the window", () => {
    const s = deriveBatchWindowStats(230, blocks, 10);
    expect(s.ordersPlaced).toBe(11); // 5 + 2 + 4
    expect(s.ordersMatched).toBe(8); // 3 + 1 + 4
    expect(s.volumeMatchedNanos).toBe(1800n); // 1000 + 500 + 300
  });

  it("ignores other markets' sidecar entries", () => {
    const s = deriveBatchWindowStats(230, blocks, 10);
    // If market 999 leaked in, placed would be 11 + 27 = 38.
    expect(s.ordersPlaced).toBe(11);
    expect(s.volumeMatchedNanos).toBe(1800n);
  });

  it("computes avg volume per batch as matched volume / blocks in window", () => {
    const s = deriveBatchWindowStats(230, blocks, 10);
    expect(s.actualBlockCount).toBe(3);
    expect(s.avgVolumePerBatchNanos).toBe(600n); // 1800 / 3
  });

  it("respects the window size and reports first/last heights", () => {
    const s = deriveBatchWindowStats(230, blocks, 1);
    expect(s.actualBlockCount).toBe(1);
    expect(s.ordersPlaced).toBe(5); // newest block only
    expect(s.volumeMatchedNanos).toBe(1000n);
    expect(s.avgVolumePerBatchNanos).toBe(1000n);
    expect(s.lastHeight).toBe(30); // newest
    expect(s.firstHeight).toBe(30); // also newest — single block
  });

  it("reports oldest as firstHeight and newest as lastHeight over a multi-block window", () => {
    const s = deriveBatchWindowStats(230, blocks, 10);
    expect(s.firstHeight).toBe(10);
    expect(s.lastHeight).toBe(30);
  });

  it("treats blocks missing the market's sidecar entry as zero", () => {
    const withGap: Block[] = [
      blk(40, { 230: { placed: 3, matched: 2, volume_nanos: "700" } }),
      blk(39, { 999: { placed: 5, matched: 5, volume_nanos: "5000" } }), // no 230 entry
    ];
    const s = deriveBatchWindowStats(230, withGap, 10);
    expect(s.ordersPlaced).toBe(3);
    expect(s.ordersMatched).toBe(2);
    expect(s.volumeMatchedNanos).toBe(700n);
    expect(s.actualBlockCount).toBe(2);
    expect(s.avgVolumePerBatchNanos).toBe(350n); // 700 / 2
  });

  it("returns zeros and null heights for an empty window", () => {
    const s = deriveBatchWindowStats(230, [], 10);
    expect(s.actualBlockCount).toBe(0);
    expect(s.ordersPlaced).toBe(0);
    expect(s.ordersMatched).toBe(0);
    expect(s.volumeMatchedNanos).toBe(0n);
    expect(s.avgVolumePerBatchNanos).toBe(0n);
    expect(s.firstHeight).toBeNull();
    expect(s.lastHeight).toBeNull();
  });
});
