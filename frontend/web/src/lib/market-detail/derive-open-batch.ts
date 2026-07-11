/**
 * Pure derivation of the open-batch (in-flight) snapshot panel.
 *
 * Everything in this legacy diagnostic panel is deterministic mock data. The
 * product UI does not treat these values as backend capabilities.
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
