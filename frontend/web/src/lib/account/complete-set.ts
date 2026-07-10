/**
 * Mirror of the sequencer's complete-set self-trade prevention, so the UI can
 * explain (and pre-empt) a `CompleteSetFormation` rejection instead of letting
 * the bettor discover it after signing.
 *
 * The engine (`GroupCoverageTracker`, crates/matching-sequencer/src/sequencer/
 * admission.rs) tracks buy-side *outcome coverage* per account within a market
 * group, seeded from the account's resting orders in earlier blocks:
 *
 *   - BuyYes on market i  → covers outcome i
 *   - BuyNo  on market i  → covers every outcome in the group EXCEPT i
 *     (in a mutually-exclusive group, "NO on i" is "YES on all the others")
 *   - SellYes / SellNo    → contribute nothing (they reduce exposure)
 *
 * An order whose acceptance would make the account's coverage span *every*
 * outcome in the group is rejected: holding that much is a complete set, i.e.
 * minting against yourself. The classic case is a resting BuyYes on outcome i
 * plus a BuyNo on the same i — together they guarantee a $1 payout.
 *
 * Only markets that are actually in a group are subject to this. Grouping is
 * NegRisk-only and is NOT the same as sharing an `event_id`, so callers must
 * pass real group membership (GET /v1/markets/groups), never event siblings.
 *
 * Pure + data-only so it's trivially testable.
 */

/** Engine order sides, as they arrive on `PendingOrderResponse.side`. */
export type OrderSideName = "BuyYes" | "BuyNo" | "SellYes" | "SellNo";

/** The subset of a resting order this rule cares about (`PendingOrderResponse`). */
export type CoverageOrder = {
  order_id: number;
  market_id: number;
  side: string;
};

/** What a single buy contributes to coverage. Sells contribute nothing. */
function contribution(
  side: string,
  marketId: number,
  groupMarkets: readonly number[],
): number[] {
  if (side === "BuyYes") return [marketId];
  if (side === "BuyNo") return groupMarkets.filter((m) => m !== marketId);
  return [];
}

/**
 * Would placing `side` on `marketId` complete a coverage set for this account?
 *
 * Returns the resting orders responsible (those covering outcomes the new order
 * doesn't already cover itself), or `null` when the order is fine — including
 * whenever the market isn't in a group, or the order is a sell.
 */
export function findCompleteSetBlockers({
  groupMarkets,
  restingOrders,
  marketId,
  side,
}: {
  /** Every market in the group `marketId` belongs to; empty when ungrouped. */
  groupMarkets: readonly number[];
  /** The account's resting orders (any market; filtered here). */
  restingOrders: readonly CoverageOrder[];
  marketId: number;
  side: OrderSideName;
}): CoverageOrder[] | null {
  if (groupMarkets.length === 0 || !groupMarkets.includes(marketId)) return null;

  const candidate = new Set(contribution(side, marketId, groupMarkets));
  if (candidate.size === 0) return null; // a sell never completes a set

  const inGroup = restingOrders.filter((o) => groupMarkets.includes(o.market_id));

  const covered = new Set(candidate);
  for (const order of inGroup) {
    for (const m of contribution(order.side, order.market_id, groupMarkets)) {
      covered.add(m);
    }
  }
  if (covered.size < groupMarkets.length) return null;

  // Name only the orders that reach outcomes the new order can't cover alone —
  // those are what the bettor has to cancel.
  const blockers = inGroup.filter((order) =>
    contribution(order.side, order.market_id, groupMarkets).some(
      (m) => !candidate.has(m),
    ),
  );
  // Coverage completed, so at least one resting order must be responsible; fall
  // back to every in-group buy rather than claiming "blocked by nothing".
  return blockers.length > 0
    ? blockers
    : inGroup.filter(
        (o) => contribution(o.side, o.market_id, groupMarkets).length > 0,
      );
}

/**
 * One line of plain copy naming why the order is blocked and what to do.
 * `labelOf` resolves a market id to its outcome label for the multi-outcome
 * case; `noun` matches the surface's voice — "bet" in Degen, "order" in Pro.
 */
export function completeSetReason(
  blockers: readonly CoverageOrder[],
  side: OrderSideName,
  marketId: number,
  labelOf: (m: number) => string | null,
  noun: "bet" | "order" = "bet",
): string {
  const opposite = side === "BuyNo" ? "YES" : "NO";
  const verb = noun === "bet" ? "bet" : "buy";
  const sameMarket = blockers.find((b) => b.market_id === marketId);
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
