import { describe, expect, it } from "vitest";
import {
  baselinePnlByBot,
  toBotRows,
  WINDOW_DAYS,
} from "./use-bot-leaderboard";
import { mergeAndRank, type LeaderboardRow } from "./use-leaderboard";

const DOLLAR = 1_000_000_000n;

function summary(over: Record<string, unknown>) {
  return {
    trader_name: "Bot",
    account_id: 2,
    active: true,
    scored: true,
    decision_count: 0,
    pnl: 0,
    portfolio_value: 500,
    ...over,
  };
}

function feed(summaries: ReturnType<typeof summary>[]) {
  // Only the fields toBotRows reads; the wire type carries far more.
  return { db_available: true, summaries } as never;
}

function human(over: Partial<LeaderboardRow>): LeaderboardRow {
  return {
    rank: 1,
    kind: "human",
    accountId: 39,
    label: "vgvg",
    pnlNanos: 0n,
    roiBps: 0,
    marketsTraded: 1,
    equityNanos: 1000n * DOLLAR,
    ...over,
  };
}

describe("toBotRows", () => {
  it("keeps only scored competitors, not the load and noise cohorts", () => {
    const rows = toBotRows(
      feed([
        summary({ trader_name: "Contrarian (Kelly)", account_id: 4 }),
        summary({ trader_name: "Fast-0", account_id: 8, scored: false }),
        summary({ trader_name: "Noise-0", account_id: 13, scored: false }),
      ]),
      new Map(),
    );

    expect(rows.map((r) => r.label)).toEqual(["Contrarian (Kelly)"]);
    expect(rows[0]?.kind).toBe("bot");
  });

  it("drops bots with no sequencer account rather than keying on null", () => {
    const rows = toBotRows(
      feed([summary({ trader_name: "Orphan", account_id: null })]),
      new Map(),
    );
    expect(rows).toEqual([]);
  });

  it("converts dollar doubles to nanodollars and derives ROI from opening capital", () => {
    // Holds $402.90 after losing $97.10, so it opened the window with $500.
    const rows = toBotRows(
      feed([summary({ pnl: -97.1, portfolio_value: 402.9 })]),
      new Map(),
    );

    expect(rows[0]?.pnlNanos).toBe(-97_100_000_000n);
    expect(rows[0]?.equityNanos).toBe(402_900_000_000n);
    expect(rows[0]?.roiBps).toBe(-1942); // -97.10 / 500 = -19.42%
  });

  it("subtracts the window baseline so a window is not reported as all-time", () => {
    const rows = toBotRows(
      feed([summary({ trader_name: "B", pnl: -100, portfolio_value: 400 })]),
      new Map([["B", -60]]),
    );
    // Lost 100 overall but only 40 inside the window.
    expect(rows[0]?.pnlNanos).toBe(-40n * DOLLAR);
  });

  it("reports markets as unknown rather than substituting a fill count", () => {
    const rows = toBotRows(feed([summary({})]), new Map());
    expect(rows[0]?.marketsTraded).toBeNull();
  });
});

describe("baselinePnlByBot", () => {
  it("takes each bot's first in-window snapshot, not its last", () => {
    const baseline = baselinePnlByBot([
      { id: 1, trader_name: "A", pnl: -5 },
      { id: 2, trader_name: "A", pnl: -9 },
      { id: 3, trader_name: "B", pnl: 2 },
    ] as never);

    expect(baseline.get("A")).toBe(-5);
    expect(baseline.get("B")).toBe(2);
  });

  it("treats an absent series as no baseline", () => {
    expect(baselinePnlByBot(undefined).size).toBe(0);
  });
});

describe("mergeAndRank", () => {
  it("renumbers across both ledgers instead of trusting the server rank", () => {
    const merged = mergeAndRank(
      [human({ rank: 1, pnlNanos: -3n * DOLLAR })],
      [
        { ...human({}), kind: "bot", accountId: 4, label: "Bot A", pnlNanos: 5n * DOLLAR },
        { ...human({}), kind: "bot", accountId: 5, label: "Bot B", pnlNanos: -9n * DOLLAR },
      ],
    );

    expect(merged.map((r) => [r.rank, r.label])).toEqual([
      [1, "Bot A"],
      [2, "vgvg"],
      [3, "Bot B"],
    ]);
  });

  it("orders ties deterministically so refetches do not reshuffle rows", () => {
    const tie = { pnlNanos: 0n };
    const a = mergeAndRank(
      [human({ ...tie, accountId: 39 })],
      [{ ...human({ ...tie }), kind: "bot", accountId: 4 }],
    );
    const b = mergeAndRank(
      [human({ ...tie, accountId: 39 })],
      [{ ...human({ ...tie }), kind: "bot", accountId: 4 }],
    );
    expect(a.map((r) => r.kind)).toEqual(["human", "bot"]);
    expect(a).toEqual(b);
  });
});

describe("window lookback", () => {
  it("maps tabs to their lookback, with all-time unbounded", () => {
    expect(WINDOW_DAYS["7D"]).toBe(7);
    expect(WINDOW_DAYS["30D"]).toBe(30);
    expect(WINDOW_DAYS.ALL).toBeNull();
  });
});
