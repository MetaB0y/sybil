/**
 * Domain types for the specific-market page. Money fields are bigint nanos
 * (1 unit = 1e9 nanos). Counts are plain numbers.
 */

import type { components } from "../api/schema";

export type Market = components["schemas"]["MarketResponse"];
export type Block = components["schemas"]["PublicBlockResponse"];

/** Aggregate stats panel for one market. */
export type MarketStats = {
  marketId: number;
  /** Lifetime cumulative volume (real). */
  totalVolumeNanos: bigint;
  /** Last 24h volume from the persisted aggregate read model. */
  volume24hNanos: bigint;
  /** Lifetime unique traders from the persisted aggregate read model. */
  traders: number;
  /** Last-ten-batch liquidity aggregate in nanos. */
  liquidityNanos: bigint;
  /**
   * Market age in milliseconds — `latestBlock.timestamp_ms − created_at_ms`.
   * `null` when `created_at_ms` is missing or there's no latest block yet.
   * Display via `formatAge`.
   */
  marketAgeMs: number | null;
  mocked: {
    volume24h: boolean;
    traders: boolean;
    liquidity: boolean;
  };
};

/** Snapshot of the currently-open (in-flight) batch for one market. */
export type OpenBatchSnapshot = {
  marketId: number;
  /** The latest committed block's height — the batch that's open is height+1. */
  latestHeight: number | null;
  /** Traders in the open batch (mocked on the legacy diagnostic route). */
  tradersInBatch: number;
  /** Indicative clearing YES price in nanos, if the batch closed now (mocked — #7). */
  indicativeYesPriceNanos: bigint;
  /** Indicative total volume that would clear (mocked — #7). */
  indicativeVolumeNanos: bigint;
  /** Buys-vs-sells imbalance in basis points, range -10000..+10000 (mocked — #6/#7). */
  imbalanceBps: number;
  mocked: {
    tradersInBatch: boolean;
    indicativePrice: boolean;
    indicativeVolume: boolean;
    imbalance: boolean;
  };
};

/** Recent-batches window size — user-selectable on the page. */
export type WindowSize = 1 | 5 | 10 | 50;

/**
 * Rolled-up stats across the last N batches for one market. Every field is
 * real, summed from the per-block per-market sidecar (`BlockResponse.by_market`).
 */
export type BatchWindowStats = {
  marketId: number;
  /** What the user asked for. */
  requestedWindow: WindowSize;
  /** How many blocks the store actually has in this window (≤ requestedWindow). */
  actualBlockCount: number;
  /** Earliest / latest heights of the blocks that contributed. */
  firstHeight: number | null;
  lastHeight: number | null;
  /** Orders touching this market across the window (Σ `by_market[mid].placed`). */
  ordersPlaced: number;
  /** Orders that exited the book after ≥1 fill (Σ `by_market[mid].matched`). */
  ordersMatched: number;
  /** Matched volume for this market across the window (Σ `by_market[mid].volume_nanos`). */
  volumeMatchedNanos: bigint;
  /** Matched volume divided by the number of batches in the window. */
  avgVolumePerBatchNanos: bigint;
};
