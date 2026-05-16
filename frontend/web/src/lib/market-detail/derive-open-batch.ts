/**
 * Pure derivation of the open-batch (in-flight) snapshot panel.
 *
 * Everything in this panel is mocked today (OPEN_QUESTIONS #6, #7). The
 * deriver still exists so we have one place to update when backend lands a
 * real endpoint.
 *
 * - tradersInBatch: MOCK
 * - indicativeYesPrice: MOCK (anchored to last committed price)
 * - indicativeVolume: MOCK
 * - imbalance: MOCK
 */

import {
  mockImbalanceBps,
  mockIndicativeVolumeNanos,
  mockIndicativeYesPriceNanos,
  mockTradersInOpenBatch,
} from "./mocks";
import type { Block, OpenBatchSnapshot } from "./types";

export function deriveOpenBatchSnapshot(
  marketId: number,
  latestBlock: Block | null,
  currentYesPriceNanos: bigint | null,
): OpenBatchSnapshot {
  const latestHeight = latestBlock?.height ?? 0;
  return {
    marketId,
    latestHeight: latestBlock?.height ?? null,
    tradersInBatch: mockTradersInOpenBatch(marketId, latestHeight),
    indicativeYesPriceNanos: mockIndicativeYesPriceNanos(
      marketId,
      latestHeight,
      currentYesPriceNanos,
    ),
    indicativeVolumeNanos: mockIndicativeVolumeNanos(marketId, latestHeight),
    imbalanceBps: mockImbalanceBps(marketId, latestHeight),
    mocked: {
      tradersInBatch: true,
      indicativePrice: true,
      indicativeVolume: true,
      imbalance: true,
    },
  };
}
