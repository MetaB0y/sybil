/**
 * Mirror the sequencer complete-set self-trade rule so the UI can explain and
 * pre-empt a rejection before asking the user to sign.
 *
 * Only real protocol MarketGroups count. Sharing an event id is not enough.
 */

export type OrderSideName = "BuyYes" | "BuyNo" | "SellYes" | "SellNo";

export type CoverageOrder = {
  order_id: number;
  market_id: number;
  side: string;
  limit_price_nanos: string | number | bigint;
};

const ONE_DOLLAR_NANOS = 1_000_000_000n;

function asNanos(value: string | number | bigint): bigint {
  return typeof value === "bigint" ? value : BigInt(value);
}

export function findCompleteSetBlockers({
  groupMarkets,
  restingOrders,
  marketId,
  side,
  limitPriceNanos,
}: {
  groupMarkets: readonly number[];
  restingOrders: readonly CoverageOrder[];
  marketId: number;
  side: OrderSideName;
  limitPriceNanos: string | number | bigint;
}): CoverageOrder[] | null {
  if (side !== "BuyYes" && side !== "BuyNo") return null;

  const hasProtocolGroup = groupMarkets.includes(marketId);
  if (!hasProtocolGroup) return null;
  const relevantOrders = restingOrders.filter((order) =>
    groupMarkets.includes(order.market_id),
  );
  const opposite = side === "BuyYes" ? "BuyNo" : "BuyYes";
  const crossingSameMarket = relevantOrders.filter(
    (order) =>
      order.market_id === marketId &&
      order.side === opposite &&
      asNanos(order.limit_price_nanos) + asNanos(limitPriceNanos) >=
        ONE_DOLLAR_NANOS,
  );
  if (crossingSameMarket.length > 0) return crossingSameMarket;

  if (side !== "BuyYes") return null;

  const highestYesByMarket = new Map<number, CoverageOrder>();
  for (const order of relevantOrders) {
    if (order.side !== "BuyYes") continue;
    const current = highestYesByMarket.get(order.market_id);
    if (
      current == null ||
      asNanos(order.limit_price_nanos) > asNanos(current.limit_price_nanos)
    ) {
      highestYesByMarket.set(order.market_id, order);
    }
  }
  highestYesByMarket.delete(marketId);
  if (highestYesByMarket.size !== groupMarkets.length - 1) return null;

  const blockers = [...highestYesByMarket.values()];
  const totalLimit = blockers.reduce(
    (sum, order) => sum + asNanos(order.limit_price_nanos),
    asNanos(limitPriceNanos),
  );
  return totalLimit >= ONE_DOLLAR_NANOS ? blockers : null;
}

export function completeSetReason(
  blockers: readonly CoverageOrder[],
  side: OrderSideName,
  marketId: number,
  labelOf: (marketId: number) => string | null,
  noun: "bet" | "order" = "bet",
): string {
  const opposite = side === "BuyNo" ? "YES" : "NO";
  const verb = noun === "bet" ? "bet" : "buy";
  const sameMarket = blockers.find((order) => order.market_id === marketId);
  if (sameMarket) {
    return `Your open ${opposite} order on this outcome blocks it — cancel that order to ${verb} ${side === "BuyNo" ? "NO" : "YES"}.`;
  }
  const only = blockers.length === 1 ? blockers[0] : undefined;
  if (only) {
    const label = labelOf(only.market_id);
    return `Your open order${label ? ` on ${label}` : ""} already covers the other outcomes — cancel it to ${verb} here.`;
  }
  return `Your ${blockers.length} open orders in this event already cover the other outcomes — cancel one to ${verb} here.`;
}
