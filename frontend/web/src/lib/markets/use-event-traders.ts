/**
 * Per-event unique trader count.
 *
 * Per-market `trader_count` is NOT additive across an event — one account
 * trading several markets of the same event would be counted once per
 * market. The backend exposes a proper set-union via
 * `GET /v1/events/{event_id}/traders`; this hook fetches that union, one
 * query per event, cached by react-query.
 */

import { useMemo } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";

async function fetchEventTraders(eventId: string): Promise<number> {
  const { data, error } = await api.GET("/v1/events/{event_id}/traders", {
    params: { path: { event_id: eventId } },
  });
  if (error || !data) {
    throw new Error(`fetch /v1/events/${eventId}/traders failed`);
  }
  return data.trader_count;
}

/** Union trader count for an event. `undefined` eventId disables the query. */
export function useEventTraders(eventId: string | undefined) {
  return useQuery({
    queryKey: ["event-traders", eventId],
    queryFn: () => fetchEventTraders(eventId!),
    enabled: !!eventId,
    staleTime: 60_000,
  });
}

/**
 * Bulk variant for the markets-index "traders" sort: fans out one query per
 * event and returns an `eventId → count` map. Shares query keys with
 * {@link useEventTraders}, so MultiCards already on screen don't refetch.
 * `enabled` gates the fan-out — pass `false` unless the traders sort is
 * active so the ~52 requests only fire when actually needed.
 */
export function useEventTradersMap(
  eventIds: string[],
  enabled: boolean
): Map<string, number> {
  const results = useQueries({
    queries: eventIds.map((id) => ({
      queryKey: ["event-traders", id],
      queryFn: () => fetchEventTraders(id),
      enabled,
      staleTime: 60_000,
    })),
  });

  return useMemo(() => {
    const map = new Map<string, number>();
    eventIds.forEach((id, i) => {
      const count = results[i]?.data;
      if (typeof count === "number") map.set(id, count);
    });
    return map;
  }, [eventIds, results]);
}
