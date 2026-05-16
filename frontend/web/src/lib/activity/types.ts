/**
 * Domain types for the Activity page. All money fields are bigint nanos
 * (1 unit = 1e9 nanos). Counts are plain numbers.
 */

import type { components } from "../api/schema";

export type Block = components["schemas"]["BlockResponse"];

/** One row in the batches table (collapsed). */
export type BatchRow = {
  height: number;
  timestampMs: number;
  matchedVolumeNanos: bigint;
  welfareNanos: bigint;
  ordersPlaced: number;
  ordersMatched: number;
  ordersUnmatched: number;
  /** Markets that produced at least one clearing price this batch. */
  marketsTouched: number;
  /** Distinct account_ids across this block's fills[]. */
  uniqueTraders: number;
};

/** Marks per-market fields that are placeholder values today (see OPEN_QUESTIONS #4–#6). */
export type MarketRowMockFlags = {
  matchedVolume: boolean;
  welfare: boolean;
  placedMatched: boolean;
  imbalance: boolean;
};

/** One row inside an expanded batch detail. */
export type BatchMarketRow = {
  marketId: number;
  title: string;
  category: string | null;
  /** YES-side clearing price in nanos (range 0..1e9 ≙ 0..100¢). */
  clearPriceNanos: bigint;
  /** Signed delta vs the same market's price in the previous batch, in nanos.
   *  `null` when there's no prior batch available. */
  deltaNanos: bigint | null;
  matchedVolumeNanos: bigint;
  welfareNanos: bigint;
  ordersPlaced: number;
  ordersMatched: number;
  /** Buys-vs-sells imbalance in basis points, range -10000..+10000. */
  imbalanceBps: number;
  mocked: MarketRowMockFlags;
};

/**
 * The five count/volume figures shared by the Activity hero and the 24h
 * pulse strip. Every field is real — `GET /v1/activity/overview` — and
 * reads `"—"` / `null` until the first response lands.
 */
export type Last24hStats = {
  matchedVolume: string; // formatted; "—" until loaded
  traders: number | null; // null until loaded
  ordersPlaced: number | null; // null until loaded
  ordersMatched: number | null; // null until loaded
  ordersUnmatched: number | null; // null until loaded
};

/**
 * All-time stats for the Activity hero — the shared figures plus three
 * hero-only fields. `genesisAge` is the one mocked field.
 */
export type AllTimeStats = Last24hStats & {
  totalBatches: number; // real — from latestBlock.height
  liveMarkets: number; // real — from /v1/markets/summary status count
  genesisAge: string; // mocked — not tracked
};

export type ActivityOverview = {
  allTime: AllTimeStats;
  last24h: Last24hStats;
};
