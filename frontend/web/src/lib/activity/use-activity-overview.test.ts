import { describe, expect, it } from "vitest";
import type { components } from "../api/schema";
import { countLiveProductMarkets } from "./use-activity-overview";

type MarketSummary = components["schemas"]["MarketSummaryResponse"];

function market(
  marketId: number,
  eventId: string | null = null,
  overrides: Partial<MarketSummary> = {},
): MarketSummary {
  return {
    market_id: marketId,
    name: `Market ${marketId}`,
    event_id: eventId,
    yes_price_nanos: null,
    no_price_nanos: null,
    reference_price_nanos: null,
    reference_price_expires_at_ms: null,
    volume_nanos: "0",
    status: "active",
    trader_count: 0,
    volume_24h_nanos: "0",
    yes_price_24h_ago_nanos: null,
    no_price_24h_ago_nanos: null,
    liquidity_avg10_nanos: "0",
    liquidity_band_nanos: "0",
    orders_placed_total: 0,
    orders_matched_total: 0,
    orders_unmatched_total: 0,
    ...overrides,
  };
}

describe("countLiveProductMarkets", () => {
  it("counts one visible card for grouped component markets", () => {
    expect(
      countLiveProductMarkets([
        market(1, "event-a"),
        market(2, "event-a"),
        market(3, "event-b"),
        market(4),
      ]),
    ).toBe(3);
  });

  it("excludes resolved and externally closed components", () => {
    expect(
      countLiveProductMarkets([
        market(1, "event-a", { closed: true }),
        market(2, "event-a", { closed: true }),
        market(3, "event-b", { status: "resolved" }),
        market(4),
      ]),
    ).toBe(1);
  });
});
