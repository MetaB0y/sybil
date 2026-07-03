import { describe, expect, it } from "vitest";
import type { components } from "@/lib/api/schema";
import { avgEntryPriceNanos } from "./positions";

type Fill = components["schemas"]["AccountFillResponse"];
type Position = components["schemas"]["PositionValueResponse"];

const NO_PRICE = 520_000_000n; // 52¢ — the NO price actually paid

function fill(
  marketId: number,
  outcome: string,
  delta: number,
  priceNanos: bigint,
): Fill {
  return {
    block_height: 1,
    cursor: "1.1",
    // fill_price_nanos is the filled side's OWN price (a NO fill carries the
    // NO price), NOT a YES-clearing price — used directly, no flip.
    fill_price_nanos: String(priceNanos),
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
  it("YES position: entry is the fill's side price, used directly", () => {
    expect(avgEntryPriceNanos([fill(7, "YES", 18, 480_000_000n)], 7, "YES")).toBe(
      480_000_000n,
    );
  });

  it("NO position: entry is the fill's NO side price, used directly (no flip)", () => {
    // Regression: fill_price is already the NO price (52¢) — do NOT flip it to 48¢.
    expect(avgEntryPriceNanos([fill(7, "NO", 18, NO_PRICE)], 7, "NO")).toBe(NO_PRICE);
  });

  it("prefers the backend avg_entry_price_nanos when it is > 0", () => {
    const p = pos(7, "NO", NO_PRICE);
    expect(avgEntryPriceNanos([fill(7, "NO", 18, 100_000_000n)], 7, "NO", p)).toBe(
      NO_PRICE,
    );
  });

  it("falls back to fills (side price) when backend avg_entry is 0", () => {
    const p = pos(7, "NO", 0n);
    expect(avgEntryPriceNanos([fill(7, "NO", 18, NO_PRICE)], 7, "NO", p)).toBe(NO_PRICE);
  });

  it("returns null when no matching fills", () => {
    expect(avgEntryPriceNanos([], 7, "NO")).toBeNull();
  });
});
