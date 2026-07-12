"use client";

/**
 * Live open-batch state for one market — polled from
 * `GET /v1/markets/{id}/open-batch` every ~1s. Backs the trade rail's
 * "traders in this batch" count and the pro hero's indicative trio.
 *
 * All real backend computations:
 *  - `uniquePlacers` — distinct non-MM traders with a resting order in the
 *    open batch (`open_batch_unique_placers`).
 *  - `indicativeYesPriceNanos` / `indicativeVolumeNanos` — a ~750ms
 *    shadow-solve over the resting book (backend C2). Price is `null` when
 *    the market has no resting orders to solve; volume is `0n` when nothing
 *    crosses.
 *
 * Polling pauses while the tab is hidden. Query state remains explicit so the
 * rail never turns an unavailable live read into a real-looking empty batch.
 * Expect placers 0 / price null on most markets — the resting book is near-empty
 * (same reason `liq` reads 0).
 */

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import { parseNanos } from "@/lib/format/nanos";
import { BLOCK_INTERVAL_MS } from "@/lib/constants";

/** Poll cadence — scales with the batch cadence (BLOCK_INTERVAL_MS) so the rail
 *  samples the open batch ~4× per window: live enough to show orders resting in,
 *  without hammering the ~750ms shadow-solve. (Was 1s, tuned to the old 2s batch.) */
const OPEN_BATCH_POLL_MS = Math.round(BLOCK_INTERVAL_MS / 4);

export type OpenBatchLive = {
  /** Distinct non-MM traders with a resting order in the open batch. */
  uniquePlacers: number;
  /** Indicative YES clearing price (nanos) from the shadow-solve, or `null`
   *  when the market has no resting orders to solve. */
  indicativeYesPriceNanos: bigint | null;
  /** Notional volume (nanos) the shadow-solve would clear — `0n` when
   *  nothing crosses. */
  indicativeVolumeNanos: bigint;
};

async function fetchOpenBatchLive(marketId: number): Promise<OpenBatchLive> {
  const { data, error } = await api.GET("/v1/markets/{id}/open-batch", {
    params: { path: { id: marketId } },
  });
  if (error || !data) {
    throw new Error(`fetch /v1/markets/${marketId}/open-batch failed`);
  }
  return {
    uniquePlacers: data.unique_placers,
    indicativeYesPriceNanos:
      data.indicative_yes_price_nanos == null
        ? null
        : parseNanos(data.indicative_yes_price_nanos),
    indicativeVolumeNanos: parseNanos(data.indicative_volume_nanos ?? 0),
  };
}

export type OpenBatchReadState = "loading" | "ready" | "unavailable" | "stale";

export type UseOpenBatchLiveResult = {
  data: OpenBatchLive | null;
  readState: OpenBatchReadState;
  isRetrying: boolean;
  retry: () => void;
};

/** Live open-batch snapshot plus truthful transport/cache state. */
export function useOpenBatchLive(marketId: number): UseOpenBatchLiveResult {
  const q = useQuery({
    queryKey: ["open-batch", marketId],
    queryFn: () => fetchOpenBatchLive(marketId),
    refetchInterval: OPEN_BATCH_POLL_MS,
  });
  const hasData = q.data !== undefined;
  return {
    data: q.data ?? null,
    readState: q.isPending
      ? "loading"
      : q.error == null
        ? "ready"
        : hasData
          ? "stale"
          : "unavailable",
    isRetrying: q.isFetching,
    retry: () => {
      void q.refetch();
    },
  };
}
