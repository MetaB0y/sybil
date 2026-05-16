"use client";

import { useMemo } from "react";
import {
  selectLatestBlock,
  selectPricesByMarketId,
  useStore,
} from "@/lib/store";
import { deriveOpenBatchSnapshot } from "./derive-open-batch";
import type { OpenBatchSnapshot } from "./types";

/**
 * Hook for the open-batch snapshot panel. Re-derives whenever the latest
 * committed block changes (the "open batch" is logically `latestBlock + 1`,
 * so every new committed block resets the snapshot's seed).
 *
 * Returns a non-null snapshot even before hydration (with `latestHeight: null`)
 * so the page can render skeleton-like values immediately.
 */
export function useOpenBatch(marketId: number): OpenBatchSnapshot {
  const latestBlock = useStore(selectLatestBlock);
  const prices = useStore(selectPricesByMarketId);
  const currentYes = prices[marketId]?.yes ?? null;

  return useMemo(
    () => deriveOpenBatchSnapshot(marketId, latestBlock, currentYes),
    [marketId, latestBlock, currentYes],
  );
}
