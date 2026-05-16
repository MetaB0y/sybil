"use client";

/**
 * Live count of distinct non-MM traders that currently have a resting order
 * in a market's open (in-flight) batch — the value behind "N traders in this
 * batch" on the trade rail.
 *
 * Polls `GET /v1/markets/{id}/open-batch` every ~1s. Each response is the
 * backend's instantaneous count (`open_batch_unique_placers` rebuilds the set
 * from the resting book per call), so the displayed value trails "now" by at
 * most one poll interval — and drops when a trader cancels, as it should.
 * react-query pauses polling while the tab is hidden.
 *
 * Expect 0 on most markets: the number is real, but real users rarely leave
 * resting orders (same reason `liq` reads 0). Returns `null` until the first
 * response lands.
 */

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";

/** Poll cadence. The FBA batch is 2s, so ~1s gives two samples per batch. */
const OPEN_BATCH_POLL_MS = 1_000;

async function fetchOpenBatchPlacers(marketId: number): Promise<number> {
  const { data, error } = await api.GET("/v1/markets/{id}/open-batch", {
    params: { path: { id: marketId } },
  });
  if (error || !data) {
    throw new Error(`fetch /v1/markets/${marketId}/open-batch failed`);
  }
  return data.unique_placers;
}

/** Open-batch unique-placer count for one market; `null` until first fetch. */
export function useOpenBatchPlacers(marketId: number): number | null {
  const q = useQuery({
    queryKey: ["open-batch", marketId],
    queryFn: () => fetchOpenBatchPlacers(marketId),
    refetchInterval: OPEN_BATCH_POLL_MS,
  });
  return q.data ?? null;
}
