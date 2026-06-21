/**
 * Hook for the Activity page hero + 24h pulse strip. Both blocks come from
 * `GET /v1/activity/overview` — the `all_time` and `last_24h` buckets.
 *
 * Every figure is real and reads `"—"` / `null` until the first response
 * lands.
 *
 * `last_24h` equals `all_time` while the chain is younger than 24h (it runs
 * in-memory and resets on redeploy) — correct, not a bug.
 */

"use client";

import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import type { components } from "../api/schema";
import { formatCompactDollars, parseNanos } from "../format/nanos";
import { selectLatestBlock, useStore } from "../store";
import type { ActivityOverview, AllTimeStats, Last24hStats } from "./types";

export type UseActivityOverviewResult = ActivityOverview & {
  isLoading: boolean;
};

type OverviewBucket = components["schemas"]["OverviewBucketResponse"];

export function useActivityOverview(): UseActivityOverviewResult {
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

  // All-time + 24h numbers — slow-moving, so a lazy poll is plenty.
  const overviewQ = useQuery({
    queryKey: ["activity-overview"],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/activity/overview");
      if (error || !data) throw new Error("/v1/activity/overview failed");
      return data;
    },
    refetchInterval: 10_000,
  });

  const liveMarkets = summaryQ.data
    ? summaryQ.data.filter((m) => m.status === "active").length
    : 0;

  // Real figures from /v1/activity/overview. `null` / "—" until the first
  // response lands, so the page never flashes a stale mock as if real.
  const ov = overviewQ.data;
  const allTime: AllTimeStats = {
    ...bucketStats(ov?.all_time),
    totalBatches: latestBlock?.height ?? 0,
    liveMarkets,
  };
  const last24h: Last24hStats = bucketStats(ov?.last_24h);

  return {
    allTime,
    last24h,
    isLoading: summaryQ.isPending || overviewQ.isPending,
  };
}

/** Shape one `/v1/activity/overview` bucket into display stats. */
function bucketStats(bucket: OverviewBucket | undefined): Last24hStats {
  if (!bucket) {
    return {
      matchedVolume: "—",
      welfare: "—",
      traders: null,
      ordersPlaced: null,
      ordersMatched: null,
      ordersUnmatched: null,
    };
  }
  return {
    matchedVolume: formatCompactDollars(
      parseNanos(bucket.total_volume_nanos ?? 0)
    ),
    welfare: formatCompactDollars(parseNanos(bucket.total_welfare_nanos ?? 0)),
    traders: bucket.unique_traders ?? 0,
    ordersPlaced: bucket.orders?.placed ?? 0,
    ordersMatched: bucket.orders?.matched ?? 0,
    ordersUnmatched: bucket.orders?.unmatched ?? 0,
  };
}
