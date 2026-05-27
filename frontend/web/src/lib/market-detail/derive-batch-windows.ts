/**
 * Pure derivation of the recent-batches window panel for one market.
 *
 * The store's ring buffer is capped at ~80 blocks (lib/store/index.ts), so a
 * `requestedWindow` larger than that will always be partial. We surface
 * `actualBlockCount`, `firstHeight`, and `lastHeight` so the UI can be honest
 * about what we actually have — same pattern as the activity page's
 * "buffer window" annotation.
 *
 * Every value is REAL: orders placed/matched and matched volume come straight
 * from the per-block per-market sidecar (`BlockResponse.by_market[mid]`),
 * summed across the window. No mocks.
 */

import { parseNanos } from "../format/nanos";
import type { BatchWindowStats, Block, WindowSize } from "./types";

export function deriveBatchWindowStats(
  marketId: number,
  recentBlocks: Block[],
  requestedWindow: WindowSize,
): BatchWindowStats {
  // recentBlocks is newest-first; take the first N.
  const window = recentBlocks.slice(0, requestedWindow);
  const actualBlockCount = window.length;

  if (actualBlockCount === 0) {
    return emptyWindow(marketId, requestedWindow);
  }

  const key = String(marketId);
  let ordersPlaced = 0;
  let ordersMatched = 0;
  let volumeMatchedNanos = 0n;

  for (const b of window) {
    const stats = b.by_market?.[key];
    if (!stats) continue;
    ordersPlaced += stats.placed ?? 0;
    ordersMatched += stats.matched ?? 0;
    if (stats.volume_nanos != null) {
      volumeMatchedNanos += parseNanos(stats.volume_nanos);
    }
  }

  const avgVolumePerBatchNanos = volumeMatchedNanos / BigInt(actualBlockCount);

  // Newest-first array, so index 0 is newest, last is oldest.
  const lastHeight = window[0]!.height;
  const firstHeight = window[actualBlockCount - 1]!.height;

  return {
    marketId,
    requestedWindow,
    actualBlockCount,
    firstHeight,
    lastHeight,
    ordersPlaced,
    ordersMatched,
    volumeMatchedNanos,
    avgVolumePerBatchNanos,
  };
}

function emptyWindow(
  marketId: number,
  requestedWindow: WindowSize,
): BatchWindowStats {
  return {
    marketId,
    requestedWindow,
    actualBlockCount: 0,
    firstHeight: null,
    lastHeight: null,
    ordersPlaced: 0,
    ordersMatched: 0,
    volumeMatchedNanos: 0n,
    avgVolumePerBatchNanos: 0n,
  };
}
