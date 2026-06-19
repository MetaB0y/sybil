import { describe, expect, it } from "vitest";
import type { components } from "@/lib/api/schema";
import { avgEntryPriceNanos } from "./positions";

type Fill = components["schemas"]["AccountFillResponse"];
type Position = components["schemas"]["PositionValueResponse"];

const ONE = 1_000_000_000n;
const YES_CLEARING = 480_000_000n; // 48¢ YES → 52¢ NO

function fill(marketId: number, outcome: string, delta: number): Fill {
  return {
    block_height: 1,
    // fill_price_nanos is always the YES clearing price, regardless of side.
    fill_price_nanos: String(YES_CLEARING),
    fill_qty: Math.abs(delta),
    order_id: 1,
    timestamp_ms: 0,
    position_deltas: [{ market_id: marketId, outcome, delta }],
  };
}

function pos(marketId: number, outcome: string, avgEntryNanos: bigint): Position {
  return {
    avg_entry_price_nanos: String(avgEntryNanos),
    current_price_nanos: "0",
    market_id: marketId,
    outcome,
    quantity: 1,
    value_nanos: "0",
  };
}

describe("avgEntryPriceNanos", () => {
  it("YES position: entry is the YES clearing price", () => {
    expect(avgEntryPriceNanos([fill(7, "YES", 18)], 7, "YES")).toBe(YES_CLEARING);
  });

  it("NO position: entry is side-adjusted ($1 − YES clearing)", () => {
    // Regression: the fills fallback used the raw YES clearing price (48¢)
    // instead of the NO price actually paid (52¢).
    expect(avgEntryPriceNanos([fill(7, "NO", 18)], 7, "NO")).toBe(ONE - YES_CLEARING);
  });

  it("prefers the backend avg_entry_price_nanos when it is > 0", () => {
    const p = pos(7, "NO", 520_000_000n);
    expect(avgEntryPriceNanos([fill(7, "NO", 18)], 7, "NO", p)).toBe(520_000_000n);
  });

  it("falls back to fills (side-adjusted) when backend avg_entry is 0", () => {
    const p = pos(7, "NO", 0n);
    expect(avgEntryPriceNanos([fill(7, "NO", 18)], 7, "NO", p)).toBe(ONE - YES_CLEARING);
  });

  it("returns null when no matching fills", () => {
    expect(avgEntryPriceNanos([], 7, "NO")).toBeNull();
  });
});
