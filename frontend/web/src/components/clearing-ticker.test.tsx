import { describe, expect, it } from "vitest";

import type { IndexMarket } from "@/lib/markets/use-markets";
import type { Block } from "@/lib/ws/types";
import { deriveRecentTrades, recentTradesEmptyCopy } from "./clearing-ticker";

const MARKETS = new Map<number, IndexMarket>([
  [
    7,
    {
      market_id: 7,
      name: "Will the feed stay truthful?",
      status: "open",
    },
  ],
]);

function block(
  height: number,
  trades: Array<{ id: number; yes: string; volume: string }>,
  fillCount = trades.length,
): Block {
  return {
    bridge: { deposit_count: 0, deposit_root_hex: "" },
    by_market: Object.fromEntries(
      trades.map((trade) => [String(trade.id), { volume_nanos: trade.volume }]),
    ),
    clearing_prices_nanos: Object.fromEntries(
      trades.map((trade) => [
        String(trade.id),
        [trade.yes, String(1_000_000_000n - BigInt(trade.yes))],
      ]),
    ),
    events_root: "",
    fill_count: fillCount,
    height,
    order_count: fillCount,
    orders_filled: fillCount,
    parent_hash: "",
    rejection_count: 0,
    state_root: "",
    timestamp_ms: height * 1000,
    total_volume_nanos: "0",
    total_welfare_nanos: "0",
  };
}

describe("deriveRecentTrades", () => {
  it("keeps first, flat, tiny, and material traded clears", () => {
    const recent = [
      block(4, [{ id: 7, yes: "510500000", volume: "1000000000" }]),
      block(3, [{ id: 7, yes: "510000000", volume: "1000000000" }]),
      block(2, [{ id: 7, yes: "510000000", volume: "1000000000" }]),
      block(1, [{ id: 7, yes: "500000000", volume: "1000000000" }]),
    ];

    const events = deriveRecentTrades(recent, MARKETS);

    expect(events.map((event) => event.height)).toEqual([4, 3, 2, 1]);
    expect(events.map((event) => event.ppChange)).toEqual([0.05, 0, 1, null]);
    expect(events[3]).toMatchObject({
      name: "Will the feed stay truthful?",
      yes: 500_000_000n,
    });
  });

  it("excludes clears without positive traded volume", () => {
    const events = deriveRecentTrades(
      [
        block(2, [{ id: 7, yes: "520000000", volume: "0" }], 0),
        block(1, [{ id: 7, yes: "500000000", volume: "1000000000" }]),
      ],
      MARKETS,
    );

    expect(events.map((event) => event.height)).toEqual([1]);
  });

  it("orders market ids deterministically within a block and honors the cap", () => {
    const events = deriveRecentTrades(
      [
        block(1, [
          { id: 9, yes: "500000000", volume: "1" },
          { id: 2, yes: "500000000", volume: "1" },
        ]),
      ],
      MARKETS,
      1,
    );

    expect(events.map((event) => event.id)).toEqual([2]);
  });
});

describe("recentTradesEmptyCopy", () => {
  it("distinguishes loading, outages, real emptiness, and missing attribution", () => {
    expect(recentTradesEmptyCopy("loading", [])).toBe("loading recent trades…");
    expect(recentTradesEmptyCopy("error", [])).toBe(
      "recent trades unavailable",
    );
    expect(recentTradesEmptyCopy("ready", [block(1, [], 0)])).toBe(
      "no trades in recent blocks",
    );
    expect(recentTradesEmptyCopy("ready", [block(1, [], 2)])).toBe(
      "recent trade details unavailable",
    );
  });
});
