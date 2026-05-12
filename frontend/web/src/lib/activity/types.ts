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

/** Time-window rollup (real, derived from blocks the store has seen). */
export type WindowStats = {
  matchedVolumeNanos: bigint;
  ordersPlaced: number;
  ordersMatched: number;
  ordersUnmatched: number;
  /** Distinct account_ids across all fills in this window. */
  traders: number;
  /** How many blocks contributed — for "based on N blocks" annotations. */
  blockCount: number;
  /** Earliest / latest timestamps of the blocks that contributed, in epoch ms.
   *  `null` when the window is empty. Used to label what we *actually* have
   *  (e.g. "last 2m 34s") when the buffer can't supply a full 24h. */
  firstTimestampMs: number | null;
  lastTimestampMs: number | null;
};

/** All-time stats. Most fields are mocked (see flags + OPEN_QUESTIONS #3). */
export type AllTimeStats = {
  matchedVolume: string; // formatted, e.g. "$487.2M"
  traders: number;
  ordersPlaced: number;
  ordersMatched: number;
  ordersUnmatched: number;
  totalBatches: number; // real — from latestBlock.height
  liveMarkets: number; // real — from /v1/markets/summary status count
  uptime: string;
  genesisAge: string;
  mocked: {
    matchedVolume: boolean;
    traders: boolean;
    orders: boolean;
    uptime: boolean;
    genesisAge: boolean;
  };
};

export type ActivityOverview = {
  allTime: AllTimeStats;
  last24h: WindowStats;
  prior24h: WindowStats;
};
