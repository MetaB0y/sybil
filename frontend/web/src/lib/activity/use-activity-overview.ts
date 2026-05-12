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
import type { ActivityOverview, WindowStats } from "./types";

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
  // the rollup stable across re-renders and matches the data we actually have.
  // Until the first block arrives, both windows are empty — that's correct,
  // the page hasn't hydrated yet.
  const { last24h, prior24h } = useMemo(() => {
    if (latestBlock == null) {
      return {
        last24h: emptyWindow(),
        prior24h: emptyWindow(),
      };
    }
    return deriveWindowedStats(recentBlocks, latestBlock.timestamp_ms);
  }, [recentBlocks, latestBlock]);

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

function emptyWindow(): WindowStats {
  return {
    matchedVolumeNanos: 0n,
    ordersPlaced: 0,
    ordersMatched: 0,
    ordersUnmatched: 0,
    traders: 0,
    blockCount: 0,
  };
}
