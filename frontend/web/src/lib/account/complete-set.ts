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
};

function contribution(
  side: string,
  marketId: number,
  groupMarkets: readonly number[],
): number[] {
  if (side === "BuyYes") return [marketId];
  if (side === "BuyNo") return groupMarkets.filter((id) => id !== marketId);
  return [];
}

export function findCompleteSetBlockers({
  groupMarkets,
  restingOrders,
  marketId,
  side,
}: {
  groupMarkets: readonly number[];
  restingOrders: readonly CoverageOrder[];
  marketId: number;
  side: OrderSideName;
}): CoverageOrder[] | null {
  if (groupMarkets.length === 0 || !groupMarkets.includes(marketId)) return null;

  const candidate = new Set(contribution(side, marketId, groupMarkets));
  if (candidate.size === 0) return null;

  const inGroup = restingOrders.filter((order) =>
    groupMarkets.includes(order.market_id),
  );
  const covered = new Set(candidate);
  for (const order of inGroup) {
    for (const id of contribution(order.side, order.market_id, groupMarkets)) {
      covered.add(id);
    }
  }
  if (covered.size < groupMarkets.length) return null;

  const blockers = inGroup.filter((order) =>
    contribution(order.side, order.market_id, groupMarkets).some(
      (id) => !candidate.has(id),
    ),
  );
  return blockers.length > 0
    ? blockers
    : inGroup.filter(
        (order) =>
          contribution(order.side, order.market_id, groupMarkets).length > 0,
      );
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
