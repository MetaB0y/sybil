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
import { deriveShortLabels } from "@/lib/market-detail/outcome-labels";

export type EventOutcome = {
  marketId: number;
  /** Full label for this outcome — the market's `name` field. For multi-outcome
   *  Polymarket events the mirror sets `name` to the full outcome question. */
  label: string;
  /** `label` with the question text shared by all siblings stripped, so the
   *  picker/CTA show "(LOW) $70" rather than the whole sentence. Equals
   *  `label` for singleton (binary) groups. See `outcome-labels.ts`. */
  shortLabel: string;
  yesPriceNanos: bigint | null;
  noPriceNanos: bigint | null;
  /** YES price in integer cents (0..100), or `null` while prices haven't
   *  arrived. Provided as a convenience for picker UI. */
  yesCents: number | null;
  /** Signed 24h delta in cents, derived from `yes_price_24h_ago_nanos`. `0`
   *  when the 24h-ago snapshot is missing (market younger than 24h or wiped on
   *  restart). */
  delta24Cents: number;
  /** Rolling 24h trading volume in nanos ($). Used to rank chart lines. */
  volume24hNanos: bigint;
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

    // Short labels need the full sibling set — derive once, index-aligned.
    const shortLabels = deriveShortLabels(siblings.map((m) => m.name));

    const outcomes: EventOutcome[] = siblings.map((m, i) => {
      const price = prices[m.market_id];
      const yesNanos = price?.yes ?? null;
      const noNanos = price?.no ?? null;
      const yesCents = yesNanos == null ? null : Math.round(Number(yesNanos) / 1e7);
      // Real 24h delta from the list snapshot (current YES − YES 24h ago), in
      // cents; both fields ride `/v1/markets`. Self-consistent (same payload)
      // and independent of the live store price.
      const curYes = m.yes_price_nanos != null ? BigInt(m.yes_price_nanos) : null;
      const agoYes =
        m.yes_price_24h_ago_nanos != null
          ? BigInt(m.yes_price_24h_ago_nanos)
          : null;
      const delta24Cents =
        curYes != null && agoYes != null ? Number(curYes - agoYes) / 1e7 : 0;
      return {
        marketId: m.market_id,
        label: m.name,
        shortLabel: shortLabels[i] ?? m.name,
        yesPriceNanos: yesNanos,
        noPriceNanos: noNanos,
        yesCents,
        delta24Cents,
        volume24hNanos: m.volume_24h_nanos ? BigInt(m.volume_24h_nanos) : 0n,
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
