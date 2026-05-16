"use client";

import { useMemo } from "react";
import { useMarket } from "@/lib/markets/use-market";
import { selectLatestBlock, useStore } from "@/lib/store";
import { deriveMarketStats } from "./derive-market-stats";
import type { MarketStats } from "./types";

/**
 * Hook for the lifetime market-stats panel. Returns `stats: null` while
 * `useMarket` is still loading or errored; the page is responsible for the
 * loading/error UI.
 *
 * Re-derives when either the market metadata or the latest block changes —
 * "batches existed for" ticks every block.
 */
export function useMarketStats(marketId: number): {
  stats: MarketStats | null;
  isPending: boolean;
  isError: boolean;
} {
  const marketQ = useMarket(marketId);
  const latestBlock = useStore(selectLatestBlock);

  const stats = useMemo(() => {
    if (!marketQ.data) return null;
    return deriveMarketStats(marketQ.data, latestBlock);
  }, [marketQ.data, latestBlock]);

  return {
    stats,
    isPending: marketQ.isPending,
    isError: marketQ.isError,
  };
}
