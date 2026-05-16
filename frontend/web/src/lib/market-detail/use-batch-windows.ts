"use client";

import { useMemo } from "react";
import { selectRecentBlocks, useStore } from "@/lib/store";
import { deriveBatchWindowStats } from "./derive-batch-windows";
import type { BatchWindowStats, WindowSize } from "./types";

export const WINDOW_SIZES: readonly WindowSize[] = [1, 5, 10, 100] as const;

/**
 * Hook for the recent-batches window panel. Re-derives whenever the store's
 * ring buffer changes (every committed block) or the user changes the
 * selected window size.
 *
 * For windowSize=100 the buffer can't supply that many blocks (cap is ~80);
 * the returned `actualBlockCount` will be lower and the UI should label it.
 */
export function useBatchWindowStats(
  marketId: number,
  windowSize: WindowSize,
): BatchWindowStats {
  const recentBlocks = useStore(selectRecentBlocks);
  return useMemo(
    () => deriveBatchWindowStats(marketId, recentBlocks, windowSize),
    [marketId, recentBlocks, windowSize],
  );
}
