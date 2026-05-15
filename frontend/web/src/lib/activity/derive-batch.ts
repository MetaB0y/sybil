/**
 * Pure derivation of batch-level + per-market data from BlockResponse(s).
 * No React, no store access, no fetches — input → output only. Suitable
 * for unit tests when Vitest lands.
 */

import { parseNanos } from "../format/nanos";
import {
  mockImbalanceBps,
  splitBigintByMarket,
  splitIntByMarket,
} from "./mocks";
import type { BatchMarketRow, BatchRow, Block } from "./types";

/** Derive the collapsed-row data for one block. */
export function deriveBatchRow(block: Block): BatchRow {
  const fills = block.fills ?? [];
  const rejections = block.rejections ?? [];

  const ordersPlaced = block.order_count;
  const ordersMatched = block.orders_filled;
  // Rejections never reach the matching stage, so the "unmatched (during clearing)"
  // count excludes them — matches the design copy "placed orders that were live
  // during matching" (cancelled is not modeled today).
  const ordersUnmatched = Math.max(
    0,
    ordersPlaced - ordersMatched - rejections.length
  );

  const uniqueTraders = countUniqueAccountIds(fills);

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
 * The block-level totals (volume, welfare, placed/matched) are split uniformly
 * across markets that cleared this batch — this is mocked until the backend
 * denormalizes market_id onto FillResponse (OPEN_QUESTIONS #4–#5). Imbalance is
 * mocked deterministically from (market_id, height) (#6).
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
  const marketIds = Object.keys(clearing)
    .map((k) => Number(k))
    .filter((n) => Number.isFinite(n))
    .sort((a, b) => a - b);

  if (marketIds.length === 0) return [];

  const totalVolume = parseNanos(block.total_volume_nanos);
  const totalWelfare = parseNanos(block.total_welfare_nanos);
  const totalPlaced = block.order_count;
  const totalMatched = block.orders_filled;

  const volByMarket = splitBigintByMarket(totalVolume, marketIds);
  const welfareByMarket = splitBigintByMarket(totalWelfare, marketIds);
  const placedByMarket = splitIntByMarket(totalPlaced, marketIds);
  const matchedByMarket = splitIntByMarket(totalMatched, marketIds);

  return marketIds.map((marketId) => {
    const yesArr = clearing[String(marketId)];
    const yesNow = yesArr && yesArr[0] != null ? parseNanos(yesArr[0]) : 0n;
    const prevYesArr = prevClearing[String(marketId)];
    const yesPrev =
      prevYesArr && prevYesArr[0] != null
        ? parseNanos(prevYesArr[0])
        : null;

    const meta = marketMeta(marketId);

    return {
      marketId,
      title: meta.title,
      category: meta.category,
      clearPriceNanos: yesNow,
      deltaNanos: yesPrev == null ? null : yesNow - yesPrev,
      matchedVolumeNanos: volByMarket.get(marketId) ?? 0n,
      welfareNanos: welfareByMarket.get(marketId) ?? 0n,
      ordersPlaced: placedByMarket.get(marketId) ?? 0,
      ordersMatched: matchedByMarket.get(marketId) ?? 0,
      imbalanceBps: mockImbalanceBps(marketId, block.height),
      mocked: {
        matchedVolume: true,
        welfare: true,
        placedMatched: true,
        imbalance: true,
      },
    };
  });
}

function countUniqueAccountIds(
  fills: { account_id?: number | null }[]
): number {
  const set = new Set<number>();
  for (const f of fills) {
    if (f.account_id != null) set.add(f.account_id);
  }
  return set.size;
}
