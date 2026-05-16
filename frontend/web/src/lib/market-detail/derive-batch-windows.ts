/**
 * Pure derivation of the recent-batches window panel for one market.
 *
 * The store's ring buffer is capped at ~80 blocks (lib/store/index.ts), so a
 * `requestedWindow` of 100 will always be partial today. We surface
 * `actualBlockCount`, `firstHeight`, and `lastHeight` so the UI can be
 * honest about what we actually have — same pattern as the activity page's
 * "buffer window" annotation.
 *
 * What's real vs mocked:
 *  - uniqueTradersMatched (chain-wide): REAL — union of fills[].account_id
 *  - volumeMatched (chain-wide): REAL — sum of total_volume_nanos
 *  - per-market scoping of both: MOCK (OPEN_QUESTIONS #5)
 *  - uniqueTradersPlaced: MOCK (OPEN_QUESTIONS #8)
 *  - volumePlaced: MOCK (OPEN_QUESTIONS #8)
 */

import { parseNanos } from "../format/nanos";
import {
  mockUniquePlacersInWindow,
  mockVolumePlacedNanos,
  splitBigintUniform,
  splitIntUniform,
} from "./mocks";
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

  // Chain-level aggregates — real numbers.
  let totalOrdersPlaced = 0;
  let volumeMatchedChainWideNanos = 0n;
  const matchedTraderIds = new Set<number>();
  // Track distinct markets the window touched — used for the uniform per-market split.
  const touchedMarketIds = new Set<number>();

  for (const b of window) {
    totalOrdersPlaced += b.order_count;
    volumeMatchedChainWideNanos += parseNanos(b.total_volume_nanos);
    if (b.fills) {
      for (const f of b.fills) {
        if (f.account_id != null) matchedTraderIds.add(f.account_id);
      }
    }
    if (b.clearing_prices_nanos) {
      for (const k of Object.keys(b.clearing_prices_nanos)) {
        const id = Number(k);
        if (Number.isFinite(id)) touchedMarketIds.add(id);
      }
    }
  }

  const uniqueTradersMatchedChainWide = matchedTraderIds.size;
  const marketCount = Math.max(1, touchedMarketIds.size);

  // Per-market scoping is mocked via uniform split (OPEN_QUESTIONS #5).
  const uniqueTradersMatched = splitIntUniform(
    uniqueTradersMatchedChainWide,
    marketCount,
  );
  const volumeMatchedNanos = splitBigintUniform(
    volumeMatchedChainWideNanos,
    marketCount,
  );

  // Placed-side is fully mocked (OPEN_QUESTIONS #8).
  const uniquePlacersChainWide = mockUniquePlacersInWindow(
    marketId,
    totalOrdersPlaced,
  );
  const uniqueTradersPlaced = splitIntUniform(
    uniquePlacersChainWide,
    marketCount,
  );
  const volumePlacedNanos = mockVolumePlacedNanos(
    marketId,
    volumeMatchedNanos,
  );

  // Newest-first array, so last in array is the oldest.
  const lastHeight = window[0]!.height;
  const firstHeight = window[actualBlockCount - 1]!.height;

  return {
    marketId,
    requestedWindow,
    actualBlockCount,
    firstHeight,
    lastHeight,
    uniqueTradersPlaced,
    uniqueTradersMatched,
    uniqueTradersMatchedChainWide,
    volumePlacedNanos,
    volumeMatchedNanos,
    volumeMatchedChainWideNanos,
    mocked: {
      uniqueTradersPlaced: true,
      uniqueTradersMatched: true, // because of per-market scoping
      volumePlaced: true,
      volumeMatched: true, // because of per-market scoping
    },
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
    uniqueTradersPlaced: 0,
    uniqueTradersMatched: 0,
    uniqueTradersMatchedChainWide: 0,
    volumePlacedNanos: 0n,
    volumeMatchedNanos: 0n,
    volumeMatchedChainWideNanos: 0n,
    mocked: {
      uniqueTradersPlaced: true,
      uniqueTradersMatched: true,
      volumePlaced: true,
      volumeMatched: true,
    },
  };
}
