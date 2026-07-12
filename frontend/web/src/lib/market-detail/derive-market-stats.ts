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
 *  - liquidity: REAL (MarketResponse.liquidity_avg10_nanos, B4), divided by the
 *    ring length so we show the average band depth per batch — see
 *    `avgLiquidityNanos`. The wire field is a 10-block sum despite its name.
 *  - marketAgeMs: REAL timestamp delta (created_at_ms → latest block).
 *
 * Each "REAL" field is `#[serde(default)]` on the wire — markets with no
 * activity yet (or pre-Phase-B trackers) report 0. We surface that 0
 * directly instead of falling back to a synthesized mock, so the UI is
 * always truthful.
 */

import { parseNanos } from "../format/nanos";
import { avgLiquidityNanos } from "../markets/liquidity";
import type { Block, Market, MarketStats } from "./types";

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
    liquidityNanos: avgLiquidityNanos(parseNanos(market.liquidity_avg10_nanos ?? 0)),
    marketAgeMs: marketAgeMs(market, latestBlock),
    mocked: {
      volume24h: false,
      traders: false,
      liquidity: false,
    },
  };
}

/**
 * Market age in ms — `latestBlock.timestamp_ms − market.created_at_ms`,
 * clamped at 0. `null` when `created_at_ms` is missing or there's no latest
 * block yet.
 */
function marketAgeMs(market: Market, latestBlock: Block | null): number | null {
  const createdMs = market.created_at_ms ?? null;
  if (createdMs == null || latestBlock == null) return null;
  return Math.max(0, latestBlock.timestamp_ms - createdMs);
}
