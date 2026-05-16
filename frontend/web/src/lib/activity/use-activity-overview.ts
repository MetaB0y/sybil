/**
 * Hook for the Activity page hero + 24h pulse strip.
 *
 * - All-time block: matched volume, active traders and matched / unmatched
 *   orders are real — `GET /v1/activity/overview` (`all_time` bucket).
 *   `totalBatches` = latestBlock.height; `liveMarkets` = active markets in
 *   /v1/markets/summary. Placed orders, uptime and genesis age stay mocked
 *   (see mocks.ts) — the hero wraps those with <MockValue>.
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
import { formatCompactDollars, parseNanos } from "../format/nanos";
import {
  selectLatestBlock,
  selectRecentBlocks,
  useStore,
} from "../store";
import { deriveWindowedStats } from "./derive-overview";
import { MOCK_ALL_TIME } from "./mocks";
import type { ActivityOverview, AllTimeStats, WindowStats } from "./types";

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

  // All-time hero numbers — slow-moving, so a lazy poll is plenty.
  const overviewQ = useQuery({
    queryKey: ["activity-overview"],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/activity/overview");
      if (error || !data) throw new Error("/v1/activity/overview failed");
      return data;
    },
    refetchInterval: 10_000,
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

  // Real all-time figures from /v1/activity/overview. `null` / "—" until the
  // first response lands so the hero never flashes a stale mock as if real.
  const ov = overviewQ.data;
  const allTime: AllTimeStats = {
    matchedVolume: ov
      ? formatCompactDollars(parseNanos(ov.all_time.total_volume_nanos ?? 0))
      : "—",
    traders: ov ? (ov.all_time.unique_traders ?? 0) : null,
    ordersPlaced: MOCK_ALL_TIME.ordersPlaced,
    ordersMatched: ov ? (ov.all_time.orders?.matched ?? 0) : null,
    ordersUnmatched: ov ? (ov.all_time.orders?.unmatched ?? 0) : null,
    totalBatches: latestBlock?.height ?? 0,
    liveMarkets,
    uptime: MOCK_ALL_TIME.uptime,
    genesisAge: MOCK_ALL_TIME.genesisAge,
  };

  return {
    allTime,
    last24h,
    prior24h,
    isLoading: summaryQ.isPending || overviewQ.isPending,
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
    firstTimestampMs: null,
    lastTimestampMs: null,
  };
}
