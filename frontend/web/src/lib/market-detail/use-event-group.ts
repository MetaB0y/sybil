"use client";

/**
 * Resolve the event-group context for a specific market.
 *
 * Sybil markets are individually binary (YES/NO). Polymarket-mirrored events
 * with multiple outcomes (e.g. "FOMC March: 25bp cut / Hold / 50bp cut /
 * 25bp hike") are modelled as N separate binary markets that share an
 * `event_id`. The frontend groups them client-side for display.
 *
 * - When the market has an `event_id` and ≥2 siblings exist: returns the
 *   group with `isMultiOutcome: true`. Outcomes are sorted by descending
 *   YES probability so the favourite is first.
 * - When the market has no `event_id` OR is the only sibling: returns a
 *   singleton group with `isMultiOutcome: false`. The outcome picker UI
 *   should hide itself in that case; only YES/NO toggle applies.
 *
 * Reuses `useMarketsList` so we don't re-fetch /v1/markets — the markets
 * index already pulls it.
 */

import { useMemo } from "react";
import { useMarketsList } from "@/lib/markets/use-markets";
import { selectPricesByMarketId, useStore } from "@/lib/store";
import { mockDelta } from "@/lib/mock";

export type EventOutcome = {
  marketId: number;
  /** Display label for this outcome inside the picker. Today this is the
   *  market's `name` field — for multi-outcome Polymarket events the mirror
   *  sets `name` to the outcome description (e.g. "25bp cut"). For sybil-native
   *  binaries the name is just the market title. */
  label: string;
  yesPriceNanos: bigint | null;
  noPriceNanos: bigint | null;
  /** YES price in integer cents (0..100), or `null` while prices haven't
   *  arrived. Provided as a convenience for picker UI. */
  yesCents: number | null;
  /** Signed 24h delta in cents. MOCK — see `lib/mock.ts:33`. */
  delta24Cents: number;
};

export type EventGroup = {
  eventId: string | null;
  eventTitle: string | null;
  isMultiOutcome: boolean;
  /** Outcomes sorted by descending YES probability. */
  outcomes: EventOutcome[];
  /** The market_id the user landed on. Always present in `outcomes`. */
  currentMarketId: number;
};

export function useEventGroup(marketId: number): {
  group: EventGroup | null;
  isPending: boolean;
} {
  const { bundle, isPending } = useMarketsList();
  const prices = useStore(selectPricesByMarketId);

  const group = useMemo<EventGroup | null>(() => {
    if (!bundle) return null;
    const currentMarket = bundle.byId.get(marketId);
    if (!currentMarket) return null;

    const siblings =
      currentMarket.event_id != null
        ? bundle.groups.find((g) => g.eventId === currentMarket.event_id)
            ?.markets ?? [currentMarket]
        : [currentMarket];

    const outcomes: EventOutcome[] = siblings.map((m) => {
      const price = prices[m.market_id];
      const yesNanos = price?.yes ?? null;
      const noNanos = price?.no ?? null;
      const yesCents = yesNanos == null ? null : Math.round(Number(yesNanos) / 1e7);
      return {
        marketId: m.market_id,
        label: m.name,
        yesPriceNanos: yesNanos,
        noPriceNanos: noNanos,
        yesCents,
        delta24Cents: mockDelta(m.market_id, yesCents),
      };
    });

    // Sort by YES probability descending — favourite first. Markets without a
    // price land last (use -1 sentinel so undefineds cluster at the bottom).
    outcomes.sort(
      (a, b) => (b.yesCents ?? -1) - (a.yesCents ?? -1),
    );

    return {
      eventId: currentMarket.event_id ?? null,
      eventTitle: currentMarket.event_title ?? null,
      isMultiOutcome: outcomes.length >= 2,
      outcomes,
      currentMarketId: marketId,
    };
  }, [bundle, marketId, prices]);

  return { group, isPending };
}
