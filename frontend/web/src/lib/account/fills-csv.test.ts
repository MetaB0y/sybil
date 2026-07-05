import { describe, expect, it } from "vitest";
import type { components } from "@/lib/api/schema";
import type { HistoryEvent, HistoryEventType } from "./use-account-history";
import { fillRowCount, fillsToCsv } from "./fills-csv";

type Market = components["schemas"]["MarketResponse"];

function market(id: number, name: string): Market {
  return { market_id: id, name, status: "open" };
}

function ev(
  partial: Partial<HistoryEvent> & { id: string; timestampMs: number },
): HistoryEvent {
  return {
    type: "filled" as HistoryEventType,
    blockHeight: 42,
    ...partial,
  };
}

const markets = new Map<number, Market>([[7, market(7, "Will BTC top $100k, in 2026?")]]);

describe("fillsToCsv", () => {
  it("emits a header plus one row per fill, in share/dollar units", () => {
    const events: HistoryEvent[] = [
      ev({
        id: "42.0",
        timestampMs: 0, // 1970-01-01T00:00:00.000Z
        marketId: 7,
        orderId: 9,
        side: "SELL",
        outcome: "YES",
        qty: 1500, // 1.5 shares (1000 units = 1 share)
        priceNanos: 620_000_000n, // $0.62
        realizedPnlNanos: 90_000_000n, // $0.09
      }),
    ];
    const csv = fillsToCsv(events, markets);
    const [header, row] = csv.split("\r\n");
    expect(header).toBe(
      "Time (UTC),Block,Market,Order ID,Side,Outcome,Shares,Price ($),Value ($),Realized PnL ($)",
    );
    // Market name has a comma → must be quoted. Value = 1.5 × $0.62 = $0.93.
    expect(row).toBe(
      '1970-01-01T00:00:00.000Z,42,"Will BTC top $100k, in 2026?",9,SELL,YES,1.5,0.6200,0.9300,0.0900',
    );
  });

  it("leaves realized PnL blank for fills that did not realize anything", () => {
    const events: HistoryEvent[] = [
      ev({
        id: "42.0",
        timestampMs: 0,
        marketId: 7,
        orderId: 3,
        side: "BUY",
        outcome: "NO",
        qty: 1000,
        priceNanos: 380_000_000n,
      }),
    ];
    const csv = fillsToCsv(events, markets);
    const row = csv.split("\r\n")[1]!;
    expect(row.endsWith(",")).toBe(true); // trailing empty realized-PnL field
    expect(row).toContain(",1,0.3800,0.3800,"); // value = 1 × $0.38, blank realized
  });

  it("drops non-fill events (placed / cancelled / resolved) and counts fills", () => {
    const events: HistoryEvent[] = [
      ev({ id: "1.0", timestampMs: 10, type: "placed" }),
      ev({ id: "1.1", timestampMs: 20, type: "partial_fill", marketId: 7, qty: 500, priceNanos: 500_000_000n }),
      ev({ id: "1.2", timestampMs: 30, type: "cancelled" }),
      ev({ id: "2.0", timestampMs: 40, type: "resolved" }),
    ];
    expect(fillRowCount(events)).toBe(1);
    // header + exactly one data row.
    expect(fillsToCsv(events, markets).split("\r\n")).toHaveLength(2);
  });

  it("falls back to #id when the market name is unknown", () => {
    const events: HistoryEvent[] = [
      ev({ id: "1.0", timestampMs: 0, marketId: 99, qty: 1000, priceNanos: 100_000_000n }),
    ];
    expect(fillsToCsv(events, markets).split("\r\n")[1]).toContain(",#99,");
  });
});
