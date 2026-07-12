"use client";

import { useQueries, useQuery } from "@tanstack/react-query";

/** The subset of a raw Polymarket market we read from the event snapshot. */
export type RawEventMarket = {
  conditionId?: string;
  groupItemTitle?: string;
  startDate?: string;
  /** Full Polymarket question — used to label markets whose `conditionId` link
   *  is missing, by matching against a non-NegRisk market's `name`. */
  question?: string;
  /** Full Polymarket market description (usually includes the resolution
   *  criteria text). Shown on the market detail page. */
  description?: string;
  /** Resolution source URL. Frequently an empty string on Gamma (the criteria
   *  live in `description`); rendered as a link only when non-empty. */
  resolutionSource?: string;
  /** Real NegRisk (mutually-exclusive outcomes) flag from the Gamma event,
   *  mirrored onto every market. Lets the chart stack only true NegRisk events
   *  instead of inferring it from a price sum. */
  negRisk?: boolean;
};

const DEFAULT_API_BASE = "https://172-104-31-54.nip.io";
const baseUrl = process.env.NEXT_PUBLIC_API_BASE ?? DEFAULT_API_BASE;

const RAW_STALE_MS = 30 * 60_000;
const RAW_GC_MS = 60 * 60_000;

/**
 * Only Polymarket-mirrored events have a raw snapshot at `/v1/events/{id}/raw`.
 * Sybil-native events (id prefixed `native:`) have none — requesting one is a
 * guaranteed 400 — so skip the fetch for them.
 */
function hasRawSnapshot(eventId: string | undefined): eventId is string {
  return !!eventId && !eventId.startsWith("native:");
}

/**
 * Fetch one event's raw Polymarket JSON and index its markets by `conditionId`.
 * Kept separate from the hook so the query contract remains easy to test.
 */
async function fetchEventRawMap(
  eventId: string,
): Promise<Map<string, RawEventMarket>> {
  const res = await fetch(`${baseUrl}/v1/events/${eventId}/raw`);
  if (!res.ok) return new Map();
  const ev = (await res.json()) as { markets?: RawEventMarket[] };
  const map = new Map<string, RawEventMarket>();
  for (const m of ev.markets ?? []) {
    if (m.conditionId) map.set(m.conditionId, m);
  }
  return map;
}

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
    enabled: enabled && hasRawSnapshot(eventId),
    staleTime: RAW_STALE_MS,
    gcTime: RAW_GC_MS,
    retry: 1,
    queryFn: () => fetchEventRawMap(eventId!),
  });
}

/**
 * Resolve natural Polymarket questions for the events currently represented in
 * a portfolio. The queries share `useEventRaw`'s cache; native events are
 * skipped because they have no raw snapshot and callers retain their names.
 */
export function useEventQuestions(eventIds: string[]): Map<string, string> {
  return useQueries({
    queries: eventIds.filter(hasRawSnapshot).map((id) => ({
      queryKey: ["event-raw", id],
      staleTime: RAW_STALE_MS,
      gcTime: RAW_GC_MS,
      retry: 1,
      queryFn: () => fetchEventRawMap(id),
    })),
    combine: (results) => {
      const byCondition = new Map<string, string>();
      for (const result of results) {
        if (!result.data) continue;
        for (const [conditionId, market] of result.data) {
          const question = market.question?.trim();
          if (question) byCondition.set(conditionId, question);
        }
      }
      return byCondition;
    },
  });
}
