/**
 * Pure derivation of batch-level + per-market data from BlockResponse(s).
 * No React, no store access, no fetches — input → output only. Suitable
 * for unit tests when Vitest lands.
 */

import { parseNanos } from "../format/nanos";
import type { BatchMarketRow, BatchRow, Block } from "./types";

/** Derive the collapsed-row data for one block. */
export function deriveBatchRow(block: Block): BatchRow {
  const ordersPlaced = block.order_count;
  const ordersMatched = block.orders_filled;
  // Rejections never reach the matching stage, so the "unmatched (during clearing)"
  // count excludes them — matches the design copy "placed orders that were live
  // during matching" (cancelled is not modeled today).
  const ordersUnmatched = Math.max(
    0,
    ordersPlaced - ordersMatched - (block.rejection_count ?? 0)
  );

  const uniqueTraders = block.unique_placers ?? 0;

  const clearingPrices = block.clearing_prices_nanos ?? {};
  const marketsTouched = Object.keys(clearingPrices).length;

  return {
    height: block.height,
    timestampMs: block.timestamp_ms,
    matchedVolumeNanos: parseNanos(block.total_volume_nanos),
    welfareNanos: parseNanos(block.total_welfare_nanos),
    ordersPlaced,
    ordersMatched,
    ordersUnmatched,
    marketsTouched,
    uniqueTraders,
  };
}

/**
 * Per-market rows for an expanded batch detail.
 *
 * Volume, welfare and placed/matched counts are real per-market figures from
 * `BlockResponse.by_market` — `0` for a market that cleared a price but had
 * no order activity that block.
 *
 * `prev` is the immediately-previous block; pass `null` if unavailable and the
 * `deltaNanos` field will be `null` for every row.
 */
export function deriveBatchMarketRows(
  block: Block,
  prev: Block | null,
  marketMeta: (marketId: number) => {
    title: string;
    category: string | null;
  }
): BatchMarketRow[] {
  const clearing = block.clearing_prices_nanos ?? {};
  const prevClearing = prev?.clearing_prices_nanos ?? {};
  const byMarket = block.by_market ?? {};
  const marketIds = Object.keys(clearing)
    .map((k) => Number(k))
    .filter((n) => Number.isFinite(n))
    .sort((a, b) => a - b);

  if (marketIds.length === 0) return [];

  return marketIds.map((marketId) => {
    const yesArr = clearing[String(marketId)];
    const yesNow = yesArr && yesArr[0] != null ? parseNanos(yesArr[0]) : 0n;
    const prevYesArr = prevClearing[String(marketId)];
    const yesPrev =
      prevYesArr && prevYesArr[0] != null
        ? parseNanos(prevYesArr[0])
        : null;

    const meta = marketMeta(marketId);
    // Real per-market stats for this block; absent when the market cleared a
    // price but saw no non-MM order activity and no fills.
    const stats = byMarket[String(marketId)];

    return {
      marketId,
      title: meta.title,
      category: meta.category,
      clearPriceNanos: yesNow,
      deltaNanos: yesPrev == null ? null : yesNow - yesPrev,
      matchedVolumeNanos: parseNanos(stats?.volume_nanos ?? 0),
      welfareNanos: parseNanos(stats?.welfare_nanos ?? 0),
      ordersPlaced: stats?.placed ?? 0,
      ordersMatched: stats?.matched ?? 0,
    };
  });
}
