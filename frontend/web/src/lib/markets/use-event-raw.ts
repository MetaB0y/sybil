"use client";

import { useQuery } from "@tanstack/react-query";

/** The subset of a raw Polymarket market we read from the event snapshot. */
export type RawEventMarket = {
  conditionId?: string;
  groupItemTitle?: string;
  startDate?: string;
};

const DEFAULT_API_BASE = "https://172-104-31-54.nip.io";
const baseUrl = process.env.NEXT_PUBLIC_API_BASE ?? DEFAULT_API_BASE;

/**
 * Fetch the raw Polymarket event JSON (`GET /v1/events/{id}/raw`) and index its
 * markets by `conditionId`. Lets a card resolve per-market fields that aren't on
 * `MarketResponse` (e.g. `groupItemTitle`) by joining on the market's
 * `polymarket_condition_id`.
 *
 * Untyped on purpose — the `/raw` endpoint is a passthrough blob with no OpenAPI
 * schema. Lazy via `enabled` (so only in-view cards fetch) and cached per event
 * with a long `staleTime` (the snapshot changes at most once per mirror cycle).
 */
export function useEventRaw(eventId: string | undefined, enabled: boolean) {
  return useQuery({
    queryKey: ["event-raw", eventId],
    enabled: enabled && !!eventId,
    staleTime: 30 * 60_000,
    gcTime: 60 * 60_000,
    retry: 1,
    queryFn: async (): Promise<Map<string, RawEventMarket>> => {
      const res = await fetch(`${baseUrl}/v1/events/${eventId}/raw`);
      if (!res.ok) return new Map();
      const ev = (await res.json()) as { markets?: RawEventMarket[] };
      const map = new Map<string, RawEventMarket>();
      for (const m of ev.markets ?? []) {
        if (m.conditionId) map.set(m.conditionId, m);
      }
      return map;
    },
  });
}
