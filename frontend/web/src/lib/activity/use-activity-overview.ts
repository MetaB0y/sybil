/**
 * Hook for the Activity page hero + 24h pulse strip.
 *
 * - All-time block: mocked constants from `mocks.ts`, with two fields real:
 *     totalBatches  = latestBlock.height
 *     liveMarkets   = count(/v1/markets/summary where status === "active")
 *   Everything else is flagged via `mocked.*` so the UI can wrap with
 *   <MockValue>. Replaced wholesale when /v1/activity/overview lands.
 *
 * - Last-24h and prior-24h: derived from whatever blocks the store has.
 *   The result is partial — at 60s cadence a full 24h is ~1440 blocks and
 *   we only carry 80 in the ring buffer. The UI surfaces `blockCount` so we
 *   can be honest ("based on N blocks").
 */

"use client";

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import {
  selectLatestBlock,
  selectRecentBlocks,
  useStore,
} from "../store";
import { deriveWindowedStats } from "./derive-overview";
import { MOCK_ALL_TIME } from "./mocks";
import type { ActivityOverview } from "./types";

export type UseActivityOverviewResult = ActivityOverview & {
  isLoading: boolean;
};

export function useActivityOverview(): UseActivityOverviewResult {
  const recentBlocks = useStore(selectRecentBlocks);
  const latestBlock = useStore(selectLatestBlock);

  const summaryQ = useQuery({
    queryKey: ["markets-summary"],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/markets/summary");
      if (error || !data) throw new Error("/v1/markets/summary failed");
      return data;
    },
    staleTime: 60_000,
  });

  // Anchor the window to the latest block's timestamp, not wall-clock. Keeps
  // the rollup stable as React re-renders between live blocks and matches
  // the actual data we have.
  const nowMs = latestBlock?.timestamp_ms ?? Date.now();

  const { last24h, prior24h } = useMemo(
    () => deriveWindowedStats(recentBlocks, nowMs),
    [recentBlocks, nowMs]
  );

  const liveMarkets = summaryQ.data
    ? summaryQ.data.filter((m) => m.status === "active").length
    : 0;

  const allTime = {
    ...MOCK_ALL_TIME,
    totalBatches: latestBlock?.height ?? 0,
    liveMarkets,
  };

  return {
    allTime,
    last24h,
    prior24h,
    isLoading: summaryQ.isPending,
  };
}
