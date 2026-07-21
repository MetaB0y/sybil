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
  /** Polymarket has closed/resolved this outcome — render read-only / greyed. */
  closed: boolean;
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
  /** All-time matched volume in nanos ($), used for event-level totals. */
  volumeNanos?: bigint;
  /** Last-ten-batch near-price depth sum. Display sites divide by the ring
   * length to present average resting liquidity per batch. */
  liquidityNanos: bigint;
  /** Real market creation time (epoch ms), or `null` if unknown. Bounds how
   *  far back the price chart holds the line flat — never before the market
   *  existed. See `build-chart-series`. */
  createdAtMs: number | null;
  /** Resolution / end date (epoch ms), or `null`. For a closed outcome this is
   *  the best proxy for "when it closed" — used to sort closed outcomes most-
   *  recently-closed first in the outcome picker. */
  endDateMs: number | null;
};

export type EventGroup = {
  eventId: string | null;
  eventTitle: string | null;
  isMultiOutcome: boolean;
  /** Outcomes sorted closed-last, then by descending YES probability. */
  outcomes: EventOutcome[];
  /** The market_id the user landed on. Always present in `outcomes`. */
  currentMarketId: number;
};

export function useEventGroup(marketId: number): {
  group: EventGroup | null;
  isPending: boolean;
  isFetching: boolean;
  error: unknown;
  refetch: () => void;
} {
  const { bundle, isPending, isFetching, error, refetch } = useMarketsList();
  const prices = useStore(selectPricesByMarketId);

  const group = useMemo<EventGroup | null>(() => {
    if (!bundle) return null;
    const currentMarket = bundle.byId.get(marketId);
    if (!currentMarket) return null;

    const siblings =
      currentMarket.event_id != null
        ? (bundle.groups.find((g) => g.eventId === currentMarket.event_id)
            ?.markets ?? [currentMarket])
        : [currentMarket];

    // Short labels need the full sibling set — derive once, index-aligned.
    const shortLabels = deriveShortLabels(siblings.map((m) => m.name));

    // Each outcome is paired with the price the ordering below sorts on. That
    // key is deliberately the `/v1/markets` snapshot, NOT the live store price
    // used for display: the live price ticks every batch, and sorting on it
    // reshuffled the chip strip on its own whenever two outcomes crossed —
    // taking each outcome's palette colour with it, since colours are keyed to
    // position in this array. The snapshot only moves on a markets-list refetch.
    const ranked: { outcome: EventOutcome; rankYesNanos: bigint | null }[] =
      siblings.map((m, i) => {
        const price = prices[m.market_id];
        const yesNanos = price?.yes ?? null;
        const noNanos = price?.no ?? null;
        const yesCents =
          yesNanos == null ? null : Math.round(Number(yesNanos) / 1e7);
        // Real 24h delta from the list snapshot (current YES − YES 24h ago), in
        // cents; both fields ride `/v1/markets`. Self-consistent (same payload)
        // and independent of the live store price.
        const curYes =
          m.yes_price_nanos != null ? BigInt(m.yes_price_nanos) : null;
        const agoYes =
          m.yes_price_24h_ago_nanos != null
            ? BigInt(m.yes_price_24h_ago_nanos)
            : null;
        const delta24Cents =
          curYes != null && agoYes != null ? Number(curYes - agoYes) / 1e7 : 0;
        const outcome: EventOutcome = {
          marketId: m.market_id,
          closed: m.closed === true,
          label: m.name,
          shortLabel: shortLabels[i] ?? m.name,
          yesPriceNanos: yesNanos,
          noPriceNanos: noNanos,
          yesCents,
          delta24Cents,
          volume24hNanos: m.volume_24h_nanos ? BigInt(m.volume_24h_nanos) : 0n,
          volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
          liquidityNanos: m.liquidity_avg10_nanos
            ? BigInt(m.liquidity_avg10_nanos)
            : 0n,
          createdAtMs: m.created_at_ms ?? null,
          endDateMs: m.market_end_date_ms ?? m.expiry_timestamp_ms ?? null,
        };
        return { outcome, rankYesNanos: curYes };
      });

    // Closed outcomes always sort below open ones, so the picker/chart default
    // lands on a tradeable one; within each tier, favourite first.
    ranked.sort((a, b) => {
      if (a.outcome.closed !== b.outcome.closed)
        return a.outcome.closed ? 1 : -1;
      const av = a.rankYesNanos ?? -1n;
      const bv = b.rankYesNanos ?? -1n;
      if (av !== bv) return av > bv ? -1 : 1;
      // Ties (a fresh event where nothing has traded) resolve on market_id so
      // the order is at least deterministic instead of feed-arrival order.
      return a.outcome.marketId - b.outcome.marketId;
    });
    const outcomes = ranked.map((r) => r.outcome);

    return {
      eventId: currentMarket.event_id ?? null,
      eventTitle: currentMarket.event_title ?? null,
      isMultiOutcome: outcomes.length >= 2,
      outcomes,
      currentMarketId: marketId,
    };
  }, [bundle, marketId, prices]);

  return { group, isPending, isFetching, error, refetch };
}
