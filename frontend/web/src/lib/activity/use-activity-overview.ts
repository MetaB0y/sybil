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
import { BLOCK_INTERVAL_MS } from "../constants";
import { selectLatestBlock, useStore } from "../store";
import type { ActivityOverview, AllTimeStats, Last24hStats } from "./types";

export type UseActivityOverviewResult = ActivityOverview & {
  isLoading: boolean;
  state: ActivityReadState;
  isRetrying: boolean;
  retryFailed: () => Promise<void>;
};

export type ActivityReadState =
  | "loading"
  | "unavailable"
  | "stale"
  | "ready";

type ReadSource = {
  hasData: boolean;
  isPending: boolean;
  isFetching: boolean;
  error: unknown;
  refetch: () => Promise<unknown>;
};

type OverviewBucket = components["schemas"]["OverviewBucketResponse"];

export function useActivityOverview(): UseActivityOverviewResult {
  const latestBlock = useStore(selectLatestBlock);

  // The realtime provider normally owns latest-block hydration. Activity also
  // needs an independently retryable read so a failed global hydration cannot
  // turn the chain height into an authoritative zero.
  const latestBlockQ = useQuery({
    queryKey: ["activity-latest-block"],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/blocks/latest");
      if (error || !data) throw new Error("/v1/blocks/latest failed");
      return data;
    },
    enabled: latestBlock == null,
    staleTime: 10_000,
    refetchInterval: latestBlock == null ? BLOCK_INTERVAL_MS : false,
  });

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
    : null;
  const block = latestBlock ?? latestBlockQ.data ?? null;

  // Real figures from /v1/activity/overview. `null` / "—" until the first
  // response lands, so the page never flashes a stale mock as if real.
  const ov = overviewQ.data;
  const allTime: AllTimeStats = {
    ...bucketStats(ov?.all_time),
    totalBatches: block?.height ?? null,
    liveMarkets,
  };
  const last24h: Last24hStats = bucketStats(ov?.last_24h);

  const sources: ReadSource[] = [
    querySource(summaryQ),
    querySource(overviewQ),
    latestBlock == null
      ? querySource(latestBlockQ)
      : {
          hasData: true,
          isPending: false,
          isFetching: false,
          error: null,
          refetch: latestBlockQ.refetch,
        },
  ];
  const state = deriveActivityReadState(sources);
  const failed = sources.filter((source) => source.error != null);

  return {
    allTime,
    last24h,
    isLoading: state === "loading",
    state,
    isRetrying: failed.some((source) => source.isFetching),
    retryFailed: async () => {
      await Promise.all(failed.map((source) => source.refetch()));
    },
  };
}

export function deriveActivityReadState(
  sources: Array<Pick<ReadSource, "hasData" | "isPending" | "error">>,
): ActivityReadState {
  if (sources.some((source) => source.error != null && !source.hasData)) {
    return "unavailable";
  }
  if (sources.some((source) => source.isPending && !source.hasData)) {
    return "loading";
  }
  if (sources.some((source) => source.error != null)) return "stale";
  return "ready";
}

function querySource(query: {
  data: unknown;
  isPending: boolean;
  isFetching: boolean;
  error: unknown;
  refetch: () => Promise<unknown>;
}): ReadSource {
  return {
    hasData: query.data !== undefined,
    isPending: query.isPending,
    isFetching: query.isFetching,
    error: query.error,
    refetch: query.refetch,
  };
}

/** Shape one `/v1/activity/overview` bucket into display stats. */
function bucketStats(bucket: OverviewBucket | undefined): Last24hStats {
  if (!bucket) {
    return {
      matchedVolume: "—",
      welfare: "—",
      traders: null,
      ordersPlacedDistinct: null,
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
    ordersPlacedDistinct: bucket.orders?.placed_distinct ?? 0,
    ordersMatched: bucket.orders?.matched ?? 0,
    ordersUnmatched: bucket.orders?.unmatched ?? 0,
  };
}
