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
 * Polling pauses while the tab is hidden. Returns `null` until the first
 * response lands. Expect placers 0 / price null on most markets — the
 * resting book is near-empty (same reason `liq` reads 0).
 */

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import { parseNanos } from "@/lib/format/nanos";

/** Poll cadence. The FBA batch is 2s, so ~1s gives two samples per batch. */
const OPEN_BATCH_POLL_MS = 1_000;

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

/** Live open-batch snapshot for one market; `null` until the first fetch. */
export function useOpenBatchLive(marketId: number): OpenBatchLive | null {
  const q = useQuery({
    queryKey: ["open-batch", marketId],
    queryFn: () => fetchOpenBatchLive(marketId),
    refetchInterval: OPEN_BATCH_POLL_MS,
  });
  return q.data ?? null;
}
