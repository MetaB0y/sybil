/**
 * Pure derivation of the lifetime market-stats panel.
 *
 * Inputs: a `Market` (from `useMarket`) + an optional `LatestBlock` (from the
 * store). No React, no fetches, no store access — safe to unit test.
 *
 * After Phase B landed:
 *  - totalVolume: REAL (MarketResponse.volume_nanos)
 *  - 24h volume: REAL (MarketResponse.volume_24h_nanos, B2)
 *  - traders: REAL (MarketResponse.trader_count, B1)
 *  - liquidity: REAL (MarketResponse.liquidity_avg10_nanos, B4)
 *  - batchesExistedFor: APPROX from timestamps (still tracked as #9).
 *
 * Each "REAL" field is `#[serde(default)]` on the wire — markets with no
 * activity yet (or pre-Phase-B trackers) report 0. We surface that 0
 * directly instead of falling back to a synthesized mock, so the UI is
 * always truthful.
 */

import { parseNanos } from "../format/nanos";
import type { Block, Market, MarketStats } from "./types";

/** 2-second uniform clearing cadence. See frontend/CLAUDE.md. */
const BATCH_INTERVAL_MS = 2_000;

export function deriveMarketStats(
  market: Market,
  latestBlock: Block | null,
): MarketStats {
  const marketId = market.market_id;
  const totalVolumeNanos = market.volume_nanos
    ? parseNanos(market.volume_nanos)
    : 0n;

  return {
    marketId,
    totalVolumeNanos,
    volume24hNanos: parseNanos(market.volume_24h_nanos ?? 0),
    traders: market.trader_count ?? 0,
    liquidityNanos: parseNanos(market.liquidity_avg10_nanos ?? 0),
    ...batchesExistedFor(market, latestBlock),
    mocked: {
      volume24h: false,
      traders: false,
      liquidity: false,
    },
  };
}

/**
 * Approximate count of batches the market has existed for, using the
 * timestamp delta between `latestBlock.timestamp_ms` and `market.created_at_ms`.
 * Exact only if blocks land on a perfect 2s grid — flagged as approximate
 * so the UI can show a tilde/asterisk. See OPEN_QUESTIONS #9.
 */
function batchesExistedFor(
  market: Market,
  latestBlock: Block | null,
): { batchesExistedFor: number | null; batchesExistedIsApproximate: boolean } {
  const createdMs = market.created_at_ms ?? null;
  if (createdMs == null || latestBlock == null) {
    return { batchesExistedFor: null, batchesExistedIsApproximate: true };
  }
  const elapsedMs = latestBlock.timestamp_ms - createdMs;
  if (elapsedMs < 0) {
    return { batchesExistedFor: 0, batchesExistedIsApproximate: true };
  }
  // +1 so a brand-new market created in this batch reads as "1 batch existed"
  // rather than "0".
  const count = Math.floor(elapsedMs / BATCH_INTERVAL_MS) + 1;
  return { batchesExistedFor: count, batchesExistedIsApproximate: true };
}
