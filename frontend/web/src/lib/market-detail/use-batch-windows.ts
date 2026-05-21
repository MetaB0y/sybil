"use client";

import { useMemo } from "react";
import { selectRecentBlocks, useStore } from "@/lib/store";
import { deriveBatchWindowStats } from "./derive-batch-windows";
import type { BatchWindowStats, WindowSize } from "./types";

export const WINDOW_SIZES: readonly WindowSize[] = [1, 5, 10, 50] as const;

/**
 * Hook for the recent-batches window panel. Re-derives whenever the store's
 * ring buffer changes (every committed block) or the user changes the
 * selected window size.
 *
 * The store's ring buffer caps at ~80 blocks, so every window size here fits;
 * when fewer blocks exist (early in a session) `actualBlockCount` is lower and
 * the UI labels it.
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
